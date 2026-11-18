//! Where the window may go (RFC 0261 §4): the local start-up page and the console's own origin.
//! Any other address opens in the system browser instead.

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

/// Whether the window may load `url`, given the console's origin once it is known.
pub fn allowed(url: &Url, console: Option<&Url>) -> bool {
    if is_startup_page(url) || url.scheme() == "about" {
        return true;
    }
    console.is_some_and(|c| c.origin() == url.origin())
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
        assert!(allowed(&u("tauri://localhost/index.html"), None));
        assert!(allowed(&u("http://tauri.localhost/index.html"), None));
        assert!(!allowed(&u("http://127.0.0.1:8000/w/1"), None));
        assert!(allowed(&u("http://127.0.0.1:8000/w/1"), Some(&console)));
        assert!(!allowed(&u("http://127.0.0.1:8001/"), Some(&console)));
        assert!(!allowed(&u("http://localhost:8000/"), Some(&console)));
        assert!(!allowed(&u("https://github.com/x"), Some(&console)));
    }
}
