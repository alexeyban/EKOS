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
/// RFC 0261 §2.3: the console's answer to a stop it refused, naming the runs that hold it.
pub const REFUSED: &str = "stop-refused";
/// RFC 0261 §2.3: the runs queued or running, kept current by the console for `status --json`.
pub const ACTIVE: &str = "active-runs.json";
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
    for m in [STOP, RESTART, REFUSED, ACTIVE] {
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

/// This program's own file, with links resolved: an OS package links `/usr/bin/ekos` or
/// `/usr/local/bin/ekos` to the program in its install directory, and the console and setup
/// find the package (`console/`, `installer.toml`) beside the real file, not beside the link.
/// macOS reports the path as started; Linux has resolved it already. Not on Windows, where
/// resolving gives a `\\?\` path and nothing links the program.
pub fn program_exe() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the ekos binary")?;
    if cfg!(windows) {
        return Ok(exe);
    }
    Ok(std::fs::canonicalize(&exe).unwrap_or(exe))
}

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
    let exe = program_exe()?;
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
    let exe = program_exe()?;
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

/// `ekos uninstall --ui --detach` (RFC 0188): the uninstall page in a new background process,
/// so it outlives the console that asked for it. Prints the page's address once it answers.
///
/// Under the console's systemd user unit, a child — however detached — is in the unit's
/// cgroup and is ended when the unit stops, which uninstalling does. `systemd-run --user`
/// starts the page in a unit of its own.
pub fn detach_uninstall_page(home: &Path, port: u16) -> Result<()> {
    let exe = program_exe()?;
    // Not in the home: a purge removes it.
    let log_path = std::env::temp_dir().join(format!("ekos-uninstall-{}.log", std::process::id()));
    let page_args: Vec<std::ffi::OsString> = vec![
        exe.clone().into(),
        "--home".into(),
        home.into(),
        "uninstall".into(),
        "--ui".into(),
        "--no-browser".into(),
        "--port".into(),
        port.to_string().into(),
    ];
    let under_systemd = cfg!(target_os = "linux")
        && super::setup::read_manifest(home).service.as_deref() == Some("linux")
        && Command::new("systemd-run")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    let mut child = None;
    if under_systemd {
        let status = Command::new("systemd-run")
            .args(["--user", "--collect", "--quiet"])
            .arg(format!("--unit=ekos-uninstall-{}", std::process::id()))
            .arg(format!(
                "--property=StandardOutput=file:{}",
                log_path.display()
            ))
            .arg("--property=StandardError=inherit")
            .args(&page_args)
            .stdin(Stdio::null())
            .status()
            .context("starting the uninstall page with systemd-run")?;
        if !status.success() {
            bail!("systemd-run could not start the uninstall page ({status})");
        }
    } else {
        let log = std::fs::File::create(&log_path)
            .with_context(|| format!("creating {}", log_path.display()))?;
        let mut cmd = Command::new(&page_args[0]);
        cmd.args(&page_args[1..])
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log);
        detach(&mut cmd);
        child = Some(cmd.spawn().context("starting the uninstall page")?);
    }
    let prefix = super::uninstall_server::URL_PREFIX;
    let mut line = None;
    wait_until(Duration::from_secs(20), || {
        line = std::fs::read_to_string(&log_path).ok().and_then(|t| {
            t.lines()
                .find(|l| l.starts_with(prefix))
                .map(str::to_string)
        });
        // A page that already exited will never print its link.
        let exited = child
            .as_mut()
            .is_some_and(|c| c.try_wait().ok().flatten().is_some());
        line.is_some() || exited
    });
    match line {
        Some(l) => {
            println!("{l}");
            Ok(())
        }
        None => bail!(
            "the uninstall page did not start: {}",
            std::fs::read_to_string(&log_path)
                .unwrap_or_default()
                .trim()
        ),
    }
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

/// How `ekos console stop` treats runs in progress (RFC 0261 §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopMode {
    /// Stop only when nothing runs; otherwise name the runs and leave the console running.
    Ask,
    /// Wait for the runs to finish, then stop.
    Wait,
    /// Stop now; runs in progress end as interrupted. Uninstall and the installers, after
    /// telling the person.
    Force,
}

/// A run that holds the console, as the console writes it in [`REFUSED`] and [`ACTIVE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveRun {
    pub id: String,
    pub workspace: String,
    pub command: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub since: String,
}

/// What `ekos console stop --json` prints: `{stopped: true}`, or `{stopped: false, active_runs}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StopOutcome {
    pub stopped: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub active_runs: Vec<ActiveRun>,
}

impl StopOutcome {
    fn stopped() -> Self {
        Self {
            stopped: true,
            active_runs: Vec::new(),
        }
    }
}

fn read_runs(path: &Path) -> Option<Vec<ActiveRun>> {
    let text = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    // `stop-refused` is `{active_runs: [...]}`, `active-runs.json` the list itself.
    let list = value.get("active_runs").cloned().unwrap_or(value);
    serde_json::from_value(list).ok()
}

/// Asks the console to stop and waits for its answer: gone, or refused because runs are active
/// (a console from before RFC 0261 never refuses). A console that answers neither within 20 s
/// is hung and is ended, as before.
pub fn stop_console(home: &Path, mode: StopMode) -> Result<StopOutcome> {
    let Some(pid) = running(home) else {
        let _ = std::fs::remove_file(control(home).join(PID_FILE));
        return Ok(StopOutcome::stopped());
    };
    let pid_file = control(home).join(PID_FILE);
    let refused = control(home).join(REFUSED);
    let marker: &[u8] = if mode == StopMode::Force {
        b"force"
    } else {
        b""
    };
    let mut announced = false;
    loop {
        let _ = std::fs::remove_file(&refused);
        std::fs::write(control(home).join(STOP), marker)?;
        if !wait_until(Duration::from_secs(20), || {
            !pid_file.exists() || refused.exists()
        }) {
            eprintln!("The console did not stop within 20 seconds; ending it.");
            if let Some(c) = pid.console {
                force_end(c);
            }
            force_end(pid.supervisor);
            remove_markers(home);
            let _ = std::fs::remove_file(&pid_file);
            return Ok(StopOutcome::stopped());
        }
        if !pid_file.exists() {
            let _ = std::fs::remove_file(&refused);
            return Ok(StopOutcome::stopped());
        }
        let runs = read_runs(&refused).unwrap_or_default();
        let _ = std::fs::remove_file(&refused);
        if mode != StopMode::Wait {
            return Ok(StopOutcome {
                stopped: false,
                active_runs: runs,
            });
        }
        if !announced {
            eprintln!("Waiting for {} to finish…", runs_words(&runs));
            announced = true;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn runs_words(runs: &[ActiveRun]) -> String {
    match runs.len() {
        1 => "1 run".into(),
        n => format!("{n} runs"),
    }
}

/// `ekos console stop [--wait|--force] [--json]`. Refused, it names the runs and exits non-zero.
pub fn stop(home: &Path, mode: StopMode, json: bool) -> Result<()> {
    let outcome = stop_console(home, mode)?;
    if json {
        println!("{}", serde_json::to_string(&outcome)?);
    } else if outcome.stopped {
        println!("The console stopped.");
    } else {
        println!(
            "The console keeps running: {} in progress would be lost.",
            runs_words(&outcome.active_runs)
        );
        for r in &outcome.active_runs {
            println!(
                "  {}  {}  {} since {}",
                r.id, r.workspace, r.command, r.since
            );
        }
        println!("Wait for them with `ekos console stop --wait`, or end them with `--force`.");
    }
    if !outcome.stopped {
        bail!("the console was not stopped: runs are in progress");
    }
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

/// What `ekos console status --json` prints (RFC 0261 §2.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusJson {
    /// `running` (it answers), `starting` (a supervisor, no answer yet) or `stopped`.
    pub state: &'static str,
    /// The home has its `console.toml`: `ekos setup` ran. Without it this is a first run, and the
    /// desktop app sends the person to EKOS Setup (RFC 0261 §2.3 step 2).
    pub set_up: bool,
    pub mode: String,
    pub host: String,
    pub port: u16,
    pub since: Option<String>,
    pub supervisor: Option<u32>,
    pub console: Option<u32>,
    /// The runs in progress, while it runs; `null` from a console that does not report them.
    pub active_runs: Option<Vec<ActiveRun>>,
}

pub fn status_json(home: &Path) -> Result<StatusJson> {
    let conf = server_conf(home, &std_env)?;
    let pid = read_pid(home);
    let up = pid.as_ref().is_some_and(|p| healthy(&p.host, p.port));
    let state = match (&pid, up) {
        (Some(_), true) => "running",
        (Some(_), false) => "starting",
        (None, _) => "stopped",
    };
    let active_runs = match state {
        "running" => read_runs(&control(home).join(ACTIVE)),
        _ => Some(Vec::new()),
    };
    Ok(StatusJson {
        state,
        set_up: config_file(home, &std_env).is_file(),
        mode: conf.mode.clone(),
        host: pid
            .as_ref()
            .map_or(conf.host.clone(), |p| url_host(&p.host)),
        port: pid.as_ref().map_or(conf.port, |p| p.port),
        since: pid.as_ref().map(|p| p.started.clone()),
        supervisor: pid.as_ref().map(|p| p.supervisor),
        console: pid.as_ref().and_then(|p| p.console),
        active_runs,
    })
}

/// `ekos console status [--json]`.
pub fn status(home: &Path, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(&status_json(home)?)?);
        return Ok(());
    }
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

/// What `ekos console open --json` prints (RFC 0261 §2.3): the console's address and, in local
/// mode, a fresh one-time code (`null` in team mode). The desktop app signs in with
/// `{url}/api/auth/local?code={code}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenTarget {
    pub url: String,
    pub code: Option<String>,
}

impl OpenTarget {
    /// Where a browser goes: the sign-in link with the code, else the address.
    pub fn link(&self) -> String {
        match &self.code {
            Some(code) => format!("{}/api/auth/local?code={code}", self.url),
            None => self.url.clone(),
        }
    }
}

/// The target for `conf`; `code` is kept in local mode only.
pub fn open_target(conf: &ServerConf, code: Option<&str>) -> OpenTarget {
    OpenTarget {
        url: url(conf),
        code: code.filter(|_| conf.mode == "local").map(str::to_string),
    }
}

/// `ekos console open`. `json` prints the target as one JSON object and opens nothing.
pub fn open(home: &Path, no_browser: bool, json: bool) -> Result<()> {
    let conf = server_conf(home, &std_env)?;
    if !healthy(&conf.host, conf.port) {
        bail!(
            "the console is not running at {}; start it with `ekos console start`",
            url(&conf)
        );
    }
    let code = if conf.mode == "local" {
        Some(issue_code(home)?)
    } else {
        None
    };
    let target = open_target(&conf, code.as_deref());
    if json {
        println!("{}", serde_json::to_string(&target)?);
        return Ok(());
    }
    if let Some(code) = &code {
        println!("One-time sign-in code (two minutes): {code}");
    }
    let link = target.link();
    if no_browser || !open_browser(&link) {
        println!("Open {link}");
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
    fn the_open_target_carries_a_code_in_local_mode_only() {
        let local = ServerConf {
            mode: "local".into(),
            host: "127.0.0.1".into(),
            port: 8123,
        };
        let t = open_target(&local, Some("abc"));
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            serde_json::json!({"url": "http://127.0.0.1:8123", "code": "abc"})
        );
        assert_eq!(t.link(), "http://127.0.0.1:8123/api/auth/local?code=abc");

        let team = ServerConf {
            mode: "team".into(),
            ..local
        };
        let t = open_target(&team, Some("abc"));
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            serde_json::json!({"url": "http://127.0.0.1:8123", "code": null})
        );
        assert_eq!(t.link(), "http://127.0.0.1:8123");
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

    /// A stand-in for a running console: answers `/api/health` with 200 while `up` is set,
    /// 503 otherwise. Returns its port.
    fn fake_console(up: std::sync::Arc<std::sync::atomic::AtomicBool>) -> u16 {
        use std::sync::atomic::Ordering;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buf = [0u8; 512];
                let _ = stream.read(&mut buf);
                let status = if up.load(Ordering::SeqCst) {
                    "200 OK"
                } else {
                    "503 Service Unavailable"
                };
                let _ = stream.write_all(
                    format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes(),
                );
            }
        });
        port
    }

    fn home_running_at(port: u16, mode: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(control(dir.path())).unwrap();
        std::fs::write(
            control(dir.path()).join("console.toml"),
            format!("mode = \"{mode}\"\nhost = \"127.0.0.1\"\nport = {port}\n"),
        )
        .unwrap();
        write_pid(
            dir.path(),
            &PidFile {
                // Never reached: the tests remove the pid file before `stop` would end anything.
                supervisor: u32::MAX - 1,
                console: None,
                host: "127.0.0.1".into(),
                port,
                started: "2026-10-14T00:00:00Z".into(),
            },
        )
        .unwrap();
        dir
    }

    #[test]
    fn with_nothing_running_status_stop_and_logs_say_so() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        std::fs::create_dir_all(control(home)).unwrap();
        status(home, false).unwrap();
        // A pid file left by a console that is gone is removed by stop.
        write_pid(
            home,
            &PidFile {
                supervisor: u32::MAX - 1,
                console: None,
                host: "127.0.0.1".into(),
                port: 1,
                started: String::new(),
            },
        )
        .unwrap();
        status(home, true).unwrap();
        stop(home, StopMode::Ask, false).unwrap();
        assert!(read_pid(home).is_none());
        assert!(
            logs(home, 10).is_err(),
            "no log yet is an error that says where"
        );
        std::fs::write(control(home).join(LOG_FILE), "one\ntwo\nthree\n").unwrap();
        logs(home, 2).unwrap();
        // A port nothing listens on: the default (8000) may be a real console on this machine.
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        std::fs::write(
            control(home).join("console.toml"),
            format!("port = {port}\n"),
        )
        .unwrap();
        assert!(
            open(home, true, false).is_err(),
            "open needs a running console"
        );
    }

    #[test]
    fn a_running_console_is_reported_and_opened_with_a_code() {
        use std::sync::atomic::AtomicBool;
        let up = std::sync::Arc::new(AtomicBool::new(true));
        let port = fake_console(up.clone());
        let local = home_running_at(port, "local");
        status(local.path(), false).unwrap();
        open(local.path(), true, false).unwrap();
        let codes = std::fs::read_dir(control(local.path()).join(SIGNIN_DIR)).unwrap();
        assert_eq!(codes.count(), 1, "local mode signs in with one stored code");

        let team = home_running_at(port, "team");
        open(team.path(), true, false).unwrap();
        open(team.path(), false, true).unwrap();
        assert!(
            !control(team.path()).join(SIGNIN_DIR).exists(),
            "team mode issues no code"
        );

        // Running but not answering.
        up.store(false, std::sync::atomic::Ordering::SeqCst);
        status(team.path(), false).unwrap();
        assert_eq!(status_json(team.path()).unwrap().state, "starting");
    }

    #[test]
    fn stop_asks_by_marker_and_returns_once_the_pid_file_is_gone() {
        use std::sync::atomic::AtomicBool;
        let port = fake_console(std::sync::Arc::new(AtomicBool::new(true)));
        let dir = home_running_at(port, "team");
        let home = dir.path().to_path_buf();
        let watcher = {
            let home = home.clone();
            std::thread::spawn(move || {
                // The supervisor's part: see the marker, remove the pid file.
                while !control(&home).join(STOP).exists() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                std::fs::remove_file(control(&home).join(PID_FILE)).unwrap();
            })
        };
        stop(&home, StopMode::Ask, false).unwrap();
        watcher.join().unwrap();
        assert!(read_pid(&home).is_none());
    }

    /// A fake supervisor for RFC 0261 §2.3: answers each stop request with a refusal naming one
    /// run, `refusals` times, then stops (removes the pid file). Returns the markers' texts.
    fn refusing_console(home: PathBuf, refusals: usize) -> std::thread::JoinHandle<Vec<String>> {
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            let run = r#"{"active_runs":[{"id":"r1","workspace":"shop","command":"build","status":"running","since":"t0"}]}"#;
            loop {
                let marker = control(&home).join(STOP);
                let Ok(text) = std::fs::read_to_string(&marker) else {
                    std::thread::sleep(Duration::from_millis(20));
                    continue;
                };
                seen.push(text.clone());
                let _ = std::fs::remove_file(&marker);
                if seen.len() > refusals || text.contains("force") {
                    std::fs::remove_file(control(&home).join(PID_FILE)).unwrap();
                    return seen;
                }
                std::fs::write(control(&home).join(REFUSED), run).unwrap();
            }
        })
    }

    #[test]
    fn a_stop_with_runs_in_progress_is_refused_and_names_them() {
        use std::sync::atomic::AtomicBool;
        let port = fake_console(std::sync::Arc::new(AtomicBool::new(true)));
        let dir = home_running_at(port, "local");
        let home = dir.path().to_path_buf();
        let console = refusing_console(home.clone(), usize::MAX);
        let out = stop_console(&home, StopMode::Ask).unwrap();
        assert!(!out.stopped);
        assert_eq!(out.active_runs.len(), 1);
        assert_eq!(out.active_runs[0].command, "build");
        assert_eq!(
            serde_json::to_value(&out).unwrap()["active_runs"][0]["id"],
            "r1",
            "the JSON names the run"
        );
        assert!(read_pid(&home).is_some(), "the console keeps running");
        assert!(
            !control(&home).join(REFUSED).exists(),
            "the answer is read once"
        );
        // Let the fake console go: a forced stop ends it.
        assert!(stop_console(&home, StopMode::Force).unwrap().stopped);
        assert_eq!(
            console.join().unwrap().last().map(String::as_str),
            Some("force")
        );
    }

    #[test]
    fn a_waiting_stop_asks_again_until_the_runs_are_done() {
        use std::sync::atomic::AtomicBool;
        let port = fake_console(std::sync::Arc::new(AtomicBool::new(true)));
        let dir = home_running_at(port, "local");
        let home = dir.path().to_path_buf();
        let console = refusing_console(home.clone(), 1);
        let out = stop_console(&home, StopMode::Wait).unwrap();
        assert_eq!(out, StopOutcome::stopped());
        assert_eq!(
            console.join().unwrap(),
            ["", ""],
            "asked twice, never forced"
        );
        assert_eq!(serde_json::to_string(&out).unwrap(), r#"{"stopped":true}"#);
    }

    #[test]
    fn status_json_reports_the_state_and_the_runs() {
        use std::sync::atomic::AtomicBool;
        let stopped = tempfile::tempdir().unwrap();
        let s = status_json(stopped.path()).unwrap();
        assert_eq!(
            (s.state, s.supervisor, s.active_runs),
            ("stopped", None, Some(vec![]))
        );
        assert!(!s.set_up, "no console.toml: a first run");

        let port = fake_console(std::sync::Arc::new(AtomicBool::new(true)));
        let dir = home_running_at(port, "local");
        // A console from before RFC 0261 writes no list: unknown, not empty.
        assert_eq!(status_json(dir.path()).unwrap().active_runs, None);
        std::fs::write(
            control(dir.path()).join(ACTIVE),
            r#"[{"id":"r1","workspace":"shop","command":"build"}]"#,
        )
        .unwrap();
        let s = status_json(dir.path()).unwrap();
        assert_eq!(s.state, "running");
        assert!(s.set_up);
        assert_eq!(s.port, port);
        assert_eq!(s.active_runs.unwrap()[0].workspace, "shop");
    }

    #[test]
    fn restart_asks_by_marker_and_waits_for_down_then_up() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let up = std::sync::Arc::new(AtomicBool::new(true));
        let port = fake_console(up.clone());
        let dir = home_running_at(port, "team");
        let home = dir.path().to_path_buf();
        let supervisor = {
            let (home, up) = (home.clone(), up.clone());
            std::thread::spawn(move || {
                while !control(&home).join(RESTART).exists() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                up.store(false, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(400));
                up.store(true, Ordering::SeqCst);
            })
        };
        restart(&home).unwrap();
        supervisor.join().unwrap();
        assert!(healthy("127.0.0.1", port));
    }

    #[test]
    fn markers_decide_and_are_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(control(dir.path())).unwrap();
        std::fs::write(control(dir.path()).join(RESTART), "").unwrap();
        assert_eq!(after_exit(&control(dir.path()), false), Next::Restart);
        remove_markers(dir.path());
        assert_eq!(after_exit(&control(dir.path()), false), Next::Crashed);
    }
}
