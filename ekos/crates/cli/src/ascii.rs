//! Plain-ASCII terminal output on Windows.
//!
//! EKOS prints a few typographic characters: rules (`─`), dashes (`—`), arrows (`→`), `§`, `…`.
//! On Windows a shell that captures a program's output decodes it with its own code page.
//! PowerShell's `ekos doctor > log.txt`, for example, turns every `─` into `â”€`, and the program
//! cannot prevent that. So on Windows the CLI writes those characters as ASCII (`-`, `--`, `->`).
//!
//! - **Which characters.** Only the symbols in [`replacement`] are changed. Any other non-ASCII
//!   text is data, such as a path or a name in another script, and is written as it is.
//! - **JSON is never changed.** A printed JSON document is machine output, and its strings are
//!   values.
//! - **Turning it on or off.** `EKOS_ASCII=1` turns it on anywhere, which is how the tests check
//!   it on Linux. `EKOS_ASCII=0` turns it off on Windows.
//!
//! The crate's `println!`/`print!`/`eprintln!`/`eprint!` (defined in `lib.rs`) pass through
//! [`console`]. [`Writer`] does the same for the log layers and the few direct writes.

use std::borrow::Cow;
use std::io::{self, Write};
use std::sync::OnceLock;

/// Whether terminal output is written as ASCII: on Windows unless `EKOS_ASCII=0`, elsewhere only
/// with `EKOS_ASCII=1`. Read once.
pub fn active() -> bool {
    static ACTIVE: OnceLock<bool> = OnceLock::new();
    *ACTIVE.get_or_init(|| decide(std::env::var("EKOS_ASCII").ok().as_deref(), cfg!(windows)))
}

fn decide(setting: Option<&str>, windows: bool) -> bool {
    match setting.map(str::trim) {
        Some("1" | "true" | "yes") => true,
        Some("0" | "false" | "no") => false,
        _ => windows,
    }
}

/// `text` as the terminal gets it: [`plain`] when [`active`] and `text` is not a JSON document.
pub fn console(text: &str) -> Cow<'_, str> {
    if active() && !is_json(text) {
        plain(text)
    } else {
        Cow::Borrowed(text)
    }
}

fn is_json(text: &str) -> bool {
    let t = text.trim_start();
    (t.starts_with('{') || t.starts_with('['))
        && serde_json::from_str::<serde::de::IgnoredAny>(text).is_ok()
}

/// `text` with every symbol [`replacement`] knows written in ASCII. Other characters are kept.
pub fn plain(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|c| replacement(c).is_some()) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match replacement(c) {
            Some(r) => out.push_str(r),
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// The ASCII form of a typographic symbol EKOS prints, or `None` for any other character.
pub fn replacement(c: char) -> Option<&'static str> {
    Some(match c {
        '─' | '━' | '–' | '‐' | '‑' | '−' | '·' | '•' => "-",
        '═' => "=",
        '│' | '┃' | '║' => "|",
        '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯' | '╰' => {
            "+"
        }
        '—' => "--",
        '→' | '⟶' => "->",
        '←' => "<-",
        '↔' => "<->",
        '⇒' => "=>",
        '…' => "...",
        '§' => "sec. ",
        '×' => "x",
        '≥' => ">=",
        '≤' => "<=",
        '≠' => "!=",
        '≈' => "~",
        '✓' | '✔' => "ok",
        '✗' | '✘' => "x",
        '⚠' => "!",
        '∅' => "(none)",
        '‘' | '’' | '′' => "'",
        '“' | '”' | '″' => "\"",
        '«' => "<<",
        '»' => ">>",
        '\u{a0}' => " ",
        '▁' => "_",
        '▂' | '▃' => ".",
        '▄' => "-",
        '▅' | '▆' => "=",
        '▇' | '█' => "#",
        _ => return None,
    })
}

/// A writer that passes UTF-8 text through [`console`] on its way to `inner`. Bytes that are not
/// UTF-8 go through unchanged.
pub struct Writer<W>(pub W);

impl<W: Write> Write for Writer<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match std::str::from_utf8(buf) {
            Ok(text) if active() => {
                self.0.write_all(console(text).as_bytes())?;
                Ok(buf.len())
            }
            _ => self.0.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// Standard error through [`Writer`]: a `tracing_subscriber` `MakeWriter` (`with_writer(ascii::stderr)`).
pub fn stderr() -> Writer<io::Stderr> {
    Writer(io::stderr())
}

/// Standard output through [`Writer`].
pub fn stdout() -> Writer<io::Stdout> {
    Writer(io::stdout())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_doctor_rule_and_dash_become_ascii() {
        assert_eq!(plain("──── a — b → c §9 …"), "---- a -- b -> c sec. 9 ...");
    }

    #[test]
    fn text_without_symbols_is_not_copied() {
        assert!(matches!(plain("EKOS Doctor"), Cow::Borrowed(_)));
    }

    #[test]
    fn other_non_ascii_text_is_data_and_kept() {
        assert_eq!(plain(r"C:\Users\Алексей\é — x"), r"C:\Users\Алексей\é -- x");
    }

    #[test]
    fn every_replacement_is_ascii() {
        for c in (0x80u32..0x3_0000).filter_map(char::from_u32) {
            if let Some(r) = replacement(c) {
                assert!(r.is_ascii(), "{c:?} -> {r:?}");
            }
        }
    }

    #[test]
    fn json_is_recognised_and_left_alone() {
        assert!(is_json("{\"note\": \"a — b\"}\n"));
        assert!(is_json("[1, 2]"));
        assert!(!is_json("[OK]   Rust toolchain — x"));
        assert!(!is_json("plain — text"));
    }

    #[test]
    fn on_by_default_only_on_windows() {
        assert!(decide(None, true));
        assert!(!decide(None, false));
        assert!(decide(Some("1"), false));
        assert!(!decide(Some("0"), true));
        assert!(decide(Some("other"), true));
    }

    #[test]
    fn the_writer_passes_bytes_that_are_not_utf8() {
        let mut w = Writer(Vec::new());
        w.write_all(&[0xff, 0xfe]).unwrap();
        assert_eq!(w.0, vec![0xff, 0xfe]);
    }
}
