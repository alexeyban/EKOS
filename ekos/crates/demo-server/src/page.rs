//! Minimal shared page chrome — styled to match `docs-gen`'s embedded-CSS
//! look (`ekos-docs-gen/src/lib.rs`'s `EMBEDDED_CSS`/`html_document`) so the
//! bake-time pre-rendered curated docs and the live server's own chrome
//! (repo picker, ask box) read as one product, not two. Not a literal import
//! — `docs-gen`'s constants are crate-private by design (RFC 0045 doesn't
//! widen `docs-gen`'s own surface, only `cli`'s), so this is a deliberately
//! close, independent restatement.

pub const PAGE_CSS: &str = r#"
:root {
  color-scheme: light dark;
  --bg: #0f1117;
  --fg: #e6e6e6;
  --muted: #9aa0ab;
  --accent: #7dd3fc;
  --border: #262a33;
  --code-bg: #1a1d24;
}
@media (prefers-color-scheme: light) {
  :root { --bg: #ffffff; --fg: #1a1a1a; --muted: #5a6472; --accent: #0369a1; --border: #e2e5ea; --code-bg: #f4f5f7; }
}
* { box-sizing: border-box; }
body {
  margin: 0; background: var(--bg); color: var(--fg);
  font: 15px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
}
main { max-width: 860px; margin: 0 auto; padding: 2rem 1.25rem 4rem; }
a { color: var(--accent); }
h1, h2, h3 { line-height: 1.25; }
h1 { font-size: 1.6rem; }
code, pre { background: var(--code-bg); border-radius: 4px; }
pre { padding: 0.75rem 1rem; overflow-x: auto; }
code { padding: 0.1rem 0.35rem; }
pre code { padding: 0; }
table { border-collapse: collapse; width: 100%; margin: 1rem 0; }
th, td { border: 1px solid var(--border); padding: 0.4rem 0.6rem; text-align: left; }
.nav { display: flex; gap: 1rem; padding: 1rem 1.25rem; border-bottom: 1px solid var(--border); }
.muted { color: var(--muted); }
.evidence { border-left: 3px solid var(--accent); padding-left: 0.75rem; margin: 0.5rem 0; font-size: 0.9rem; }
.repo-picker { display: flex; gap: 1rem; flex-wrap: wrap; margin: 1.5rem 0; }
.repo-card { border: 1px solid var(--border); border-radius: 8px; padding: 1rem; flex: 1 1 260px; }
#ask-answer { white-space: pre-wrap; }
"#;

pub fn html_document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n<style>{PAGE_CSS}</style>\n</head>\n<body>\n{body}\n</body>\n</html>\n"
    )
}
