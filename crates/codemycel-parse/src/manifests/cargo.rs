use codemycel_core::{AdapterError, RepoPath};
use toml::{Table, Value};

use super::{Dependency, DependencyKind, Ecosystem, PackageParse, directory, malformed};

const SECTIONS: [(&str, DependencyKind); 3] = [
    ("dependencies", DependencyKind::Normal),
    ("dev-dependencies", DependencyKind::Dev),
    ("build-dependencies", DependencyKind::Build),
];

pub fn parse_cargo(manifest: &RepoPath, bytes: &[u8]) -> Result<PackageParse, AdapterError> {
    let text = std::str::from_utf8(bytes).map_err(|e| malformed(manifest, e))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let table: Table = toml::from_str(text).map_err(|e| malformed(manifest, e))?;
    let package = table.get("package").and_then(Value::as_table);
    let workspace = table.get("workspace").and_then(Value::as_table);
    let mut dependencies = Vec::new();
    for (section, kind) in SECTIONS {
        let Some(entries) = table.get(section).and_then(Value::as_table) else {
            continue;
        };
        for (key, spec) in entries {
            let renamed = spec.get("package").and_then(Value::as_str);
            dependencies.push(Dependency {
                name: renamed.unwrap_or(key).to_owned(),
                kind,
                workspace_protocol: false,
            });
        }
    }
    Ok(PackageParse {
        ecosystem: Ecosystem::Cargo,
        manifest: manifest.clone(),
        directory: directory(manifest)?,
        name: package.and_then(|p| string(p, "name")),
        version: package.and_then(|p| string(p, "version")),
        dependencies,
        workspace_members: workspace.map(|w| strings(w, "members")).unwrap_or_default(),
        workspace_exclude: workspace.map(|w| strings(w, "exclude")).unwrap_or_default(),
    })
}

fn string(table: &Table, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn strings(table: &Table, key: &str) -> Vec<String> {
    table
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
