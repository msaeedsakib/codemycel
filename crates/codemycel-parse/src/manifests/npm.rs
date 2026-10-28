use codemycel_core::{AdapterError, RepoPath};
use serde_json::{Map, Value};

use super::{Dependency, DependencyKind, Ecosystem, PackageParse, directory, malformed};

const SECTIONS: [(&str, DependencyKind); 3] = [
    ("dependencies", DependencyKind::Normal),
    ("devDependencies", DependencyKind::Dev),
    ("peerDependencies", DependencyKind::Peer),
];

pub fn parse_npm(manifest: &RepoPath, bytes: &[u8]) -> Result<PackageParse, AdapterError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let json: Map<String, Value> =
        serde_json::from_slice(bytes).map_err(|e| malformed(manifest, e))?;
    let mut dependencies = Vec::new();
    for (section, kind) in SECTIONS {
        let Some(entries) = json.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (name, version) in entries {
            dependencies.push(Dependency {
                name: name.clone(),
                kind,
                workspace_protocol: version
                    .as_str()
                    .is_some_and(|v| v.starts_with("workspace:")),
            });
        }
    }
    let workspaces = match json.get("workspaces") {
        Some(Value::Object(object)) => object.get("packages"),
        other => other,
    };
    Ok(PackageParse {
        ecosystem: Ecosystem::Npm,
        manifest: manifest.clone(),
        directory: directory(manifest)?,
        name: string(&json, "name"),
        version: string(&json, "version"),
        dependencies,
        workspace_members: workspaces
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|glob| !glob.starts_with('!'))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        workspace_exclude: workspaces
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|glob| glob.strip_prefix('!'))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
    })
}

fn string(json: &Map<String, Value>, key: &str) -> Option<String> {
    json.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
