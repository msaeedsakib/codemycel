#![allow(clippy::unwrap_used)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use codemycel_core::config::{self, Config, RepoConfig, RootOverride, ServerConfig, paths};
use codemycel_core::{Language, RepoPath};
use tempfile::TempDir;

fn write_repo_config(repo: &Path, text: &str) {
    let dir = repo.join(".codemycel");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("config.toml"), text).unwrap();
}

fn write_home_config(home: &Path, text: &str) {
    std::fs::create_dir_all(home).unwrap();
    std::fs::write(home.join("config.toml"), text).unwrap();
}

#[test]
fn template_config_parses_with_all_keys_commented() {
    let parsed: RepoConfig = toml::from_str(config::TEMPLATE).unwrap();
    assert_eq!(parsed, RepoConfig::default());

    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_repo_config(repo.path(), config::TEMPLATE);
    let loaded = Config::load_with_home(repo.path(), home.path()).unwrap();
    assert_eq!(loaded, Config::default());
    assert_eq!(loaded.max_file_size, 2 * 1024 * 1024);
    assert_eq!(loaded.scip_path, PathBuf::from("index.scip"));
    assert!(!loaded.no_download);
    assert!(loaded.build_scripts(Language::Rust));
}

#[test]
fn repo_config_overrides_home_config() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_home_config(
        home.path(),
        r#"
no_download = true

[servers.rust]
path = "/home/rust-analyzer"
build_scripts = false

[servers.typescript]
path = "/home/tsc"
"#,
    );
    write_repo_config(
        repo.path(),
        r#"
[index]
max_file_size = "512KiB"

[servers.rust]
path = "/repo/rust-analyzer"
"#,
    );

    let config = Config::load_with_home(repo.path(), home.path()).unwrap();

    assert_eq!(
        config.server(Language::Rust),
        ServerConfig {
            path: Some(PathBuf::from("/repo/rust-analyzer")),
            build_scripts: Some(false),
        }
    );
    assert!(!config.build_scripts(Language::Rust));
    assert_eq!(
        config.server(Language::TypeScript).path,
        Some(PathBuf::from("/home/tsc"))
    );
    assert_eq!(config.server(Language::JavaScript), ServerConfig::default());
    assert!(config.no_download);
    assert_eq!(config.max_file_size, 512 * 1024);
}

#[test]
fn codemycel_home_env_redirects_home_config() {
    let custom = TempDir::new().unwrap();
    let user_home = PathBuf::from("/nonexistent/user");
    let env = |name: &str| (name == "CODEMYCEL_HOME").then(|| OsString::from(custom.path()));

    let home = paths::home_dir_with(env, Some(user_home.clone())).unwrap();
    assert_eq!(home, custom.path());
    assert_eq!(
        paths::home_dir_with(|_| None, Some(user_home.clone())).unwrap(),
        user_home.join(".codemycel")
    );

    write_home_config(&home, "no_download = true\n");
    let repo = TempDir::new().unwrap();
    assert!(
        Config::load_with_home(repo.path(), &home)
            .unwrap()
            .no_download
    );
}

#[test]
fn unknown_key_is_an_error_naming_the_key() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_repo_config(repo.path(), "[index]\nmax_file_sise = \"1MiB\"\n");

    let message = Config::load_with_home(repo.path(), home.path())
        .unwrap_err()
        .to_string();
    assert!(message.contains("max_file_sise"), "{message}");
    assert!(message.contains("config.toml"), "{message}");

    let repo = TempDir::new().unwrap();
    write_home_config(home.path(), "[servers.rust]\nbuild_script = true\n");
    let message = Config::load_with_home(repo.path(), home.path())
        .unwrap_err()
        .to_string();
    assert!(message.contains("build_script"), "{message}");
}

#[test]
fn root_override_entries_parse() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    write_repo_config(
        repo.path(),
        r#"
[[root]]
path = "apps/web"
language = "typescript"

[[root]]
path = "apps/desktop/src-tauri"
language = "rust"
disabled = true
"#,
    );

    let config = Config::load_with_home(repo.path(), home.path()).unwrap();
    assert_eq!(
        config.roots,
        vec![
            RootOverride {
                path: RepoPath::new("apps/web").unwrap(),
                language: Language::TypeScript,
                disabled: false,
            },
            RootOverride {
                path: RepoPath::new("apps/desktop/src-tauri").unwrap(),
                language: Language::Rust,
                disabled: true,
            },
        ]
    );
}

#[test]
fn max_file_size_accepts_units_and_bare_integers() {
    let size = |value: &str| -> Result<u64, String> {
        toml::from_str::<RepoConfig>(&format!("[index]\nmax_file_size = {value}\n"))
            .map(|config| config.index.max_file_size.unwrap().0)
            .map_err(|error| error.to_string())
    };
    assert_eq!(size("4096").unwrap(), 4096);
    assert_eq!(size("\"4096\"").unwrap(), 4096);
    assert_eq!(size("\"10B\"").unwrap(), 10);
    assert_eq!(size("\"3KiB\"").unwrap(), 3 * 1024);
    assert_eq!(size("\"2 MiB\"").unwrap(), 2 * 1024 * 1024);
    assert_eq!(size("\"1GiB\"").unwrap(), 1024 * 1024 * 1024);
    assert!(size("\"2MB\"").unwrap_err().contains("invalid size"));
    assert!(size("-1").is_err());
}

#[test]
fn config_writer_changes_only_the_target_key() {
    let repo = TempDir::new().unwrap();
    let home = TempDir::new().unwrap();
    let path = paths::repo_config_path(repo.path());
    let read = || std::fs::read_to_string(&path).unwrap();

    config::ConfigWriter::set_scip_commit(repo.path(), "abc123").unwrap();
    let first = read();
    let added: Vec<&str> = first
        .lines()
        .filter(|line| !config::TEMPLATE.lines().any(|t| t == *line))
        .collect();
    assert_eq!(added, ["commit = \"abc123\""]);
    assert_eq!(first.lines().count(), config::TEMPLATE.lines().count() + 1);
    let loaded = Config::load_with_home(repo.path(), home.path()).unwrap();
    assert_eq!(loaded.scip_commit.as_deref(), Some("abc123"));

    config::ConfigWriter::set_scip_commit(repo.path(), "def456").unwrap();
    assert_eq!(read(), first.replace("abc123", "def456"));

    let custom = "# mine\n[index]\nignore = [\"x/**\"] # keep\n\n[scip]\npath = \"out/index.scip\"\ncommit = \"old\" # stale\n\n[servers.rust]\nbuild_scripts = false\n";
    write_repo_config(repo.path(), custom);
    config::ConfigWriter::set_scip_commit(repo.path(), "new").unwrap();
    assert_eq!(
        read(),
        custom.replace("commit = \"old\" # stale", "commit = \"new\"")
    );

    write_repo_config(repo.path(), "[index]\nignore = []\n");
    config::ConfigWriter::set_scip_commit(repo.path(), "sha").unwrap();
    assert_eq!(read(), "[index]\nignore = []\n\n[scip]\ncommit = \"sha\"\n");

    write_repo_config(
        repo.path(),
        "[scip]  # imported index\npath = \"out/index.scip\"\n",
    );
    config::ConfigWriter::set_scip_commit(repo.path(), "sha").unwrap();
    assert_eq!(
        read(),
        "[scip]  # imported index\ncommit = \"sha\"\npath = \"out/index.scip\"\n"
    );
}
