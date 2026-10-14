//! `ekos setup` and `ekos uninstall` (RFC 0185 §3–4).
//!
//! The person's answers (from the browser wizard in `setup_server`, or a TOML file with
//! `--answers`) become a list of [`Action`]s — `plan` is pure, so what will happen can be shown
//! before anything does — and `apply` performs them in order, reporting each. What was done is
//! recorded in `<home>/control/install.json` so `uninstall` undoes exactly that.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

pub const MANIFEST: &str = "install.json";
pub const PATH_MARKER: &str = "# added by ekos setup";
pub const SERVICE_NAME: &str = "ekos-console";
pub const LAUNCHD_LABEL: &str = "com.ekos.console";
pub const WINDOWS_TASK: &str = "EKOS Console";

// ── answers ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Answers {
    /// Where `bin/` and `console/` go. Empty: the home.
    pub program_dir: String,
    /// The EKOS home. Empty: `~/.ekos`.
    pub home: String,
    pub add_to_path: bool,
    pub console: ConsoleAnswers,
    pub project: Option<ProjectAnswers>,
    pub agents: AgentAnswers,
    /// RFC 0187: the chat model.
    pub chat: ChatAnswers,
    pub start: StartAnswers,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ChatAnswers {
    /// `anthropic`, `openai`, `ollama`, or empty for no chat.
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// The name of the variable holding the key.
    pub api_key_env: String,
    /// Goes to the keychain under `api_key_env`, never to a file.
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ConsoleAnswers {
    pub mode: String,
    pub host: String,
    pub port: u16,
    pub oidc_issuer: String,
    pub oidc_client_id: String,
    /// Goes to the keychain, never to a file.
    pub oidc_client_secret: String,
    pub oidc_redirect_uri: String,
    pub owners: String,
    pub production: bool,
    pub mcp_idle_seconds: u32,
    pub mcp_max_servers: u32,
}

impl Default for ConsoleAnswers {
    fn default() -> Self {
        Self {
            mode: "local".into(),
            host: "127.0.0.1".into(),
            port: 8000,
            oidc_issuer: String::new(),
            oidc_client_id: String::new(),
            oidc_client_secret: String::new(),
            oidc_redirect_uri: String::new(),
            owners: String::new(),
            production: false,
            mcp_idle_seconds: 900,
            mcp_max_servers: 8,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectAnswers {
    pub name: String,
    /// `<name>=<dir>` or `<dir>`, as `ekos project add --source`.
    pub sources: Vec<String>,
    pub detect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AgentAnswers {
    /// The project agents are connected to. Empty: the new project, if any.
    pub project: String,
    pub claude_code: bool,
    pub claude_desktop: bool,
    pub vscode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StartAnswers {
    pub at_login: bool,
    pub now: bool,
    pub open: bool,
}

impl Default for StartAnswers {
    fn default() -> Self {
        Self {
            at_login: false,
            now: true,
            open: true,
        }
    }
}

// ── the machine ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Macos
        } else {
            Os::Linux
        }
    }
    fn exe(self) -> &'static str {
        if self == Os::Windows {
            "ekos.exe"
        } else {
            "ekos"
        }
    }
}

/// What `plan` needs to know about the machine; tests make one up.
#[derive(Debug, Clone)]
pub struct Machine {
    pub os: Os,
    pub user_home: PathBuf,
    /// `%APPDATA%` on Windows.
    pub appdata: Option<PathBuf>,
    /// The package this setup runs from: the directory holding `bin/` (and `console/`).
    pub package: PathBuf,
    pub shell: Option<String>,
}

impl Machine {
    pub fn detect(exe: &Path) -> Result<Self> {
        let user_home = crate::home::user_home_from_env().context("no HOME / USERPROFILE")?;
        let package = exe
            .parent()
            .and_then(Path::parent)
            .context("cannot locate the package around the ekos binary")?
            .to_path_buf();
        Ok(Self {
            os: Os::current(),
            user_home,
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            package,
            shell: std::env::var("SHELL").ok(),
        })
    }

    pub fn default_home(&self) -> PathBuf {
        self.user_home.join(".ekos")
    }

    fn config_dir(&self) -> PathBuf {
        match self.os {
            Os::Macos => self.user_home.join("Library").join("Application Support"),
            Os::Windows => self
                .appdata
                .clone()
                .unwrap_or_else(|| self.user_home.join("AppData").join("Roaming")),
            Os::Linux => self.user_home.join(".config"),
        }
    }

    pub fn claude_desktop_config(&self) -> PathBuf {
        self.config_dir()
            .join("Claude")
            .join("claude_desktop_config.json")
    }

    pub fn vscode_mcp_config(&self) -> PathBuf {
        self.config_dir().join("Code").join("User").join("mcp.json")
    }

    /// The shell start-up files a `PATH` line goes into.
    pub fn profile_files(&self) -> Vec<PathBuf> {
        let h = &self.user_home;
        let mut files = vec![h.join(".profile")];
        let zsh = self.shell.as_deref().is_some_and(|s| s.ends_with("zsh"))
            || self.os == Os::Macos
            || h.join(".zshrc").exists();
        if zsh {
            files.push(h.join(".zprofile"));
        }
        if h.join(".bashrc").exists() {
            files.push(h.join(".bashrc"));
        }
        files
    }
}

// ── actions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    CopyPackage {
        from: PathBuf,
        to: PathBuf,
    },
    WriteConsoleConfig {
        path: PathBuf,
        values: BTreeMap<String, toml::Value>,
    },
    StoreSecret {
        field: String,
    },
    StoreKey {
        name: String,
    },
    AddToPath {
        bin: PathBuf,
        files: Vec<PathBuf>,
    },
    AddToWindowsPath {
        bin: PathBuf,
    },
    CreateProject {
        name: String,
        sources: Vec<String>,
        detect: bool,
    },
    RegisterClaudeCode {
        name: String,
        command: PathBuf,
        args: Vec<String>,
    },
    MergeAgentConfig {
        agent: String,
        path: PathBuf,
        section: String,
        name: String,
        entry: serde_json::Value,
    },
    InstallService {
        os: Os,
        path: Option<PathBuf>,
        content: String,
    },
    StartConsole,
    OpenConsole,
}

impl Action {
    /// One line for a person.
    pub fn describe(&self) -> String {
        match self {
            Action::CopyPackage { from, to } => {
                format!("copy EKOS from {} to {}", from.display(), to.display())
            }
            Action::WriteConsoleConfig { path, values } => format!(
                "write {} ({})",
                path.display(),
                values.keys().cloned().collect::<Vec<_>>().join(", ")
            ),
            Action::StoreSecret { field } => format!("store {field} in the keychain"),
            Action::StoreKey { name } => format!("store the chat key as {name} in the keychain"),
            Action::AddToPath { bin, files } => format!(
                "put {} on PATH in {}",
                bin.display(),
                files
                    .iter()
                    .map(|f| f.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Action::AddToWindowsPath { bin } => {
                format!("put {} on your user Path", bin.display())
            }
            Action::CreateProject { name, sources, .. } => {
                format!("create project {name} from {}", sources.join(", "))
            }
            Action::RegisterClaudeCode { name, .. } => {
                format!("register MCP server {name} with Claude Code")
            }
            Action::MergeAgentConfig {
                agent, path, name, ..
            } => format!("add MCP server {name} to {agent} ({})", path.display()),
            Action::InstallService { os, path, .. } => match (os, path) {
                (_, Some(p)) => format!("start the console at login ({})", p.display()),
                (_, None) => format!("start the console at login (task \"{WINDOWS_TASK}\")"),
            },
            Action::StartConsole => "start the console".into(),
            Action::OpenConsole => "open the console in the browser".into(),
        }
    }
}

/// Where things end up, resolved from the answers.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub program_dir: PathBuf,
    pub home: PathBuf,
}

impl Layout {
    pub fn of(answers: &Answers, m: &Machine) -> Self {
        let home = non_empty(&answers.home).unwrap_or_else(|| m.default_home());
        let program_dir = non_empty(&answers.program_dir).unwrap_or_else(|| home.clone());
        Self { program_dir, home }
    }
    pub fn bin_dir(&self) -> PathBuf {
        self.program_dir.join("bin")
    }
    pub fn exe(&self, os: Os) -> PathBuf {
        self.bin_dir().join(os.exe())
    }
}

fn non_empty(s: &str) -> Option<PathBuf> {
    let t = s.trim();
    (!t.is_empty()).then(|| PathBuf::from(t))
}

pub fn check(answers: &Answers) -> Result<()> {
    let c = &answers.console;
    if c.mode != "local" && c.mode != "team" {
        bail!("console mode is `local` or `team`, not {:?}", c.mode);
    }
    if c.mode == "local" && !super::console::is_loopback(&c.host) {
        bail!("local mode listens on this machine only; host must be 127.0.0.1 or localhost");
    }
    if c.mode == "local" && !c.oidc_issuer.trim().is_empty() {
        bail!("local mode signs in without a provider; leave the OIDC issuer empty");
    }
    if c.production
        && (c.mode != "team" || c.oidc_issuer.trim().is_empty() || c.owners.trim().is_empty())
    {
        bail!("production mode needs team mode, an OIDC issuer and at least one owner");
    }
    if c.port == 0 {
        bail!("the console needs a port");
    }
    let chat = &answers.chat;
    if !chat.provider.is_empty() {
        if !["anthropic", "openai", "ollama"].contains(&chat.provider.as_str()) {
            bail!(
                "the chat provider is anthropic, openai or ollama, not {:?}",
                chat.provider
            );
        }
        if chat.model.trim().is_empty() {
            bail!("choose the chat model");
        }
        if !chat.api_key.is_empty() && !valid_variable(&chat.api_key_env) {
            bail!("name the variable the chat key is kept under, e.g. ANTHROPIC_API_KEY");
        }
    }
    if let Some(p) = &answers.project {
        crate::home::validate_name("project", &p.name)?;
        if p.sources.is_empty() {
            bail!("project {} needs at least one source directory", p.name);
        }
    }
    let a = &answers.agents;
    if (a.claude_code || a.claude_desktop || a.vscode) && agent_project(answers).is_none() {
        bail!("choose the project agents connect to");
    }
    Ok(())
}

/// An environment variable a project or the chat may name (the console's own are refused).
pub fn valid_variable(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && name.len() <= 128
        && !name.to_ascii_uppercase().starts_with("EKOS_CONSOLE_")
}

fn agent_project(answers: &Answers) -> Option<String> {
    let named = answers.agents.project.trim();
    if !named.is_empty() {
        return Some(named.to_string());
    }
    answers.project.as_ref().map(|p| p.name.clone())
}

fn console_values(c: &ConsoleAnswers, chat: &ChatAnswers) -> BTreeMap<String, toml::Value> {
    use toml::Value;
    let mut v = BTreeMap::new();
    if !chat.provider.is_empty() {
        for (k, val) in [
            ("chat_provider", &chat.provider),
            ("chat_model", &chat.model),
            ("chat_base_url", &chat.base_url),
            ("chat_api_key_env", &chat.api_key_env),
        ] {
            v.insert(k.into(), Value::String(val.trim().to_string()));
        }
    }
    v.insert("mode".into(), Value::String(c.mode.clone()));
    v.insert("host".into(), Value::String(c.host.clone()));
    v.insert("port".into(), Value::Integer(c.port.into()));
    v.insert(
        "mcp_idle_seconds".into(),
        Value::Integer(c.mcp_idle_seconds.into()),
    );
    v.insert(
        "mcp_max_servers".into(),
        Value::Integer(c.mcp_max_servers.into()),
    );
    if c.mode == "team" {
        for (k, val) in [
            ("oidc_issuer", &c.oidc_issuer),
            ("oidc_client_id", &c.oidc_client_id),
            ("oidc_redirect_uri", &c.oidc_redirect_uri),
            ("owners", &c.owners),
        ] {
            if !val.trim().is_empty() {
                v.insert(k.into(), Value::String(val.trim().to_string()));
            }
        }
        v.insert("production".into(), Value::Boolean(c.production));
    }
    v
}

/// The MCP server an agent starts: this home's project over stdio.
pub fn mcp_command(layout: &Layout, os: Os, project: &str) -> (PathBuf, Vec<String>) {
    (
        layout.exe(os),
        vec![
            "--home".into(),
            layout.home.display().to_string(),
            "--project".into(),
            project.into(),
            "mcp".into(),
            "serve".into(),
        ],
    )
}

pub fn service_file(os: Os, m: &Machine, layout: &Layout) -> (Option<PathBuf>, String) {
    let exe = layout.exe(os);
    let home = &layout.home;
    match os {
        Os::Linux => (
            Some(
                m.user_home
                    .join(".config/systemd/user")
                    .join(format!("{SERVICE_NAME}.service")),
            ),
            format!(
                "[Unit]\nDescription=EKOS console\n\n[Service]\nExecStart=\"{}\" --home \"{}\" console run\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
                exe.display(),
                home.display()
            ),
        ),
        Os::Macos => (
            Some(
                m.user_home
                    .join("Library/LaunchAgents")
                    .join(format!("{LAUNCHD_LABEL}.plist")),
            ),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LAUNCHD_LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string><string>--home</string><string>{}</string>
    <string>console</string><string>run</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>{}</string>
  <key>StandardErrorPath</key><string>{}</string>
</dict>
</plist>
"#,
                xml(&exe.display().to_string()),
                xml(&home.display().to_string()),
                xml(&home.join("control/console.log").display().to_string()),
                xml(&home.join("control/console.log").display().to_string()),
            ),
        ),
        // The task runs `console start`, which detaches without a window and returns.
        Os::Windows => (
            None,
            format!(
                "\"{}\" --home \"{}\" console start",
                exe.display(),
                home.display()
            ),
        ),
    }
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// What applying `answers` on `m` will do, in order.
pub fn plan(answers: &Answers, m: &Machine) -> Result<Vec<Action>> {
    check(answers)?;
    let layout = Layout::of(answers, m);
    let os = m.os;
    let mut actions = Vec::new();

    if m.package != layout.program_dir && m.package.join("bin").join(os.exe()).is_file() {
        actions.push(Action::CopyPackage {
            from: m.package.clone(),
            to: layout.program_dir.clone(),
        });
    }
    actions.push(Action::WriteConsoleConfig {
        path: layout.home.join("control").join("console.toml"),
        values: console_values(&answers.console, &answers.chat),
    });
    if !answers.chat.provider.is_empty() && !answers.chat.api_key.is_empty() {
        actions.push(Action::StoreKey {
            name: answers.chat.api_key_env.trim().to_string(),
        });
    }
    if answers.console.mode == "team" && !answers.console.oidc_client_secret.is_empty() {
        actions.push(Action::StoreSecret {
            field: "oidc_client_secret".into(),
        });
    }
    if answers.add_to_path {
        actions.push(match os {
            Os::Windows => Action::AddToWindowsPath {
                bin: layout.bin_dir(),
            },
            _ => Action::AddToPath {
                bin: layout.bin_dir(),
                files: m.profile_files(),
            },
        });
    }
    if let Some(p) = &answers.project {
        actions.push(Action::CreateProject {
            name: p.name.clone(),
            sources: p.sources.clone(),
            detect: p.detect,
        });
    }
    if let Some(project) = agent_project(answers) {
        let (command, args) = mcp_command(&layout, os, &project);
        let name = format!("ekos-{project}");
        let a = &answers.agents;
        if a.claude_code {
            actions.push(Action::RegisterClaudeCode {
                name: name.clone(),
                command: command.clone(),
                args: args.clone(),
            });
        }
        if a.claude_desktop {
            actions.push(Action::MergeAgentConfig {
                agent: "Claude Desktop".into(),
                path: m.claude_desktop_config(),
                section: "mcpServers".into(),
                name: name.clone(),
                entry: serde_json::json!({"command": command, "args": args}),
            });
        }
        if a.vscode {
            actions.push(Action::MergeAgentConfig {
                agent: "VS Code".into(),
                path: m.vscode_mcp_config(),
                section: "servers".into(),
                name,
                entry: serde_json::json!({"type": "stdio", "command": command, "args": args}),
            });
        }
    }
    if answers.start.at_login {
        let (path, content) = service_file(os, m, &layout);
        actions.push(Action::InstallService { os, path, content });
    }
    if answers.start.now {
        actions.push(Action::StartConsole);
        if answers.start.open {
            actions.push(Action::OpenConsole);
        }
    }
    Ok(actions)
}

// ── the manifest ─────────────────────────────────────────────────────────────

/// What setup did, so `uninstall` undoes exactly that.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Manifest {
    pub program_dir: PathBuf,
    pub path_files: Vec<PathBuf>,
    pub windows_path: Option<PathBuf>,
    pub service: Option<String>,
    pub service_file: Option<PathBuf>,
    pub agent_configs: Vec<(PathBuf, String, String)>,
    pub claude_code: Vec<String>,
}

fn manifest_path(home: &Path) -> PathBuf {
    home.join("control").join(MANIFEST)
}

pub fn read_manifest(home: &Path) -> Manifest {
    std::fs::read_to_string(manifest_path(home))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn write_manifest(home: &Path, m: &Manifest) -> Result<()> {
    let p = manifest_path(home);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, serde_json::to_string_pretty(m)?)?;
    Ok(())
}

// ── applying ─────────────────────────────────────────────────────────────────

/// One action's outcome.
#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub action: String,
    pub ok: bool,
    pub message: String,
}

/// Effects that reach outside the files setup writes: services, other programs, the browser.
/// Tests replace them so nothing is activated on the machine running the tests.
pub trait Effects {
    /// Run `program` with extra environment variables, feeding `stdin` when given.
    fn run_env(
        &self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        stdin: Option<&str>,
    ) -> Result<String>;
    fn open(&self, url: &str) -> bool;

    fn run(&self, program: &Path, args: &[String], stdin: Option<&str>) -> Result<String> {
        self.run_env(program, args, &[], stdin)
    }
}

pub struct RealEffects;

impl Effects for RealEffects {
    fn run_env(
        &self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        stdin: Option<&str>,
    ) -> Result<String> {
        let mut child = Command::new(program)
            .args(args)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("running {}", program.display()))?;
        if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
            use std::io::Write;
            pipe.write_all(text.as_bytes())?;
        }
        let out = child.wait_with_output()?;
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        if !out.status.success() {
            bail!("{} failed: {}", program.display(), text.trim());
        }
        Ok(text)
    }
    fn open(&self, url: &str) -> bool {
        super::console::open_browser(url)
    }
}

pub fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(from) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(from)?;
        let target = to.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("copying {}", entry.path().display()))?;
        } else if entry.file_type().is_symlink() {
            // The bundled Python links `python3` → `python3.12`; keep links as links.
            #[cfg(unix)]
            {
                let link = std::fs::read_link(entry.path())?;
                let _ = std::fs::remove_file(&target);
                std::os::unix::fs::symlink(link, &target)?;
            }
        }
    }
    Ok(())
}

/// `values` written into `path`, keeping every other key and comment.
pub fn write_console_toml(path: &Path, values: &BTreeMap<String, toml::Value>) -> Result<()> {
    let current = std::fs::read_to_string(path).unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = current
        .parse()
        .with_context(|| format!("{} is not valid TOML", path.display()))?;
    for (k, v) in values {
        let item = match v {
            toml::Value::String(s) => toml_edit::value(s.clone()),
            toml::Value::Integer(i) => toml_edit::value(*i),
            toml::Value::Boolean(b) => toml_edit::value(*b),
            other => toml_edit::value(other.to_string()),
        };
        doc[k.as_str()] = item;
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, doc.to_string())?;
    Ok(())
}

pub fn path_line(bin: &Path) -> String {
    format!("export PATH=\"{}:$PATH\"  {PATH_MARKER}", bin.display())
}

/// Append the PATH line to each file that does not have it yet. Returns the files changed.
pub fn add_path_lines(bin: &Path, files: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let line = path_line(bin);
    let mut changed = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        if text.lines().any(|l| l.trim() == line) {
            continue;
        }
        let sep = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        std::fs::write(f, format!("{text}{sep}{line}\n"))
            .with_context(|| format!("writing {}", f.display()))?;
        changed.push(f.clone());
    }
    Ok(changed)
}

/// Remove every line setup added.
pub fn remove_path_lines(files: &[PathBuf]) -> Result<()> {
    for f in files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        let kept: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim_end().ends_with(PATH_MARKER))
            .collect();
        let mut out = kept.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        std::fs::write(f, out)?;
    }
    Ok(())
}

/// Add `name` under `section` of an agent's JSON config, keeping everything else. The old file
/// is kept as `<file>.bak`. A file that is not plain JSON (VS Code allows comments) is left
/// alone, with the entry to add by hand in the error.
pub fn merge_agent_config(
    path: &Path,
    section: &str,
    name: &str,
    entry: &serde_json::Value,
) -> Result<()> {
    let mut doc = match std::fs::read_to_string(path) {
        Ok(text) if !text.trim().is_empty() => {
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(v) if v.is_object() => {
                    std::fs::write(path.with_extension("json.bak"), &text)?;
                    v
                }
                _ => bail!(
                    "{} is not plain JSON, so it was not changed. Add this under \"{section}\":\n\"{name}\": {}",
                    path.display(),
                    serde_json::to_string_pretty(entry)?
                ),
            }
        }
        _ => serde_json::json!({}),
    };
    let obj = doc.as_object_mut().unwrap();
    let sec = obj
        .entry(section.to_string())
        .or_insert_with(|| serde_json::json!({}));
    let Some(sec) = sec.as_object_mut() else {
        bail!(
            "\"{section}\" in {} is not an object; not changed",
            path.display()
        );
    };
    sec.insert(name.to_string(), entry.clone());
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, serde_json::to_string_pretty(&doc)? + "\n")?;
    Ok(())
}

pub fn remove_agent_entry(path: &Path, section: &str, name: &str) -> Result<()> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    let Ok(mut doc) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Ok(());
    };
    if let Some(sec) = doc.get_mut(section).and_then(|s| s.as_object_mut()) {
        sec.remove(name);
        std::fs::write(path, serde_json::to_string_pretty(&doc)? + "\n")?;
    }
    Ok(())
}

/// The bundled Python and its `app` package, for keychain writes.
fn keychain_command(layout: &Layout, os: Os) -> Result<(PathBuf, Vec<(String, String)>)> {
    let rt = super::console::find_runtime(&layout.exe(os), &|k| {
        std::env::var(k).ok().filter(|v| !v.is_empty())
    })?;
    let mut paths = vec![rt.app_dir.clone()];
    paths.extend(rt.site_packages.clone());
    let pythonpath = std::env::join_paths(&paths)?.to_string_lossy().into_owned();
    Ok((
        rt.python,
        vec![
            ("PYTHONPATH".into(), pythonpath),
            ("PYTHONNOUSERSITE".into(), "1".into()),
        ],
    ))
}

/// Perform `actions`. Stops at the first failure (later steps depend on earlier ones) and
/// reports every step.
pub fn apply(
    actions: &[Action],
    answers: &Answers,
    m: &Machine,
    fx: &dyn Effects,
) -> Result<Vec<Step>> {
    let layout = Layout::of(answers, m);
    let os = m.os;
    let mut manifest = read_manifest(&layout.home);
    manifest.program_dir = layout.program_dir.clone();
    std::fs::create_dir_all(layout.home.join("data"))?;
    let mut steps = Vec::new();
    for action in actions {
        let result: Result<String> = (|| match action {
            Action::CopyPackage { from, to } => {
                for part in ["bin", "console"] {
                    if from.join(part).is_dir() {
                        copy_dir(&from.join(part), &to.join(part))?;
                    }
                }
                Ok(String::new())
            }
            Action::WriteConsoleConfig { path, values } => {
                write_console_toml(path, values)?;
                Ok(String::new())
            }
            Action::StoreSecret { field } => {
                // The bundled Python stores it, so the console and setup share one keychain
                // layout. The value goes on standard input, never on a command line.
                let (python, env) = keychain_command(&layout, os)?;
                let args: Vec<String> = ["-m", "app.keychain_cli", "put", "console", field]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                fx.run_env(
                    &python,
                    &args,
                    &env,
                    Some(&answers.console.oidc_client_secret),
                )
                .map(|_| String::new())
            }
            Action::StoreKey { name } => {
                let (python, env) = keychain_command(&layout, os)?;
                let args: Vec<String> = ["-m", "app.keychain_cli", "put", "env", name]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                fx.run_env(&python, &args, &env, Some(&answers.chat.api_key))
                    .map(|_| String::new())
            }
            Action::AddToPath { bin, files } => {
                let changed = add_path_lines(bin, files)?;
                for f in files {
                    if !manifest.path_files.contains(f) {
                        manifest.path_files.push(f.clone());
                    }
                }
                Ok(if changed.is_empty() {
                    "already there".into()
                } else {
                    "open a new terminal to use it".into()
                })
            }
            Action::AddToWindowsPath { bin } => {
                let script = format!(
                    "$p=[Environment]::GetEnvironmentVariable('Path','User'); \
                     if (-not ($p -split ';' -contains '{b}')) {{ \
                     [Environment]::SetEnvironmentVariable('Path', ($p.TrimEnd(';') + ';{b}').TrimStart(';'), 'User') }}",
                    b = bin.display()
                );
                fx.run(
                    Path::new("powershell"),
                    &["-NoProfile".into(), "-Command".into(), script],
                    None,
                )?;
                manifest.windows_path = Some(bin.clone());
                Ok("open a new terminal to use it".into())
            }
            Action::CreateProject {
                name,
                sources,
                detect,
            } => {
                let mut args = vec![
                    "--home".to_string(),
                    layout.home.display().to_string(),
                    "project".into(),
                    "add".into(),
                    name.clone(),
                ];
                for s in sources {
                    args.push("--source".into());
                    args.push(s.clone());
                }
                if *detect {
                    args.push("--detect".into());
                }
                fx.run(&setup_exe(&layout, m), &args, None)
            }
            Action::RegisterClaudeCode {
                name,
                command,
                args,
            } => {
                let mut a = vec![
                    "mcp".to_string(),
                    "add".into(),
                    "-s".into(),
                    "user".into(),
                    name.clone(),
                    "--".into(),
                    command.display().to_string(),
                ];
                a.extend(args.iter().cloned());
                // `mcp add` refuses a name it has; a second setup run replaces the entry.
                let _ = fx.run(
                    Path::new("claude"),
                    &[
                        "mcp".into(),
                        "remove".into(),
                        "-s".into(),
                        "user".into(),
                        name.clone(),
                    ],
                    None,
                );
                fx.run(Path::new("claude"), &a, None)?;
                if !manifest.claude_code.contains(name) {
                    manifest.claude_code.push(name.clone());
                }
                Ok(String::new())
            }
            Action::MergeAgentConfig {
                path,
                section,
                name,
                entry,
                ..
            } => {
                merge_agent_config(path, section, name, entry)?;
                let record = (path.clone(), section.clone(), name.clone());
                if !manifest.agent_configs.contains(&record) {
                    manifest.agent_configs.push(record);
                }
                Ok("restart that program to pick it up".into())
            }
            Action::InstallService { os, path, content } => {
                install_service(*os, path.as_deref(), content, fx)?;
                manifest.service = Some(format!("{os:?}").to_lowercase());
                manifest.service_file = path.clone();
                Ok(String::new())
            }
            Action::StartConsole => {
                // A login service just installed may already have started it.
                let conf = super::console::server_conf(&layout.home, &|_| None)?;
                if super::console::healthy(&conf.host, conf.port) {
                    return Ok(format!("already running at {}", super::console::url(&conf)));
                }
                fx.run(
                    &layout.exe(os),
                    &[
                        "--home".into(),
                        layout.home.display().to_string(),
                        "console".into(),
                        "start".into(),
                    ],
                    None,
                )
            }
            Action::OpenConsole => fx.run(
                &layout.exe(os),
                &[
                    "--home".into(),
                    layout.home.display().to_string(),
                    "console".into(),
                    "open".into(),
                ],
                None,
            ),
        })();
        let ok = result.is_ok();
        steps.push(Step {
            action: action.describe(),
            ok,
            message: match result {
                Ok(m) => m.trim().to_string(),
                Err(e) => format!("{e:#}"),
            },
        });
        write_manifest(&layout.home, &manifest)?;
        if !ok {
            break;
        }
    }
    Ok(steps)
}

/// The binary that runs follow-up commands: the installed one when it exists, else this one.
fn setup_exe(layout: &Layout, m: &Machine) -> PathBuf {
    let installed = layout.exe(m.os);
    if installed.is_file() {
        installed
    } else {
        m.package.join("bin").join(m.os.exe())
    }
}

fn install_service(os: Os, path: Option<&Path>, content: &str, fx: &dyn Effects) -> Result<()> {
    match os {
        Os::Linux => {
            let path = path.context("no unit file path")?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, content)?;
            let ctl = Path::new("systemctl");
            fx.run(ctl, &["--user".into(), "daemon-reload".into()], None)?;
            fx.run(
                ctl,
                &[
                    "--user".into(),
                    "enable".into(),
                    "--now".into(),
                    format!("{SERVICE_NAME}.service"),
                ],
                None,
            )?;
        }
        Os::Macos => {
            let path = path.context("no plist path")?;
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, content)?;
            let uid = fx.run(Path::new("id"), &["-u".into()], None)?;
            fx.run(
                Path::new("launchctl"),
                &[
                    "bootstrap".into(),
                    format!("gui/{}", uid.trim()),
                    path.display().to_string(),
                ],
                None,
            )?;
        }
        Os::Windows => {
            fx.run(
                Path::new("schtasks"),
                &[
                    "/Create".into(),
                    "/TN".into(),
                    WINDOWS_TASK.into(),
                    "/TR".into(),
                    content.into(),
                    "/SC".into(),
                    "ONLOGON".into(),
                    "/RL".into(),
                    "LIMITED".into(),
                    "/F".into(),
                ],
                None,
            )?;
        }
    }
    Ok(())
}

// ── uninstall ────────────────────────────────────────────────────────────────

pub fn uninstall(home: &Path, purge: bool, m: &Machine, fx: &dyn Effects) -> Result<Vec<Step>> {
    let manifest = read_manifest(home);
    let mut steps = Vec::new();
    let mut step = |what: &str, r: Result<()>| {
        steps.push(Step {
            action: what.into(),
            ok: r.is_ok(),
            message: r.err().map(|e| format!("{e:#}")).unwrap_or_default(),
        })
    };
    step("stop the console", super::console::stop(home));
    if let Some(service) = &manifest.service {
        let r = match service.as_str() {
            "linux" => fx
                .run(
                    Path::new("systemctl"),
                    &[
                        "--user".into(),
                        "disable".into(),
                        "--now".into(),
                        format!("{SERVICE_NAME}.service"),
                    ],
                    None,
                )
                .map(|_| ()),
            "macos" => fx
                .run(Path::new("id"), &["-u".into()], None)
                .and_then(|uid| {
                    fx.run(
                        Path::new("launchctl"),
                        &[
                            "bootout".into(),
                            format!("gui/{}/{LAUNCHD_LABEL}", uid.trim()),
                        ],
                        None,
                    )
                })
                .map(|_| ()),
            _ => fx
                .run(
                    Path::new("schtasks"),
                    &[
                        "/Delete".into(),
                        "/TN".into(),
                        WINDOWS_TASK.into(),
                        "/F".into(),
                    ],
                    None,
                )
                .map(|_| ()),
        };
        if let Some(f) = &manifest.service_file {
            let _ = std::fs::remove_file(f);
        }
        step("remove the login entry", r);
    }
    step(
        "remove the PATH lines",
        remove_path_lines(&manifest.path_files),
    );
    if let Some(bin) = &manifest.windows_path {
        let script = format!(
            "$p=[Environment]::GetEnvironmentVariable('Path','User'); \
             [Environment]::SetEnvironmentVariable('Path', (($p -split ';') | Where-Object {{ $_ -ne '{}' }}) -join ';', 'User')",
            bin.display()
        );
        step(
            "remove EKOS from the user Path",
            fx.run(
                Path::new("powershell"),
                &["-NoProfile".into(), "-Command".into(), script],
                None,
            )
            .map(|_| ()),
        );
    }
    for (path, section, name) in &manifest.agent_configs {
        step(
            &format!("remove {name} from {}", path.display()),
            remove_agent_entry(path, section, name),
        );
    }
    for name in &manifest.claude_code {
        step(
            &format!("remove {name} from Claude Code"),
            fx.run(
                Path::new("claude"),
                &[
                    "mcp".into(),
                    "remove".into(),
                    "-s".into(),
                    "user".into(),
                    name.clone(),
                ],
                None,
            )
            .map(|_| ()),
        );
    }
    let program = if manifest.program_dir.as_os_str().is_empty() {
        home.to_path_buf()
    } else {
        manifest.program_dir.clone()
    };
    for part in ["console", "bin"] {
        let dir = program.join(part);
        if dir.is_dir() {
            let r = if part == "bin" && m.os == Os::Windows {
                // A running .exe cannot be deleted on Windows: remove it once this exits.
                fx.run(
                    Path::new("cmd"),
                    &[
                        "/c".into(),
                        format!(
                            "start \"\" /b cmd /c \"timeout /t 3 >nul & rmdir /s /q \"{}\"\"",
                            dir.display()
                        ),
                    ],
                    None,
                )
                .map(|_| ())
            } else {
                std::fs::remove_dir_all(&dir).map_err(Into::into)
            };
            step(&format!("remove {}", dir.display()), r);
        }
    }
    if purge {
        step(
            &format!("remove the home {}", home.display()),
            std::fs::remove_dir_all(home).map_err(Into::into),
        );
    } else {
        let _ = std::fs::remove_file(manifest_path(home));
    }
    Ok(steps)
}
