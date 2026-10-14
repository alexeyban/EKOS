//! Tests for `ekos setup` / `ekos uninstall` (RFC 0185 §3–4). Nothing here starts a service,
//! runs another program or opens a browser: `Fake` records what would have run.

use super::setup::*;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// One recorded command: program name, arguments, extra environment, standard input.
type Ran = (String, Vec<String>, Vec<(String, String)>, Option<String>);

#[derive(Default)]
struct Fake {
    ran: RefCell<Vec<Ran>>,
    fail: Option<&'static str>,
}

impl Effects for Fake {
    fn run_env(
        &self,
        program: &Path,
        args: &[String],
        env: &[(String, String)],
        stdin: Option<&str>,
    ) -> anyhow::Result<String> {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        self.ran.borrow_mut().push((
            name.clone(),
            args.to_vec(),
            env.to_vec(),
            stdin.map(str::to_string),
        ));
        if self.fail == Some(name.as_str()) {
            anyhow::bail!("{name} failed");
        }
        Ok(if name == "id" {
            "501\n".into()
        } else {
            String::new()
        })
    }
    fn open(&self, _: &str) -> bool {
        true
    }
}

impl Fake {
    fn programs(&self) -> Vec<String> {
        self.ran.borrow().iter().map(|r| r.0.clone()).collect()
    }
}

fn machine(root: &Path, os: Os) -> Machine {
    let user_home = root.join("user");
    std::fs::create_dir_all(&user_home).unwrap();
    let package = root.join("package");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::write(package.join("bin").join("ekos"), "bin").unwrap();
    std::fs::write(package.join("bin").join("ekos.exe"), "bin").unwrap();
    std::fs::create_dir_all(package.join("console").join("app")).unwrap();
    std::fs::write(package.join("console").join("app").join("main.py"), "").unwrap();
    Machine {
        os,
        user_home: user_home.clone(),
        appdata: Some(user_home.join("AppData").join("Roaming")),
        package,
        shell: Some("/bin/bash".into()),
    }
}

fn local_answers() -> Answers {
    Answers {
        add_to_path: true,
        project: Some(ProjectAnswers {
            name: "shop".into(),
            sources: vec!["api=/repos/api".into()],
            detect: true,
        }),
        agents: AgentAnswers {
            claude_desktop: true,
            vscode: true,
            ..Default::default()
        },
        start: StartAnswers {
            at_login: true,
            now: true,
            open: true,
        },
        ..Default::default()
    }
}

fn kinds(actions: &[Action]) -> Vec<String> {
    actions
        .iter()
        .map(|a| {
            serde_json::to_value(a).unwrap()["kind"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[test]
fn a_local_install_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path(), Os::Linux);
    let actions = plan(&local_answers(), &m).unwrap();
    assert_eq!(
        kinds(&actions),
        [
            "copy_package",
            "write_console_config",
            "add_to_path",
            "create_project",
            "merge_agent_config",
            "merge_agent_config",
            "install_service",
            "start_console",
            "open_console"
        ]
    );
    let Action::CopyPackage { to, .. } = &actions[0] else {
        unreachable!()
    };
    assert_eq!(to, &m.user_home.join(".ekos"));
    // Every step can be said in a line.
    assert!(actions.iter().all(|a| !a.describe().is_empty()));
}

#[test]
fn nothing_is_copied_when_running_from_the_program_directory() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path(), Os::Linux);
    let answers = Answers {
        program_dir: m.package.display().to_string(),
        ..local_answers()
    };
    assert!(!kinds(&plan(&answers, &m).unwrap()).contains(&"copy_package".to_string()));
}

#[test]
fn answers_that_cannot_work_are_refused() {
    let base = local_answers();
    let cases: Vec<(Answers, &str)> = vec![
        (
            Answers {
                console: ConsoleAnswers {
                    oidc_issuer: "https://id".into(),
                    ..Default::default()
                },
                ..base.clone()
            },
            "without a provider",
        ),
        (
            Answers {
                console: ConsoleAnswers {
                    host: "0.0.0.0".into(),
                    ..Default::default()
                },
                ..base.clone()
            },
            "this machine only",
        ),
        (
            Answers {
                console: ConsoleAnswers {
                    mode: "team".into(),
                    production: true,
                    oidc_issuer: "https://id".into(),
                    ..Default::default()
                },
                ..base.clone()
            },
            "owner",
        ),
        (
            Answers {
                project: None,
                ..base.clone()
            },
            "choose the project",
        ),
        (
            Answers {
                console: ConsoleAnswers {
                    mode: "cloud".into(),
                    ..Default::default()
                },
                ..base.clone()
            },
            "`local` or `team`",
        ),
        (
            Answers {
                project: Some(ProjectAnswers {
                    name: "../x".into(),
                    sources: vec!["/a".into()],
                    detect: false,
                }),
                ..base.clone()
            },
            "",
        ),
    ];
    for (answers, says) in cases {
        let err = check(&answers).unwrap_err().to_string();
        assert!(err.contains(says), "{err}");
    }
}

#[test]
fn applying_writes_config_path_agents_and_service() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path(), Os::Linux);
    let answers = local_answers();
    let home = m.user_home.join(".ekos");
    std::fs::create_dir_all(home.join("control")).unwrap();
    std::fs::write(
        home.join("control").join("console.toml"),
        "# my notes\nmcp_port_base = 7500\n",
    )
    .unwrap();
    // Claude Desktop already has another server.
    let desktop = m.claude_desktop_config();
    std::fs::create_dir_all(desktop.parent().unwrap()).unwrap();
    std::fs::write(
        &desktop,
        r#"{"mcpServers": {"other": {"command": "x"}}, "theme": "dark"}"#,
    )
    .unwrap();

    let fake = Fake::default();
    let actions = plan(&answers, &m).unwrap();
    let steps = apply(&actions, &answers, &m, &fake).unwrap();
    assert!(steps.iter().all(|s| s.ok), "{steps:#?}");

    // The program is copied; the console config keeps what was there.
    assert!(home.join("bin").join("ekos").is_file());
    assert!(home.join("console").join("app").join("main.py").is_file());
    let toml = std::fs::read_to_string(home.join("control").join("console.toml")).unwrap();
    assert!(toml.contains("# my notes") && toml.contains("mcp_port_base = 7500"));
    assert!(toml.contains("mode = \"local\"") && toml.contains("port = 8000"));
    assert!(!toml.contains("oidc"), "{toml}");

    // PATH, once, in the profile.
    let profile = std::fs::read_to_string(m.user_home.join(".profile")).unwrap();
    assert_eq!(profile.matches(PATH_MARKER).count(), 1);
    assert!(profile.contains(&home.join("bin").display().to_string()));

    // Agents: the other server and setting are kept, a backup exists, the entry runs this
    // home's project over stdio.
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&desktop).unwrap()).unwrap();
    assert_eq!(doc["mcpServers"]["other"]["command"], "x");
    assert_eq!(doc["theme"], "dark");
    let args = doc["mcpServers"]["ekos-shop"]["args"].as_array().unwrap();
    let args: Vec<&str> = args.iter().map(|a| a.as_str().unwrap()).collect();
    assert_eq!(
        args,
        [
            "--home",
            home.to_str().unwrap(),
            "--project",
            "shop",
            "mcp",
            "serve"
        ]
    );
    assert!(desktop.with_extension("json.bak").is_file());
    let vscode: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(m.vscode_mcp_config()).unwrap()).unwrap();
    assert_eq!(vscode["servers"]["ekos-shop"]["type"], "stdio");

    // The service file, and what would have activated it.
    let unit = m
        .user_home
        .join(".config/systemd/user/ekos-console.service");
    let text = std::fs::read_to_string(&unit).unwrap();
    assert!(text.contains("console run") && text.contains(home.to_str().unwrap()));
    let programs = fake.programs();
    assert!(programs.contains(&"systemctl".to_string()), "{programs:?}");
    assert!(programs.iter().filter(|p| *p == "ekos").count() >= 2); // project add, console …

    // A second run adds nothing twice.
    let steps = apply(&actions, &answers, &m, &Fake::default()).unwrap();
    assert!(steps.iter().all(|s| s.ok));
    let profile = std::fs::read_to_string(m.user_home.join(".profile")).unwrap();
    assert_eq!(profile.matches(PATH_MARKER).count(), 1);

    let manifest = read_manifest(&home);
    assert_eq!(manifest.service.as_deref(), Some("linux"));
    assert_eq!(manifest.agent_configs.len(), 2);
}

#[test]
fn a_config_with_comments_is_not_touched() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    let text = "{\n  // mine\n  \"servers\": {}\n}\n";
    std::fs::write(&path, text).unwrap();
    let err = merge_agent_config(&path, "servers", "ekos-shop", &serde_json::json!({"a": 1}))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("not plain JSON") && err.contains("\"ekos-shop\""),
        "{err}"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn a_secret_goes_on_standard_input_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = machine(dir.path(), Os::Linux);
    // A runtime for the keychain command.
    let py = m.package.join("console").join("python").join("bin");
    std::fs::create_dir_all(&py).unwrap();
    std::fs::write(py.join("python3"), "").unwrap();
    m.package = m.package.clone();
    let answers = Answers {
        program_dir: m.package.display().to_string(),
        console: ConsoleAnswers {
            mode: "team".into(),
            oidc_issuer: "https://id.example.com".into(),
            oidc_client_id: "ekos".into(),
            oidc_client_secret: "SECRET-VALUE-1".into(),
            owners: "a@b.c".into(),
            ..Default::default()
        },
        start: StartAnswers {
            now: false,
            ..Default::default()
        },
        ..Default::default()
    };
    let fake = Fake::default();
    let actions = plan(&answers, &m).unwrap();
    assert!(kinds(&actions).contains(&"store_secret".to_string()));
    let steps = apply(&actions, &answers, &m, &fake).unwrap();
    assert!(steps.iter().all(|s| s.ok), "{steps:#?}");
    let ran = fake.ran.borrow();
    let (prog, args, env, stdin) = ran.iter().find(|r| r.0 == "python3").unwrap();
    assert_eq!(prog, "python3");
    assert_eq!(
        args.join(" "),
        "-m app.keychain_cli put console oidc_client_secret"
    );
    assert_eq!(stdin.as_deref(), Some("SECRET-VALUE-1"));
    assert!(env.iter().any(|(k, _)| k == "PYTHONPATH"));
    let toml = std::fs::read_to_string(m.user_home.join(".ekos/control/console.toml")).unwrap();
    assert!(!toml.contains("SECRET-VALUE-1"));
    assert!(toml.contains("oidc_issuer = \"https://id.example.com\""));
    let manifest = std::fs::read_to_string(m.user_home.join(".ekos/control/install.json")).unwrap();
    assert!(!manifest.contains("SECRET-VALUE-1"));
}

#[test]
fn a_failed_step_stops_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path(), Os::Linux);
    let fake = Fake {
        fail: Some("systemctl"),
        ..Default::default()
    };
    let answers = local_answers();
    let steps = apply(&plan(&answers, &m).unwrap(), &answers, &m, &fake).unwrap();
    let last = steps.last().unwrap();
    assert!(
        !last.ok && last.message.contains("systemctl failed"),
        "{last:?}"
    );
    assert!(!steps.iter().any(|s| s.action == "start the console"));
}

#[test]
fn service_entries_for_macos_and_windows() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path(), Os::Macos);
    let layout = Layout::of(&local_answers(), &m);
    let (path, plist) = service_file(Os::Macos, &m, &layout);
    assert!(
        path.unwrap()
            .ends_with("Library/LaunchAgents/com.ekos.console.plist")
    );
    assert!(plist.contains("<string>console</string><string>run</string>"));
    assert!(plist.contains("<key>RunAtLoad</key><true/>"));
    let (path, task) = service_file(Os::Windows, &m, &layout);
    assert!(path.is_none());
    assert!(task.contains("ekos.exe\" --home") && task.ends_with("console start"));
    // macOS profiles include zsh's.
    assert!(m.profile_files().iter().any(|p| p.ends_with(".zprofile")));
}

#[test]
fn uninstall_undoes_what_setup_did_and_keeps_the_home() {
    let dir = tempfile::tempdir().unwrap();
    let m = machine(dir.path(), Os::Linux);
    let answers = local_answers();
    let home = m.user_home.join(".ekos");
    apply(&plan(&answers, &m).unwrap(), &answers, &m, &Fake::default()).unwrap();
    std::fs::create_dir_all(home.join("data/default/shop")).unwrap();
    let desktop = m.claude_desktop_config();

    let fake = Fake::default();
    let steps = uninstall(&home, false, &m, &fake).unwrap();
    assert!(steps.iter().all(|s| s.ok), "{steps:#?}");
    assert!(!home.join("bin").exists() && !home.join("console").exists());
    assert!(home.join("data/default/shop").is_dir(), "the home is kept");
    let profile = std::fs::read_to_string(m.user_home.join(".profile")).unwrap();
    assert!(!profile.contains(PATH_MARKER));
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&desktop).unwrap()).unwrap();
    assert!(doc["mcpServers"].get("ekos-shop").is_none());
    assert!(
        !m.user_home
            .join(".config/systemd/user/ekos-console.service")
            .exists()
    );
    assert!(
        fake.ran
            .borrow()
            .iter()
            .any(|r| r.0 == "systemctl" && r.1.contains(&"disable".to_string()))
    );

    // --purge removes the home too.
    apply(&plan(&answers, &m).unwrap(), &answers, &m, &Fake::default()).unwrap();
    uninstall(&home, true, &m, &Fake::default()).unwrap();
    assert!(!home.exists());
}

#[test]
fn answers_read_from_toml() {
    let text = r#"
home = "/srv/ekos"
add_to_path = false
[console]
mode = "team"
port = 9000
oidc_issuer = "https://id.example.com"
[project]
name = "shop"
sources = ["/repos/shop"]
[start]
now = false
"#;
    let a: Answers = toml::from_str(text).unwrap();
    assert_eq!(a.console.port, 9000);
    assert_eq!(a.console.mcp_max_servers, 8, "defaults fill the rest");
    assert!(!a.start.now && a.start.open);
    assert!(
        toml::from_str::<Answers>("nope = 1").is_err(),
        "unknown keys are refused"
    );
    let _ = PathBuf::new();
}
