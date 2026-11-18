//! EKOS.DEV desktop shell (RFC 0261, D0 spike).
//!
//! A window that shows the console's own UI, plus the four desktop moments of the design: the
//! start-up screen, the tray menu, the end-of-run notification and the quit dialog that knows
//! about runs in progress. The console's lifecycle goes through `ekos console …` only.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod ekos;
mod nav;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};
use tauri_plugin_notification::NotificationExt;
use url::Url;

use crate::api::{Api, Project, Run, SignedOut};
use crate::ekos::Ekos;

const MAIN: &str = "main";
const TRAY: &str = "ekos-tray";
const POLL: Duration = Duration::from_secs(3);
/// Projects are listed again every this many polls (30 s).
const PROJECTS_EVERY: u32 = 10;

// The quit dialog's labels (design: "Quitting with a run in progress"). On Linux, closing the
// dialog reports its third ("cancel") button, so that slot holds the harmless choice.
const WAIT_THEN_QUIT: &str = "Wait for the run, then quit";
const CANCEL_AND_QUIT: &str = "Cancel the run and quit";
const KEEP_RUNNING: &str = "Keep EKOS.DEV running in the background";

/// Everything the shell knows, shared by the start-up thread, the poller and menu handlers.
struct Shell {
    ekos: Ekos,
    /// The console's origin, once the start-up steps found it; the window may load only it.
    console: Mutex<Option<Url>>,
    /// The shell ran `ekos console start`, so quitting stops the console again.
    started_console: AtomicBool,
    /// "Wait for the run, then quit" was chosen.
    quit_when_idle: AtomicBool,
    api: Mutex<Option<Api>>,
    active: Mutex<Vec<Run>>,
    watch: Mutex<api::RunWatch>,
    projects: Mutex<Vec<Project>>,
    /// The tray's last shown state, so the menu is rebuilt only when it changes.
    shown: Mutex<(String, Vec<Project>)>,
    startup_begun: AtomicBool,
}

type Shared = Arc<Shell>;

fn main() {
    let shell: Shared = Arc::new(Shell {
        ekos: Ekos::from_env(),
        console: Mutex::new(None),
        started_console: AtomicBool::new(false),
        quit_when_idle: AtomicBool::new(false),
        api: Mutex::new(None),
        active: Mutex::new(Vec::new()),
        watch: Mutex::new(api::RunWatch::default()),
        projects: Mutex::new(Vec::new()),
        shown: Mutex::new((String::new(), Vec::new())),
        startup_begun: AtomicBool::new(false),
    });
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(shell.clone())
        .invoke_handler(tauri::generate_handler![
            startup_ready,
            show_log,
            open_in_browser
        ])
        .setup(move |app| {
            build_window(app.handle(), shell.clone())?;
            build_tray(app.handle(), &shell)?;
            let handle = app.handle().clone();
            let poller = shell.clone();
            std::thread::spawn(move || poll_loop(&handle, &poller));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("running the EKOS.DEV shell");
}

fn build_window(app: &AppHandle, shell: Shared) -> tauri::Result<WebviewWindow> {
    let guard = shell.clone();
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
        .title("EKOS.DEV")
        .inner_size(1440.0, 940.0)
        .min_inner_size(960.0, 640.0)
        .on_navigation(move |url| {
            let console = guard.console.lock().unwrap().clone();
            if nav::allowed(url, console.as_ref()) {
                true
            } else {
                nav::open_externally(url.as_str());
                false
            }
        })
        .build()
        .inspect(|w| {
            let win = w.clone();
            w.on_window_event(move |event| {
                // Closing the window keeps the app (and the console) in the tray.
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = win.hide();
                }
            });
        })
}

// ── start-up screen ──────────────────────────────────────────────────────────

/// Called by the start-up page once it has loaded, so no step is reported before it can show.
#[tauri::command]
fn startup_ready(app: AppHandle, shell: tauri::State<'_, Shared>) {
    if shell.startup_begun.swap(true, Ordering::SeqCst) {
        return;
    }
    let shell = shell.inner().clone();
    std::thread::spawn(move || {
        if let Err(e) = start_up(&app, &shell) {
            page(&app, &format!("ekos.fail({})", js(&format!("{e:#}"))));
            if let Ok(tail) = shell.ekos.log_tail(20) {
                page(&app, &format!("ekos.log({})", js(&tail)));
            }
        }
    });
}

#[tauri::command]
fn show_log(app: AppHandle, shell: tauri::State<'_, Shared>) {
    let shell = shell.inner().clone();
    std::thread::spawn(move || {
        let text = shell.ekos.log_tail(60).unwrap_or_else(|e| format!("{e:#}"));
        page(&app, &format!("ekos.log({})", js(&text)));
    });
}

#[tauri::command]
fn open_in_browser(shell: tauri::State<'_, Shared>) {
    let shell = shell.inner().clone();
    std::thread::spawn(move || {
        let _ = shell.ekos.open_in_browser();
    });
}

/// The design's three steps: found the EKOS home, starting the console, signing you in.
fn start_up(app: &AppHandle, shell: &Shared) -> anyhow::Result<()> {
    page(app, "ekos.step(1,'running')");
    let running = shell.ekos.is_running()?;
    page(app, "ekos.step(1,'done')");

    page(app, "ekos.step(2,'running')");
    if !running {
        page(
            app,
            &format!("ekos.detail({})", js("waiting for the console to answer")),
        );
        shell.ekos.start()?;
        shell.started_console.store(true, Ordering::SeqCst);
    }
    let target = shell.ekos.open_target()?;
    page(app, "ekos.step(2,'done')");

    page(app, "ekos.step(3,'running')");
    // Team mode (no one-time code): v1 does not sign in through a provider (RFC 0261 §2.3 step 4).
    let Some(signin) = target.signin_url() else {
        page(app, "ekos.teamMode()");
        return Ok(());
    };
    let origin = Url::parse(&target.url)?;
    *shell.console.lock().unwrap() = Some(origin);
    sign_in_shell(shell, &target.url);
    page(app, "ekos.step(3,'done')");

    if let Some(w) = app.get_webview_window(MAIN) {
        w.navigate(Url::parse(&signin)?)?;
    }
    Ok(())
}

/// The shell's own session (a second one-time code). RFC 0261 §6.2 asks whether a hidden
/// window keeps its event stream; until D0 has measured that, the shell watches runs itself.
fn sign_in_shell(shell: &Shared, base: &str) {
    let api = shell
        .ekos
        .open_target()
        .ok()
        .and_then(|t| t.signin_url())
        .and_then(|link| Api::sign_in(base, &link).ok());
    *shell.api.lock().unwrap() = api;
}

fn page(app: &AppHandle, script: &str) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.eval(script);
    }
}

/// A JavaScript string literal.
fn js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

// ── runs, notifications, tray ────────────────────────────────────────────────

fn poll_loop(app: &AppHandle, shell: &Shared) {
    let mut tick: u32 = 0;
    loop {
        std::thread::sleep(POLL);
        let base = shell.console.lock().unwrap().clone();
        let Some(base) = base else { continue };
        if poll_once(app, shell, tick.is_multiple_of(PROJECTS_EVERY))
            .is_err_and(|e| e.is::<SignedOut>())
        {
            sign_in_shell(shell, base.as_str());
        }
        tick = tick.wrapping_add(1);
        refresh_tray(app, shell);
        if shell.quit_when_idle.load(Ordering::SeqCst) && shell.active.lock().unwrap().is_empty() {
            quit_now(app, shell);
        }
    }
}

fn poll_once(app: &AppHandle, shell: &Shared, with_projects: bool) -> anyhow::Result<()> {
    let guard = shell.api.lock().unwrap();
    let Some(api) = guard.as_ref() else {
        return Ok(());
    };
    if with_projects {
        *shell.projects.lock().unwrap() = api.projects()?;
    }
    let recent = api.recent_runs()?;
    *shell.active.lock().unwrap() = recent
        .iter()
        .filter(|r| api::is_active(r))
        .cloned()
        .collect();
    let news: Vec<Run> = shell
        .watch
        .lock()
        .unwrap()
        .observe(&recent)
        .into_iter()
        .cloned()
        .collect();
    for run in news {
        let project = shell
            .projects
            .lock()
            .unwrap()
            .iter()
            .find(|p| p.id == run.workspace_id)
            .map(|p| p.name.clone());
        let (title, body) = api::ended_message(&run, project.as_deref());
        let _ = app.notification().builder().title(title).body(body).show();
    }
    Ok(())
}

fn tray_menu(
    app: &AppHandle,
    status: &str,
    projects: &[Project],
) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "status",
        status,
        false,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open EKOS.DEV",
        true,
        None::<&str>,
    )?)?;
    for p in projects {
        menu.append(&MenuItem::with_id(
            app,
            format!("project:{}", p.id),
            &p.name,
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "browser",
        "Open in browser",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "restart",
        "Restart the console",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit EKOS.DEV",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

fn build_tray(app: &AppHandle, shell: &Shared) -> tauri::Result<TrayIcon> {
    let status = "Starting…".to_string();
    let menu = tray_menu(app, &status, &[])?;
    *shell.shown.lock().unwrap() = (status, Vec::new());
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("EKOS.DEV")
        .menu(&menu)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()));
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)
}

fn refresh_tray(app: &AppHandle, shell: &Shared) {
    let status = api::status_line(
        shell.active.lock().unwrap().len(),
        shell.quit_when_idle.load(Ordering::SeqCst),
    );
    let projects = shell.projects.lock().unwrap().clone();
    let mut shown = shell.shown.lock().unwrap();
    if shown.0 == status && shown.1 == projects {
        return;
    }
    let Some(tray) = app.tray_by_id(TRAY) else {
        return;
    };
    if let Ok(menu) = tray_menu(app, &status, &projects) {
        let _ = tray.set_menu(Some(menu));
        *shown = (status, projects);
    }
}

fn on_menu(app: &AppHandle, id: &str) {
    let shell = app.state::<Shared>().inner().clone();
    match id {
        "open" => show_window(app, None),
        "browser" => {
            std::thread::spawn(move || {
                let _ = shell.ekos.open_in_browser();
            });
        }
        "restart" => {
            let app = app.clone();
            std::thread::spawn(move || restart_console(&app, &shell));
        }
        "quit" => {
            let app = app.clone();
            std::thread::spawn(move || ask_to_quit(&app, &shell));
        }
        other => {
            if let Some(project) = other.strip_prefix("project:") {
                show_window(app, Some(&format!("/w/{project}")));
            }
        }
    }
}

fn show_window(app: &AppHandle, path: Option<&str>) {
    let Some(w) = app.get_webview_window(MAIN) else {
        return;
    };
    if let Some(path) = path {
        let console = app.state::<Shared>().console.lock().unwrap().clone();
        if let Some(dest) = console.and_then(|c| c.join(path).ok()) {
            let _ = w.navigate(dest);
        }
    }
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}

/// `ekos console restart`, then a fresh sign-in for the window and the shell (a restart keeps
/// sessions only if the console's secret is stable, which the shell does not assume).
fn restart_console(app: &AppHandle, shell: &Shared) {
    if shell.ekos.restart().is_err() {
        return;
    }
    let Ok(target) = shell.ekos.open_target() else {
        return;
    };
    sign_in_shell(shell, &target.url);
    let dest = target.signin_url().unwrap_or_else(|| target.url.clone());
    if let (Some(w), Ok(dest)) = (app.get_webview_window(MAIN), Url::parse(&dest)) {
        let _ = w.navigate(dest);
    }
}

// ── quit ─────────────────────────────────────────────────────────────────────

/// Runs on its own thread: the dialog blocks until the person answers.
fn ask_to_quit(app: &AppHandle, shell: &Shared) {
    let active = shell.active.lock().unwrap().clone();
    let Some(first) = active.first() else {
        quit_now(app, shell);
        return;
    };
    let project = shell
        .projects
        .lock()
        .unwrap()
        .iter()
        .find(|p| p.id == first.workspace_id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| first.workspace_id.clone());
    let others = match active.len() {
        1 => String::new(),
        n => format!(" ({} more runs are in progress too)", n - 1),
    };
    // D0 finding: a dialog without a parent opened behind other windows while the main window
    // was hidden. Show the window and parent the dialog to it.
    show_window(app, None);
    let mut dialog = app
        .dialog()
        .message(format!(
            "{} on {project} is {}{others}. Quitting now would lose it.",
            api::command_words(&first.command),
            first.status
        ))
        .title("A run is in progress")
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::YesNoCancelCustom(
            WAIT_THEN_QUIT.into(),
            CANCEL_AND_QUIT.into(),
            KEEP_RUNNING.into(),
        ));
    if let Some(w) = app.get_webview_window(MAIN) {
        dialog = dialog.parent(&w);
    }
    let answer = dialog.blocking_show_with_result();
    match answer {
        MessageDialogResult::Custom(label) if label == WAIT_THEN_QUIT => {
            shell.quit_when_idle.store(true, Ordering::SeqCst);
            hide_window(app);
        }
        MessageDialogResult::Custom(label) if label == CANCEL_AND_QUIT => {
            if let Some(api) = shell.api.lock().unwrap().as_ref() {
                for run in &active {
                    let _ = api.cancel(&run.id);
                }
            }
            quit_now(app, shell);
        }
        // "Keep running in the background", or the dialog closed.
        _ => hide_window(app),
    }
}

fn hide_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
}

/// Stops the console only when the shell started it, then exits.
fn quit_now(app: &AppHandle, shell: &Shared) {
    if shell.started_console.load(Ordering::SeqCst) {
        let _ = shell.ekos.stop();
    }
    app.exit(0);
}
