use std::io::Write as _;
use std::process::{Command, Stdio};

use anyhow::Context as _;
use codemycel_ui::UiServer;

use super::ExitCode;
use crate::cli::{Context, UiArgs};

/// Serves the web view on 127.0.0.1 until interrupted, after printing its URL and opening it in
/// the system browser. A browser that fails to open is not an error.
pub fn run(args: &UiArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let location = super::locate(ctx)?;
    if super::open_index(&location, ctx.json)?.is_none() {
        return Ok(ExitCode::Degraded);
    }
    let server = UiServer::bind(&location.root, args.port).context("binding 127.0.0.1")?;
    let url = format!("http://{}/", server.addr()?);
    println!("{url}");
    std::io::stdout().flush()?;
    open_browser(&url);
    server.serve().context("serving the web view")?;
    Ok(ExitCode::Ok)
}

fn open_browser(url: &str) {
    if std::env::var_os("CODEMYCEL_TEST_NO_BROWSER").is_some() {
        return;
    }
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    } else {
        Command::new("xdg-open")
    };
    let spawned = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        std::thread::spawn(move || child.wait());
    }
}
