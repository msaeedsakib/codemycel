use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context as _, bail};

const USAGE: &str = "usage: grade schema | grade prompt <tasks.toml> <task-logs> | \
                     grade store <tasks.toml> <task-logs> <grader-log> | \
                     grade edit <tasks.toml> <task-logs> <log> <baseline-errors> <errors> <edited-tree> | \
                     grade planted <tasks.toml> <task-logs> <patch> <baseline-errors> <errors> <edited-tree> <caught>";

/// The steps of `grade.sh` and `check.sh`: print the grader's output schema, print a task's blind
/// grading prompt, store a grader log's verdicts beside the graded runs, grade an edit run from
/// the checker's errors before and after its patch, or check one planted patch of the self-test.
/// `planted` fails when the patch's definition does not hold the task's signature, adds the sites
/// the patch breaks to the `<caught>` file, and prints the task's sites no planted patch has caught
/// yet, one per line.
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = |tasks: &str, task_logs: &str| -> anyhow::Result<codemycel_eval::Task> {
        let id = PathBuf::from(task_logs)
            .file_name()
            .context("the task log directory has no name")?
            .to_string_lossy()
            .into_owned();
        codemycel_eval::load_tasks(tasks.as_ref())?
            .into_iter()
            .find(|task| task.id == id)
            .with_context(|| format!("no task {id} in {tasks}"))
    };
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["schema"] => println!("{}", codemycel_eval::grading::SCHEMA),
        ["prompt", tasks, task_logs] => {
            if let Some(prompt) =
                codemycel_eval::grading::prompt(task_logs.as_ref(), &task(tasks, task_logs)?)?
            {
                print!("{prompt}");
            }
        }
        ["store", tasks, task_logs, grader_log] => {
            let log = std::fs::read_to_string(grader_log)?;
            let stored =
                codemycel_eval::grading::store(task_logs.as_ref(), &task(tasks, task_logs)?, &log)?;
            println!("stored {stored} grades");
        }
        ["edit", tasks, task_logs, log, baseline, after, edited_tree] => {
            let new = codemycel_eval::grading::new_errors(
                &std::fs::read_to_string(baseline)?,
                &std::fs::read_to_string(after)?,
            );
            let patch = std::fs::read_to_string(PathBuf::from(log).with_extension("patch"))
                .unwrap_or_default();
            let edited =
                |file: &str| std::fs::read_to_string(PathBuf::from(edited_tree).join(file)).ok();
            let grade = codemycel_eval::grading::edit_grade(
                &task(tasks, task_logs)?,
                &patch,
                &edited,
                &new,
            );
            std::fs::write(
                PathBuf::from(log).with_extension("grade.json"),
                serde_json::to_string_pretty(&grade)?,
            )?;
            println!("{} new errors", new.len());
        }
        [
            "planted",
            tasks,
            task_logs,
            patch,
            baseline,
            after,
            edited_tree,
            caught,
        ] => {
            let task = task(tasks, task_logs)?;
            let new = codemycel_eval::grading::new_errors(
                &std::fs::read_to_string(baseline)?,
                &std::fs::read_to_string(after)?,
            );
            let patch_text = std::fs::read_to_string(patch)?;
            let edited =
                |file: &str| std::fs::read_to_string(PathBuf::from(edited_tree).join(file)).ok();
            if !codemycel_eval::grading::signature_holds(&task, &patch_text, &edited) {
                bail!("{patch}: the definition does not hold the task's signature");
            }
            let mut sites: BTreeSet<String> = std::fs::read_to_string(caught)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect();
            sites.extend(codemycel_eval::grading::caught_sites(
                &task,
                &patch_text,
                &new,
            ));
            std::fs::write(
                caught,
                sites
                    .iter()
                    .map(|site| format!("{site}\n"))
                    .collect::<String>(),
            )?;
            for site in task.sites.iter().flatten() {
                if !sites.contains(site) {
                    println!("{site}");
                }
            }
        }
        _ => bail!(USAGE),
    }
    Ok(())
}
