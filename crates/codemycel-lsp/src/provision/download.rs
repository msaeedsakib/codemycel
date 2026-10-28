use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

use codemycel_core::ServerAdapter;
use sha2::{Digest, Sha256};

use super::cache;

/// Set to `1` to make every download fail before any connection is opened.
pub const DENY_NETWORK_ENV: &str = "CODEMYCEL_TEST_DENY_NETWORK";

/// Downloads `url`, checks it against `sha256`, extracts the binary, and moves the version
/// directory into the cache in one rename. Nothing is left in the cache on failure.
pub fn download(
    home: &Path,
    adapter: &dyn ServerAdapter,
    url: &str,
    sha256: &str,
    member: Option<&str>,
) -> Result<PathBuf, String> {
    if std::env::var_os(DENY_NETWORK_ENV).is_some_and(|v| v == "1") {
        return Err(format!("network access denied by {DENY_NETWORK_ENV}"));
    }
    let server_dir = cache::server_dir(home, adapter);
    std::fs::create_dir_all(&server_dir)
        .map_err(|e| format!("creating {}: {e}", server_dir.display()))?;
    let staging = tempfile::Builder::new()
        .prefix(".download-")
        .tempdir_in(&server_dir)
        .map_err(|e| format!("creating a staging directory: {e}"))?;
    let archive_path = staging.path().join("archive");
    let digest = fetch(url, &archive_path)?;
    if !digest.eq_ignore_ascii_case(sha256) {
        return Err(format!(
            "checksum mismatch for {url}: expected {sha256}, got {digest}"
        ));
    }
    let version_dir = staging.path().join("version");
    std::fs::create_dir(&version_dir).map_err(|e| e.to_string())?;
    let binary = version_dir.join(cache::binary_file_name(adapter));
    extract(&archive_path, url, member, &binary).map_err(|e| format!("extracting {url}: {e}"))?;
    make_executable(&binary).map_err(|e| e.to_string())?;
    let target = cache::version_dir(home, adapter);
    if target.exists() {
        std::fs::remove_dir_all(&target)
            .map_err(|e| format!("replacing {}: {e}", target.display()))?;
    }
    std::fs::rename(&version_dir, &target)
        .map_err(|e| format!("moving the download into {}: {e}", target.display()))?;
    Ok(cache::cached_binary(home, adapter))
}

/// Streams `url` into `dest` and returns the lowercase hex SHA-256 of the bytes.
fn fetch(url: &str, dest: &Path) -> Result<String, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("downloading {url}: {e}"))?;
    let mut reader = response.into_body().into_reader();
    let mut file = File::create(dest).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|e| format!("downloading {url}: {e}"))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// `.tgz`/`.tar.gz` and `.zip` archives yield `member` as `dest`, and every other file under the
/// member's directory beside it, keeping their relative paths, since a binary may need the
/// files shipped next to it. A `.gz` is the binary itself.
fn extract(archive: &Path, url: &str, member: Option<&str>, dest: &Path) -> io::Result<()> {
    let file = File::open(archive)?;
    let name = url.rsplit('/').next().unwrap_or(url);
    let dir = dest.parent().unwrap_or(dest);
    if name.ends_with(".tgz") || name.ends_with(".tar.gz") {
        let member = member.ok_or_else(|| missing("a tar archive needs a member path"))?;
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut found = false;
        for entry in tar.entries()? {
            let mut entry = entry?;
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry.path()?.into_owned();
            if let Some(target) = placement(&path, member, dest, dir) {
                found |= target == dest;
                copy_into(&mut entry, &target)?;
            }
        }
        found
            .then_some(())
            .ok_or_else(|| missing(&format!("`{member}` is not in the archive")))
    } else if name.ends_with(".zip") {
        let member = member.ok_or_else(|| missing("a zip archive needs a member path"))?;
        let mut zip = zip::ZipArchive::new(file).map_err(io::Error::other)?;
        zip.by_name(member).map_err(io::Error::other)?;
        for index in 0..zip.len() {
            let mut entry = zip.by_index(index).map_err(io::Error::other)?;
            if !entry.is_file() {
                continue;
            }
            let Some(path) = entry.enclosed_name() else {
                continue;
            };
            if let Some(target) = placement(&path, member, dest, dir) {
                copy_into(&mut entry, &target)?;
            }
        }
        Ok(())
    } else if name.ends_with(".gz") {
        copy_into(&mut flate2::read::GzDecoder::new(file), dest)
    } else {
        let mut file = file;
        file.rewind()?;
        copy_into(&mut file, dest)
    }
}

/// Where an archive entry goes: `dest` for the member, `dir/<relative path>` for a file under
/// the member's directory, nowhere for anything else or a path that climbs out.
fn placement(path: &Path, member: &str, dest: &Path, dir: &Path) -> Option<PathBuf> {
    let member = Path::new(member);
    if path == member {
        return Some(dest.to_owned());
    }
    let relative = path.strip_prefix(member.parent()?).ok()?;
    let safe = relative
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (safe && relative.components().next().is_some()).then(|| dir.join(relative))
}

fn copy_into(reader: &mut impl Read, dest: &Path) -> io::Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = File::create(dest)?;
    io::copy(reader, &mut out)?;
    out.flush()
}

fn missing(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, message.to_owned())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> io::Result<()> {
    Ok(())
}
