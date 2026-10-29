//! Blind grading: every ungraded answer to a task goes to one grader session under a neutral
//! number, in an order that does not follow the mode, and the verdicts are stored per run.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use serde::{Deserialize, Serialize};

use crate::{Grade, MODES, Task, result};

/// The JSON schema the grader's structured output follows.
pub const SCHEMA: &str = r#"{"type":"object","required":["answers"],"properties":{"answers":{"type":"array","items":{"type":"object","required":["answer","correctness","wrong_claims","items_named"],"properties":{"answer":{"type":"integer"},"correctness":{"type":"integer","minimum":0,"maximum":100},"wrong_claims":{"type":"array","items":{"type":"string"}},"items_named":{"type":"array","items":{"type":"integer"}}}}}}}"#;

const KEY_FILE: &str = "grade-key.json";

/// Which run each neutral answer number stands for.
#[derive(Debug, Serialize, Deserialize)]
pub struct KeyEntry {
    pub answer: usize,
    pub mode: String,
    pub run: String,
}

#[derive(Deserialize)]
struct Verdicts {
    answers: Vec<Verdict>,
}

#[derive(Deserialize)]
struct Verdict {
    answer: usize,
    correctness: u32,
    wrong_claims: Vec<String>,
    items_named: Vec<usize>,
}

/// The `<run>.jsonl` logs in a mode directory with their run names, sorted.
pub fn logs(dir: &Path) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let mut logs = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.extension() == Some("jsonl".as_ref())
            && let Some(stem) = path.file_stem()
        {
            logs.push((path.clone(), stem.to_string_lossy().into_owned()));
        }
    }
    logs.sort();
    Ok(logs)
}

fn grade_path(log: &Path) -> PathBuf {
    log.with_extension("grade.json")
}

pub fn load_grade(log: &Path) -> anyhow::Result<Option<Grade>> {
    let path = grade_path(log);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(Some(
        serde_json::from_str(&text).with_context(|| path.display().to_string())?,
    ))
}

/// The grading prompt for every answered, ungraded run of a task, or `None` when there is none.
/// Writes the key from neutral numbers to runs into the task's log directory.
pub fn prompt(task_logs: &Path, task: &Task) -> anyhow::Result<Option<String>> {
    if task.check.is_some() {
        return Ok(None);
    }
    let mut answers = Vec::new();
    for mode in MODES {
        let dir = task_logs.join(mode);
        if !dir.is_dir() {
            continue;
        }
        for (log, run) in logs(&dir)? {
            if grade_path(&log).exists() {
                continue;
            }
            let text = std::fs::read_to_string(&log)?;
            let Some(result) = result(&text)? else {
                continue;
            };
            let answer = result["result"].as_str().unwrap_or_default().to_owned();
            let mut hasher = DefaultHasher::new();
            (&answer, mode, &run).hash(&mut hasher);
            answers.push((hasher.finish(), mode, run, answer));
        }
    }
    if answers.is_empty() {
        return Ok(None);
    }
    answers.sort();

    let items: String = task
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| format!("{}. {item}\n", i + 1))
        .collect();
    let mut prompt = format!(
        "You are grading answers to a question about the repository in the current directory, \
         checked out at the commit the question is about. The answers come from different \
         anonymous assistants. Grade each one on its own; do not compare them with each other.\n\n\
         ## Question\n\n{}\n\n## The shape of a correct answer\n\n{}\n\n## Checklist\n\n{items}\n\
         ## How to grade\n\n\
         Correctness (0–100): check every concrete claim in the answer (files, functions, \
         types, callers, implementations, call chains, line numbers, what breaks) against the \
         code with Read, Grep and Glob. Do not trust the reference shape over the code. 100 \
         means every claim is true. Deduct for each false claim in proportion to how much it \
         would mislead someone acting on the answer: an invented file or function, a wrong \
         caller, or a missed implementation stated as the full list are serious; a slightly \
         wrong line number is minor. List each false claim in `wrong_claims`, quoting it and \
         saying what the code shows. Every point deducted belongs to a listed claim: an answer \
         with no false claim scores 100.\n\n\
         Completeness: list in `items_named` the numbers of the checklist items the answer \
         names. An item counts only if the answer states it correctly.\n\n\
         Ignore style, length, formatting and any mention of tools, indexes or freshness.\n\n\
         ## Answers\n\n",
        task.prompt.split_whitespace().collect::<Vec<_>>().join(" "),
        task.expected
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
    );
    let mut key = Vec::new();
    for (number, (_, mode, run, answer)) in answers.into_iter().enumerate() {
        let number = number + 1;
        prompt.push_str(&format!(
            "### Answer {number}\n\n<answer>\n{answer}\n</answer>\n\n"
        ));
        key.push(KeyEntry {
            answer: number,
            mode: mode.to_owned(),
            run,
        });
    }
    std::fs::write(
        task_logs.join(KEY_FILE),
        serde_json::to_string_pretty(&key)?,
    )?;
    Ok(Some(prompt))
}

/// Stores the verdicts of a grader log as `<run>.grade.json` beside each graded run's log.
pub fn store(task_logs: &Path, task: &Task, grader_log: &str) -> anyhow::Result<usize> {
    let key: Vec<KeyEntry> = serde_json::from_str(
        &std::fs::read_to_string(task_logs.join(KEY_FILE)).context("reading the grade key")?,
    )?;
    let Some(result) = result(grader_log)? else {
        bail!("the grader log has no successful result");
    };
    let verdicts: Verdicts = serde_json::from_value(result["structured_output"].clone())
        .context("the grader's structured output")?;
    let mut stored = 0;
    for entry in &key {
        let Some(verdict) = verdicts.answers.iter().find(|v| v.answer == entry.answer) else {
            bail!("the grader skipped answer {}", entry.answer);
        };
        let named: Vec<usize> = (1..=task.items.len())
            .filter(|n| verdict.items_named.contains(n))
            .collect();
        let grade = Grade {
            correctness: verdict.correctness.min(100),
            wrong_claims: verdict.wrong_claims.clone(),
            completeness: named.len() as f64 / task.items.len() as f64,
            missing: task
                .items
                .iter()
                .enumerate()
                .filter(|(i, _)| !named.contains(&(i + 1)))
                .map(|(_, item)| item.clone())
                .collect(),
        };
        let log = task_logs
            .join(&entry.mode)
            .join(format!("{}.jsonl", entry.run));
        std::fs::write(grade_path(&log), serde_json::to_string_pretty(&grade)?)?;
        stored += 1;
    }
    std::fs::remove_file(task_logs.join(KEY_FILE))?;
    Ok(stored)
}

/// The checker's errors in `after` that `baseline` does not hold, compared without positions so
/// that lines moved by the edit still match. Each line is one error as the checker prints it.
pub fn new_errors(baseline: &str, after: &str) -> Vec<String> {
    let mut known: Vec<String> = baseline.lines().filter_map(error_key).collect();
    let mut new = Vec::new();
    for line in after.lines() {
        let Some(key) = error_key(line) else {
            continue;
        };
        match known.iter().position(|k| *k == key) {
            Some(at) => {
                known.swap_remove(at);
            }
            None => new.push(line.trim().to_owned()),
        }
    }
    new
}

/// `src/a.ts(39,23): error TS2345: …` or `src/a.rs:39:23: error[E0061]: …` without its position.
fn error_key(line: &str) -> Option<String> {
    let at = line.find(": error")?;
    let location = &line[..at];
    let file = location.split_once('(').map_or_else(
        || location.split(':').next().unwrap_or(location),
        |(f, _)| f,
    );
    Some(format!("{file}{}", &line[at..]))
}

/// The grade of an edit run: nothing counts unless the definition was changed as asked; then each
/// error the edit introduced is a call site it missed or broke.
pub fn edit_grade(task: &Task, signature_changed: bool, new_errors: &[String]) -> Grade {
    let sites = task.items.len().max(1);
    if !signature_changed {
        return Grade {
            correctness: 0,
            wrong_claims: vec!["the definition was not changed as asked".to_owned()],
            completeness: 0.0,
            missing: task.items.clone(),
        };
    }
    let missed = new_errors.len().min(sites);
    Grade {
        correctness: 100u32.saturating_sub(25 * new_errors.len() as u32),
        wrong_claims: new_errors.to_vec(),
        completeness: (sites - missed) as f64 / sites as f64,
        missing: Vec::new(),
    }
}
