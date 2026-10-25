//! `ekos commands --json`: every command, its arguments and what kind of command it is
//! (RFC 0186 §1). The web console builds its run forms from this and decides who may run what
//! from the class; it never runs a command that is not here, with an argument that is not here.
//!
//! The class of each command is a decision, so it is a table, and a test fails when a command
//! exists without a row (or a row names a command that does not exist).

use std::path::PathBuf;

use serde::Serialize;

/// What running a command means, for deciding who may run it from the web.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Class {
    /// Reads the project; changes nothing.
    Read,
    /// Changes the project's state (compiles, writes the ledger, edits its config).
    Write,
    /// A human decision: confirm, reject, resolve, approve. Never by a token or an agent.
    Review,
    /// Destructive or structural: needs an admin and a typed confirmation.
    Admin,
    /// Reaches outside the home: a live database, a public service. Admin and a confirmation.
    External,
    /// Not run from the web: servers, the installer, the console itself, the home as a whole.
    Local,
}

/// (command path, class, may call the configured model)
pub const CLASSES: &[(&str, Class, bool)] = &[
    ("init", Class::Local, false),
    ("project add", Class::Local, false),
    ("project list", Class::Local, false),
    ("project show", Class::Local, false),
    ("project remove", Class::Local, false),
    ("home backup", Class::Local, false),
    ("home restore", Class::Local, false),
    ("setup", Class::Local, false),
    ("uninstall", Class::Local, false),
    ("commands", Class::Local, false),
    ("extensions", Class::Local, false),
    ("console run", Class::Local, false),
    ("console start", Class::Local, false),
    ("console stop", Class::Local, false),
    ("console restart", Class::Local, false),
    ("console status", Class::Local, false),
    ("console open", Class::Local, false),
    ("console logs", Class::Local, false),
    ("source add", Class::Write, false),
    ("source list", Class::Read, false),
    ("source remove", Class::Admin, false),
    ("build", Class::Write, false),
    ("recover", Class::Write, true),
    ("resolve", Class::Write, false),
    ("compile", Class::Write, false),
    ("commit", Class::Write, true),
    ("ledger status", Class::Read, false),
    ("ledger migrate", Class::Admin, false),
    ("ledger repair", Class::Write, false),
    ("ledger timeline", Class::Read, false),
    ("ledger audit", Class::Read, false),
    ("clean", Class::Admin, false),
    ("coverage", Class::Read, false),
    ("freshness", Class::Read, false),
    ("llm routes", Class::Read, false),
    ("llm usage", Class::Read, false),
    ("llm probe", Class::External, true),
    ("doctor", Class::Read, false),
    ("config validate", Class::Read, false),
    ("config preview-scan", Class::Read, false),
    ("config schema", Class::Read, false),
    ("status", Class::Read, false),
    ("query object", Class::Read, false),
    ("query find", Class::Read, false),
    ("query neighbourhood", Class::Read, false),
    ("ask", Class::Read, true),
    ("clickhouse ask", Class::External, true),
    ("ekl", Class::Read, false),
    ("diff", Class::Read, false),
    ("semantics list", Class::Read, false),
    ("semantics confirm", Class::Review, false),
    ("semantics reject", Class::Review, false),
    ("semantics edit", Class::Review, false),
    ("semantics show", Class::Read, false),
    ("semantics gaps", Class::Read, false),
    ("semantics gold-template", Class::Read, false),
    ("semantics eval", Class::Read, false),
    ("conflicts list", Class::Read, false),
    ("conflicts show", Class::Read, false),
    ("conflicts resolve", Class::Review, false),
    ("review list", Class::Read, false),
    ("review show", Class::Read, false),
    ("review scope", Class::Review, false),
    ("review pii", Class::Review, false),
    ("review write-behaviour", Class::Review, false),
    ("signoff show", Class::Read, false),
    ("workflow status", Class::Read, false),
    ("signoff approve", Class::Review, false),
    ("signoff revoke", Class::Review, false),
    ("export linkml", Class::Read, false),
    ("import linkml", Class::Review, false),
    ("identity scan", Class::Write, false),
    ("treasury scan", Class::External, false),
    ("treasury confirm", Class::Review, false),
    ("treasury reject", Class::Review, false),
    ("branch create", Class::Write, false),
    ("branch list", Class::Read, false),
    ("branch merge", Class::Admin, false),
    ("branch delete", Class::Admin, false),
    ("mcp serve", Class::Local, false),
    ("artifact repack", Class::Write, false),
    ("coordinator serve", Class::Local, false),
    ("coordinator status", Class::Local, false),
    ("compile-worker run", Class::Local, false),
    ("query-worker serve", Class::Local, false),
    // Publishing to X needs a person at a terminal approving each post (RFC 0030).
    ("marketing publish", Class::Local, true),
    ("marketing post", Class::Local, false),
    ("docs generate", Class::Write, true),
    ("graph export", Class::Read, false),
    ("dbt generate", Class::Write, false),
    ("architecture investigate", Class::Write, true),
    ("architecture diff", Class::Read, false),
    ("simulate", Class::Write, true),
    ("replay", Class::Read, false),
    ("eval run", Class::Write, true),
    ("eval regrade", Class::Write, false),
    ("eval history", Class::Read, false),
    ("migrate init", Class::Write, false),
    ("migrate discover", Class::External, false),
    ("migrate profile", Class::External, false),
    ("migrate assess", Class::External, false),
    ("migrate map", Class::Write, false),
    ("migrate load", Class::External, false),
    ("migrate validate", Class::External, false),
    ("migrate review", Class::Review, false),
    ("migrate approve", Class::Review, false),
    ("migrate reject", Class::Review, false),
    ("migrate report", Class::Write, false),
    ("migrate lower", Class::Read, false),
    ("migrate v5", Class::External, false),
    ("migrate reconstruct", Class::External, true),
    ("migrate check-candidate", Class::Read, false),
    ("migrate status", Class::Read, false),
    ("session note", Class::Write, false),
    ("session status", Class::Read, false),
    ("session commit", Class::Write, false),
    ("session recall", Class::Read, false),
    ("session brief", Class::Read, false),
    ("session review", Class::Review, false),
    ("session purge", Class::Admin, false),
    ("session capture", Class::Write, false),
    ("session extract", Class::Write, true),
    ("session eval", Class::Read, false),
    ("session fingerprint-noise", Class::Read, false),
];

/// Arguments the console sets itself and a person never does: which project, which home, which
/// config, which read scope.
pub const CONSOLE_MANAGED: &[&str] = &[
    "config",
    "home",
    "project",
    "tenant",
    "read_scope",
    "workspace",
    "help",
    "version",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ArgSpec {
    pub id: String,
    /// `--long`, or `None` for a positional argument.
    pub long: Option<String>,
    /// 1-based position for a positional argument.
    pub index: Option<usize>,
    /// `flag` (no value), `value`, or `path` (a file or directory on the server).
    pub kind: &'static str,
    pub multiple: bool,
    pub required: bool,
    pub possible: Vec<String>,
    pub default: Vec<String>,
    pub value_name: Option<String>,
    pub help: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Entry {
    pub path: Vec<String>,
    pub about: String,
    pub class: Class,
    pub metered: bool,
    pub args: Vec<ArgSpec>,
}

fn arg_spec(a: &clap::Arg) -> ArgSpec {
    use clap::ArgAction;
    let flag = matches!(
        a.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count
    );
    let path = a.get_value_parser().type_id() == std::any::TypeId::of::<PathBuf>();
    let multiple = matches!(a.get_action(), ArgAction::Append)
        || a.get_num_args().is_some_and(|n| n.max_values() > 1);
    ArgSpec {
        id: a.get_id().to_string(),
        long: a.get_long().map(str::to_string),
        index: a.get_index(),
        kind: if flag {
            "flag"
        } else if path {
            "path"
        } else {
            "value"
        },
        multiple,
        required: a.is_required_set(),
        possible: a
            .get_possible_values()
            .iter()
            .map(|v| v.get_name().to_string())
            .collect(),
        default: a
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect(),
        value_name: a
            .get_value_names()
            .and_then(|n| n.first())
            .map(|n| n.to_string()),
        help: a.get_help().map(|h| h.to_string()).unwrap_or_default(),
    }
}

/// Every leaf command under `root`, with its path.
pub fn leaves(root: &clap::Command) -> Vec<(Vec<String>, clap::Command)> {
    fn walk(
        c: &clap::Command,
        path: &mut Vec<String>,
        out: &mut Vec<(Vec<String>, clap::Command)>,
    ) {
        if c.get_subcommands().next().is_none() && !path.is_empty() {
            out.push((path.clone(), c.clone()));
        }
        // `build()` adds a generated `help` subcommand to every group; it is not a command.
        for s in c.get_subcommands().filter(|s| s.get_name() != "help") {
            path.push(s.get_name().to_string());
            walk(s, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

fn class_of(path: &str) -> Option<(Class, bool)> {
    class_in(CLASSES, path)
}

fn class_in(table: &[(&str, Class, bool)], path: &str) -> Option<(Class, bool)> {
    table
        .iter()
        .find(|(p, _, _)| *p == path)
        .map(|(_, c, m)| (*c, *m))
}

/// The catalogue. A command without a row is left out (and fails the guard test).
pub fn catalogue(root: &clap::Command) -> Vec<Entry> {
    catalogue_with(root, &[])
}

/// The catalogue with extra rows for commands an extension added (RFC 0191). The built-in table
/// is consulted first, so an extension cannot reclassify a built-in command.
pub fn catalogue_with(root: &clap::Command, extra: &[(&str, Class, bool)]) -> Vec<Entry> {
    leaves(root)
        .into_iter()
        .filter_map(|(path, cmd)| {
            let key = path.join(" ");
            let (class, metered) = class_of(&key).or_else(|| class_in(extra, &key))?;
            let args = cmd
                .get_arguments()
                .filter(|a| !a.is_global_set() && !CONSOLE_MANAGED.contains(&a.get_id().as_str()))
                .filter(|a| !a.is_hide_set())
                .map(arg_spec)
                .collect();
            Some(Entry {
                path,
                about: cmd.get_about().map(|a| a.to_string()).unwrap_or_default(),
                class,
                metered,
                args,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> clap::Command {
        let mut c = crate::app::cli_command();
        c.build();
        c
    }

    #[test]
    fn every_command_has_a_class_and_every_class_a_command() {
        let paths: Vec<String> = leaves(&root()).iter().map(|(p, _)| p.join(" ")).collect();
        let missing: Vec<&String> = paths.iter().filter(|p| class_of(p).is_none()).collect();
        assert!(
            missing.is_empty(),
            "commands without a class in catalogue::CLASSES (decide who may run them from the \
             web): {missing:?}"
        );
        let stale: Vec<&str> = CLASSES
            .iter()
            .map(|(p, _, _)| *p)
            .filter(|p| !paths.iter().any(|q| q == p))
            .collect();
        assert!(
            stale.is_empty(),
            "classes for commands that do not exist: {stale:?}"
        );
    }

    #[test]
    fn human_decisions_are_review() {
        // The same commands `commands/mcp.rs` must never reach (CLAUDE.md, human-only).
        for p in [
            "semantics confirm",
            "semantics reject",
            "semantics edit",
            "conflicts resolve",
            "import linkml",
            "migrate approve",
            "migrate reject",
            "session review",
            "treasury confirm",
            "treasury reject",
        ] {
            assert_eq!(class_of(p).unwrap().0, Class::Review, "{p}");
        }
    }

    #[test]
    fn arguments_are_described() {
        let cat = catalogue(&root());
        let find = cat.iter().find(|e| e.path == ["query", "find"]).unwrap();
        assert_eq!(find.class, Class::Read);
        let mode = find.args.iter().find(|a| a.id == "mode").unwrap();
        assert_eq!(mode.long.as_deref(), Some("mode"));
        assert_eq!(mode.default, ["lexical"], "{mode:?}");
        let text = find.args.iter().find(|a| a.index == Some(1)).unwrap();
        assert_eq!(text.kind, "value");
        // The console sets these itself.
        for e in &cat {
            for a in &e.args {
                assert!(
                    !CONSOLE_MANAGED.contains(&a.id.as_str()),
                    "{:?} {}",
                    e.path,
                    a.id
                );
            }
        }
        let restore = cat.iter().find(|e| e.path == ["import", "linkml"]).unwrap();
        assert!(restore.args.iter().any(|a| a.kind == "path"), "{restore:?}");
        let commit = cat.iter().find(|e| e.path == ["commit"]).unwrap();
        assert!(commit.metered);
        assert!(
            commit
                .args
                .iter()
                .any(|a| a.id == "yes" && a.kind == "flag")
        );
    }
}
