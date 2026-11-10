//! RFC 0253 §3.3: the inbox — what people send (files, links) as a source like any other.
//!
//! `ekos inbox add <file|url>` stores the original under
//! `<state>/inbox/<status>/<yyyy-mm>/<sha256-prefix>/<name>` with a `meta.json` (who, channel,
//! link, when, source URL); identical content is stored once and gains another sender entry. Only
//! **accepted** material is compiled: `ekos build` observes `<state>/inbox/accepted` as the source
//! `inbox` with the local-documents connector alone (images through local OCR whatever
//! `[ocr] images` says), so every text it holds passes the normal redaction before anything is
//! held. Originals stay outside the ledger and can be purged (§3.9); the ledger keeps only the
//! redacted extracted text.
//!
//! A link is fetched once as an HTML snapshot: `https` (or `http`) only, at most 3 redirects, and
//! never from a loopback, private or link-local address unless the host is in
//! `[inbox] allow-hosts`.

use anyhow::{Context, Result, anyhow, bail};
use ekos_compiler_core::{EkosConfig, ObserveBase};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// The source key inbox material is compiled under.
pub const SOURCE_KEY: &str = "inbox";

/// Extensions the inbox takes (the local-documents connector reads them all). Nothing else is
/// stored; archives are not expanded; nothing is executed.
const ALLOWED: &[&str] = &[
    "pdf", "docx", "html", "htm", "md", "markdown", "txt", "text", "eml", "png", "jpg", "jpeg",
    "tif", "tiff", "webp", "xlsx", "csv", "pptx",
];

const MAX_REDIRECTS: usize = 3;

fn root(config: &EkosConfig, cwd: &Path) -> PathBuf {
    config.ekos_dir(cwd).join("inbox")
}

/// The build's observe base for accepted material, when there is any.
pub fn observe_base(config: &EkosConfig, cwd: &Path) -> Option<ObserveBase> {
    let path = root(config, cwd).join("accepted");
    std::fs::read_dir(&path).ok()?.next()?;
    Some(ObserveBase {
        path,
        key: SOURCE_KEY.into(),
        source: Some(SOURCE_KEY.into()),
    })
}

/// Who sent something, through what.
pub struct Sender<'a> {
    pub by: &'a str,
    pub channel: &'a str,
    pub permalink: Option<&'a str>,
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn safe_name(name: &str) -> String {
    let n: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let n = n.trim_start_matches('.').to_string();
    if n.is_empty() {
        "file".into()
    } else {
        n.chars().take(120).collect()
    }
}

fn extension(name: &str) -> String {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Stores `bytes` as `name`; returns the item's id (its sha256 prefix) and directory.
fn store(
    config: &EkosConfig,
    cwd: &Path,
    name: &str,
    bytes: &[u8],
    sender: &Sender<'_>,
    source_url: Option<&str>,
    pending: bool,
) -> Result<(String, PathBuf, bool)> {
    let ext = extension(name);
    if !ALLOWED.contains(&ext.as_str()) {
        bail!(
            "the inbox does not take .{ext} files (allowed: {})",
            ALLOWED.join(", ")
        );
    }
    let max = config.inbox.max_file_mb.max(1) * 1024 * 1024;
    if bytes.len() as u64 > max {
        bail!(
            "{name} is larger than [inbox] max-file-mb ({} MB)",
            config.inbox.max_file_mb
        );
    }
    let sha = hex::encode(Sha256::digest(bytes));
    let id = sha[..16].to_string();
    // The same content already here (either status) gains a sender, nothing more.
    if let Some(dir) = find(config, cwd, &id) {
        let mut meta = read_meta(&dir)?;
        let senders = meta["senders"]
            .as_array_mut()
            .ok_or_else(|| anyhow!("bad meta.json"))?;
        senders.push(sender_json(sender, source_url));
        write_meta(&dir, &meta)?;
        return Ok((id, dir, false));
    }
    let status = if pending || config.inbox.require_accept {
        "pending"
    } else {
        "accepted"
    };
    let month = chrono::Utc::now().format("%Y-%m").to_string();
    let dir = root(config, cwd).join(status).join(month).join(&id);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(safe_name(name)), bytes)?;
    write_meta(
        &dir,
        &json!({
            "id": id, "sha256": sha, "name": safe_name(name), "size": bytes.len(),
            "status": status, "received_at": now(), "senders": [sender_json(sender, source_url)],
        }),
    )?;
    Ok((id, dir, true))
}

fn sender_json(s: &Sender<'_>, source_url: Option<&str>) -> Value {
    json!({"by": s.by, "channel": s.channel, "permalink": s.permalink, "at": now(), "source_url": source_url})
}

fn read_meta(dir: &Path) -> Result<Value> {
    let p = dir.join("meta.json");
    serde_json::from_slice(&std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?)
        .with_context(|| format!("parsing {}", p.display()))
}

fn write_meta(dir: &Path, v: &Value) -> Result<()> {
    std::fs::write(dir.join("meta.json"), serde_json::to_vec_pretty(v)?)?;
    Ok(())
}

/// Every item directory, with its status.
fn items(config: &EkosConfig, cwd: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for status in ["pending", "accepted"] {
        let base = root(config, cwd).join(status);
        let Ok(months) = std::fs::read_dir(&base) else {
            continue;
        };
        for m in months.flatten() {
            let Ok(ids) = std::fs::read_dir(m.path()) else {
                continue;
            };
            for i in ids.flatten() {
                if i.path().join("meta.json").exists() {
                    out.push((status.to_string(), i.path()));
                }
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

fn find(config: &EkosConfig, cwd: &Path, id: &str) -> Option<PathBuf> {
    items(config, cwd).into_iter().map(|(_, p)| p).find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(id))
    })
}

/// Whether `ip` is one a link may never be fetched from by default.
fn is_internal(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_loopback()
                || v.is_private()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.octets()[0] == 100 && (v.octets()[1] & 0xc0) == 64
        }
        IpAddr::V6(v) => {
            v.is_loopback()
                || v.is_unspecified()
                || (v.segments()[0] & 0xfe00) == 0xfc00
                || (v.segments()[0] & 0xffc0) == 0xfe80
                || v.to_ipv4_mapped()
                    .is_some_and(|m| is_internal(IpAddr::V4(m)))
        }
    }
}

/// Refuses a URL whose host resolves to an internal address (unless allowed), or a scheme other
/// than http(s). Returns the URL parsed.
async fn check_url(config: &EkosConfig, raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw).with_context(|| format!("not a URL: {raw}"))?;
    if !matches!(url.scheme(), "https" | "http") {
        bail!("only http(s) links are fetched, not {}", url.scheme());
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("{raw} has no host"))?
        .to_string();
    if config
        .inbox
        .allow_hosts
        .iter()
        .any(|h| h.eq_ignore_ascii_case(&host))
    {
        return Ok(url);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .with_context(|| format!("resolving {host}"))?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() || addrs.iter().any(|ip| is_internal(*ip)) {
        bail!(
            "{host} resolves to an internal address; add it to [inbox] allow-hosts if it is meant to be fetched"
        );
    }
    Ok(url)
}

/// Fetches a link once, following at most [`MAX_REDIRECTS`] redirects, each one checked again.
async fn fetch(config: &EkosConfig, raw: &str) -> Result<(String, Vec<u8>, String)> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(concat!("ekos-inbox/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let mut url = check_url(config, raw).await?;
    for _ in 0..=MAX_REDIRECTS {
        let resp = client.get(url.clone()).send().await?;
        if resp.status().is_redirection() {
            let next = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| anyhow!("a redirect without a location"))?;
            url = check_url(config, url.join(next)?.as_str()).await?;
            continue;
        }
        if !resp.status().is_success() {
            bail!("{url} answered {}", resp.status());
        }
        let max = config.inbox.max_file_mb.max(1) * 1024 * 1024;
        let bytes = resp.bytes().await?;
        if bytes.len() as u64 > max {
            bail!("{url} is larger than [inbox] max-file-mb");
        }
        let name = format!(
            "{}{}.html",
            url.host_str().unwrap_or("page"),
            url.path().replace('/', "_")
        );
        return Ok((name, bytes.to_vec(), url.to_string()));
    }
    bail!("more than {MAX_REDIRECTS} redirects")
}

/// `ekos inbox add <file|url> --as <person>`.
pub async fn add(
    config: &EkosConfig,
    cwd: &Path,
    what: &str,
    sender: Sender<'_>,
    pending: bool,
    json_out: bool,
) -> Result<()> {
    let (name, bytes, source) = if what.starts_with("http://") || what.starts_with("https://") {
        let (name, bytes, final_url) = fetch(config, what).await?;
        (name, bytes, Some(final_url))
    } else {
        let p = Path::new(what);
        let bytes = std::fs::read(p).with_context(|| format!("reading {what}"))?;
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        (name, bytes, None)
    };
    let (id, dir, new) = store(
        config,
        cwd,
        &name,
        &bytes,
        &sender,
        source.as_deref(),
        pending,
    )?;
    let meta = read_meta(&dir)?;
    let out = json!({
        "id": id, "name": meta["name"], "status": meta["status"], "new": new, "size": meta["size"],
        "source_url": source,
        "note": if meta["status"] == "pending" { "waits for `ekos inbox accept` before it is compiled" } else { "compiled on the next `ekos build`" },
    });
    if json_out {
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "{} {} ({} bytes, {}) — {}",
            if new {
                "Stored"
            } else {
                "Already stored; sender added:"
            },
            out["name"].as_str().unwrap_or(""),
            out["size"],
            out["status"].as_str().unwrap_or(""),
            out["note"].as_str().unwrap_or("")
        );
        println!("  id {id}");
    }
    Ok(())
}

/// `ekos inbox list`.
pub fn list(config: &EkosConfig, cwd: &Path, status: Option<&str>, json_out: bool) -> Result<()> {
    let mut rows = Vec::new();
    for (st, dir) in items(config, cwd) {
        if status.is_some_and(|s| s != st) {
            continue;
        }
        let m = read_meta(&dir)?;
        rows.push(json!({
            "id": m["id"], "name": m["name"], "status": st, "size": m["size"],
            "received_at": m["received_at"], "senders": m["senders"],
            "accepted_by": m.get("accepted_by"),
        }));
    }
    if json_out {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("The inbox is empty.");
    }
    for r in &rows {
        let by = r["senders"][0]["by"].as_str().unwrap_or("?");
        println!(
            "{} {:9} {} — from {by} ({}), {}",
            r["id"].as_str().unwrap_or(""),
            r["status"].as_str().unwrap_or(""),
            r["name"].as_str().unwrap_or(""),
            r["senders"][0]["channel"].as_str().unwrap_or(""),
            r["received_at"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

/// `ekos inbox accept <id> --as <writer>`: pending → accepted, recorded with who and when.
pub fn accept(config: &EkosConfig, cwd: &Path, id: &str, by: &str) -> Result<()> {
    let dir = find(config, cwd, id).ok_or_else(|| anyhow!("no inbox item {id}"))?;
    let mut meta = read_meta(&dir)?;
    if meta["status"] == "accepted" {
        println!("{id} is already accepted.");
        return Ok(());
    }
    meta["status"] = json!("accepted");
    meta["accepted_by"] = json!(by);
    meta["accepted_at"] = json!(now());
    write_meta(&dir, &meta)?;
    let month = dir
        .parent()
        .and_then(|p| p.file_name())
        .ok_or_else(|| anyhow!("bad inbox path"))?;
    let target = root(config, cwd)
        .join("accepted")
        .join(month)
        .join(dir.file_name().unwrap());
    std::fs::create_dir_all(target.parent().unwrap())?;
    std::fs::rename(&dir, &target)?;
    println!(
        "Accepted {}; compiled on the next `ekos build`.",
        meta["name"].as_str().unwrap_or("")
    );
    Ok(())
}

/// `ekos inbox purge <id> --as <writer>`: deletes the original. The ledger keeps the redacted
/// text it already holds (append-only); the item is listed as purged.
pub fn purge(config: &EkosConfig, cwd: &Path, id: &str, by: &str) -> Result<()> {
    let dir = find(config, cwd, id).ok_or_else(|| anyhow!("no inbox item {id}"))?;
    let meta = read_meta(&dir)?;
    let name = meta["name"].as_str().unwrap_or("").to_string();
    let file = dir.join(&name);
    if file.exists() {
        std::fs::remove_file(&file)?;
    }
    let log = root(config, cwd).join("purged.jsonl");
    let line =
        json!({"id": meta["id"], "name": name, "sha256": meta["sha256"], "by": by, "at": now()});
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)?;
    writeln!(f, "{line}")?;
    std::fs::remove_file(dir.join("meta.json"))?;
    std::fs::remove_dir(&dir).ok();
    println!(
        "Purged the original of {name}. Text already compiled stays in the ledger, redacted (it cannot forget)."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &Path) -> EkosConfig {
        let mut c = EkosConfig::default();
        c.workspace.root = dir.to_path_buf();
        c
    }

    fn sender() -> Sender<'static> {
        Sender {
            by: "ana",
            channel: "console",
            permalink: None,
        }
    }

    #[test]
    fn identical_content_is_stored_once_and_gains_a_sender() {
        let t = tempfile::tempdir().unwrap();
        let c = config(t.path());
        let (a, _, new_a) = store(
            &c,
            t.path(),
            "note.md",
            b"# Status 7",
            &sender(),
            None,
            false,
        )
        .unwrap();
        let (b, dir, new_b) = store(
            &c,
            t.path(),
            "copy.md",
            b"# Status 7",
            &sender(),
            None,
            false,
        )
        .unwrap();
        assert_eq!(a, b);
        assert!(new_a && !new_b);
        assert_eq!(
            read_meta(&dir).unwrap()["senders"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert!(observe_base(&c, t.path()).is_some());
    }

    #[test]
    fn pending_material_is_not_observed_until_accepted() {
        let t = tempfile::tempdir().unwrap();
        let c = config(t.path());
        let (id, _, _) = store(&c, t.path(), "x.txt", b"hello", &sender(), None, true).unwrap();
        assert!(observe_base(&c, t.path()).is_none());
        accept(&c, t.path(), &id, "writer").unwrap();
        assert!(observe_base(&c, t.path()).is_some());
    }

    #[test]
    fn unknown_types_and_oversize_files_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let mut c = config(t.path());
        assert!(store(&c, t.path(), "run.sh", b"rm", &sender(), None, false).is_err());
        assert!(store(&c, t.path(), "a.zip", b"PK", &sender(), None, false).is_err());
        c.inbox.max_file_mb = 1;
        let big = vec![b'a'; 2 * 1024 * 1024];
        assert!(store(&c, t.path(), "big.txt", &big, &sender(), None, false).is_err());
    }

    #[test]
    fn internal_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "169.254.169.254",
            "::1",
            "fd00::1",
            "100.64.0.1",
        ] {
            assert!(is_internal(ip.parse().unwrap()), "{ip}");
        }
        assert!(!is_internal("93.184.216.34".parse().unwrap()));
    }

    #[tokio::test]
    async fn a_link_to_a_loopback_or_a_non_http_scheme_is_refused() {
        let c = EkosConfig::default();
        assert!(check_url(&c, "http://127.0.0.1:8080/x").await.is_err());
        assert!(check_url(&c, "file:///etc/passwd").await.is_err());
        assert!(check_url(&c, "http://localhost/").await.is_err());
        let mut allowed = EkosConfig::default();
        allowed.inbox.allow_hosts = vec!["localhost".into()];
        assert!(check_url(&allowed, "http://localhost/").await.is_ok());
    }
}
