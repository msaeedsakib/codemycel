use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context as _, bail};
use sha2::{Digest, Sha256};

use super::ExitCode;
use crate::cli::{Context, UpgradeArgs};
use crate::update_check::{self, Release};

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(600);
const BINARY: &str = if cfg!(windows) {
    "codemycel.exe"
} else {
    "codemycel"
};

/// Replaces this binary with the latest release's, after checking the archive against the
/// release's `<archive>.sha256`.
pub fn run(_args: &UpgradeArgs, _ctx: &Context) -> anyhow::Result<ExitCode> {
    if update_check::network_denied() {
        bail!(
            "network access denied by {}",
            codemycel_lsp::provision::download::DENY_NETWORK_ENV
        );
    }
    let agent = update_check::agent(DOWNLOAD_TIMEOUT);
    let release = update_check::latest_release(&agent).map_err(anyhow::Error::msg)?;
    let current = update_check::current_version();
    let latest = release
        .version()
        .with_context(|| format!("release tag `{}` is not a version", release.tag_name))?;
    if latest <= current {
        println!("codemycel {current} is the latest release");
        return Ok(ExitCode::Ok);
    }
    let archive_name = archive_name().context("no release is built for this platform")?;
    let archive = download(&agent, &release, &archive_name)?;
    let checksum = download(&agent, &release, &format!("{archive_name}.sha256"))?;
    let expected = String::from_utf8_lossy(&checksum)
        .split_whitespace()
        .next()
        .map(str::to_lowercase)
        .context("the checksum file is empty")?;
    let actual: String = Sha256::digest(&archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if actual != expected {
        bail!("checksum mismatch for {archive_name}: expected {expected}, got {actual}");
    }
    let binary = extract(&archive_name, archive)?;
    let exe = std::env::current_exe().context("locating this binary")?;
    replace(&exe, &binary)?;
    println!("upgraded codemycel {current} to {latest}");
    println!("changelog: {}", release.html_url);
    Ok(ExitCode::Ok)
}

/// `codemycel-<target triple>.tar.gz`, or `.zip` on Windows.
fn archive_name() -> Option<String> {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        _ => return None,
    };
    let extension = if cfg!(windows) { "zip" } else { "tar.gz" };
    Some(format!("codemycel-{target}.{extension}"))
}

fn download(agent: &ureq::Agent, release: &Release, name: &str) -> anyhow::Result<Vec<u8>> {
    let asset = release
        .asset(name)
        .with_context(|| format!("release {} has no {name}", release.tag_name))?;
    let url = &asset.browser_download_url;
    let mut bytes = Vec::new();
    agent
        .get(url)
        .call()
        .with_context(|| format!("downloading {url}"))?
        .into_body()
        .into_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("downloading {url}"))?;
    Ok(bytes)
}

/// The `codemycel` binary inside the archive, wherever it sits.
fn extract(archive_name: &str, archive: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let is_binary = |path: &Path| path.file_name().is_some_and(|name| name == BINARY);
    let mut binary = Vec::new();
    if archive_name.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(Cursor::new(archive))?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index)?;
            if entry.enclosed_name().is_some_and(|path| is_binary(&path)) {
                entry.read_to_end(&mut binary)?;
                return Ok(binary);
            }
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(Cursor::new(archive)));
        for entry in tar.entries()? {
            let mut entry = entry?;
            if is_binary(&entry.path()?) {
                entry.read_to_end(&mut binary)?;
                return Ok(binary);
            }
        }
    }
    bail!("{archive_name} holds no {BINARY}")
}

/// Writes `binary` beside `exe` and moves it into place. Windows cannot overwrite a running
/// executable, so there the old one is renamed aside first.
fn replace(exe: &Path, binary: &[u8]) -> anyhow::Result<()> {
    let dir = exe
        .parent()
        .context("this binary has no parent directory")?;
    let staged = tempfile::NamedTempFile::new_in(dir)
        .with_context(|| format!("staging in {}", dir.display()))?;
    std::fs::write(staged.path(), binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staged.path(), std::fs::Permissions::from_mode(0o755))?;
    }
    if cfg!(windows) {
        let old = exe.with_extension("exe.old");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old).with_context(|| format!("moving {} aside", exe.display()))?;
    }
    staged
        .persist(exe)
        .with_context(|| format!("replacing {}", exe.display()))?;
    Ok(())
}
