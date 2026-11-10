//! RFC 0253 §3.4–3.7: testimony — what a person told EKOS, recorded with who, where and when.
//!
//! `ekos testimony add` is the **only** writer and it is human-only (`review_guard::HUMAN_ONLY`;
//! no MCP tool reaches it): what is saved is the person's own text (or a proposal they saw and
//! accepted in a channel, which the console passes here as their text), attributed to them, with
//! the original message linked. A testimony is evidence, never a decision: it can answer an open
//! question (RFC 0254), state a code value's meaning (a new `EnumMeaning` meaning source,
//! `testimony`, folded in at the next semantics step) — and when that meaning disagrees with
//! what code says, an RFC 0172 `testimony_mismatch` conflict is recorded; nothing is overwritten.
//! Withdrawal is a new version (the ledger is append-only); readers take the newest.

use anyhow::{Result, anyhow, bail};
use ekos_compiler_core::EkosConfig;
use ekos_kir::{
    KirEvidence, KirId, KirObject, KirRelationship, ObjectKind, RelationshipKind, SourceLocation,
};
use ekos_ledger::KnowledgeReader;
use ekos_semantic::questions::TESTIMONY;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
use uuid::Uuid;

/// RFC 0172 conflict type of a meaning that disagrees with code.
pub const MISMATCH: &str = "testimony_mismatch";

fn kid(key: &str) -> KirId {
    KirId(Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()))
}

fn s<'a>(o: &'a KirObject, k: &str) -> &'a str {
    o.properties.get(k).and_then(Value::as_str).unwrap_or("")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// What a person said.
pub struct Statement<'a> {
    pub by: &'a str,
    pub text: &'a str,
    pub channel: &'a str,
    /// The channel's own message id; with the channel it keys the testimony.
    pub message_ref: Option<&'a str>,
    pub permalink: Option<&'a str>,
    /// Entities it is about (ids or exact names).
    pub about: &'a [String],
    /// The open question it answers (id or prefix).
    pub answers: Option<&'a str>,
    /// A code value's meaning: the statement is about one `EnumMeaning` (in `about`).
    pub meaning: Option<&'a str>,
    /// Recorded by an operator for someone else (a workshop answer): who said it, as text.
    pub on_behalf_of: Option<&'a str>,
    /// Inbox items sent with it.
    pub attachments: &'a [String],
}

/// The newest version of each testimony.
pub fn current(objects: &[KirObject]) -> Vec<&KirObject> {
    let mut newest: BTreeMap<String, &KirObject> = BTreeMap::new();
    for t in objects
        .iter()
        .filter(|o| matches!(&o.kind, ObjectKind::Custom(k) if k == TESTIMONY))
    {
        let k = t.id.to_string();
        if newest
            .get(&k)
            .is_none_or(|old| s(t, "updated_at") >= s(old, "updated_at"))
        {
            newest.insert(k, t);
        }
    }
    newest.into_values().collect()
}

fn resolve<'a>(objects: &'a [KirObject], what: &str) -> Result<&'a KirObject> {
    if let Some(o) = objects.iter().find(|o| o.id.to_string() == what) {
        return Ok(o);
    }
    let m: Vec<&KirObject> = objects.iter().filter(|o| o.name == what).collect();
    match m.as_slice() {
        [one] => Ok(one),
        [] => bail!("nothing named {what:?} (give an id)"),
        _ => bail!("{what:?} names {} objects; give an id", m.len()),
    }
}

/// The testimony object, its evidence, and its links. Pure.
fn build(
    st: &Statement<'_>,
    about: &[&KirObject],
    answers: Option<String>,
    redaction: &ekos_common::redaction::RedactionConfig,
) -> (KirObject, KirEvidence, Vec<KirRelationship>) {
    let text = ekos_common::redaction::redact_personal_data(&ekos_common::redaction::redact(
        st.text, redaction,
    ));
    let sent_at = now();
    let message_ref = st.message_ref.map(str::to_string).unwrap_or_else(|| {
        kid(&format!("{}:{}:{}", st.by, sent_at, text))
            .0
            .to_string()
    });
    let id = kid(&format!("testimony:{}:{message_ref}", st.channel));
    let mut o = KirObject::new(
        format!("{}: {}", st.by, text.chars().take(80).collect::<String>()),
        ObjectKind::Custom(TESTIMONY.into()),
    );
    o.id = id;
    for (k, v) in [
        ("author", json!(st.by)),
        ("channel", json!(st.channel)),
        ("message_ref", json!(message_ref)),
        ("permalink", json!(st.permalink)),
        ("sent_at", json!(sent_at)),
        ("updated_at", json!(sent_at)),
        ("text", json!(text)),
        (
            "about",
            json!(about.iter().map(|a| a.id.to_string()).collect::<Vec<_>>()),
        ),
        ("answers", json!(answers.unwrap_or_default())),
        (
            "meaning",
            json!(
                st.meaning
                    .map(|m| ekos_common::redaction::redact(m, redaction))
            ),
        ),
        ("on_behalf_of", json!(st.on_behalf_of)),
        ("attachments", json!(st.attachments)),
        ("status", json!("active")),
    ] {
        o.properties.insert(k.into(), v);
    }
    let date = sent_at.get(..10).unwrap_or("");
    let who = match st.on_behalf_of {
        Some(w) => format!("{w} (recorded by {})", st.by),
        None => st.by.to_string(),
    };
    let mut ev = KirEvidence::new(
        SourceLocation {
            path: format!("testimony://{id}"),
            line: None,
            column: None,
        },
        format!("According to {who} ({}, {date}): {text}", st.channel),
    );
    ev.id = kid(&format!("{id}:evidence"));
    o.evidence = vec![ev.id];
    let links = about
        .iter()
        .map(|a| {
            KirRelationship::deterministic(
                RelationshipKind::Custom("Describes".into()),
                id,
                a.id,
                "",
            )
        })
        .collect();
    (o, ev, links)
}

/// `testimony_mismatch` when the meaning a person states differs from the label code gives.
fn mismatch(
    enum_meaning: &KirObject,
    testimony: &KirObject,
    meaning: &str,
) -> Option<ekos_kir::KirGraph> {
    let code_label = enum_meaning
        .properties
        .get("label")
        .and_then(Value::as_str)?;
    let norm = |x: &str| x.trim().trim_matches('\'').to_lowercase();
    if norm(code_label) == norm(meaning) {
        return None;
    }
    let source = enum_meaning
        .properties
        .get("meanings")
        .and_then(Value::as_array)
        .and_then(|m| m.first());
    let claims = vec![
        ekos_semantic::conflicts::Claim {
            value: json!(code_label),
            path: source
                .and_then(|m| m["path"].as_str())
                .unwrap_or("")
                .to_string(),
            line: source.and_then(|m| m["line"].as_u64()).map(|l| l as u32),
            source: source
                .and_then(|m| m["source"].as_str())
                .unwrap_or("code")
                .to_string(),
        },
        ekos_semantic::conflicts::Claim {
            value: json!(meaning),
            path: format!("testimony://{}", testimony.id),
            line: None,
            source: format!("testimony by {}", s(testimony, "author")),
        },
    ];
    Some(ekos_semantic::conflicts::conflict(
        enum_meaning,
        "meaning",
        MISMATCH,
        claims,
        None,
    ))
}

/// `ekos testimony add` — human-only.
pub fn add(config: &EkosConfig, cwd: &Path, st: Statement<'_>, json_out: bool) -> Result<()> {
    if st.text.trim().is_empty() {
        bail!("a testimony needs the person's text");
    }
    let ledger = super::store::open_store(config, cwd)?;
    let objects = ledger.all_objects()?;
    let about: Vec<&KirObject> = st
        .about
        .iter()
        .map(|a| resolve(&objects, a))
        .collect::<Result<_>>()?;
    let answers = match st.answers {
        Some(q) => {
            let all = super::questions::read(
                config,
                cwd,
                &*ledger,
                &super::questions::Filter {
                    status: Some("all"),
                    ..Default::default()
                },
            )?;
            let m: Vec<&Value> = all
                .iter()
                .filter(|x| x["id"].as_str().is_some_and(|i| i.starts_with(q)))
                .collect();
            match m.as_slice() {
                [one] => Some(one["id"].as_str().unwrap_or("").to_string()),
                [] => bail!("no question {q}"),
                _ => bail!("{q} names several questions"),
            }
        }
        None => None,
    };
    if st.meaning.is_some()
        && !about
            .iter()
            .any(|a| matches!(&a.kind, ObjectKind::Custom(k) if k == "EnumMeaning"))
    {
        bail!(
            "--meaning needs --about naming the code value (an EnumMeaning, e.g. \"orders.status = 7\")"
        );
    }
    let (o, ev, links) = build(&st, &about, answers, &config.redaction_config());
    ledger.append_evidence(&ev)?;
    ledger.append_object(&o)?;
    for r in &links {
        ledger.append_relationship(r)?;
    }
    let mut conflicts = 0;
    if let Some(meaning) = st.meaning {
        for e in about
            .iter()
            .filter(|a| matches!(&a.kind, ObjectKind::Custom(k) if k == "EnumMeaning"))
        {
            if let Some(g) = mismatch(e, &o, meaning) {
                for ev in &g.evidence {
                    ledger.append_evidence(ev)?;
                }
                for c in &g.objects {
                    ledger.append_object(c)?;
                }
                for r in &g.relationships {
                    ledger.append_relationship(r)?;
                }
                super::conflicts::add_current(
                    config,
                    cwd,
                    ledger.as_ref(),
                    g.objects.iter().map(|c| c.id.to_string()),
                )?;
                conflicts += g.objects.len();
            }
        }
    }
    let out = json!({
        "id": o.id.to_string(), "author": st.by, "answers": o.properties.get("answers"),
        "about": o.properties.get("about"), "conflicts": conflicts,
        "decides": config.testimony.decides_for.iter().any(|p| p.eq_ignore_ascii_case(st.by)),
    });
    if json_out {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "Recorded testimony {} by {}.",
            &out["id"].as_str().unwrap_or("")[..8],
            st.by
        );
        if conflicts > 0 {
            println!(
                "  It disagrees with what code says: {conflicts} conflict(s) to review (`ekos conflicts list`)."
            );
        }
        if st.meaning.is_some() {
            println!(
                "  The meaning reaches the code value at the next `ekos commit` (meaning source: testimony)."
            );
        }
    }
    Ok(())
}

/// `ekos testimony withdraw <id> --as <author>`: only the author withdraws their own.
pub fn withdraw(config: &EkosConfig, cwd: &Path, id: &str, by: &str) -> Result<()> {
    let ledger = super::store::open_store(config, cwd)?;
    let objects = ledger.all_objects()?;
    let all = current(&objects);
    let m: Vec<&&KirObject> = all
        .iter()
        .filter(|t| t.id.to_string().starts_with(id))
        .collect();
    let t = match m.as_slice() {
        [one] => **one,
        [] => bail!("no testimony {id}"),
        _ => bail!("{id} names several testimonies"),
    };
    if !s(t, "author").eq_ignore_ascii_case(by) {
        bail!("only {} can withdraw this testimony", s(t, "author"));
    }
    let mut w = t.clone();
    w.properties.insert("status".into(), json!("withdrawn"));
    w.properties.insert("updated_at".into(), json!(now()));
    ledger.append_object(&w)?;
    println!("Withdrawn. Answers and documents stop citing it; the ledger keeps both versions.");
    Ok(())
}

fn view(t: &KirObject) -> Value {
    json!({
        "id": t.id.to_string(), "author": s(t, "author"), "on_behalf_of": t.properties.get("on_behalf_of"),
        "channel": s(t, "channel"), "permalink": t.properties.get("permalink"), "sent_at": s(t, "sent_at"),
        "text": s(t, "text"), "about": t.properties.get("about"), "answers": s(t, "answers"),
        "meaning": t.properties.get("meaning"), "status": s(t, "status"),
    })
}

/// The current testimonies, filtered.
pub fn read(
    ledger: &dyn KnowledgeReader,
    about: Option<&str>,
    author: Option<&str>,
    question: Option<&str>,
    all: bool,
) -> Result<Vec<Value>> {
    let objects = ledger.all_objects()?;
    let mut out: Vec<Value> = current(&objects)
        .into_iter()
        .filter(|t| all || s(t, "status") == "active")
        .filter(|t| author.is_none_or(|a| s(t, "author").eq_ignore_ascii_case(a)))
        .filter(|t| question.is_none_or(|q| s(t, "answers").starts_with(q)))
        .filter(|t| {
            about.is_none_or(|a| {
                t.properties
                    .get("about")
                    .and_then(Value::as_array)
                    .is_some_and(|ids| {
                        ids.iter()
                            .any(|x| x.as_str().is_some_and(|x| x.starts_with(a)))
                    })
                    || objects.iter().any(|o| {
                        o.name == a
                            && t.properties
                                .get("about")
                                .is_some_and(|ids| ids.to_string().contains(&o.id.to_string()))
                    })
            })
        })
        .map(view)
        .collect();
    out.sort_by(|a, b| b["sent_at"].as_str().cmp(&a["sent_at"].as_str()));
    Ok(out)
}

/// `ekos testimony list`.
pub fn list(
    config: &EkosConfig,
    cwd: &Path,
    about: Option<&str>,
    author: Option<&str>,
    all: bool,
    json_out: bool,
) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let rows = read(&*ledger, about, author, None, all)?;
    if json_out {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("No testimony.");
    }
    for r in &rows {
        println!(
            "{} {} ({}, {}){}: {}",
            &r["id"].as_str().unwrap_or("")[..8],
            r["author"].as_str().unwrap_or(""),
            r["channel"].as_str().unwrap_or(""),
            r["sent_at"].as_str().unwrap_or("").get(..10).unwrap_or(""),
            if r["status"] == "withdrawn" {
                " [withdrawn]"
            } else {
                ""
            },
            r["text"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

/// `ekos testimony show <id>`.
pub fn show(config: &EkosConfig, cwd: &Path, id: &str) -> Result<()> {
    let ledger = super::store::open_store_read_only(config, cwd)?;
    let rows = read(&*ledger, None, None, None, true)?;
    let r = rows
        .iter()
        .find(|r| r["id"].as_str().is_some_and(|x| x.starts_with(id)))
        .ok_or_else(|| anyhow!("no testimony {id}"))?;
    println!("{}", serde_json::to_string_pretty(r)?);
    Ok(())
}

/// MCP `ekos_testimony` (read-only).
pub fn agent_list(ledger: &dyn KnowledgeReader, args: &Value) -> Result<Value> {
    let get = |k: &str| args.get(k).and_then(Value::as_str);
    let rows = read(ledger, get("about"), get("author"), get("question"), false)?;
    Ok(json!({
        "testimony": rows.into_iter().take(200).collect::<Vec<_>>(),
        "note": "What people told EKOS, in their words. Evidence, not a decision: cite it as \"According to <author> (<channel>, <date>)\". Never invent or paraphrase testimony.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st<'a>(text: &'a str, about: &'a [String]) -> Statement<'a> {
        Statement {
            by: "ana",
            text,
            channel: "console",
            message_ref: Some("m1"),
            permalink: None,
            about,
            answers: None,
            meaning: Some("cancelled by the customer"),
            on_behalf_of: None,
            attachments: &[],
        }
    }

    #[test]
    fn a_statement_is_redacted_attributed_and_keyed_by_channel_message() {
        let (o, ev, _) = build(
            &st("status 7 — mail me at ana@example.com", &[]),
            &[],
            None,
            &Default::default(),
        );
        assert!(!s(&o, "text").contains("ana@example.com"));
        assert!(ev.fragment.starts_with("According to ana (console, "));
        let (o2, ..) = build(&st("edited", &[]), &[], None, &Default::default());
        assert_eq!(o.id, o2.id, "same channel message, same testimony");
    }

    #[test]
    fn a_meaning_that_disagrees_with_code_is_a_conflict_not_an_overwrite() {
        let mut e = KirObject::new(
            "orders.status = 7",
            ObjectKind::Custom("EnumMeaning".into()),
        );
        e.properties.insert("label".into(), json!("suspended"));
        let (t, ..) = build(&st("x", &[]), &[], None, &Default::default());
        let g = mismatch(&e, &t, "cancelled by the customer").unwrap();
        assert_eq!(g.objects[0].properties["conflict_type"], MISMATCH);
        assert_eq!(e.properties["label"], "suspended");
        assert!(mismatch(&e, &t, "Suspended").is_none());
    }

    #[test]
    fn the_newest_version_wins() {
        let (mut a, ..) = build(&st("x", &[]), &[], None, &Default::default());
        let mut b = a.clone();
        b.properties.insert("status".into(), json!("withdrawn"));
        b.properties.insert("updated_at".into(), json!("9999"));
        a.properties.insert("updated_at".into(), json!("0001"));
        let objs = vec![b.clone(), a];
        assert_eq!(s(current(&objs)[0], "status"), "withdrawn");
    }
}
