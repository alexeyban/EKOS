//! RFC 0220 end to end, through the real binary: every pipeline command writes its stages to the
//! run log, ends with the model-call count and its outcome, and a chained run shares one file.

use serde_json::Value;
use std::path::Path;
use std::process::Command;

fn ekos(cwd: &Path, args: &[&str], run_log: Option<&Path>) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ekos"));
    cmd.args(args)
        .current_dir(cwd)
        .env("EKOS_HOME", cwd.join(".home"))
        .env_remove("EKOS_PROJECT")
        .env_remove("EKOS_TENANT")
        .env_remove("EKOS_CONFIG")
        .env_remove("EKOS_WORKSPACE")
        .env_remove("EKOS_RUN_LOG");
    if let Some(path) = run_log {
        cmd.env("EKOS_RUN_LOG", path);
    }
    let out = cmd.output().expect("running the ekos binary");
    assert!(
        out.status.success(),
        "ekos {args:?} failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn read(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}")))
        .collect()
}

fn workspace(dir: &Path) {
    std::fs::create_dir_all(dir.join("db")).unwrap();
    std::fs::write(
        dir.join("db/schema.sql"),
        "CREATE TABLE customer (id INT PRIMARY KEY, name TEXT);\n\
         CREATE TABLE invoice (id INT PRIMARY KEY, customer_id INT REFERENCES customer(id));\n",
    )
    .unwrap();
    ekos(dir, &["init"], None);
}

#[test]
fn a_chained_pipeline_run_logs_every_stage_and_no_model_call() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = &tmp.path().join("ws");
    workspace(dir);
    // Outside the sources, as the console keeps it: a file in them would be a changed source.
    let log = tmp.path().join("run-42.jsonl");
    let pipeline: [&[&str]; 5] = [
        &["build"],
        &["recover"],
        &["resolve", "--force"],
        &["compile"],
        &["commit", "--yes"],
    ];
    for args in pipeline {
        ekos(dir, args, Some(&log));
    }
    let events = read(&log);
    let stage = |cmd: &str, stage: &str| {
        events
            .iter()
            .filter(|e| e["cmd"] == cmd && e["stage"] == stage)
            .collect::<Vec<_>>()
    };

    for cmd in ["build", "recover", "resolve", "compile", "commit"] {
        let run = stage(cmd, "run");
        assert_eq!(run.len(), 2, "{cmd}: {run:?}");
        assert_eq!(run[0]["msg"], "started");
        assert_eq!(run[1]["msg"], "finished");
        assert_eq!(run[1]["ok"], true, "{cmd}: {:?}", run[1]);
        let llm = stage(cmd, "llm");
        assert_eq!(llm.len(), 1, "{cmd}");
        assert_eq!(llm[0]["count"], 0, "{cmd}: {:?}", llm[0]);
        assert_eq!(llm[0]["embeddings"], 0, "{cmd}");
    }
    // Each command's last two lines are its model calls and its outcome, in that order.
    let finished: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["stage"] == "run" && e["msg"] == "finished")
        .map(|(i, _)| i)
        .collect();
    for i in finished {
        assert_eq!(events[i - 1]["stage"], "llm");
        assert_eq!(events[i - 1]["cmd"], events[i]["cmd"]);
    }

    assert!(!stage("build", "observe").is_empty());
    assert!(stage("build", "build")[0]["files_new"].as_u64().unwrap() >= 1);
    let passes = stage("recover", "pass");
    let summary = passes.iter().find(|e| e["msg"] == "passes").unwrap();
    assert!(summary["ran"].as_u64().unwrap() >= 1, "{summary}");
    assert_eq!(stage("recover", "recover")[0]["sql_files"], 1);
    assert_eq!(stage("compile", "semantic.compile")[0]["objects"], 2);
    let commit = &stage("commit", "ledger.commit")[0];
    assert!(commit["objects"].as_u64().unwrap() > 0, "{commit}");
    assert!(stage("resolve", "identity.resolve")[0]["candidates"].is_u64());
}

#[test]
fn without_the_environment_each_command_gets_its_own_file_in_the_state_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    workspace(dir);
    ekos(dir, &["build"], None);
    let runs: Vec<_> = std::fs::read_dir(dir.join(".ekos/runs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(runs.len(), 1, "{runs:?}");
    assert!(runs[0].to_string_lossy().ends_with("-build.jsonl"));
    let events = read(&runs[0]);
    assert_eq!(events.first().unwrap()["msg"], "started");
    assert_eq!(events.last().unwrap()["msg"], "finished");
}
