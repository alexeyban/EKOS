//! RFC 0191: an extension adds commands of its own. They parse with the global flags applied,
//! dispatch to the extension with the loaded config, and appear in the command catalogue with
//! their classes; a name that is taken, or a leaf without a class, stops the binary.
//!
//! `main_with_args` installs the global logger, so this file calls it exactly once.

use async_trait::async_trait;
use ekos::commands::catalogue::{Class, catalogue, catalogue_with, leaves};
use ekos::extension::{CommandContext, EkosExtension, Extensions};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// (command, subcommand, --flag value, config's workspace root, cwd, config path)
type Run = (String, String, String, String, String, String);

#[derive(Default)]
struct Seen {
    run: Mutex<Option<Run>>,
}

struct Lake {
    seen: Arc<Seen>,
    classes: Vec<(&'static str, Class, bool)>,
    name: &'static str,
}

impl Lake {
    fn new(seen: Arc<Seen>) -> Self {
        Self {
            seen,
            classes: vec![
                ("lake status", Class::Read, false),
                ("lake apply", Class::Write, false),
            ],
            name: "lake",
        }
    }
}

#[async_trait(?Send)]
impl EkosExtension for Lake {
    fn name(&self) -> &'static str {
        "lake-test"
    }

    fn commands(&self) -> Vec<clap::Command> {
        vec![
            clap::Command::new(self.name)
                .about("Test command group (RFC 0191)")
                .subcommand_required(true)
                .subcommand(
                    clap::Command::new("status").about("Read something").arg(
                        clap::Arg::new("flag")
                            .long("flag")
                            .default_value("unset")
                            .help("A value to echo"),
                    ),
                )
                .subcommand(clap::Command::new("apply").about("Write something")),
        ]
    }

    fn command_classes(&self) -> Vec<(&'static str, Class, bool)> {
        self.classes.clone()
    }

    async fn run_command(
        &self,
        name: &str,
        matches: &clap::ArgMatches,
        ctx: &CommandContext<'_>,
    ) -> anyhow::Result<()> {
        let (sub, sub_matches) = matches.subcommand().expect("subcommand_required");
        let flag = sub_matches
            .try_get_one::<String>("flag")
            .ok()
            .flatten()
            .cloned()
            .unwrap_or_default();
        *self.seen.run.lock().unwrap() = Some((
            name.to_string(),
            sub.to_string(),
            flag,
            ctx.config.workspace.root.display().to_string(),
            ctx.cwd.display().to_string(),
            ctx.config_path.display().to_string(),
        ));
        Ok(())
    }
}

#[tokio::test]
async fn an_extension_command_parses_and_dispatches_with_the_loaded_config() {
    let tmp = TempDir::new().unwrap();
    let config_path = tmp.path().join("custom.toml");
    std::fs::write(&config_path, "[workspace]\nroot = \"from-the-config\"\n").unwrap();
    let seen = Arc::new(Seen::default());
    let ext = Extensions::new(vec![Arc::new(Lake::new(seen.clone()))]);

    ekos::app::main_with_args(
        ext,
        [
            "ekos",
            "--config",
            config_path.to_str().unwrap(),
            "lake",
            "status",
            "--flag",
            "hello",
        ],
    )
    .await
    .unwrap();

    let (name, sub, flag, root, cwd, path) = seen.run.lock().unwrap().clone().unwrap();
    assert_eq!(
        (name.as_str(), sub.as_str(), flag.as_str()),
        ("lake", "status", "hello")
    );
    assert_eq!(
        root, "from-the-config",
        "the extension gets the config --config named"
    );
    assert_eq!(path, config_path.display().to_string());
    assert_eq!(cwd, std::env::current_dir().unwrap().display().to_string());
}

#[test]
fn extension_commands_are_in_the_catalogue_with_their_classes() {
    let ext = Extensions::new(vec![Arc::new(Lake::new(Arc::default()))]);
    let mut root = ext.extend_cli(ekos::app::cli_command()).unwrap();
    root.build();
    let cat = catalogue_with(&root, &ext.command_classes());
    let status = cat.iter().find(|e| e.path == ["lake", "status"]).unwrap();
    assert_eq!(status.class, Class::Read);
    assert!(status.args.iter().any(|a| a.id == "flag"));
    let apply = cat.iter().find(|e| e.path == ["lake", "apply"]).unwrap();
    assert_eq!(apply.class, Class::Write);

    // Every leaf of the extended tree is classified.
    let n_leaves = leaves(&root).len();
    assert_eq!(
        cat.len(),
        n_leaves,
        "a leaf of the extended tree has no class"
    );
}

#[test]
fn without_extensions_the_tree_and_catalogue_are_unchanged() {
    let mut plain = ekos::app::cli_command();
    plain.build();
    let mut extended = Extensions::none()
        .extend_cli(ekos::app::cli_command())
        .unwrap();
    extended.build();
    let a = serde_json::to_string(&catalogue(&plain)).unwrap();
    let b = serde_json::to_string(&catalogue_with(&extended, &[])).unwrap();
    assert_eq!(a, b);
}

#[test]
fn a_taken_name_stops_the_binary() {
    for taken in ["build", "migrate"] {
        let mut lake = Lake::new(Arc::default());
        lake.name = taken;
        let ext = Extensions::new(vec![Arc::new(lake)]);
        let err = ext
            .extend_cli(ekos::app::cli_command())
            .unwrap_err()
            .to_string();
        assert!(err.contains(&format!("`{taken}`")), "{err}");
    }
    // Two extensions adding the same command collide too.
    let ext = Extensions::new(vec![
        Arc::new(Lake::new(Arc::default())),
        Arc::new(Lake::new(Arc::default())),
    ]);
    let err = ext
        .extend_cli(ekos::app::cli_command())
        .unwrap_err()
        .to_string();
    assert!(err.contains("already exists"), "{err}");
}

#[test]
fn a_leaf_without_a_class_stops_the_binary() {
    let mut lake = Lake::new(Arc::default());
    lake.classes.retain(|(p, _, _)| *p != "lake apply");
    let ext = Extensions::new(vec![Arc::new(lake)]);
    let err = ext
        .extend_cli(ekos::app::cli_command())
        .unwrap_err()
        .to_string();
    assert!(err.contains("lake apply"), "{err}");
}

#[test]
fn an_extension_cannot_reclassify_a_built_in() {
    let root = {
        let mut r = ekos::app::cli_command();
        r.build();
        r
    };
    let cat = catalogue_with(&root, &[("migrate approve", Class::Read, false)]);
    let approve = cat
        .iter()
        .find(|e| e.path == ["migrate", "approve"])
        .unwrap();
    assert_eq!(approve.class, Class::Review);
}
