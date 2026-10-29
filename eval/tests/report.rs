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
fn committed_task_files_hold_five_tasks_each() {
    for repo in ["hono", "irixmail"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tasks/{repo}.toml"));
        let tasks = codemycel_eval::load_tasks(&path).unwrap();
        let mut ids: Vec<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!((tasks.len(), ids.len()), (5, 5), "{repo}");
    }
}

#[test]
fn run_counts_tool_uses_and_sums_tokens_over_models() {
    let run = Run::parse(&log(2, &[(1, 2, 3, 4), (10, 20, 30, 40)]))
        .unwrap()
        .unwrap();
    assert_eq!(
        run,
        Run {
            tokens: 110,
            tool_calls: 2
        }
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

    assert_eq!(
        table(root).unwrap(),
        "| Repository | Task | Mode | Tokens | Correctness | Completeness | Tool calls | Runs (graded) |\n\
         |---|---|---|---:|---:|---:|---:|---:|\n\
         | hono | router | codemycel | 600 | 100 | 100 | 3 | 3 (3) |\n\
         | hono | router | gitnexus | 2000 | 90 | 50 | 10 | 3 (3) |\n\
         | hono | router | serena | 100 | n/a | n/a | 1 | 1 (0) |\n\
         \n\
         | Mode | Tasks | Total median tokens | Mean correctness | Mean completeness |\n\
         |---|---:|---:|---:|---:|\n\
         | codemycel | 1 | 600 | 100 | 100 |\n\
         | gitnexus | 1 | 2000 | 90 | 50 |\n\
         | serena | 1 | 100 | n/a | n/a |\n\
         \n\
         | codemycel against | Tasks won 2× (correctness ≥, completeness ≥, tokens ≤ ½) |\n\
         |---|---:|\n\
         | gitnexus | 1 of 1 |\n\
         | serena | 0 of 1 |\n"
    );
}
