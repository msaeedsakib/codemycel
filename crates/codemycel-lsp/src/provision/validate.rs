use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a validation command may run before the candidate is rejected.
pub const VALIDATE_TIMEOUT: Duration = Duration::from_secs(20);

/// Runs `binary args` and returns its trimmed stdout, or why it is rejected: it would not start,
/// ran longer than `timeout`, or exited unsuccessfully. For `ServerAdapter::validate`.
pub fn command_output(binary: &Path, args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run `{}`: {error}", binary.display()))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < timeout => {
                std::thread::sleep(Duration::from_millis(20))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "`{} {}` did not finish within {timeout:?}",
                    binary.display(),
                    args.join(" ")
                ));
            }
            Err(error) => return Err(format!("waiting for `{}`: {error}", binary.display())),
        }
    };
    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }
    if status.success() {
        Ok(stdout.trim().to_owned())
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Err(format!(
            "`{} {}` failed ({status}): {detail}",
            binary.display(),
            args.join(" ")
        ))
    }
}
