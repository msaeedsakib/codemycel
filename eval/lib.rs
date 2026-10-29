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
    /// For an edit task, text the edited definition must contain, whitespace aside.
    pub signature: Option<String>,
    /// For an edit task, each call site as `<file>:<line>` of the untouched tree, in `items` order.
    #[serde(default)]
    pub sites: Option<Vec<String>>,
    /// For an edit task, the name of the function whose calls the edit must keep.
    #[serde(default)]
    pub callee: Option<String>,
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

/// What one `claude -p --output-format stream-json` run cost and how it spent its tool calls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Run {
    /// Input, output, cache-read, and cache-creation tokens over every model the run used.
    pub tokens: u64,
    /// The same without cache reads, which every turn repeats.
    pub uncached: u64,
    /// The run's cost in millionths of a dollar, as Claude Code reports it.
    pub micro_dollars: u64,
    pub tool_calls: u64,
    /// Calls to the mode's MCP tools.
    pub graph_calls: u64,
    pub bash_calls: u64,
    pub read_calls: u64,
    pub edit_calls: u64,
    /// Characters of every tool result the model read.
    pub result_chars: u64,
}

impl Run {
    /// `None` when the log has no successful `result` line.
    pub fn parse(log: &str) -> anyhow::Result<Option<Run>> {
        let mut run = Run::default();
        for event in events(log)? {
            let blocks = event["message"]["content"].as_array().into_iter().flatten();
            match event["type"].as_str() {
                Some("assistant") => {
                    for block in blocks.filter(|block| block["type"] == "tool_use") {
                        run.tool_calls += 1;
                        match block["name"].as_str().unwrap_or_default() {
                            name if name.starts_with("mcp__") => run.graph_calls += 1,
                            "Bash" => run.bash_calls += 1,
                            "Read" | "Grep" | "Glob" => run.read_calls += 1,
                            "Edit" | "Write" | "MultiEdit" => run.edit_calls += 1,
                            _ => {}
                        }
                    }
                }
                Some("user") => {
                    for block in blocks.filter(|block| block["type"] == "tool_result") {
                        run.result_chars += match &block["content"] {
                            Value::String(text) => text.chars().count() as u64,
                            other => other.to_string().chars().count() as u64,
                        };
                    }
                }
                _ => {}
            }
        }
        let Some(result) = result(log)? else {
            return Ok(None);
        };
        let Some(models) = result["modelUsage"].as_object() else {
            bail!("the result line has no modelUsage");
        };
        let sum = |keys: &[&str]| -> u64 {
            models
                .values()
                .flat_map(|usage| keys.iter().map(|key| usage[*key].as_u64().unwrap_or(0)))
                .sum()
        };
        run.uncached = sum(&["inputTokens", "outputTokens", "cacheCreationInputTokens"]);
        run.tokens = run.uncached + sum(&["cacheReadInputTokens"]);
        run.micro_dollars = (result["total_cost_usd"].as_f64().unwrap_or(0.0) * 1e6).round() as u64;
        Ok(Some(run))
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
    fn median_of(&self, field: impl Fn(&Run) -> u64) -> Option<u64> {
        median(&mut self.runs.iter().map(field).collect::<Vec<_>>())
    }

    fn tokens(&self) -> Option<u64> {
        self.median_of(|r| r.tokens)
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

/// Each task's kind, from the task files beside the logs (`<logs>/../tasks/<repo>.toml`).
fn kinds(logs: &Path) -> BTreeMap<(String, String), String> {
    let mut kinds = BTreeMap::new();
    let Some(tasks) = logs.parent().map(|eval| eval.join("tasks")) else {
        return kinds;
    };
    for repo in sorted_dirs(logs).unwrap_or_default() {
        for task in load_tasks(&tasks.join(format!("{repo}.toml"))).unwrap_or_default() {
            kinds.insert((repo.clone(), task.id), task.kind);
        }
    }
    kinds
}

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

fn dollars(micro: u64) -> String {
    format!("{:.3}", micro as f64 / 1e6)
}

/// Points of correctness or completeness inside which a difference between two means of three
/// runs is treated as noise when reporting near misses. The verdict itself stays strict.
const NOISE: f64 = 3.0;

/// Whether codemycel beats a competitor on one task: correctness and completeness at least equal
/// (less `slack` points), and at most half the median tokens.
fn beats(ours: &Cell, theirs: &Cell, slack: f64) -> Option<bool> {
    let at_least = |a: Option<f64>, b: Option<f64>| Some(a? + slack >= b?);
    Some(
        at_least(ours.correctness(), theirs.correctness())?
            && at_least(ours.completeness(), theirs.completeness())?
            && ours.tokens()? * 2 <= theirs.tokens()?,
    )
}

/// Sums and means over a set of cells, for the totals and per-kind tables.
struct Totals {
    tasks: usize,
    tokens: u64,
    uncached: u64,
    micro_dollars: u64,
    correctness: Option<f64>,
    completeness: Option<f64>,
}

impl Totals {
    fn of(cells: &[&Cell]) -> Totals {
        let sum = |field: fn(&Run) -> u64| -> u64 {
            cells.iter().filter_map(|c| c.median_of(field)).sum()
        };
        Totals {
            tasks: cells.len(),
            tokens: sum(|r| r.tokens),
            uncached: sum(|r| r.uncached),
            micro_dollars: sum(|r| r.micro_dollars),
            correctness: mean(cells.iter().filter_map(|c| c.correctness())),
            completeness: mean(cells.iter().filter_map(|c| c.completeness())),
        }
    }
}

/// The Markdown report. Per task and mode: median tokens (with and without cache reads), median
/// cost, mean graded correctness and completeness, and how the median run spent its tool calls.
/// Then totals per mode, totals per task kind, and codemycel against each competitor.
pub fn table(logs: &Path) -> anyhow::Result<String> {
    let cells = collect(logs)?;
    let kinds = kinds(logs);
    let kind_of = |key: &(String, String)| kinds.get(key).map_or("?", String::as_str);
    let mut out = String::from(
        "| Repository | Task | Kind | Mode | Tokens | Uncached | Cost $ | Correctness | Completeness \
         | Graph · Bash · Read · Edit calls | Result chars | Runs (graded) |\n\
         |---|---|---|---|---:|---:|---:|---:|---:|---:|---:|---:|\n",
    );
    for (key, modes) in &cells {
        for (mode, cell) in MODES
            .iter()
            .filter_map(|mode| Some((mode, modes.get(mode)?)))
        {
            let calls = [
                cell.median_of(|r| r.graph_calls),
                cell.median_of(|r| r.bash_calls),
                cell.median_of(|r| r.read_calls),
                cell.median_of(|r| r.edit_calls),
            ]
            .map(number)
            .join(" · ");
            out.push_str(&format!(
                "| {} | {} | {} | {mode} | {} | {} | {} | {} | {} | {calls} | {} | {} ({}) |\n",
                key.0,
                key.1,
                kind_of(key),
                number(cell.tokens()),
                number(cell.median_of(|r| r.uncached)),
                cell.median_of(|r| r.micro_dollars)
                    .map_or("n/a".to_owned(), dollars),
                score(cell.correctness()),
                score(cell.completeness()),
                number(cell.median_of(|r| r.result_chars)),
                cell.runs.len(),
                cell.grades.len(),
            ));
        }
    }

    let mode_cells = |mode: &str, kind: Option<&str>| -> Vec<&Cell> {
        cells
            .iter()
            .filter(|(key, _)| kind.is_none_or(|kind| kind_of(key) == kind))
            .filter_map(|(_, modes)| modes.get(mode))
            .filter(|cell| !cell.runs.is_empty())
            .collect()
    };
    out.push_str(
        "\n| Mode | Tasks | Total median tokens | Uncached | Cost $ | Mean correctness \
         | Mean completeness |\n|---|---:|---:|---:|---:|---:|---:|\n",
    );
    for mode in MODES {
        let selected = mode_cells(mode, None);
        if selected.is_empty() {
            continue;
        }
        let totals = Totals::of(&selected);
        out.push_str(&format!(
            "| {mode} | {} | {} | {} | {} | {} | {} |\n",
            totals.tasks,
            totals.tokens,
            totals.uncached,
            dollars(totals.micro_dollars),
            score(totals.correctness),
            score(totals.completeness),
        ));
    }

    let mut task_kinds: Vec<&str> = cells.keys().map(kind_of).collect();
    task_kinds.sort_unstable();
    task_kinds.dedup();
    out.push_str(
        "\n| Kind | Mode | Tasks | Total median tokens | Mean correctness | Mean completeness |\n\
         |---|---|---:|---:|---:|---:|\n",
    );
    for kind in task_kinds {
        for mode in MODES {
            let selected = mode_cells(mode, Some(kind));
            if selected.is_empty() {
                continue;
            }
            let totals = Totals::of(&selected);
            out.push_str(&format!(
                "| {kind} | {mode} | {} | {} | {} | {} |\n",
                totals.tasks,
                totals.tokens,
                score(totals.correctness),
                score(totals.completeness),
            ));
        }
    }

    let mut verdicts = String::new();
    for competitor in COMPETITORS {
        let pairs: Vec<(&Cell, &Cell)> = cells
            .values()
            .filter_map(|modes| Some((modes.get("codemycel")?, modes.get(competitor)?)))
            .filter(|(ours, theirs)| !ours.runs.is_empty() && !theirs.runs.is_empty())
            .collect();
        if pairs.is_empty() {
            continue;
        }
        let won = pairs
            .iter()
            .filter(|(ours, theirs)| beats(ours, theirs, 0.0) == Some(true))
            .count();
        let near = pairs
            .iter()
            .filter(|(ours, theirs)| {
                beats(ours, theirs, 0.0) != Some(true) && beats(ours, theirs, NOISE) == Some(true)
            })
            .count();
        let ours = Totals::of(&pairs.iter().map(|(ours, _)| *ours).collect::<Vec<_>>());
        let theirs = Totals::of(&pairs.iter().map(|(_, theirs)| *theirs).collect::<Vec<_>>());
        let overall = matches!(
            (ours.correctness, theirs.correctness, ours.completeness, theirs.completeness),
            (Some(a), Some(b), Some(c), Some(d)) if a >= b && c >= d
        ) && ours.tokens * 2 <= theirs.tokens;
        verdicts.push_str(&format!(
            "| {competitor} | {won} of {} | {near} | {} |\n",
            pairs.len(),
            if overall { "yes" } else { "no" },
        ));
    }
    if !verdicts.is_empty() {
        out.push_str(
            "\n| codemycel against | Tasks won 2× (correctness ≥, completeness ≥, tokens ≤ ½) \
             | Near misses (within 3 points) | Overall 2× on the shared tasks |\n\
             |---|---:|---:|---|\n",
        );
        out.push_str(&verdicts);
    }
    Ok(out)
}
