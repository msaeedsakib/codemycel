#![allow(clippy::unwrap_used)]

use codemycel_core::RepoPath;
use codemycel_parse::manifests::{Dependency, DependencyKind, Ecosystem, parse_cargo, parse_npm};

fn repo(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

fn dep(name: &str, kind: DependencyKind, workspace_protocol: bool) -> Dependency {
    Dependency {
        name: name.to_owned(),
        kind,
        workspace_protocol,
    }
}

#[test]
fn cargo_workspace_members_and_dependencies() {
    let root = parse_cargo(
        &repo("Cargo.toml"),
        b"[workspace]\nmembers = [\"crates/*\", \"tools/gen\"]\nexclude = [\"crates/scratch\"]\n",
    )
    .unwrap();
    assert_eq!(root.ecosystem, Ecosystem::Cargo);
    assert_eq!(root.name, None);
    assert!(root.directory.is_root());
    assert!(root.has_member(&repo("crates/core")));
    assert!(root.has_member(&repo("tools/gen")));
    assert!(!root.has_member(&repo("crates/scratch")));
    assert!(!root.has_member(&repo("crates/core/nested")));

    let member = parse_cargo(
        &repo("crates/cli/Cargo.toml"),
        br#"[package]
name = "cli"
version.workspace = true

[dependencies]
core = { path = "../core" }
json = { package = "serde_json", version = "1" }

[dev-dependencies]
tempfile = "3"

[build-dependencies]
cc = "1"
"#,
    )
    .unwrap();
    assert_eq!(member.name.as_deref(), Some("cli"));
    assert_eq!(member.version, None);
    assert_eq!(member.directory, repo("crates/cli"));
    assert_eq!(
        member.dependencies,
        [
            dep("core", DependencyKind::Normal, false),
            dep("serde_json", DependencyKind::Normal, false),
            dep("tempfile", DependencyKind::Dev, false),
            dep("cc", DependencyKind::Build, false),
        ]
    );
    assert!(parse_cargo(&repo("Cargo.toml"), b"[package\n").is_err());
}

#[test]
fn npm_workspaces_and_workspace_protocol() {
    let root = parse_npm(
        &repo("package.json"),
        br#"{ "private": true, "workspaces": ["packages/*", "!packages/legacy"] }"#,
    )
    .unwrap();
    assert_eq!(root.name, None);
    assert!(root.has_member(&repo("packages/app")));
    assert!(!root.has_member(&repo("packages/legacy")));

    let object_form = parse_npm(
        &repo("web/package.json"),
        br#"{ "name": "web", "workspaces": { "packages": ["libs/*"] } }"#,
    )
    .unwrap();
    assert!(object_form.has_member(&repo("web/libs/ui")));
    assert!(!object_form.has_member(&repo("libs/ui")));

    let app = parse_npm(
        &repo("packages/app/package.json"),
        br#"{
  "name": "@fixture/app",
  "version": "1.2.0",
  "dependencies": { "@fixture/core": "workspace:*", "react": "^19.0.0" },
  "devDependencies": { "typescript": "7.0.2" },
  "peerDependencies": { "react-dom": "*" }
}"#,
    )
    .unwrap();
    assert_eq!(app.name.as_deref(), Some("@fixture/app"));
    assert_eq!(app.version.as_deref(), Some("1.2.0"));
    assert_eq!(
        app.dependencies,
        [
            dep("@fixture/core", DependencyKind::Normal, true),
            dep("react", DependencyKind::Normal, false),
            dep("typescript", DependencyKind::Dev, false),
            dep("react-dom", DependencyKind::Peer, false),
        ]
    );
}
