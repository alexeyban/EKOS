//! Where the window may go (RFC 0261 §4): the local start-up page, the console's own origin, and
//! while Set up runs the set-up wizard's (§8). Any other address opens in the system browser.

use url::Url;

/// The start-up page's origins (`tauri://localhost` on Linux and macOS, `http://tauri.localhost`
/// on Windows).
fn is_startup_page(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    }
}

/// RFC 0261 §9: a page `ekos` serves on this machine for one person, with its one-time token in
/// the URL (`?t=`): the set-up wizard and the uninstall page (RFC 0188). They open in the window,
/// not in a browser.
fn is_local_token_page(url: &Url) -> bool {
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]"));
    url.scheme() == "http" && loopback && url.query_pairs().any(|(k, v)| k == "t" && v.len() >= 32)
}

/// Whether the window may load `url`, given the console's origin once it is known and the
/// wizard's while it runs.
pub fn allowed(url: &Url, console: Option<&Url>, setup: Option<&Url>) -> bool {
    if is_startup_page(url) || url.scheme() == "about" || is_local_token_page(url) {
        return true;
    }
    [console, setup]
        .into_iter()
        .flatten()
        .any(|o| o.origin() == url.origin())
}

/// Opens `url` in the person's default browser. Failing is not fatal: nothing else depends on it.
pub fn open_externally(url: &str) {
    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn only_the_start_up_page_and_the_console_origin_load_in_the_window() {
        let console = u("http://127.0.0.1:8000");
        assert!(allowed(&u("tauri://localhost/index.html"), None, None));
        assert!(allowed(&u("http://tauri.localhost/index.html"), None, None));
        assert!(!allowed(&u("http://127.0.0.1:8000/w/1"), None, None));
        assert!(allowed(
            &u("http://127.0.0.1:8000/w/1"),
            Some(&console),
            None
        ));
        assert!(!allowed(&u("http://127.0.0.1:8001/"), Some(&console), None));
        assert!(!allowed(&u("http://localhost:8000/"), Some(&console), None));
        assert!(!allowed(&u("https://github.com/x"), Some(&console), None));
    }

    #[test]
    fn a_local_page_with_its_token_opens_in_the_window() {
        let token = "a".repeat(64);
        assert!(allowed(
            &u(&format!("http://127.0.0.1:41000/?t={token}")),
            None,
            None
        ));
        assert!(!allowed(&u("http://127.0.0.1:41000/?t=short"), None, None));
        assert!(!allowed(
            &u(&format!("http://example.com/?t={token}")),
            None,
            None
        ));
        assert!(!allowed(
            &u(&format!("https://127.0.0.1:41000/?t={token}")),
            None,
            None
        ));
    }

    #[test]
    fn the_wizard_loads_only_while_set_up_runs() {
        let wizard = u("http://127.0.0.1:40123/?t=abc&app=1");
        assert!(allowed(
            &u("http://127.0.0.1:40123/api/plan"),
            None,
            Some(&wizard)
        ));
        assert!(!allowed(&u("http://127.0.0.1:40123/"), None, None));
        assert!(!allowed(&u("http://127.0.0.1:40124/"), None, Some(&wizard)));
    }
}
