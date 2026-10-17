//! `ekos extensions --json` (RFC 0198): every extension's console pages, checked against the
//! command catalogue so that a panel can only ever run a read command with fixed flags.

use crate::commands::catalogue::{Class, Entry};
use crate::extension::{ConsolePage, Extensions, View};
use anyhow::{Result, bail};
use serde_json::{Value, json};

/// The catalogue entry whose path is the longest prefix of `argv`, and the rest of `argv`.
pub fn entry_for<'a, 'b>(
    catalogue: &'a [Entry],
    argv: &'b [&'b str],
) -> Option<(&'a Entry, &'b [&'b str])> {
    catalogue
        .iter()
        .filter(|e| e.path.len() <= argv.len() && e.path.iter().zip(argv).all(|(p, a)| p == a))
        .max_by_key(|e| e.path.len())
        .map(|e| (e, &argv[e.path.len()..]))
}

fn check_pointer(p: &str, what: &str) -> Result<()> {
    if !(p.is_empty() || p.starts_with('/')) {
        bail!("{what}: `{p}` is not a JSON pointer (empty, or starting with `/`)");
    }
    Ok(())
}

/// Why `page` cannot be served, if it cannot.
pub fn check_page(ext: &str, page: &ConsolePage, catalogue: &[Entry]) -> Result<()> {
    let at = format!("extension {ext}, page {}", page.id);
    if page.id.is_empty()
        || !page
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        bail!("{at}: an id is lowercase letters, digits and `-`");
    }
    let mut seen = std::collections::BTreeSet::new();
    for panel in &page.panels {
        let at = format!("{at}, panel {}", panel.id);
        if !seen.insert(panel.id) {
            bail!("{at}: two panels with this id");
        }
        let Some((entry, rest)) = entry_for(catalogue, &panel.command) else {
            bail!(
                "{at}: `{}` is not a catalogue command",
                panel.command.join(" ")
            );
        };
        if entry.class != Class::Read {
            bail!(
                "{at}: `{}` is {:?}; a panel only runs read commands",
                entry.path.join(" "),
                entry.class
            );
        }
        if let Some(required) = entry.args.iter().find(|a| a.required) {
            bail!(
                "{at}: `{}` needs `{}`, which a panel cannot give",
                entry.path.join(" "),
                required.id
            );
        }
        for flag in rest {
            let known = flag.strip_prefix("--").is_some_and(|f| {
                entry
                    .args
                    .iter()
                    .any(|a| a.kind == "flag" && a.long.as_deref() == Some(f))
            });
            if !known {
                bail!("{at}: `{flag}` is not a flag of `{}`", entry.path.join(" "));
            }
        }
        match &panel.view {
            View::Table { rows, columns } => {
                check_pointer(rows, &at)?;
                for c in columns {
                    check_pointer(c.key, &at)?;
                }
            }
            View::Fields { fields } => {
                for f in fields {
                    check_pointer(f.key, &at)?;
                }
            }
            View::Text { pointer } => check_pointer(pointer, &at)?,
        }
    }
    for action in &page.actions {
        let at = format!("{at}, action {}", action.label);
        let Some(entry) = catalogue.iter().find(|e| e.path == action.command) else {
            bail!(
                "{at}: `{}` is not a catalogue command",
                action.command.join(" ")
            );
        };
        if entry.class == Class::Local {
            bail!("{at}: `{}` is not run from the web", entry.path.join(" "));
        }
        if let Some(required) = entry.args.iter().find(|a| a.required) {
            bail!(
                "{at}: `{}` needs `{}`, which an action cannot give",
                entry.path.join(" "),
                required.id
            );
        }
    }
    Ok(())
}

/// The JSON `ekos extensions --json` prints; fails on the first page that breaks a rule.
pub fn describe(extensions: &Extensions, catalogue: &[Entry]) -> Result<Value> {
    let mut out = vec![];
    let mut ids = std::collections::BTreeSet::new();
    for e in extensions.iter() {
        let pages = e.console_pages();
        for p in &pages {
            check_page(e.name(), p, catalogue)?;
            if !ids.insert(p.id) {
                bail!("extension {}: page id `{}` is taken", e.name(), p.id);
            }
        }
        out.push(json!({"name": e.name(), "logic_version": e.logic_version(), "pages": pages}));
    }
    Ok(json!({"schema_version": 1, "version": env!("CARGO_PKG_VERSION"), "extensions": out}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::catalogue::ArgSpec;
    use crate::extension::{Action, Column, Panel};

    fn arg(id: &str, kind: &'static str, required: bool) -> ArgSpec {
        ArgSpec {
            id: id.into(),
            long: Some(id.into()),
            index: None,
            kind,
            multiple: false,
            required,
            possible: vec![],
            default: vec![],
            value_name: None,
            help: String::new(),
        }
    }

    fn entry(path: &[&str], class: Class, args: Vec<ArgSpec>) -> Entry {
        Entry {
            path: path.iter().map(|s| s.to_string()).collect(),
            about: String::new(),
            class,
            metered: false,
            args,
        }
    }

    fn catalogue() -> Vec<Entry> {
        vec![
            entry(
                &["x", "status"],
                Class::Read,
                vec![arg("json", "flag", false)],
            ),
            entry(&["x", "sync"], Class::External, vec![]),
            entry(
                &["x", "draft"],
                Class::External,
                vec![arg("file", "value", true)],
            ),
            entry(&["x", "write"], Class::Write, vec![]),
            entry(&["console"], Class::Local, vec![]),
        ]
    }

    fn page(command: Vec<&'static str>, actions: Vec<Vec<&'static str>>) -> ConsolePage {
        ConsolePage {
            id: "x",
            title: "X",
            description: "",
            panels: vec![Panel {
                id: "p",
                title: "P",
                command,
                view: View::Table {
                    rows: "/rows",
                    columns: vec![Column::badge("/state", "State")],
                },
                empty: "nothing",
            }],
            actions: actions
                .into_iter()
                .map(|command| Action {
                    label: "go",
                    command,
                })
                .collect(),
        }
    }

    #[test]
    fn a_read_panel_and_runnable_actions_pass() {
        let p = page(
            vec!["x", "status", "--json"],
            vec![vec!["x", "sync"], vec!["x", "write"]],
        );
        check_page("x", &p, &catalogue()).unwrap();
    }

    #[test]
    fn a_panel_never_runs_anything_but_a_read_command_with_its_own_flags() {
        let cat = catalogue();
        for (argv, why) in [
            (vec!["x", "sync"], "read commands"),
            (vec!["x", "write"], "read commands"),
            (vec!["x", "status", "--output=/etc"], "not a flag"),
            (vec!["x", "status", "/etc/passwd"], "not a flag"),
            (vec!["x", "nothing"], "not a catalogue command"),
        ] {
            let err = check_page("x", &page(argv.clone(), vec![]), &cat).unwrap_err();
            assert!(err.to_string().contains(why), "{argv:?}: {err}");
        }
    }

    #[test]
    fn an_action_needs_a_web_runnable_command_without_required_arguments() {
        let cat = catalogue();
        let read = vec!["x", "status", "--json"];
        let err = check_page("x", &page(read.clone(), vec![vec!["console"]]), &cat).unwrap_err();
        assert!(err.to_string().contains("not run from the web"), "{err}");
        let err = check_page("x", &page(read, vec![vec!["x", "draft"]]), &cat).unwrap_err();
        assert!(err.to_string().contains("needs `file`"), "{err}");
    }

    #[test]
    fn ids_and_pointers_are_checked() {
        let cat = catalogue();
        let mut p = page(vec!["x", "status", "--json"], vec![]);
        p.id = "Bad Id";
        assert!(check_page("x", &p, &cat).is_err());
        let mut p = page(vec!["x", "status", "--json"], vec![]);
        p.panels[0].view = View::Text { pointer: "rows" };
        assert!(
            check_page("x", &p, &cat)
                .unwrap_err()
                .to_string()
                .contains("JSON pointer")
        );
    }

    #[test]
    fn the_public_binary_declares_no_pages() {
        let mut root = crate::app::cli_command();
        root.build();
        let cat = crate::commands::catalogue::catalogue(&root);
        let v = describe(&Extensions::none(), &cat).unwrap();
        assert_eq!(v["extensions"], json!([]));
        assert_eq!(v["schema_version"], 1);
    }
}
