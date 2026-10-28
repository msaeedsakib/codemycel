use std::path::Path;

use super::{ConfigError, TEMPLATE, paths};

/// Rewrites single keys of `.codemycel/config.toml`, leaving every other line as written.
pub struct ConfigWriter;

impl ConfigWriter {
    /// Sets `[scip] commit`. An uncommented `commit` line in the table is replaced; otherwise the
    /// key goes on its own line after the table's commented `commit` line or its header, and a
    /// missing table is appended. A missing file starts from the template.
    pub fn set_scip_commit(repo_root: &Path, sha: &str) -> Result<(), ConfigError> {
        let path = paths::repo_config_path(repo_root);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => TEMPLATE.to_owned(),
            Err(source) => return Err(ConfigError::Read { path, source }),
        };
        let line = format!("commit = \"{sha}\"");
        let updated = set_key(&text, "scip", "commit", &line);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| ConfigError::Write {
                path: path.clone(),
                source,
            })?;
        }
        std::fs::write(&path, updated).map_err(|source| ConfigError::Write { path, source })
    }
}

fn set_key(text: &str, table: &str, key: &str, line: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(ToOwned::to_owned).collect();
    let header = format!("[{table}]");
    let Some(start) = lines.iter().position(|l| l.trim() == header) else {
        let mut out = text.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("\n{header}\n{line}\n"));
        return out;
    };
    let end = lines[start + 1..]
        .iter()
        .position(|l| l.trim_start().starts_with('['))
        .map_or(lines.len(), |offset| start + 1 + offset);
    let assigns = |l: &str, commented: bool| {
        let l = l.trim_start();
        let l = if commented {
            match l.strip_prefix('#') {
                Some(rest) => rest.trim_start(),
                None => return false,
            }
        } else {
            l
        };
        l.strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    if let Some(index) = (start + 1..end).find(|&i| assigns(&lines[i], false)) {
        lines[index] = line.to_owned();
    } else {
        let after = (start + 1..end)
            .find(|&i| assigns(&lines[i], true))
            .unwrap_or(start);
        lines.insert(after + 1, line.to_owned());
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}
