//! Reads the eval's task files and raw Claude Code logs, and renders the before/after table.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, bail};
use serde::Deserialize;
use serde_json::Value;

pub const MODES: [&str; 2] = ["without", "with"];

#[derive(Debug, Deserialize)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub prompt: String,
    /// The shape of a correct answer, for a reader judging the logs.
    pub expected: String,
}

#[derive(Deserialize)]
struct TaskFile {
    task: Vec<Task>,
}

pub fn load_tasks(path: &Path) -> anyhow::Result<Vec<Task>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let file: TaskFile = toml::from_str(&text).with_context(|| path.display().to_string())?;
    Ok(file.task)
}

/// What one `claude -p --output-format stream-json` run cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    /// Input, output, cache-read, and cache-creation tokens over every model the run used.
    pub tokens: u64,
    pub tool_calls: u64,
}

impl Run {
    /// `None` when the log has no successful `result` line.
    pub fn parse(log: &str) -> anyhow::Result<Option<Run>> {
        let mut tool_calls = 0;
        let mut result = None;
        for line in log.lines().filter(|line| !line.trim().is_empty()) {
            let event: Value = serde_json::from_str(line).context("a log line is not JSON")?;
            match event["type"].as_str() {
                Some("assistant") => {
                    tool_calls += event["message"]["content"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|block| block["type"] == "tool_use")
                        .count() as u64;
                }
                Some("result") => result = Some(event),
                _ => {}
            }
        }
        let Some(result) = result else {
            return Ok(None);
        };
        if result["is_error"] == true {
            return Ok(None);
        }
        let Some(models) = result["modelUsage"].as_object() else {
            bail!("the result line has no modelUsage");
        };
        let tokens = models
            .values()
            .flat_map(|usage| {
                [
                    "inputTokens",
                    "outputTokens",
                    "cacheReadInputTokens",
                    "cacheCreationInputTokens",
                ]
                .map(|key| usage[key].as_u64().unwrap_or(0))
            })
            .sum();
        Ok(Some(Run { tokens, tool_calls }))
    }
}

/// The median, rounding the mean of the middle pair down.
pub fn median(values: &mut [u64]) -> Option<u64> {
    values.sort_unstable();
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        n if n % 2 == 1 => Some(values[middle]),
        _ => Some((values[middle - 1] + values[middle]) / 2),
    }
}

type Runs = BTreeMap<(String, String), [Vec<Run>; 2]>;

/// Every run under `logs/<repo>/<task>/<mode>/<run>.jsonl`.
fn collect(logs: &Path) -> anyhow::Result<Runs> {
    let mut runs = Runs::new();
    for repo in sorted_dirs(logs)? {
        for task in sorted_dirs(&logs.join(&repo))? {
            let entry = runs.entry((repo.clone(), task.clone())).or_default();
            for (index, mode) in MODES.iter().enumerate() {
                let dir = logs.join(&repo).join(&task).join(mode);
                if !dir.is_dir() {
                    continue;
                }
                let mut files: Vec<_> = std::fs::read_dir(&dir)?
                    .map(|entry| entry.map(|entry| entry.path()))
                    .collect::<Result<_, _>>()?;
                files.sort();
                for file in files
                    .iter()
                    .filter(|f| f.extension() == Some("jsonl".as_ref()))
                {
                    let text = std::fs::read_to_string(file)
                        .with_context(|| format!("reading {}", file.display()))?;
                    let run = Run::parse(&text).with_context(|| file.display().to_string())?;
                    entry[index].extend(run);
                }
            }
        }
    }
    Ok(runs)
}

fn sorted_dirs(dir: &Path) -> anyhow::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            names.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    names.sort();
    Ok(names)
}

/// The Markdown table of median tokens and tool calls per task, without and with codemycel.
pub fn table(logs: &Path) -> anyhow::Result<String> {
    let mut out = String::from(
        "| Repository | Task | Tokens without | Tokens with | Change | Tool calls without | Tool calls with | Runs |\n\
         |---|---|---:|---:|---:|---:|---:|---:|\n",
    );
    for ((repo, task), [without, with]) in collect(logs)? {
        let tokens = |runs: &[Run]| median(&mut runs.iter().map(|r| r.tokens).collect::<Vec<_>>());
        let calls =
            |runs: &[Run]| median(&mut runs.iter().map(|r| r.tool_calls).collect::<Vec<_>>());
        let (before, after) = (tokens(&without), tokens(&with));
        let change = match (before, after) {
            (Some(before), Some(after)) if before > 0 => {
                let percent = (after as f64 - before as f64) * 100.0 / before as f64;
                format!("{percent:+.0}%")
            }
            _ => "n/a".to_owned(),
        };
        let cell = |value: Option<u64>| value.map_or("n/a".to_owned(), |v| v.to_string());
        out.push_str(&format!(
            "| {repo} | {task} | {} | {} | {change} | {} | {} | {}+{} |\n",
            cell(before),
            cell(after),
            cell(calls(&without)),
            cell(calls(&with)),
            without.len(),
            with.len(),
        ));
    }
    Ok(out)
}
