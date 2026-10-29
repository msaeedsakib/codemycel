//! Grading. Answers to questions are graded blind: every ungraded answer to a task goes to one
//! grader session under a neutral number, in an order that does not follow the mode. Edit runs are
//! graded from their patch and the checker's errors, with no model.

use std::collections::BTreeMap;
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
         with no false claim scores 100. A claim is false only when you can cite code that \
         contradicts it (file and line). Not finding something with a text search is not proof: \
         calls through trait objects, interfaces, re-exports, aliases or macros are easy to miss, \
         so follow the types before deciding a named caller or implementation does not exist.\n\n\
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

/// `src/a.ts(39,23): error …` or `src/a.rs:39:23: error …` as its file and line.
fn error_location(line: &str) -> Option<(&str, u32)> {
    let location = &line[..line.find(": error")?];
    if let Some((file, rest)) = location.split_once('(') {
        return Some((file, rest.split(',').next()?.parse().ok()?));
    }
    let mut parts = location.split(':');
    Some((parts.next()?, parts.next()?.parse().ok()?))
}

/// Text an edit must not add: it silences the checker instead of fixing the code.
const EVASIONS: [&str; 10] = [
    "as any",
    "@ts-ignore",
    "@ts-expect-error",
    "@ts-nocheck",
    "eslint-disable",
    "todo!(",
    "unimplemented!(",
    "unreachable!(",
    "#[allow(",
    "#![allow(",
];

#[derive(Default)]
struct Hunk {
    old_start: u32,
    old_len: u32,
    new_start: u32,
    /// Old line numbers the hunk removes.
    removed: Vec<(u32, String)>,
    /// Added lines with the old line they are inserted before.
    added: Vec<(u32, String)>,
}

/// The hunks of a unified diff, by the file they change.
fn hunks(patch: &str) -> BTreeMap<String, Vec<Hunk>> {
    let mut files: BTreeMap<String, Vec<Hunk>> = BTreeMap::new();
    let mut file = None;
    let mut old = 0u32;
    for line in patch.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            file = Some(path.to_owned());
        } else if line == "+++ /dev/null" {
            file = None;
        } else if line.starts_with("--- a/") || line == "--- /dev/null" || line.starts_with("diff ")
        {
        } else if let Some(header) = line.strip_prefix("@@ -") {
            let numbers = |text: &str| -> (u32, u32) {
                let mut parts = text.split(',');
                let start = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                (
                    start,
                    parts.next().and_then(|s| s.parse().ok()).unwrap_or(1),
                )
            };
            let mut ranges = header.split(' ');
            let (old_start, old_len) = numbers(ranges.next().unwrap_or("0"));
            let (new_start, _) = numbers(ranges.next().unwrap_or("+0").trim_start_matches('+'));
            old = old_start;
            if let Some(file) = &file {
                files.entry(file.clone()).or_default().push(Hunk {
                    old_start,
                    old_len,
                    new_start,
                    ..Hunk::default()
                });
            }
        } else if let Some(hunk) = file
            .as_ref()
            .and_then(|file| files.get_mut(file))
            .and_then(|hunks| hunks.last_mut())
        {
            match line.chars().next() {
                Some('-') => {
                    hunk.removed.push((old, line[1..].to_owned()));
                    old += 1;
                }
                Some('+') => hunk.added.push((old, line[1..].to_owned())),
                _ => old += 1,
            }
        }
    }
    files
}

/// The line of the untouched file that line `new` of the edited file stands for.
fn old_line(hunks: &[Hunk], new: u32) -> u32 {
    let mut shift: i64 = 0;
    for hunk in hunks {
        let new_len = hunk.old_len as i64 - hunk.removed.len() as i64 + hunk.added.len() as i64;
        if i64::from(new) < i64::from(hunk.new_start) {
            break;
        }
        if i64::from(new) < i64::from(hunk.new_start) + new_len {
            let inside = (new - hunk.new_start).min(hunk.old_len.saturating_sub(1));
            return hunk.old_start + inside;
        }
        shift = i64::from(hunk.new_start) + new_len
            - i64::from(hunk.old_start)
            - i64::from(hunk.old_len);
    }
    (i64::from(new) - shift).max(1) as u32
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether a file the patch changes holds the task's signature, whitespace aside, in its edited text.
pub fn signature_holds(task: &Task, patch: &str, edited: &dyn Fn(&str) -> Option<String>) -> bool {
    let signature = squeeze(task.signature.as_deref().unwrap_or_default());
    !signature.is_empty()
        && hunks(patch)
            .keys()
            .filter_map(|file| edited(file))
            .any(|text| squeeze(&text).contains(&signature))
}

/// Per listed call site, in order: whether the patch changes a line in the site's window, and
/// whether a new error lands in it. A window runs from the site's line to the file's next listed
/// site, or 12 lines when there is none.
fn site_checks(
    task: &Task,
    files: &BTreeMap<String, Vec<Hunk>>,
    new_errors: &[String],
) -> Vec<(bool, bool)> {
    let sites: Vec<(&str, u32)> = task
        .sites
        .iter()
        .flatten()
        .filter_map(|site| {
            let (file, line) = site.rsplit_once(':')?;
            Some((file, line.parse().ok()?))
        })
        .collect();
    let error_lines: Vec<(&str, u32)> = new_errors
        .iter()
        .filter_map(|error| {
            let (file, new) = error_location(error)?;
            Some((
                file,
                files.get(file).map_or(new, |hunks| old_line(hunks, new)),
            ))
        })
        .collect();
    sites
        .iter()
        .map(|&(file, line)| {
            let end = sites
                .iter()
                .filter(|(f, l)| *f == file && *l > line)
                .map(|(_, l)| *l)
                .min()
                .unwrap_or(line + 12);
            let inside = |at: u32| line <= at && at < end;
            let touched = files.get(file).is_some_and(|hunks| {
                hunks.iter().any(|hunk| {
                    hunk.removed.iter().any(|(l, _)| inside(*l))
                        || hunk.added.iter().any(|(l, _)| inside(*l))
                })
            });
            let broken = error_lines.iter().any(|(f, l)| *f == file && inside(*l));
            (touched, broken)
        })
        .collect()
}

/// The listed call sites a planted patch leaves unchanged and the checker breaks: a new error
/// lands in the site's window. The self-test in `check.sh` requires every site of a task to be
/// caught by one of its planted patches before the task's runs are graded.
pub fn caught_sites(task: &Task, patch: &str, new_errors: &[String]) -> Vec<String> {
    site_checks(task, &hunks(patch), new_errors)
        .into_iter()
        .zip(task.sites.iter().flatten())
        .filter(|((touched, broken), _)| !touched && *broken)
        .map(|(_, site)| site.clone())
        .collect()
}

/// The grade of an edit run from its patch, the edited files' text, and the errors the checker
/// reports beyond the untouched tree's. Nothing counts unless the definition reads as the task asks
/// (whitespace aside). A call site is updated when the patch changes it and no new error lands on
/// it. New errors, text that silences the checker, and calls to the target the patch deletes are
/// each a wrong claim.
pub fn edit_grade(
    task: &Task,
    patch: &str,
    edited: &dyn Fn(&str) -> Option<String>,
    new_errors: &[String],
) -> Grade {
    let files = hunks(patch);
    if !signature_holds(task, patch, edited) {
        return Grade {
            correctness: 0,
            wrong_claims: vec!["the definition was not changed as asked".to_owned()],
            completeness: 0.0,
            missing: task.items.clone(),
        };
    }

    let mut wrong: Vec<String> = new_errors.to_vec();
    let callee = task.callee.as_deref().map(|name| format!("{name}("));
    for (file, file_hunks) in &files {
        for hunk in file_hunks {
            for (_, line) in &hunk.added {
                if let Some(evasion) = EVASIONS.iter().find(|e| line.contains(*e)) {
                    wrong.push(format!("{file}: adds `{evasion}`: {}", line.trim()));
                }
            }
            if let Some(callee) = &callee {
                let calls = |lines: &[(u32, String)]| {
                    lines
                        .iter()
                        .filter(|(_, l)| l.contains(callee.as_str()))
                        .count()
                };
                if calls(&hunk.removed) > calls(&hunk.added) {
                    wrong.push(format!(
                        "{file}: deletes a call to `{callee}` near line {}",
                        hunk.old_start
                    ));
                }
            }
        }
    }

    let checks = site_checks(task, &files, new_errors);
    let mut missing = Vec::new();
    for (at, &(touched, broken)) in checks.iter().enumerate() {
        if !touched || broken {
            missing.push(task.items.get(at).cloned().unwrap_or_else(|| {
                task.sites
                    .iter()
                    .flatten()
                    .nth(at)
                    .cloned()
                    .unwrap_or_default()
            }));
        }
    }
    let updated = checks.len() - missing.len();
    Grade {
        correctness: 100u32.saturating_sub(25 * wrong.len() as u32),
        wrong_claims: wrong,
        completeness: if checks.is_empty() {
            0.0
        } else {
            updated as f64 / checks.len() as f64
        },
        missing,
    }
}
