//! RFC 0240 end to end through the real binary: `ekos eval bench prepare` builds one EKOS project
//! per fixture database with the real pipeline and scans each ledger for leakage; `run` answers
//! from those ledgers through the real Ollama provider, pointed at a stand-in server that
//! answers like the fixture's scripted model; `compare`, `export` and `history` read the reports.

use ekos_evals::bench::fixture;
use ekos_evals::bench::report::BenchReport;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ekos(cwd: &Path, args: &[&str]) -> std::process::Output {
    ekos_with(cwd, args, None)
}

/// With `ollama`, the Ollama provider talks to that server (it reads `OLLAMA_BASE_URL`, not
/// `[llm] base-url`).
fn ekos_with(cwd: &Path, args: &[&str], ollama: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ekos"));
    cmd.current_dir(cwd).args(args);
    if let Some(url) = ollama {
        cmd.env("OLLAMA_BASE_URL", url);
    }
    for var in [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "OPENCODE_API_KEY",
        "EKOS_HOME",
    ] {
        cmd.env_remove(var);
    }
    cmd.output().unwrap()
}

fn ok(out: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        out.status.success(),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// A one-thread `/api/chat` server: every message's text goes to the scripted model.
fn fake_ollama() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(v) = lower.strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body).unwrap();
            let req: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let text: String = req["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|m| m["content"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let resp = serde_json::json!({
                "model": req["model"], "done": true,
                "message": {"role": "assistant", "content": fixture::scripted_answer(&text)},
                "prompt_eval_count": text.len() / 4, "eval_count": 20,
            })
            .to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{resp}",
                resp.len()
            );
        }
    });
    format!("http://{addr}")
}

fn newest_report(dir: &Path, cell: &str) -> PathBuf {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        // Reports only: a run also leaves its checkpoint (`*.partial.jsonl`) beside them.
        .filter(|p| {
            p.extension().is_some_and(|x| x == "json")
                && p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(&format!("-{cell}-"))
        })
        .collect();
    found.sort();
    found.pop().unwrap()
}

#[test]
fn prepare_run_compare_export_on_the_fixture_suite() {
    let tmp = tempfile::tempdir().unwrap();
    let bench = tmp.path().join("evals").join("bench");
    let suite = bench.join("fixture");
    fixture::write_suite(&suite).unwrap();
    let base_url = fake_ollama();
    let cwd = tmp.path();

    let out = ekos(
        cwd,
        &["eval", "bench", "prepare", "--suite", "fixture", "--json"],
    );
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let out = ok(&out);
    let prepared: serde_json::Value = serde_json::from_str(&out)
        .unwrap_or_else(|e| panic!("{e}\nstdout:\n{out}\nstderr:\n{stderr}"));
    assert_eq!(prepared["leak_hits"], 0);
    for db in ["shop", "school"] {
        let d = &prepared["databases"][db];
        assert_eq!(d["tables"], 2, "{out}");
        assert!(d["relationships"].as_u64().unwrap() >= 1, "{out}");
    }
    // Idempotent: nothing is rebuilt and the fingerprints stay.
    let again = ekos(
        cwd,
        &["eval", "bench", "prepare", "--suite", "fixture", "--json"],
    );
    assert!(String::from_utf8_lossy(&again.stderr).contains("up to date"));
    let again: serde_json::Value = serde_json::from_str(&ok(&again)).unwrap();
    assert_eq!(
        again["databases"]["shop"]["ledger_fingerprint"],
        prepared["databases"]["shop"]["ledger_fingerprint"]
    );

    // The dictionary reaches the pack: COMMENT ON lines on quoted identifiers (RFC 0241 §6).
    let pack = ok(&ekos(
        cwd,
        &[
            "eval", "bench", "context", "--suite", "fixture", "--cell", "e", "--db", "shop",
        ],
    ));
    assert!(
        pack.contains("customer segment; values: retail: a private person"),
        "{pack}"
    );
    assert!(pack.contains("-- 4 rows"), "the profile is shown: {pack}");

    let listing = ok(&ekos(cwd, &["eval", "bench", "list"]));
    assert!(
        listing.contains("fixture") && listing.contains("c-prime"),
        "{listing}"
    );

    for cell in ["a", "c"] {
        let out = ok(&ekos_with(
            cwd,
            &["eval", "bench", "run", "--suite", "fixture", "--cell", cell],
            Some(&base_url),
        ));
        assert!(out.contains("n = 10 of 10 planned"), "{out}");
    }
    let reports = cwd.join("evals/reports/bench/fixture");
    let (a, c) = (newest_report(&reports, "a"), newest_report(&reports, "c"));
    let report_c = BenchReport::load(&c).unwrap();
    // The pack came from the compiled ledger: column comments from the dictionary and the
    // foreign keys made it, and value lookup found the three values.
    assert_eq!(
        report_c.metrics.ex.mean.round(),
        90.0,
        "{:#?}",
        report_c.questions.first()
    );
    assert_eq!(BenchReport::load(&a).unwrap().metrics.ex.mean.round(), 60.0);
    assert_eq!(report_c.metrics.join_recall, Some(100.0));
    assert_eq!(report_c.ledger_fingerprints.len(), 2);
    assert_eq!(
        report_c.ledger_fingerprints["shop"],
        prepared["databases"]["shop"]["ledger_fingerprint"]
            .as_str()
            .unwrap()
    );
    assert_eq!(report_c.metrics.cost_usd, 0.0);
    assert!(report_c.metrics.cost_known);

    let cmp = ok(&ekos(
        cwd,
        &[
            "eval",
            "bench",
            "compare",
            c.to_str().unwrap(),
            a.to_str().unwrap(),
        ],
    ));
    assert!(cmp.contains("difference +30.00 points"), "{cmp}");
    assert!(cmp.contains("Only C right (3): 1, 6, 7"), "{cmp}");

    let exported = ok(&ekos(
        cwd,
        &[
            "eval",
            "bench",
            "export",
            c.to_str().unwrap(),
            "--target",
            "bird-submission",
        ],
    ));
    assert!(
        exported.contains("predict_fixture_c_all.json"),
        "{exported}"
    );

    let history = ok(&ekos(cwd, &["eval", "history"]));
    assert!(history.contains("Benchmark runs (RFC 0240)"), "{history}");
}

#[test]
fn a_metered_route_needs_a_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let suite = tmp.path().join("evals/bench/fixture");
    fixture::write_suite(&suite).unwrap();
    let path = suite.join("configs/a.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap()
        .replace("provider = \"ollama\"", "provider = \"anthropic\"");
    std::fs::write(&path, text).unwrap();
    let out = ekos(
        tmp.path(),
        &["eval", "bench", "run", "--suite", "fixture", "--cell", "a"],
    );
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--allow-metered"), "{stderr}");
}
