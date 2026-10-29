#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_eval::{Grade, Run, Task, grading, table};

fn log(tool_calls: usize, models: &[(u64, u64, u64, u64)]) -> String {
    let mut lines = Vec::new();
    for _ in 0..tool_calls {
        lines.push(
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"x"},{"type":"tool_use","name":"Grep","input":{}}]}}"#
                .to_owned(),
        );
    }
    let usage: Vec<String> = models
        .iter()
        .enumerate()
        .map(|(i, (input, output, read, create))| {
            format!(
                r#""m{i}":{{"inputTokens":{input},"outputTokens":{output},"cacheReadInputTokens":{read},"cacheCreationInputTokens":{create}}}"#
            )
        })
        .collect();
    lines.push(format!(
        r#"{{"type":"result","is_error":false,"modelUsage":{{{}}}}}"#,
        usage.join(",")
    ));
    lines.join("\n")
}

fn write(root: &Path, repo: &str, task: &str, mode: &str, run: u32, text: &str) {
    let dir = root.join(repo).join(task).join(mode);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{run}.jsonl")), text).unwrap();
}

#[test]
fn committed_task_files_have_unique_ids_and_complete_edit_tasks() {
    for repo in ["hono", "irixmail", "rust-analyzer", "nest"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tasks/{repo}.toml"));
        let tasks = codemycel_eval::load_tasks(&path).unwrap();
        let mut ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), tasks.len(), "{repo}");
        for task in &tasks {
            assert!(!task.items.is_empty(), "{repo} {}", task.id);
            let edit = task.kind == "edit";
            assert_eq!(task.check.is_some(), edit, "{repo} {}", task.id);
            assert_eq!(task.signature.is_some(), edit, "{repo} {}", task.id);
            if edit {
                let sites = task.sites.as_ref().unwrap();
                assert_eq!(sites.len(), task.items.len(), "{repo} {}", task.id);
                assert!(task.callee.is_some(), "{repo} {}", task.id);
                let planted = Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tasks/planted")
                    .join(&task.id);
                let patches = std::fs::read_dir(&planted)
                    .map(|entries| {
                        entries
                            .filter(|entry| {
                                entry.as_ref().unwrap().path().extension() == Some("patch".as_ref())
                            })
                            .count()
                    })
                    .unwrap_or(0);
                assert!(patches > 0, "{repo} {} has no planted patch", task.id);
            }
        }
    }
}

fn edit_task() -> Task {
    Task {
        kind: "edit".to_owned(),
        items: vec!["first call".to_owned(), "second call".to_owned()],
        signature: Some("fn f(a: u8, b: u8)".to_owned()),
        sites: Some(vec!["src/x.rs:10".to_owned(), "src/x.rs:20".to_owned()]),
        callee: Some("f".to_owned()),
        ..task()
    }
}

const DEFINITION: &str = "diff --git a/src/f.rs b/src/f.rs\n--- a/src/f.rs\n+++ b/src/f.rs\n\
                          @@ -1,1 +1,1 @@\n-fn f(a: u8) {}\n+fn f(a: u8, b: u8) {}\n";

fn edited(file: &str) -> Option<String> {
    (file == "src/f.rs").then(|| "fn f(a: u8,\n    b: u8) {}\n".to_owned())
}

#[test]
fn new_errors_are_compared_without_positions() {
    let baseline = "src/a.ts(3,1): error TS2591: Cannot find name 'process'.\n";
    let after = "src/a.ts(9,1): error TS2591: Cannot find name 'process'.\n\
                 src/b.ts(4,2): error TS2554: Expected 2 arguments, but got 1.\n";
    assert_eq!(
        grading::new_errors(baseline, after),
        ["src/b.ts(4,2): error TS2554: Expected 2 arguments, but got 1."]
    );
}

#[test]
fn an_edit_counts_the_sites_it_updates_without_new_errors() {
    let task = edit_task();
    let both = format!(
        "{DEFINITION}diff --git a/src/x.rs b/src/x.rs\n--- a/src/x.rs\n+++ b/src/x.rs\n\
         @@ -10,1 +10,1 @@\n-    f(1);\n+    f(1, 0);\n@@ -20,1 +20,1 @@\n-    f(2);\n+    f(2, 0);\n"
    );
    let grade = grading::edit_grade(&task, &both, &edited, &[]);
    assert_eq!((grade.correctness, grade.completeness), (100, 1.0));

    let one = format!(
        "{DEFINITION}diff --git a/src/x.rs b/src/x.rs\n--- a/src/x.rs\n+++ b/src/x.rs\n\
         @@ -10,1 +10,1 @@\n-    f(1);\n+    f(1, 0);\n"
    );
    let error = "src/x.rs:20:5: error[E0061]: this function takes 2 arguments but 1 was supplied"
        .to_owned();
    let grade = grading::edit_grade(&task, &one, &edited, &[error]);
    assert_eq!((grade.correctness, grade.completeness), (75, 0.5));
    assert_eq!(grade.missing, ["second call"]);

    let unchanged = grading::edit_grade(&task, "", &|_| None, &[]);
    assert_eq!((unchanged.correctness, unchanged.completeness), (0, 0.0));
}

#[test]
fn a_planted_patch_catches_only_the_unchanged_sites_the_checker_breaks() {
    let task = edit_task();
    let one = format!(
        "{DEFINITION}diff --git a/src/x.rs b/src/x.rs\n--- a/src/x.rs\n+++ b/src/x.rs\n\
         @@ -10,1 +10,1 @@\n-    f(1);\n+    f(1, 0);\n"
    );
    assert!(grading::signature_holds(&task, &one, &edited));
    assert!(!grading::signature_holds(&task, &one, &|_| None));
    let errors = [
        "src/x.rs:10:5: error[E0308]: mismatched types".to_owned(),
        "src/x.rs:21:5: error[E0061]: this function takes 2 arguments but 1 was supplied"
            .to_owned(),
    ];
    assert_eq!(grading::caught_sites(&task, &one, &errors), ["src/x.rs:20"]);
    assert!(grading::caught_sites(&task, DEFINITION, &[]).is_empty());
}

#[test]
fn an_edit_that_silences_the_checker_or_deletes_a_call_is_wrong() {
    let task = edit_task();
    let patch = format!(
        "{DEFINITION}diff --git a/src/x.rs b/src/x.rs\n--- a/src/x.rs\n+++ b/src/x.rs\n\
         @@ -10,1 +10,1 @@\n-    f(1);\n+    todo!();\n@@ -20,1 +20,1 @@\n-    f(2);\n+    f(2, 0);\n"
    );
    let grade = grading::edit_grade(&task, &patch, &edited, &[]);
    assert_eq!(grade.wrong_claims.len(), 2, "{:?}", grade.wrong_claims);
    assert_eq!(grade.correctness, 50);
}

#[test]
fn run_counts_tool_uses_and_sums_tokens_over_models() {
    let run = Run::parse(&log(2, &[(1, 2, 3, 4), (10, 20, 30, 40)]))
        .unwrap()
        .unwrap();
    assert_eq!(
        (run.tokens, run.uncached, run.tool_calls, run.read_calls),
        (110, 77, 2, 2)
    );
    assert_eq!(
        Run::parse(r#"{"type":"result","is_error":true}"#).unwrap(),
        None
    );
}

fn task() -> Task {
    Task {
        id: "router".to_owned(),
        kind: "impact".to_owned(),
        prompt: "Who calls match?".to_owned(),
        expected: "dispatch calls it.".to_owned(),
        items: vec!["dispatch".to_owned(), "SmartRouter".to_owned()],
        check: None,
        signature: None,
        sites: None,
        callee: None,
    }
}

fn answered(text: &str, tokens: u64) -> String {
    format!(
        r#"{{"type":"result","is_error":false,"result":"{text}","modelUsage":{{"m":{{"inputTokens":{tokens}}}}}}}"#
    )
}

fn grader_log(verdicts: &str) -> String {
    format!(
        r#"{{"type":"result","is_error":false,"structured_output":{{"answers":{verdicts}}},"modelUsage":{{}}}}"#
    )
}

#[test]
fn grading_is_blind_and_stores_a_grade_per_run() {
    let logs = tempfile::tempdir().unwrap();
    let task_logs = logs.path().join("hono").join("router");
    write(
        logs.path(),
        "hono",
        "router",
        "codemycel",
        1,
        &answered("ours", 10),
    );
    write(
        logs.path(),
        "hono",
        "router",
        "gitnexus",
        1,
        &answered("theirs", 30),
    );
    write(
        logs.path(),
        "hono",
        "router",
        "serena",
        1,
        r#"{"type":"result","is_error":true}"#,
    );

    let prompt = grading::prompt(&task_logs, &task()).unwrap().unwrap();
    for hidden in ["codemycel", "gitnexus", "serena"] {
        assert!(!prompt.contains(hidden), "{hidden} leaks into the prompt");
    }
    assert!(prompt.contains("1. dispatch\n2. SmartRouter\n"));
    let first_is_ours = prompt.find("ours").unwrap() < prompt.find("theirs").unwrap();
    let (ours, theirs) = if first_is_ours { (1, 2) } else { (2, 1) };

    let verdicts = format!(
        r#"[{{"answer":{ours},"correctness":100,"wrong_claims":[],"items_named":[1,2]}},{{"answer":{theirs},"correctness":60,"wrong_claims":["x"],"items_named":[2,7]}}]"#
    );
    assert_eq!(
        grading::store(&task_logs, &task(), &grader_log(&verdicts)).unwrap(),
        2
    );
    let theirs = grading::load_grade(&task_logs.join("gitnexus").join("1.jsonl"))
        .unwrap()
        .unwrap();
    assert_eq!(
        theirs,
        Grade {
            correctness: 60,
            wrong_claims: vec!["x".to_owned()],
            completeness: 0.5,
            missing: vec!["dispatch".to_owned()],
        }
    );
    assert_eq!(grading::prompt(&task_logs, &task()).unwrap(), None);
}

#[test]
fn report_shows_scores_per_mode_and_the_two_times_verdict() {
    let logs = tempfile::tempdir().unwrap();
    let root = logs.path();
    let grade = |mode: &str, run: u32, correctness: u32, completeness: f64| {
        let path = root
            .join("hono")
            .join("router")
            .join(mode)
            .join(format!("{run}.grade.json"));
        let grade = Grade {
            correctness,
            wrong_claims: Vec::new(),
            completeness,
            missing: Vec::new(),
        };
        std::fs::write(path, serde_json::to_string(&grade).unwrap()).unwrap();
    };
    for (run, (tokens, calls)) in [(500, 2), (700, 4), (600, 3)].into_iter().enumerate() {
        write(
            root,
            "hono",
            "router",
            "codemycel",
            run as u32,
            &log(calls, &[(tokens, 0, 0, 0)]),
        );
        grade("codemycel", run as u32, 100, 1.0);
    }
    for (run, (tokens, calls)) in [(1000, 9), (3000, 12), (2000, 10)].into_iter().enumerate() {
        write(
            root,
            "hono",
            "router",
            "gitnexus",
            run as u32,
            &log(calls, &[(tokens, 0, 0, 0)]),
        );
        grade("gitnexus", run as u32, 90, 0.5);
    }
    write(
        root,
        "hono",
        "router",
        "serena",
        0,
        &log(1, &[(100, 0, 0, 0)]),
    );

    let report = table(root).unwrap();
    for row in [
        "| hono | router | ? | codemycel | 600 | 600 | 0.000 | 100 | 100 | 0 · 0 · 3 · 0 | 0 | 3 (3) |",
        "| hono | router | ? | gitnexus | 2000 | 2000 | 0.000 | 90 | 50 | 0 · 0 · 10 · 0 | 0 | 3 (3) |",
        "| hono | router | ? | serena | 100 | 100 | 0.000 | n/a | n/a | 0 · 0 · 1 · 0 | 0 | 1 (0) |",
        "| codemycel | 1 | 600 | 600 | 0.000 | 100 | 100 |",
        "| ? | gitnexus | 1 | 2000 | 90 | 50 |",
        "| gitnexus | 1 of 1 | 0 | yes |",
        "| serena | 0 of 1 | 0 | no |",
    ] {
        assert!(report.contains(row), "missing {row}\n{report}");
    }
}
