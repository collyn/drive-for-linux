// Tests build their settings step by step from Default; that reads clearer there.
#![cfg_attr(test, allow(clippy::field_reassign_with_default))]

mod app;
mod badges;
mod diag;
mod engine;
mod filemenu;
mod gdocs;
mod i18n;
mod icons;
mod mirror;
mod ransom;
mod settings;
mod theme;
mod tray;

use eframe::egui;

/// The name people see (window, tray, notifications, menus). Internal names (the program file
/// `drive-linux`, the config folder) stay as they were so settings carry over.
pub const APP_NAME: &str = "Drive For Linux";
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::Mutex;

/// The listening end that makes this the only running copy (the original runs once per user).
pub static INSTANCE: Mutex<Option<UnixListener>> = Mutex::new(None);

fn instance_socket() -> std::path::PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let user = std::env::var("USER").unwrap_or_default();
    dir.join(format!("drive-linux-{user}.sock"))
}

/// Held for the whole run: the original's single-instance lock (single_instance_mutex_posix.cc,
/// function 0x1001c76a0) — a lock file taken with an exclusive, non-blocking flock.
static LOCK_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

fn take_instance_lock() -> Option<bool> {
    let path = instance_socket().with_extension("lock");
    let open = || std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path);
    // Like the original: if the lock file can't be opened, remove it and try once more.
    let file = match open() {
        Ok(f) => f,
        Err(_) => {
            let _ = std::fs::remove_file(&path);
            open().ok()?
        }
    };
    match file.try_lock() {
        Ok(()) => {
            *LOCK_FILE.lock().unwrap() = Some(file);
            Some(true)
        }
        Err(std::fs::TryLockError::WouldBlock) => Some(false), // "Lock file is held by another process"
        Err(_) => None,
    }
}

/// File manager menu actions given on the command line (`--action <name> <path>`), handled once
/// the app is up when no other copy is running.
pub static PENDING_ACTIONS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

fn send_to_running(path: &std::path::Path, msg: &str) -> bool {
    use std::io::Write;
    match UnixStream::connect(path) {
        Ok(mut s) => {
            let _ = writeln!(s, "{msg}");
            true
        }
        Err(_) => false,
    }
}

/// True when another copy is already running (it gets `msg`: show its window, or run an action).
fn already_running(msg: &str) -> bool {
    let path = instance_socket();
    if take_instance_lock() == Some(false) {
        let _ = send_to_running(&path, msg);
        return true;
    }
    if send_to_running(&path, msg) {
        return true;
    }
    // No one answered: a leftover socket from a copy that ended; take it over.
    let _ = std::fs::remove_file(&path);
    if let Ok(l) = UnixListener::bind(&path) {
        *INSTANCE.lock().unwrap() = Some(l);
    }
    false
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    // Wayland doesn't let an app hide its own window, which "close to the tray" needs; use the
    // X11 connection (XWayland) when the desktop offers one.
    if std::env::var_os("DISPLAY").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_some() {
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    // Double-clicked .gdoc/.gsheet/.gslides link file: open the document in the browser.
    if let Some(file) = args.iter().position(|a| a == "--open-doc").and_then(|i| args.get(i + 1)) {
        let ok = gdocs::open(std::path::Path::new(file));
        std::process::exit(if ok { 0 } else { 1 });
    }
    let screenshot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let action = args.iter().position(|a| a == "--action").and_then(|i| Some((args.get(i + 1)?.clone(), args.get(i + 2)?.clone())));
    let msg = match &action {
        Some((name, path)) => format!("action {name}\t{path}"),
        None => "open".to_string(),
    };
    if screenshot.is_none() && already_running(&msg) {
        // Two copies would overwrite each other's settings; the running one shows its window.
        return Ok(());
    }

    if let Some(a) = action {
        PENDING_ACTIONS.lock().unwrap().push(a);
    }

    // On X11 the window class is completed in app::set_window_class.
    let mut viewport =
        egui::ViewportBuilder::default().with_title(APP_NAME).with_app_id("drive-linux").with_inner_size([1100.0, 720.0]).with_min_inner_size([860.0, 560.0]).with_icon(theme::app_icon());
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    // Linux-only app: the macOS "don't steal focus" builder option is not needed here.
    let options = eframe::NativeOptions { viewport, ..Default::default() };

    eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(app::DriveApp::new(cc, screenshot)))))
}
