//! RFC 0045 bake step: pre-render `docs-gen --layout curated`'s Markdown
//! output to static HTML once, offline — the live server never parses
//! Markdown on a request path (see the RFC's Design section). Not a general
//! `docs-gen` feature (`--layout curated --format html` stays unsupported
//! there, RFC 0045's Non-goals) — this only needs to work for the fixed
//! two-repo demo catalog.
//!
//! Usage: `prerender <curated-markdown-dir> <output-html-dir>`

use anyhow::{Context, Result, bail};
use ekos_demo_server::page::html_document;
use pulldown_cmark::{Options, Parser, html};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(src), Some(dst)) = (args.next(), args.next()) else {
        bail!("usage: prerender <curated-markdown-dir> <output-html-dir>");
    };
    let src = PathBuf::from(src);
    let dst = PathBuf::from(dst);
    if !src.is_dir() {
        bail!("source dir {} does not exist", src.display());
    }
    std::fs::create_dir_all(&dst)?;

    let mut rendered = 0usize;
    for entry in WalkDir::new(&src).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let rel = path.strip_prefix(&src).unwrap();
        let out_path = dst.join(rel).with_extension("html");
        render_one(path, &out_path)?;
        rendered += 1;
    }
    println!("prerender: {rendered} Markdown pages -> {} ", dst.display());
    Ok(())
}

fn render_one(src: &Path, dst: &Path) -> Result<()> {
    let raw = std::fs::read_to_string(src).with_context(|| format!("reading {}", src.display()))?;
    // Curated docs cross-link with plain `.md` paths (docs-gen's own output
    // convention); rewrite to `.html` so the pre-rendered pages navigate
    // between each other correctly.
    let rewritten = raw.replace(".md)", ".html)").replace(".md#", ".html#");

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&rewritten, opts);
    let mut body_html = String::new();
    html::push_html(&mut body_html, parser);

    let title = rewritten
        .lines()
        .find(|l| l.starts_with("# "))
        .map(|l| l.trim_start_matches("# ").to_string())
        .unwrap_or_else(|| src.file_stem().unwrap().to_string_lossy().into_owned());

    let page = html_document(&title, &body_html);

    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(dst, page).with_context(|| format!("writing {}", dst.display()))?;
    Ok(())
}
