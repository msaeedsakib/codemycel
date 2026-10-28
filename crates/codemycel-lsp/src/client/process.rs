use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// A language server process. Dropping it kills the server and every process it started.
#[derive(Debug)]
pub struct ServerProcess {
    child: Child,
    pid: Option<u32>,
}

impl ServerProcess {
    /// Spawns `command` in its own process group, with stdin and stdout piped for JSON-RPC and
    /// stderr forwarded to the log.
    pub fn spawn(
        command: &Path,
        args: &[&str],
        cwd: &Path,
        env: &[(OsString, OsString)],
    ) -> io::Result<(Self, ChildStdin, ChildStdout)> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(cwd)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn()?;
        let missing = || io::Error::other("child stdio was not captured");
        let stdin = child.stdin.take().ok_or_else(missing)?;
        let stdout = child.stdout.take().ok_or_else(missing)?;
        if let Some(stderr) = child.stderr.take() {
            let name = command.display().to_string();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::debug!(server = %name, "{line}");
                }
            });
        }
        let pid = child.id();
        Ok((Self { child, pid }, stdin, stdout))
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn has_exited(&mut self) -> bool {
        !matches!(self.child.try_wait(), Ok(None))
    }

    pub async fn wait(&mut self) -> io::Result<()> {
        self.child.wait().await.map(drop)
    }

    fn kill_tree(&mut self) {
        let Some(pid) = self.pid.take() else {
            return;
        };
        #[cfg(unix)]
        if let Ok(pid) = i32::try_from(pid) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        #[cfg(windows)]
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.child.start_kill();
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.kill_tree();
    }
}
