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
    check_params(&at, page)?;
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
        for name in &panel.params {
            if !page.params.iter().any(|p| p.name == *name) {
                bail!("{at}: selection `{name}` is not declared on the page");
            }
            let takes = entry
                .args
                .iter()
                .any(|a| a.kind == "value" && a.long.as_deref() == Some(*name));
            if !takes {
                bail!(
                    "{at}: `{}` has no value option `--{name}` for the selection",
                    entry.path.join(" ")
                );
            }
        }
        if let Some(required) = entry
            .args
            .iter()
            .find(|a| a.required && !panel.params.iter().any(|p| a.long.as_deref() == Some(*p)))
        {
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
            View::Text { pointer } | View::Markdown { pointer } => check_pointer(pointer, &at)?,
            View::Code { left, right, .. } => {
                check_pointer(left, &at)?;
                check_pointer(right, &at)?;
            }
            View::Graph {
                nodes,
                edges,
                id,
                label,
                group,
                badge,
                from,
                to,
            } => {
                for p in [nodes, edges, id, label, group, badge, from, to] {
                    check_pointer(p, &at)?;
                }
            }
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
        for (arg, param) in &action.binds {
            if !entry.args.iter().any(|a| a.id == *arg && a.kind != "flag") {
                bail!("{at}: `{}` has no argument `{arg}`", entry.path.join(" "));
            }
            if !page.params.iter().any(|p| p.name == *param) {
                bail!("{at}: selection `{param}` is not declared on the page");
            }
        }
        for flag in &action.flags {
            if !entry
                .args
                .iter()
                .any(|a| a.kind == "flag" && a.long.as_deref() == Some(*flag))
            {
                bail!(
                    "{at}: `--{flag}` is not a flag of `{}`",
                    entry.path.join(" ")
                );
            }
        }
        if let Some(required) = entry
            .args
            .iter()
            .find(|a| a.required && !action.binds.iter().any(|(b, _)| *b == a.id))
        {
            bail!(
                "{at}: `{}` needs `{}`, which an action cannot give",
                entry.path.join(" "),
                required.id
            );
        }
    }
    Ok(())
}

/// RFC 0207: each selection has a table source on the page, and no selection needs itself.
fn check_params(at: &str, page: &ConsolePage) -> Result<()> {
    let mut names = std::collections::BTreeSet::new();
    for p in &page.params {
        let at = format!("{at}, selection {}", p.name);
        if p.name.is_empty()
            || !p
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            bail!("{at}: a selection name is lowercase letters, digits and `-`");
        }
        if !names.insert(p.name) {
            bail!("{at}: declared twice");
        }
        let Some(source) = page.panels.iter().find(|x| x.id == p.source) else {
            bail!("{at}: source panel `{}` is not on the page", p.source);
        };
        if !matches!(source.view, View::Table { .. }) {
            bail!("{at}: source panel `{}` is not a table", p.source);
        }
        check_pointer(p.rows, &at)?;
        check_pointer(p.key, &at)?;
    }
    // A selection's source may need other selections, never itself through a chain.
    for p in &page.params {
        let mut seen = vec![p.name];
        let mut stack = vec![p.source];
        while let Some(panel) = stack.pop() {
            let Some(panel) = page.panels.iter().find(|x| x.id == panel) else {
                continue;
            };
            for need in &panel.params {
                if *need == p.name {
                    bail!("{at}, selection {}: its source needs it (a cycle)", p.name);
                }
                if let Some(q) = page.params.iter().find(|q| q.name == *need)
                    && !seen.contains(&q.name)
                {
                    seen.push(q.name);
                    stack.push(q.source);
                }
            }
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
    use crate::extension::{Action, Column, PageParam, Panel};

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
                ..Default::default()
            }],
            actions: actions
                .into_iter()
                .map(|command| Action {
                    label: "go",
                    command,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
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

    fn v3_catalogue() -> Vec<Entry> {
        let mut c = catalogue();
        c.push(entry(
            &["x", "report"],
            Class::Read,
            vec![
                arg("json", "flag", false),
                arg("package", "value", false),
                arg("step", "value", false),
            ],
        ));
        c.push(entry(
            &["x", "translate"],
            Class::External,
            vec![
                arg("package", "value", true),
                arg("allow-metered", "flag", false),
            ],
        ));
        c
    }

    fn v3_page() -> ConsolePage {
        let table =
            |id: &'static str, command: Vec<&'static str>, params: Vec<&'static str>| Panel {
                id,
                title: id,
                command,
                view: View::Table {
                    rows: "/rows",
                    columns: vec![Column::new("/name", "Name")],
                },
                empty: "",
                params,
            };
        ConsolePage {
            id: "p",
            title: "P",
            panels: vec![
                table("packages", vec!["x", "status", "--json"], vec![]),
                table("steps", vec!["x", "report", "--json"], vec!["package"]),
                Panel {
                    id: "step",
                    title: "Step",
                    command: vec!["x", "report", "--json"],
                    view: View::Code {
                        left: "/step/original",
                        right: "/step/sql",
                        left_label: "Kettle",
                        right_label: "SQL",
                    },
                    empty: "",
                    params: vec!["package", "step"],
                },
            ],
            params: vec![
                PageParam {
                    name: "package",
                    label: "Package",
                    source: "packages",
                    rows: "/rows",
                    key: "/name",
                },
                PageParam {
                    name: "step",
                    label: "Step",
                    source: "steps",
                    rows: "/rows",
                    key: "/name",
                },
            ],
            actions: vec![Action {
                label: "Translate",
                command: vec!["x", "translate"],
                binds: vec![("package", "package")],
                flags: vec!["allow-metered"],
                confirm: Some("uses a paid model"),
            }],
            ..Default::default()
        }
    }

    /// RFC 0207: selections, parameterized panels and bound actions.
    #[test]
    fn selections_feed_panels_and_actions_and_are_checked() {
        let cat = v3_catalogue();
        check_page("e", &v3_page(), &cat).unwrap();
        let refused = |f: &dyn Fn(&mut ConsolePage), why: &str| {
            let mut p = v3_page();
            f(&mut p);
            let e = check_page("e", &p, &cat).unwrap_err().to_string();
            assert!(e.contains(why), "{why}: {e}");
        };
        refused(&|p| p.params[0].source = "nope", "is not on the page");
        refused(&|p| p.params[0].source = "step", "is not a table");
        refused(&|p| p.panels[0].params = vec!["step"], "a cycle");
        refused(&|p| p.panels[1].params = vec!["owner"], "is not declared");
        refused(
            &|p| p.panels[1].command = vec!["x", "status", "--json"],
            "has no value option",
        );
        refused(&|p| p.actions[0].binds = vec![], "needs `package`");
        refused(
            &|p| p.actions[0].binds = vec![("nope", "package")],
            "has no argument",
        );
        refused(&|p| p.actions[0].flags = vec!["force"], "is not a flag");
        refused(&|p| p.params[1].name = "Step", "lowercase");
    }
}
