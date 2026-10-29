//! Reads the eval's task files, raw Claude Code logs and blind grades, and renders the report.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod grading;

pub const MODES: [&str; 5] = ["without", "codemycel", "gitnexus", "serena", "codegraph"];
pub const COMPETITORS: [&str; 3] = ["gitnexus", "serena", "codegraph"];

#[derive(Debug, Deserialize)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub prompt: String,
    /// The shape of a correct answer, for a reader judging the logs.
    pub expected: String,
    /// The checklist completeness is graded against, taken from `expected`. For an edit task,
    /// the call sites the edit must update.
    pub items: Vec<String>,
    /// For an edit task, the checker that judges the edited tree: `typescript` or `cargo`.
    pub check: Option<String>,
    /// For an edit task, text the edited definition must contain.
    pub signature: Option<String>,
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
        for event in events(log)? {
            if event["type"] == "assistant" {
                tool_calls += event["message"]["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|block| block["type"] == "tool_use")
                    .count() as u64;
            }
        }
        let Some(result) = result(log)? else {
            return Ok(None);
        };
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

fn events(log: &str) -> anyhow::Result<Vec<Value>> {
    log.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).context("a log line is not JSON"))
        .collect()
}

/// The last successful `result` event of a log.
pub fn result(log: &str) -> anyhow::Result<Option<Value>> {
    Ok(events(log)?
        .into_iter()
        .rfind(|event| event["type"] == "result")
        .filter(|event| event["is_error"] != true))
}

/// A blind grader's verdict on one answer, stored beside its log as `<run>.grade.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grade {
    /// 0–100: whether every claim holds for the repository at the pinned commit.
    pub correctness: u32,
    pub wrong_claims: Vec<String>,
    /// Share of the task's checklist items the answer names, 0–1.
    pub completeness: f64,
    pub missing: Vec<String>,
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

#[derive(Default)]
struct Cell {
    runs: Vec<Run>,
    grades: Vec<Grade>,
}

impl Cell {
    fn tokens(&self) -> Option<u64> {
        median(&mut self.runs.iter().map(|r| r.tokens).collect::<Vec<_>>())
    }

    fn tool_calls(&self) -> Option<u64> {
        median(&mut self.runs.iter().map(|r| r.tool_calls).collect::<Vec<_>>())
    }

    fn correctness(&self) -> Option<f64> {
        mean(self.grades.iter().map(|g| f64::from(g.correctness)))
    }

    fn completeness(&self) -> Option<f64> {
        mean(self.grades.iter().map(|g| g.completeness * 100.0))
    }
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let (sum, count) = values.fold((0.0, 0u32), |(sum, count), v| (sum + v, count + 1));
    (count > 0).then(|| sum / f64::from(count))
}

type Cells = BTreeMap<(String, String), BTreeMap<&'static str, Cell>>;

/// Every run under `logs/<repo>/<task>/<mode>/<run>.jsonl`, with its grade when one exists.
fn collect(logs: &Path) -> anyhow::Result<Cells> {
    let mut cells = Cells::new();
    for repo in sorted_dirs(logs)? {
        for task in sorted_dirs(&logs.join(&repo))? {
            let entry = cells.entry((repo.clone(), task.clone())).or_default();
            for mode in MODES {
                let dir = logs.join(&repo).join(&task).join(mode);
                if !dir.is_dir() {
                    continue;
                }
                let cell = entry.entry(mode).or_default();
                for (log, _) in grading::logs(&dir)? {
                    let text = std::fs::read_to_string(&log)
                        .with_context(|| format!("reading {}", log.display()))?;
                    let Some(run) = Run::parse(&text).with_context(|| log.display().to_string())?
                    else {
                        continue;
                    };
                    cell.runs.push(run);
                    if let Some(grade) = grading::load_grade(&log)? {
                        cell.grades.push(grade);
                    }
                }
            }
        }
    }
    Ok(cells)
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

fn number(value: Option<u64>) -> String {
    value.map_or("n/a".to_owned(), |v| v.to_string())
}

fn score(value: Option<f64>) -> String {
    value.map_or("n/a".to_owned(), |v| format!("{v:.0}"))
}

/// Whether codemycel beats a competitor on one task: correctness and completeness at least equal,
/// and at most half the median tokens.
fn beats(ours: &Cell, theirs: &Cell) -> Option<bool> {
    let at_least = |a: Option<f64>, b: Option<f64>| Some(a? >= b?);
    Some(
        at_least(ours.correctness(), theirs.correctness())?
            && at_least(ours.completeness(), theirs.completeness())?
            && ours.tokens()? * 2 <= theirs.tokens()?,
    )
}

/// The Markdown report: per task and mode the median tokens and tool calls and the mean graded
/// correctness and completeness, then totals per mode, then codemycel against each competitor.
pub fn table(logs: &Path) -> anyhow::Result<String> {
    let cells = collect(logs)?;
    let mut out = String::from(
        "| Repository | Task | Mode | Tokens | Correctness | Completeness | Tool calls | Runs (graded) |\n\
         |---|---|---|---:|---:|---:|---:|---:|\n",
    );
    for ((repo, task), modes) in &cells {
        for (mode, cell) in MODES
            .iter()
            .filter_map(|mode| Some((mode, modes.get(mode)?)))
        {
            out.push_str(&format!(
                "| {repo} | {task} | {mode} | {} | {} | {} | {} | {} ({}) |\n",
                number(cell.tokens()),
                score(cell.correctness()),
                score(cell.completeness()),
                number(cell.tool_calls()),
                cell.runs.len(),
                cell.grades.len(),
            ));
        }
    }

    out.push_str(
        "\n| Mode | Tasks | Total median tokens | Mean correctness | Mean completeness |\n\
         |---|---:|---:|---:|---:|\n",
    );
    for mode in MODES {
        let mode_cells: Vec<&Cell> = cells
            .values()
            .filter_map(|modes| modes.get(mode))
            .filter(|cell| !cell.runs.is_empty())
            .collect();
        if mode_cells.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "| {mode} | {} | {} | {} | {} |\n",
            mode_cells.len(),
            mode_cells.iter().filter_map(|c| c.tokens()).sum::<u64>(),
            score(mean(mode_cells.iter().filter_map(|c| c.correctness()))),
            score(mean(mode_cells.iter().filter_map(|c| c.completeness()))),
        ));
    }

    let mut verdicts = String::new();
    for competitor in COMPETITORS {
        let pairs: Vec<(&Cell, &Cell)> = cells
            .values()
            .filter_map(|modes| Some((modes.get("codemycel")?, modes.get(competitor)?)))
            .collect();
        if pairs.is_empty() {
            continue;
        }
        let won = pairs
            .iter()
            .filter(|(ours, theirs)| beats(ours, theirs) == Some(true))
            .count();
        verdicts.push_str(&format!("| {competitor} | {won} of {} |\n", pairs.len()));
    }
    if !verdicts.is_empty() {
        out.push_str(
            "\n| codemycel against | Tasks won 2× (correctness ≥, completeness ≥, tokens ≤ ½) |\n\
             |---|---:|\n",
        );
        out.push_str(&verdicts);
    }
    Ok(out)
}
