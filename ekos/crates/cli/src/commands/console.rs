//! `ekos console` — start, stop and open the web console (RFC 0185 §2).
//!
//! The console is a Python application. A full package carries its own interpreter and the
//! console beside the binary (`<bin>/../console/`); a checkout points at one with
//! `EKOS_CONSOLE_PYTHON` and `EKOS_CONSOLE_APP_DIR`.
//!
//! `run` is the supervisor: it starts the console and starts it again when it asks to be.
//! Stop and restart travel as files in `<home>/control/` (`stop-requested`,
//! `restart-requested`), the same on every OS: the console watches for them and shuts down
//! cleanly, and `run` reads which one it was. A console that exits with status 0 and no file
//! was stopped on purpose (Ctrl-C); any other exit is a crash and is started again, at most
//! `MAX_CRASHES` times in `CRASH_WINDOW`.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const STOP: &str = "stop-requested";
pub const RESTART: &str = "restart-requested";
pub const PID_FILE: &str = "console.pid";
pub const LOG_FILE: &str = "console.log";
pub const SIGNIN_DIR: &str = "local-signin";
const MAX_CRASHES: usize = 5;
const CRASH_WINDOW: Duration = Duration::from_secs(600);

/// Where the console's interpreter and code are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub python: PathBuf,
    /// The directory holding the `app` package.
    pub app_dir: PathBuf,
    /// The console's own dependencies, when they are not in the interpreter's environment.
    pub site_packages: Option<PathBuf>,
    pub ui_dir: Option<PathBuf>,
}

fn package_python(console: &Path) -> PathBuf {
    if cfg!(windows) {
        console.join("python").join("python.exe")
    } else {
        console.join("python").join("bin").join("python3")
    }
}

/// The runtime: `EKOS_CONSOLE_PYTHON` + `EKOS_CONSOLE_APP_DIR` when both are set, else the
/// package layout beside `exe`.
pub fn find_runtime(exe: &Path, env: &dyn Fn(&str) -> Option<String>) -> Result<Runtime> {
    if let (Some(python), Some(app)) = (env("EKOS_CONSOLE_PYTHON"), env("EKOS_CONSOLE_APP_DIR")) {
        let app_dir = PathBuf::from(app);
        if !app_dir.join("app").join("main.py").is_file() {
            bail!(
                "EKOS_CONSOLE_APP_DIR={} has no app/main.py (point it at web/api)",
                app_dir.display()
            );
        }
        let ui_dir = env("EKOS_CONSOLE_UI_DIR")
            .map(PathBuf::from)
            .or_else(|| Some(app_dir.join("..").join("ui").join("dist")))
            .filter(|p| p.is_dir());
        return Ok(Runtime {
            python: PathBuf::from(python),
            app_dir,
            site_packages: None,
            ui_dir,
        });
    }
    let console = exe
        .parent()
        .and_then(Path::parent)
        .map(|prefix| prefix.join("console"))
        .context("cannot locate the directory of the ekos binary")?;
    let python = package_python(&console);
    if !python.is_file() || !console.join("app").join("main.py").is_file() {
        bail!(
            "the web console is not installed beside this binary (looked in {}).\n\
             Install the full package (`ekos setup`), run the container image, or for a checkout \
             set EKOS_CONSOLE_PYTHON (web/api/.venv/bin/python) and EKOS_CONSOLE_APP_DIR (web/api)",
            console.display()
        );
    }
    let ui = console.join("ui");
    Ok(Runtime {
        python,
        app_dir: console.clone(),
        site_packages: Some(console.join("site-packages")),
        ui_dir: ui.is_dir().then_some(ui),
    })
}

/// Where and how the console listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerConf {
    pub mode: String,
    pub host: String,
    pub port: u16,
}

#[derive(Deserialize, Default)]
struct FileConf {
    mode: Option<String>,
    host: Option<String>,
    port: Option<u16>,
}

/// `console.toml` of the home (or `EKOS_CONSOLE_CONFIG_FILE`).
pub fn config_file(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    env("EKOS_CONSOLE_CONFIG_FILE")
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join("control").join("console.toml"))
}

pub fn is_loopback(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost")
        || h.parse::<std::net::IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

/// Mode, host and port: the environment, then `console.toml`, then defaults. Local mode on a
/// non-loopback address is refused: it signs in without a provider.
pub fn server_conf(home: &Path, env: &dyn Fn(&str) -> Option<String>) -> Result<ServerConf> {
    let path = config_file(home, env);
    let file: FileConf = match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileConf::default(),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    let mode = env("EKOS_CONSOLE_MODE")
        .or(file.mode)
        .unwrap_or_else(|| "team".into());
    let host = env("EKOS_CONSOLE_HOST")
        .or(file.host)
        .unwrap_or_else(|| "127.0.0.1".into());
    let port = match env("EKOS_CONSOLE_PORT") {
        Some(p) => p
            .parse()
            .with_context(|| format!("EKOS_CONSOLE_PORT={p} is not a port"))?,
        None => file.port.unwrap_or(8000),
    };
    if mode == "local" && !is_loopback(&host) {
        bail!(
            "local mode listens on this machine only, not on {host}; set host = \"127.0.0.1\" \
             or use mode = \"team\""
        );
    }
    Ok(ServerConf { mode, host, port })
}

/// The program, arguments and extra environment that start the console.
pub fn command_for(
    rt: &Runtime,
    conf: &ServerConf,
    home: &Path,
    exe: &Path,
) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
    let args = vec![
        "-m".into(),
        "uvicorn".into(),
        "app.main:create_app".into(),
        "--factory".into(),
        "--host".into(),
        conf.host.clone(),
        "--port".into(),
        conf.port.to_string(),
    ];
    let mut paths = vec![rt.app_dir.clone()];
    paths.extend(rt.site_packages.clone());
    let pythonpath = std::env::join_paths(&paths)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut env = vec![
        ("EKOS_HOME".into(), home.display().to_string()),
        ("EKOS_BIN".into(), exe.display().to_string()),
        ("EKOS_CONSOLE_SUPERVISED".into(), "true".into()),
        ("PYTHONPATH".into(), pythonpath),
        // The person's own Python packages must not shadow the console's.
        ("PYTHONNOUSERSITE".into(), "1".into()),
        ("PYTHONUNBUFFERED".into(), "1".into()),
    ];
    if let Some(ui) = &rt.ui_dir {
        env.push(("EKOS_CONSOLE_UI_DIR".into(), ui.display().to_string()));
    }
    (rt.python.clone(), args, env)
}

// ── state files ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PidFile {
    pub supervisor: u32,
    pub console: Option<u32>,
    pub host: String,
    pub port: u16,
    pub started: String,
}

fn control(home: &Path) -> PathBuf {
    home.join("control")
}

fn read_pid(home: &Path) -> Option<PidFile> {
    let text = std::fs::read_to_string(control(home).join(PID_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_pid(home: &Path, pid: &PidFile) -> Result<()> {
    let path = control(home).join(PID_FILE);
    std::fs::write(&path, serde_json::to_string_pretty(pid)?)
        .with_context(|| format!("writing {}", path.display()))
}

fn remove_markers(home: &Path) {
    for m in [STOP, RESTART] {
        let _ = std::fs::remove_file(control(home).join(m));
    }
}

/// What the supervisor does after the console exited.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Stop,
    Restart,
    Crashed,
}

/// Decide from the markers and the exit status. A marker wins; without one, status 0 is a
/// deliberate stop (Ctrl-C reaches both processes) and anything else a crash.
pub fn after_exit(control: &Path, success: bool) -> Next {
    if control.join(STOP).exists() {
        Next::Stop
    } else if control.join(RESTART).exists() {
        Next::Restart
    } else if success {
        Next::Stop
    } else {
        Next::Crashed
    }
}

// ── health ───────────────────────────────────────────────────────────────────

/// The address to reach a console bound to `host` from this machine.
fn reach(host: &str) -> &str {
    match host {
        "0.0.0.0" | "" => "127.0.0.1",
        "::" | "[::]" => "::1",
        other => other,
    }
}

/// Whether the console answers `/api/health` with 200.
pub fn healthy(host: &str, port: u16) -> bool {
    let target = reach(host).trim_start_matches('[').trim_end_matches(']');
    let Ok(mut addrs) = (target, port).to_socket_addrs() else {
        return false;
    };
    let Some(addr) = addrs.next() else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let request = format!(
        "GET /api/health HTTP/1.1\r\nHost: {}:{port}\r\nConnection: close\r\n\r\n",
        url_host(host)
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut head = [0u8; 16];
    let n = stream.read(&mut head).unwrap_or(0);
    String::from_utf8_lossy(&head[..n]).starts_with("HTTP/1.1 200")
}

fn url_host(host: &str) -> String {
    let h = reach(host);
    if h.contains(':') && !h.starts_with('[') {
        format!("[{h}]")
    } else {
        h.to_string()
    }
}

pub fn url(conf: &ServerConf) -> String {
    format!("http://{}:{}", url_host(&conf.host), conf.port)
}

fn wait_until(deadline: Duration, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + deadline;
    while Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    ok()
}

// ── local sign-in ────────────────────────────────────────────────────────────

/// RFC 0184 §3: a one-time code; only its SHA-256 is written, as a file name, under
/// `control/local-signin/`. The console accepts it once, within two minutes.
pub fn issue_code(home: &Path) -> Result<String> {
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    let code = hex::encode(bytes);
    let dir = control(home).join(SIGNIN_DIR);
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = hex::encode(Sha256::digest(code.as_bytes()));
    std::fs::File::create(dir.join(name))?;
    Ok(code)
}

/// Open `url` in the default browser. Failing is not an error: the URL is printed anyway.
pub fn open_browser(url: &str) -> bool {
    let status = if cfg!(target_os = "windows") {
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else {
        Command::new("xdg-open")
            .arg(url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    };
    status.map(|s| s.success()).unwrap_or(false)
}

// ── the commands ─────────────────────────────────────────────────────────────

fn std_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn log_file(home: &Path) -> Result<std::fs::File> {
    let path = control(home).join(LOG_FILE);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("opening {}", path.display()))
}

/// `ekos console run`: the console in the foreground, started again when it asks.
pub fn run(home: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    let rt = find_runtime(&exe, &std_env)?;
    let conf = server_conf(home, &std_env)?;
    std::fs::create_dir_all(control(home))?;
    if let Some(pid) = read_pid(home)
        && healthy(&pid.host, pid.port)
    {
        bail!(
            "a console already runs for this home at {}",
            url(&ServerConf {
                mode: conf.mode,
                host: pid.host,
                port: pid.port
            })
        );
    }
    remove_markers(home);
    let (program, args, env) = command_for(&rt, &conf, home, &exe);
    let mut crashes: VecDeque<Instant> = VecDeque::new();
    let mut pid = PidFile {
        supervisor: std::process::id(),
        console: None,
        host: conf.host.clone(),
        port: conf.port,
        started: chrono::Utc::now().to_rfc3339(),
    };
    eprintln!("ekos console: {} ({} mode)", url(&conf), conf.mode);
    let outcome = loop {
        let log = log_file(home)?;
        let mut child = Command::new(&program)
            .args(&args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .spawn()
            .with_context(|| format!("starting {}", program.display()))?;
        pid.console = Some(child.id());
        write_pid(home, &pid)?;
        let status = child.wait()?;
        match after_exit(&control(home), status.success()) {
            Next::Stop => break Ok(()),
            Next::Restart => {
                remove_markers(home);
                eprintln!("ekos console: restarting");
            }
            Next::Crashed => {
                let now = Instant::now();
                crashes.push_back(now);
                while crashes
                    .front()
                    .is_some_and(|t| now.duration_since(*t) > CRASH_WINDOW)
                {
                    crashes.pop_front();
                }
                if crashes.len() >= MAX_CRASHES {
                    break Err(anyhow::anyhow!(
                        "the console exited {MAX_CRASHES} times in {} minutes ({status}); see {}",
                        CRASH_WINDOW.as_secs() / 60,
                        control(home).join(LOG_FILE).display()
                    ));
                }
                eprintln!("ekos console: exited ({status}); starting it again");
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    };
    remove_markers(home);
    let _ = std::fs::remove_file(control(home).join(PID_FILE));
    outcome
}

/// `ekos console start`: `run` in the background; returns once the console answers.
pub fn start(home: &Path) -> Result<()> {
    let exe = std::env::current_exe()?;
    find_runtime(&exe, &std_env)?;
    let conf = server_conf(home, &std_env)?;
    std::fs::create_dir_all(control(home))?;
    if healthy(&conf.host, conf.port) {
        println!("The console already runs at {}", url(&conf));
        return Ok(());
    }
    let log = log_file(home)?;
    let mut cmd = Command::new(&exe);
    cmd.arg("--home")
        .arg(home)
        .args(["console", "run"])
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    detach(&mut cmd);
    cmd.spawn().context("starting `ekos console run`")?;
    if !wait_until(Duration::from_secs(60), || healthy(&conf.host, conf.port)) {
        bail!(
            "the console did not answer at {} within a minute; see {}",
            url(&conf),
            control(home).join(LOG_FILE).display()
        );
    }
    println!("The console runs at {}", url(&conf));
    if conf.mode == "local" {
        println!("Sign in: ekos console open");
    }
    Ok(())
}

#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    // Its own process group: closing the terminal or Ctrl-C there does not reach it.
    cmd.process_group(0);
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

fn running(home: &Path) -> Option<PidFile> {
    read_pid(home).filter(|p| healthy(&p.host, p.port))
}

fn force_end(pid: u32) {
    let _ = if cfg!(windows) {
        Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    } else {
        Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
    };
}

/// `ekos console stop`.
pub fn stop(home: &Path) -> Result<()> {
    let Some(pid) = running(home) else {
        let _ = std::fs::remove_file(control(home).join(PID_FILE));
        println!("The console is not running.");
        return Ok(());
    };
    std::fs::write(control(home).join(STOP), b"")?;
    let pid_file = control(home).join(PID_FILE);
    if !wait_until(Duration::from_secs(20), || !pid_file.exists()) {
        eprintln!("The console did not stop within 20 seconds; ending it.");
        if let Some(c) = pid.console {
            force_end(c);
        }
        force_end(pid.supervisor);
        remove_markers(home);
        let _ = std::fs::remove_file(&pid_file);
    }
    println!("The console stopped.");
    Ok(())
}

/// `ekos console restart`.
pub fn restart(home: &Path) -> Result<()> {
    let Some(pid) = running(home) else {
        return start(home);
    };
    std::fs::write(control(home).join(RESTART), b"")?;
    let down = wait_until(Duration::from_secs(20), || !healthy(&pid.host, pid.port));
    if !down || !wait_until(Duration::from_secs(60), || healthy(&pid.host, pid.port)) {
        bail!(
            "the console did not come back; see {}",
            control(home).join(LOG_FILE).display()
        );
    }
    println!("The console restarted.");
    Ok(())
}

/// `ekos console status`.
pub fn status(home: &Path) -> Result<()> {
    let conf = server_conf(home, &std_env)?;
    match read_pid(home) {
        Some(p) if healthy(&p.host, p.port) => println!(
            "running at http://{}:{} ({} mode) since {}; supervisor pid {}, console pid {}",
            url_host(&p.host),
            p.port,
            conf.mode,
            p.started,
            p.supervisor,
            p.console
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".into())
        ),
        Some(p) => println!(
            "not answering at http://{}:{} (supervisor pid {} may be starting it or stuck); \
             see {}",
            url_host(&p.host),
            p.port,
            p.supervisor,
            control(home).join(LOG_FILE).display()
        ),
        None => println!(
            "not running (configured for {} in {} mode)",
            url(&conf),
            conf.mode
        ),
    }
    Ok(())
}

/// `ekos console open`.
pub fn open(home: &Path, no_browser: bool) -> Result<()> {
    let conf = server_conf(home, &std_env)?;
    if !healthy(&conf.host, conf.port) {
        bail!(
            "the console is not running at {}; start it with `ekos console start`",
            url(&conf)
        );
    }
    let target = if conf.mode == "local" {
        let code = issue_code(home)?;
        println!("One-time sign-in code (two minutes): {code}");
        format!("{}/api/auth/local?code={code}", url(&conf))
    } else {
        url(&conf)
    };
    if no_browser || !open_browser(&target) {
        println!("Open {target}");
    }
    Ok(())
}

/// `ekos console logs`.
pub fn logs(home: &Path, lines: usize) -> Result<()> {
    let path = control(home).join(LOG_FILE);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no log yet at {}", path.display()))?;
    let all: Vec<&str> = text.lines().collect();
    for line in &all[all.len().saturating_sub(lines)..] {
        println!("{line}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |k| map.get(k).cloned()
    }

    fn package(root: &Path) -> PathBuf {
        let console = root.join("console");
        std::fs::create_dir_all(console.join("app")).unwrap();
        std::fs::write(console.join("app").join("main.py"), "").unwrap();
        let py = package_python(&console);
        std::fs::create_dir_all(py.parent().unwrap()).unwrap();
        std::fs::write(&py, "").unwrap();
        std::fs::create_dir_all(console.join("ui")).unwrap();
        std::fs::create_dir_all(root.join("bin")).unwrap();
        root.join("bin").join("ekos")
    }

    #[test]
    fn the_runtime_is_found_beside_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        let exe = package(dir.path());
        let rt = find_runtime(&exe, &env_of(&[])).unwrap();
        let console = dir.path().join("console");
        assert_eq!(rt.python, package_python(&console));
        assert_eq!(rt.app_dir, console);
        assert_eq!(rt.site_packages, Some(console.join("site-packages")));
        assert_eq!(rt.ui_dir, Some(console.join("ui")));
    }

    #[test]
    fn a_checkout_is_named_by_two_variables() {
        let dir = tempfile::tempdir().unwrap();
        let api = dir.path().join("web").join("api");
        std::fs::create_dir_all(api.join("app")).unwrap();
        std::fs::write(api.join("app").join("main.py"), "").unwrap();
        let env = env_of(&[
            ("EKOS_CONSOLE_PYTHON", "/venv/bin/python"),
            ("EKOS_CONSOLE_APP_DIR", api.to_str().unwrap()),
        ]);
        let rt = find_runtime(Path::new("/nowhere/bin/ekos"), &env).unwrap();
        assert_eq!(rt.python, PathBuf::from("/venv/bin/python"));
        assert_eq!(rt.app_dir, api);
        assert_eq!(rt.site_packages, None);
    }

    #[test]
    fn no_runtime_says_where_it_looked_and_what_to_set() {
        let err = find_runtime(Path::new("/nowhere/bin/ekos"), &env_of(&[]))
            .unwrap_err()
            .to_string();
        assert!(err.contains("EKOS_CONSOLE_PYTHON") && err.contains("EKOS_CONSOLE_APP_DIR"));
        assert!(err.contains("/nowhere/console"), "{err}");
    }

    #[test]
    fn server_settings_come_from_the_environment_then_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        assert_eq!(
            server_conf(home, &env_of(&[])).unwrap(),
            ServerConf {
                mode: "team".into(),
                host: "127.0.0.1".into(),
                port: 8000
            }
        );
        std::fs::create_dir_all(home.join("control")).unwrap();
        std::fs::write(
            home.join("control").join("console.toml"),
            "# mine\nmode = \"local\"\nport = 8123\noidc_scopes = \"openid\"\n",
        )
        .unwrap();
        let c = server_conf(home, &env_of(&[])).unwrap();
        assert_eq!((c.mode.as_str(), c.port), ("local", 8123));
        let c = server_conf(home, &env_of(&[("EKOS_CONSOLE_PORT", "9000")])).unwrap();
        assert_eq!(c.port, 9000);
    }

    #[test]
    fn local_mode_refuses_a_public_address() {
        let dir = tempfile::tempdir().unwrap();
        let env = env_of(&[
            ("EKOS_CONSOLE_MODE", "local"),
            ("EKOS_CONSOLE_HOST", "0.0.0.0"),
        ]);
        let err = server_conf(dir.path(), &env).unwrap_err().to_string();
        assert!(err.contains("this machine only"), "{err}");
        for host in ["127.0.0.1", "localhost", "::1", "[::1]"] {
            let env = env_of(&[("EKOS_CONSOLE_MODE", "local"), ("EKOS_CONSOLE_HOST", host)]);
            assert!(server_conf(dir.path(), &env).is_ok(), "{host}");
        }
    }

    #[test]
    fn the_command_line_and_environment() {
        let rt = Runtime {
            python: "/p/python3".into(),
            app_dir: "/p/console".into(),
            site_packages: Some("/p/console/site-packages".into()),
            ui_dir: Some("/p/console/ui".into()),
        };
        let conf = ServerConf {
            mode: "local".into(),
            host: "127.0.0.1".into(),
            port: 8123,
        };
        let (program, args, env) =
            command_for(&rt, &conf, Path::new("/h/.ekos"), Path::new("/p/bin/ekos"));
        assert_eq!(program, PathBuf::from("/p/python3"));
        assert_eq!(
            args.join(" "),
            "-m uvicorn app.main:create_app --factory --host 127.0.0.1 --port 8123"
        );
        let env: HashMap<_, _> = env.into_iter().collect();
        assert_eq!(env["EKOS_HOME"], "/h/.ekos");
        assert_eq!(env["EKOS_BIN"], "/p/bin/ekos");
        assert_eq!(env["EKOS_CONSOLE_SUPERVISED"], "true");
        assert_eq!(env["EKOS_CONSOLE_UI_DIR"], "/p/console/ui");
        assert_eq!(env["PYTHONNOUSERSITE"], "1");
        let sep = if cfg!(windows) { ";" } else { ":" };
        assert_eq!(
            env["PYTHONPATH"],
            format!("/p/console{sep}/p/console/site-packages")
        );
    }

    #[test]
    fn what_happens_after_the_console_exits() {
        let dir = tempfile::tempdir().unwrap();
        let c = dir.path();
        assert_eq!(after_exit(c, true), Next::Stop);
        assert_eq!(after_exit(c, false), Next::Crashed);
        std::fs::write(c.join(RESTART), "").unwrap();
        assert_eq!(after_exit(c, true), Next::Restart);
        std::fs::write(c.join(STOP), "").unwrap();
        assert_eq!(after_exit(c, false), Next::Stop);
    }

    #[test]
    fn a_sign_in_code_is_stored_as_its_hash() {
        let dir = tempfile::tempdir().unwrap();
        let a = issue_code(dir.path()).unwrap();
        let b = issue_code(dir.path()).unwrap();
        assert_ne!(a, b);
        let files: Vec<String> = std::fs::read_dir(dir.path().join("control").join(SIGNIN_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(files.len(), 2);
        assert!(files.contains(&hex::encode(Sha256::digest(a.as_bytes()))));
        assert!(!files.iter().any(|f| f.contains(&a)));
    }

    #[test]
    fn nothing_listening_is_not_healthy() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert!(!healthy("127.0.0.1", port));
    }

    #[test]
    fn urls_for_each_kind_of_address() {
        let conf = |host: &str| ServerConf {
            mode: "team".into(),
            host: host.into(),
            port: 8000,
        };
        assert_eq!(url(&conf("0.0.0.0")), "http://127.0.0.1:8000");
        assert_eq!(url(&conf("::1")), "http://[::1]:8000");
        assert_eq!(url(&conf("localhost")), "http://localhost:8000");
    }
}
