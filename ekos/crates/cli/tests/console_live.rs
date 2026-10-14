//! RFC 0185 §2: `ekos console` against the real console from a checkout. Skipped unless
//! `EKOS_CONSOLE_PYTHON` (web/api/.venv/bin/python) and `EKOS_CONSOLE_APP_DIR` (web/api) are set.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn ekos() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ekos"))
}

fn runtime_given() -> bool {
    std::env::var_os("EKOS_CONSOLE_PYTHON").is_some()
        && std::env::var_os("EKOS_CONSOLE_APP_DIR").is_some()
}

fn console(home: &Path, args: &[&str]) -> String {
    let out = Command::new(ekos())
        .arg("--home")
        .arg(home)
        .arg("console")
        .args(args)
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "console {args:?}: {text}");
    text
}

fn pid_file(home: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(home.join("control").join("console.pid")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn eventually(what: &str, mut ok: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(60);
    while Instant::now() < end {
        if ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    panic!("timed out waiting for {what}");
}

fn console_pid(home: &Path) -> Option<u64> {
    std::fs::read_to_string(home.join("control").join("console.pid"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["console"].as_u64())
}

#[test]
fn start_sign_in_restart_crash_and_stop() {
    if !runtime_given() {
        eprintln!("skipped: set EKOS_CONSOLE_PYTHON and EKOS_CONSOLE_APP_DIR");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    std::fs::create_dir_all(home.join("control")).unwrap();
    let port = free_port();
    std::fs::write(
        home.join("control").join("console.toml"),
        format!("mode = \"local\"\nport = {port}\n"),
    )
    .unwrap();
    let base = format!("http://127.0.0.1:{port}");
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    console(home, &["start"]);
    let first = pid_file(home);
    assert!(console(home, &["status"]).contains("running"));

    // A one-time code signs in once.
    let opened = console(home, &["open", "--no-browser"]);
    let link = opened
        .split_whitespace()
        .find(|w| w.contains("/api/auth/local?code="))
        .unwrap()
        .to_string();
    let signed_in = client.get(&link).send().unwrap();
    assert_eq!(signed_in.status().as_u16(), 303);
    let cookie = signed_in.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let me: serde_json::Value = client
        .get(format!("{base}/api/auth/me"))
        .header("cookie", &cookie)
        .send()
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(me["kind"], "local");
    assert_eq!(me["owner"], true);
    assert_eq!(client.get(&link).send().unwrap().status().as_u16(), 401);

    // The settings page's restart button: same supervisor, a new console.
    let r = client
        .post(format!("{base}/api/platform/restart"))
        .header("cookie", &cookie)
        .send()
        .unwrap();
    assert_eq!(r.status().as_u16(), 202);
    let before = first["console"].as_u64();
    eventually("a restarted console", || {
        console_pid(home).is_some_and(|p| Some(p) != before)
            && client.get(format!("{base}/api/health")).send().is_ok()
    });
    assert_eq!(pid_file(home)["supervisor"], first["supervisor"]);

    // A crash: the console is killed; the supervisor starts it again.
    #[cfg(unix)]
    {
        let victim = console_pid(home).unwrap();
        Command::new("kill")
            .args(["-KILL", &victim.to_string()])
            .status()
            .unwrap();
        eventually("a console after a crash", || {
            console_pid(home).is_some_and(|p| p != victim)
                && client.get(format!("{base}/api/health")).send().is_ok()
        });
    }

    console(home, &["stop"]);
    assert!(!home.join("control").join("console.pid").exists());
    assert!(
        client.get(format!("{base}/api/health")).send().is_err(),
        "still answering after stop"
    );
    assert!(console(home, &["status"]).contains("not running"));
}
