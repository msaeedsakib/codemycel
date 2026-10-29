#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_eval::{Run, table};

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

#[test]
fn eval_table_is_generated_from_logs() {
    let logs = tempfile::tempdir().unwrap();
    let root = logs.path();
    for (run, (tokens, calls)) in [(1000, 9), (3000, 12), (2000, 10)].into_iter().enumerate() {
        write(
            root,
            "hono",
            "router",
            "without",
            run as u32,
            &log(calls, &[(tokens, 0, 0, 0)]),
        );
    }
    for (run, (tokens, calls)) in [(500, 2), (700, 4), (600, 3)].into_iter().enumerate() {
        write(
            root,
            "hono",
            "router",
            "with",
            run as u32,
            &log(calls, &[(tokens, 0, 0, 0)]),
        );
    }
    write(
        root,
        "irixmail",
        "dkim",
        "without",
        0,
        &log(1, &[(100, 0, 0, 0)]),
    );
    write(
        root,
        "irixmail",
        "dkim",
        "with",
        0,
        r#"{"type":"result","is_error":true}"#,
    );

    assert_eq!(
        table(root).unwrap(),
        "| Repository | Task | Tokens without | Tokens with | Change | Tool calls without | Tool calls with | Runs |\n\
         |---|---|---:|---:|---:|---:|---:|---:|\n\
         | hono | router | 2000 | 600 | -70% | 10 | 3 | 3+3 |\n\
         | irixmail | dkim | 100 | n/a | n/a | 1 | n/a | 1+0 |\n"
    );
}
