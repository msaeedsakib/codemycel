use std::io::Read;
use std::path::Path;
use std::time::Duration;

use codemycel_lsp::provision::download::DENY_NETWORK_ENV;
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};

pub const DISABLE_ENV: &str = "CODEMYCEL_NO_UPDATE_CHECK";
/// Replaces the latest-release API URL, so tests can serve releases locally.
pub const RELEASES_URL_ENV: &str = "CODEMYCEL_RELEASES_URL";

const STATE_FILE: &str = "update-check.json";
const INTERVAL: SignedDuration = SignedDuration::from_hours(24);
const CHECK_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

impl Release {
    /// The tag as a version, without a leading `v`.
    pub fn version(&self) -> Option<semver::Version> {
        semver::Version::parse(self.tag_name.trim_start_matches('v')).ok()
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }
}

#[derive(Serialize, Deserialize)]
struct State {
    checked_at: Timestamp,
}

pub fn current_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(semver::Version::new(0, 0, 0))
}

pub fn network_denied() -> bool {
    std::env::var_os(DENY_NETWORK_ENV).is_some_and(|v| v == "1")
}

/// An HTTP agent whose requests give up after `timeout`.
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// The latest release on GitHub, from its releases API.
pub fn latest_release(agent: &ureq::Agent) -> Result<Release, String> {
    if network_denied() {
        return Err(format!("network access denied by {DENY_NETWORK_ENV}"));
    }
    let url = std::env::var(RELEASES_URL_ENV).unwrap_or_else(|_| {
        let repository = env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/");
        format!("https://api.github.com/repos/{repository}/releases/latest")
    });
    let mut body = String::new();
    agent
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("fetching {url}: {e}"))?
        .into_body()
        .into_reader()
        .read_to_string(&mut body)
        .map_err(|e| format!("reading {url}: {e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("reading {url}: {e}"))
}

/// At most once per 24 hours, asks for the latest release and prints one line on stderr when
/// it is newer. Best effort: every failure is silent. Off with `CODEMYCEL_NO_UPDATE_CHECK=1`.
pub fn run(home: &Path) {
    if std::env::var_os(DISABLE_ENV).is_some_and(|v| v == "1") || network_denied() {
        return;
    }
    let path = home.join(STATE_FILE);
    let now = Timestamp::now();
    let last = std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<State>(&bytes).ok());
    if last.is_some_and(|state| now.duration_since(state.checked_at) < INTERVAL) {
        return;
    }
    let state = State { checked_at: now };
    let written = std::fs::create_dir_all(home)
        .ok()
        .and_then(|()| serde_json::to_vec(&state).ok())
        .and_then(|bytes| std::fs::write(&path, bytes).ok());
    if written.is_none() {
        return;
    }
    if let Ok(release) = latest_release(&agent(CHECK_TIMEOUT))
        && let Some(latest) = release.version()
        && latest > current_version()
    {
        eprintln!(
            "codemycel {latest} is available (this is {}): run codemycel upgrade",
            current_version()
        );
    }
}
