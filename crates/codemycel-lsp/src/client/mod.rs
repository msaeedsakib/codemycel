mod errors;
mod positions;
mod process;
mod requests;
mod transport;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lsp_types::Uri;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::BufReader;
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::{Semaphore, mpsc, oneshot};

pub use errors::ClientError;
pub use positions::LineIndex;
pub use process::ServerProcess;
pub use transport::{read_message, write_message};

/// A notification the server sent, with its raw parameters.
#[derive(Clone, Debug)]
pub struct Notification {
    pub method: String,
    pub params: Value,
}

type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, ClientError>>>>>;

/// A JSON-RPC connection to one language server process. Requests carry their own timeout and
/// at most `concurrency` are in flight at once.
#[derive(Debug)]
pub struct LspClient {
    process: Mutex<ServerProcess>,
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
    pending: Pending,
    next_id: AtomicI64,
    alive: Arc<AtomicBool>,
    permits: Semaphore,
    notifications: Mutex<Option<mpsc::UnboundedReceiver<Notification>>>,
}

impl LspClient {
    /// Starts `command` and the reader and writer tasks. Must run inside a tokio runtime.
    pub fn spawn(
        command: &Path,
        args: &[&str],
        cwd: &Path,
        env: &[(OsString, OsString)],
        concurrency: usize,
    ) -> Result<Self, ClientError> {
        let (process, stdin, stdout) =
            ServerProcess::spawn(command, args, cwd, env).map_err(ClientError::Spawn)?;
        let (outgoing, outgoing_rx) = mpsc::unbounded_channel();
        let (notify_tx, notify_rx) = mpsc::unbounded_channel();
        let pending = Pending::default();
        let alive = Arc::new(AtomicBool::new(true));
        tokio::spawn(write_loop(stdin, outgoing_rx, alive.clone()));
        tokio::spawn(read_loop(
            stdout,
            pending.clone(),
            notify_tx,
            outgoing.clone(),
            alive.clone(),
        ));
        Ok(Self {
            process: Mutex::new(process),
            outgoing,
            pending,
            next_id: AtomicI64::new(1),
            alive,
            permits: Semaphore::new(concurrency.max(1)),
            notifications: Mutex::new(Some(notify_rx)),
        })
    }

    /// Whether the connection is open and the process is running.
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
            && self
                .process
                .lock()
                .map(|mut process| !process.has_exited())
                .unwrap_or(false)
    }

    pub fn pid(&self) -> Option<u32> {
        self.process.lock().ok().and_then(|process| process.pid())
    }

    /// Every notification the server sends, in order. Available once.
    pub fn notifications(&self) -> Option<mpsc::UnboundedReceiver<Notification>> {
        self.notifications.lock().ok()?.take()
    }

    pub async fn request<P: Serialize, R: DeserializeOwned>(
        &self,
        method: &str,
        params: P,
        timeout: Duration,
    ) -> Result<R, ClientError> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| ClientError::Closed)?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending
            .lock()
            .map_err(|_| ClientError::Closed)?
            .insert(id, tx);
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if !self.alive.load(Ordering::SeqCst) || self.send(&message).is_err() {
            self.forget(id);
            return Err(ClientError::Closed);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(result)) => Ok(serde_json::from_value(result?)?),
            Ok(Err(_)) => Err(ClientError::Closed),
            Err(_) => {
                self.forget(id);
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(ClientError::Timeout(timeout))
            }
        }
    }

    pub fn notify<P: Serialize>(&self, method: &str, params: P) -> Result<(), ClientError> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn send(&self, message: &Value) -> Result<(), ClientError> {
        let body = serde_json::to_vec(message)?;
        self.outgoing.send(body).map_err(|_| ClientError::Closed)
    }

    fn forget(&self, id: i64) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&id);
        }
    }

    /// `shutdown`, then `exit`, then waits briefly for the process; it is killed if it lingers.
    pub async fn shutdown_and_exit(self) {
        let grace = Duration::from_secs(5);
        if self
            .request::<_, Value>("shutdown", Value::Null, grace)
            .await
            .is_ok()
        {
            let _ = self.notify("exit", Value::Null);
        }
        let process = self.process.into_inner();
        if let Ok(mut process) = process {
            let _ = tokio::time::timeout(grace, process.wait()).await;
        }
    }
}

async fn write_loop(
    mut stdin: ChildStdin,
    mut outgoing: mpsc::UnboundedReceiver<Vec<u8>>,
    alive: Arc<AtomicBool>,
) {
    while let Some(body) = outgoing.recv().await {
        if write_message(&mut stdin, &body).await.is_err() {
            break;
        }
    }
    alive.store(false, Ordering::SeqCst);
}

async fn read_loop(
    stdout: ChildStdout,
    pending: Pending,
    notifications: mpsc::UnboundedSender<Notification>,
    outgoing: mpsc::UnboundedSender<Vec<u8>>,
    alive: Arc<AtomicBool>,
) {
    let mut reader = BufReader::new(stdout);
    while let Ok(Some(body)) = read_message(&mut reader).await {
        let Ok(message) = serde_json::from_slice::<Value>(&body) else {
            continue;
        };
        let method = message.get("method").and_then(Value::as_str);
        let id = message.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => {
                let reply = json!({"jsonrpc": "2.0", "id": id, "result": server_request_reply(method, &message)});
                if let Ok(body) = serde_json::to_vec(&reply) {
                    let _ = outgoing.send(body);
                }
            }
            (Some(method), None) => {
                let _ = notifications.send(Notification {
                    method: method.to_owned(),
                    params: message.get("params").cloned().unwrap_or(Value::Null),
                });
            }
            (None, Some(id)) => {
                let Some(id) = id.as_i64() else { continue };
                let Some(tx) = pending.lock().ok().and_then(|mut p| p.remove(&id)) else {
                    continue;
                };
                let result = match message.get("error") {
                    Some(error) => Err(ClientError::Response {
                        code: error.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    }),
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(result);
            }
            (None, None) => {}
        }
    }
    alive.store(false, Ordering::SeqCst);
    if let Ok(mut pending) = pending.lock() {
        pending.clear();
    }
}

/// The answer to a request from the server: one `null` per item for `workspace/configuration`,
/// so the server keeps its initialization options, and `null` for everything else.
fn server_request_reply(method: &str, message: &Value) -> Value {
    match method {
        "workspace/configuration" => {
            let items = message
                .pointer("/params/items")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            Value::Array(vec![Value::Null; items])
        }
        _ => Value::Null,
    }
}

/// The `file://` URI of an absolute path.
pub fn path_to_uri(path: &Path) -> Result<Uri, ClientError> {
    let url = url::Url::from_file_path(path)
        .map_err(|()| ClientError::Uri(path.display().to_string()))?;
    url.as_str()
        .parse()
        .map_err(|_| ClientError::Uri(path.display().to_string()))
}

/// The path of a `file://` URI; `None` for any other scheme.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}
