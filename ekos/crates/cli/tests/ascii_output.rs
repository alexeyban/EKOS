//! Plain-ASCII terminal output (`ekos::ascii`), through the real binary. On Windows it is the
//! default; here `EKOS_ASCII=1` turns it on.

use std::process::{Command, Output};
use tempfile::TempDir;

fn ekos(ascii: &str, args: &[&str]) -> Output {
    let home = TempDir::new().unwrap();
    Command::new(env!("CARGO_BIN_EXE_ekos"))
        .args(args)
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("EKOS_HOME", home.path().join(".ekos"))
        .env("EKOS_ASCII", ascii)
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn doctor_help_and_usage_errors_are_ascii() {
    for args in [&["doctor"][..], &["--help"], &["no-such-command"]] {
        let out = text(&ekos("1", args));
        assert!(out.is_ascii(), "ekos {args:?} printed non-ASCII:\n{out}");
    }
    assert!(text(&ekos("1", &["doctor"])).contains("\n----------"));
}

#[test]
fn without_it_the_symbols_stay() {
    assert!(text(&ekos("0", &["doctor"])).contains('─'));
}

#[test]
fn json_output_is_not_changed() {
    let out = ekos("1", &["console", "status", "--json"]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(doc.get("state").is_some(), "{doc}");
}
