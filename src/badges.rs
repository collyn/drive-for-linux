//! Sync status badges on files in the file manager, like the original's Finder badges
//! (FinderSyncExtension, -[DFSFinderSync requestBadgeIdentifierForURL:] 0x10001be80 and
//! -[DFSFinderSync setMirrorSyncStatusOverlay:] 0x100021ab0). Badge identifiers, in the original's
//! order: "pinned", "cloud", "mirror_root", "synced", "syncing", "blacklisted", "cse".
//! - Mirrored items: status 1 → "synced", 2 → "syncing", 3 → "blacklisted"; any other status gets
//!   no badge ("Unable to set badge for …").
//! - Streamed items: "pinned" when kept available offline, else "cloud" ("cse" for client-side
//!   encrypted files, which this app doesn't make).
//!
//! File managers ask through a local socket ($XDG_RUNTIME_DIR/drive-linux-$USER-status.sock):
//! `Q <path>` lines ended by a `.` line get one badge per line back (empty for none); `WATCH`
//! keeps the connection open and gets `CHANGED` whenever statuses may have changed.
use crate::engine::Shared;
use crate::mirror::{same, Entry, State};
use crate::settings::*;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let user = std::env::var("USER").unwrap_or_default();
    dir.join(format!("drive-linux-{user}-status.sock"))
}

/// `*` and `?` wildcards, as in the ignore list.
pub fn glob(pat: &str, name: &str) -> bool {
    let (p, n): (Vec<char>, Vec<char>) = (pat.chars().collect(), name.chars().collect());
    let (mut i, mut j, mut star, mut mark) = (0usize, 0usize, None::<usize>, 0usize);
    while j < n.len() {
        if i < p.len() && (p[i] == '?' || p[i] == n[j]) {
            i += 1;
            j += 1;
        } else if i < p.len() && p[i] == '*' {
            star = Some(i);
            mark = j;
            i += 1;
        } else if let Some(s) = star {
            i = s + 1;
            mark += 1;
            j = mark;
        } else {
            return false;
        }
    }
    while i < p.len() && p[i] == '*' {
        i += 1;
    }
    i == p.len()
}

fn ignored(rel: &str) -> bool {
    rel.split('/').any(|part| IGNORE_NAMES.iter().any(|g| glob(g, part)))
}

/// One mirrored folder and the list it last synced.
struct Mirror {
    folder: PathBuf,
    state: PathBuf,
}

/// Saved file lists, reloaded when their file changes.
pub struct Cache {
    /// Where the saved lists live (the app's config folder).
    state_dir: PathBuf,
    states: BTreeMap<PathBuf, (Option<SystemTime>, BTreeMap<String, Entry>)>,
}

impl Default for Cache {
    fn default() -> Self {
        Cache { state_dir: config_dir(), states: BTreeMap::new() }
    }
}

impl Cache {
    fn files(&mut self, path: &Path) -> &BTreeMap<String, Entry> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let stale = self.states.get(path).map(|(t, _)| *t != mtime).unwrap_or(true);
        if stale {
            let files = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str::<State>(&t).ok()).map(|s| s.files).unwrap_or_default();
            self.states.insert(path.to_path_buf(), (mtime, files));
        }
        &self.states[path].1
    }
}

fn mirrors(st: &Settings, sh: &Shared, state_dir: &Path) -> Vec<Mirror> {
    let mut out = Vec::new();
    for acc in &sh.accounts {
        let s = st.for_account(acc);
        if s.drive_mode != DriveMode::Mirror {
            continue;
        }
        out.push(Mirror { folder: PathBuf::from(&s.mirror_folder), state: state_dir.join(format!("mirror-state-{acc}.json")) });
        let enabled = s.synced_shared_drives.get(acc).cloned().unwrap_or_default();
        for (id, name) in sh.drive_list.get(acc).into_iter().flatten().filter(|(id, _)| enabled.contains(id)) {
            out.push(Mirror { folder: s.shared_drive_folder(acc, name), state: state_dir.join(format!("mirror-state-{acc}-sd-{id}.json")) });
        }
    }
    out
}

fn file_entry(p: &Path) -> Option<Entry> {
    let m = std::fs::metadata(p).ok()?;
    let mtime = m.modified().ok()?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(Entry { size: m.len() as i64, mtime })
}

/// The badge for one local path ("" when it gets none).
pub fn status(path: &str, st: &Settings, sh: &Shared, cache: &mut Cache) -> &'static str {
    let p = Path::new(path);
    let busy = |root: &Path, name: &str| sh.transfers.iter().any(|t| Path::new(&t.place) == root && t.name == name);
    // Mirrored My Drive and shared drives.
    let state_dir = cache.state_dir.clone();
    for m in mirrors(st, sh, &state_dir) {
        let Ok(rel) = p.strip_prefix(&m.folder) else { continue };
        let rel = rel.to_string_lossy().to_string();
        if rel.is_empty() {
            return "mirror_root";
        }
        if ignored(&rel) {
            return "blacklisted";
        }
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let failed = sh.activity.iter().take(20).any(|a| a.name == name && matches!(a.state, crate::engine::ActState::Error(_)));
        if p.is_dir() {
            // A folder is syncing while a file below it is moving (transfers carry file names only).
            let prefix = format!("{rel}/");
            let moving: Vec<&str> = sh.transfers.iter().filter(|t| Path::new(&t.place) == m.folder).map(|t| t.name.as_str()).collect();
            if moving.is_empty() {
                return "synced";
            }
            let files = cache.files(&m.state);
            let below = moving.iter().any(|n| p.join(n).exists() || files.keys().any(|k| k.starts_with(&prefix) && k.ends_with(&format!("/{n}"))));
            return if below { "syncing" } else { "synced" };
        }
        if busy(&m.folder, &name) {
            return "syncing";
        }
        if crate::gdocs::is_link(&name) {
            return "synced"; // a Google document's link file, rewritten by every mirror pass
        }
        if failed {
            return "blacklisted";
        }
        let files = cache.files(&m.state);
        return match (files.get(&rel), file_entry(p)) {
            (Some(saved), Some(now)) if same(saved, &now) => "synced",
            (_, Some(_)) => "syncing", // new or changed here, not uploaded yet
            _ => "",
        };
    }
    // Streamed folders: available offline or online only.
    for acc in &sh.accounts {
        let s = st.for_account(acc);
        if s.drive_mode != DriveMode::Stream {
            continue;
        }
        let root = PathBuf::from(s.stream_dir(acc));
        let Ok(rel) = p.strip_prefix(&root) else { continue };
        if rel.as_os_str().is_empty() || rel == Path::new("My Drive") || rel == Path::new("Shared drives") {
            return "";
        }
        let pinned = st.offline_pins.iter().any(|pin| p == Path::new(pin) || p.starts_with(pin));
        return if pinned { "pinned" } else { "cloud" };
    }
    // Folders backed up from this computer: the folder itself shows whether it is up to date.
    for f in &st.folders {
        if p == Path::new(&f.path) {
            let running = sh.job.is_some() && sh.transfers.iter().any(|t| Path::new(&t.place).starts_with(&f.path) || t.place.ends_with(&f.path));
            return if running { "syncing" } else { "synced" };
        }
    }
    ""
}

/// What changes the badges; when it differs, watchers are told.
fn fingerprint(st: &Settings, sh: &Shared) -> String {
    let mut s = format!("{:?}|{}|{:?}|{}|{}|", sh.job, sh.transfers.len(), sh.last_done, st.paused, st.offline_pins.len());
    for t in &sh.transfers {
        s.push_str(&t.name);
    }
    if let Some(a) = sh.activity.front() {
        s.push_str(&format!("{}{:?}", a.name, a.at));
    }
    for k in sh.ransomware.keys() {
        s.push_str(k);
    }
    s
}

fn answer(mut conn: UnixStream, shared: &Arc<Mutex<Shared>>, settings: &Arc<Mutex<Settings>>, cache: &Arc<Mutex<Cache>>, watchers: &Arc<Mutex<Vec<UnixStream>>>) {
    let _ = conn.set_read_timeout(Some(Duration::from_secs(2)));
    let Ok(reader) = conn.try_clone() else { return };
    let mut paths = Vec::new();
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        if line == "WATCH" {
            let _ = conn.set_read_timeout(None);
            watchers.lock().unwrap().push(conn);
            return;
        }
        if line == "." {
            break;
        }
        if let Some(p) = line.strip_prefix("Q ") {
            paths.push(p.to_string());
        }
        if paths.len() > 10_000 {
            break;
        }
    }
    let st = settings.lock().unwrap().clone();
    let mut out = String::new();
    {
        let sh = shared.lock().unwrap();
        let mut c = cache.lock().unwrap();
        for p in &paths {
            out.push_str(status(p, &st, &sh, &mut c));
            out.push('\n');
        }
    }
    let _ = conn.write_all(out.as_bytes());
}

/// Starts answering file managers (not in screenshot mode).
pub fn serve(shared: Arc<Mutex<Shared>>, settings: Arc<Mutex<Settings>>) {
    let path = socket_path();
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return };
    let cache = Arc::new(Mutex::new(Cache::default()));
    let watchers: Arc<Mutex<Vec<UnixStream>>> = Arc::default();
    {
        let (shared, settings, watchers) = (shared.clone(), settings.clone(), watchers.clone());
        std::thread::spawn(move || {
            let mut last = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(1500));
                let st = settings.lock().unwrap().clone();
                let fp = fingerprint(&st, &shared.lock().unwrap());
                if fp != last {
                    last = fp;
                    watchers.lock().unwrap().retain_mut(|w| w.write_all(b"CHANGED\n").is_ok());
                }
            }
        });
    }
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let (shared, settings, cache, watchers) = (shared.clone(), settings.clone(), cache.clone(), watchers.clone());
            std::thread::spawn(move || answer(conn, &shared, &settings, &cache, &watchers));
        }
    });
}

// ---------------------------------------------------------------- installing into file managers

/// The Dolphin plugin built by build.rs (empty when it couldn't be built).
const DOLPHIN_PLUGIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/drivelinux_overlay.so"));

/// Emblems drawn for this app (simple shapes; not Google's artwork), one per badge.
const ICONS: &[(&str, &str)] = &[
    (
        "synced",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#188038" stroke="#fff" stroke-width="1"/><path d="M4.6 8.3l2.2 2.2 4.6-4.7" fill="none" stroke="#fff" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "syncing",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#0b57d0" stroke="#fff" stroke-width="1"/><path d="M11.2 7a3.3 3.3 0 0 0-5.8-1.4M4.8 9a3.3 3.3 0 0 0 5.8 1.4" fill="none" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/><path d="M3.9 4.2l2.6.2-1.3 2.3zM12.1 11.8l-2.6-.2 1.3-2.3z" fill="#fff"/></svg>"##,
    ),
    (
        "blacklisted",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#b3261e" stroke="#fff" stroke-width="1"/><rect x="4" y="7" width="8" height="2" rx="1" fill="#fff"/></svg>"##,
    ),
    (
        "cloud",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#fff" stroke="#5f6368" stroke-width="1"/><path d="M5.3 11h5.6a2.2 2.2 0 0 0 .3-4.4 3 3 0 0 0-5.7.9A1.8 1.8 0 0 0 5.3 11z" fill="none" stroke="#5f6368" stroke-width="1.2" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "pinned",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#188038" stroke="#fff" stroke-width="1"/><path d="M8 3.8v5.4M5.7 7l2.3 2.3 2.3-2.3M5 11.7h6" fill="none" stroke="#fff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>"##,
    ),
    (
        "mirror_root",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#0b57d0" stroke="#fff" stroke-width="1"/><path d="M5.3 11h5.6a2.2 2.2 0 0 0 .3-4.4 3 3 0 0 0-5.7.9A1.8 1.8 0 0 0 5.3 11z" fill="#fff"/></svg>"##,
    ),
    (
        "cse",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="7.3" fill="#5f6368" stroke="#fff" stroke-width="1"/><rect x="5" y="7.3" width="6" height="4.4" rx="1" fill="#fff"/><path d="M6.4 7.3V6a1.6 1.6 0 0 1 3.2 0v1.3" fill="none" stroke="#fff" stroke-width="1.3"/></svg>"##,
    ),
];

/// GNOME Files (needs the nautilus-python package): adds the same emblems.
const NAUTILUS_EXTENSION: &str = r#"# Added by Drive For Linux: sync status emblems on files.
import os, socket
from urllib.parse import unquote, urlparse
import gi
for v in ("4.1", "4.0", "3.0"):
    try:
        gi.require_version("Nautilus", v)
        break
    except ValueError:
        pass
from gi.repository import GObject, Nautilus

SOCK = os.path.join(os.environ.get("XDG_RUNTIME_DIR", "/tmp"), "drive-linux-%s-status.sock" % os.environ.get("USER", ""))

def ask(path):
    try:
        s = socket.socket(socket.AF_UNIX)
        s.settimeout(0.3)
        s.connect(SOCK)
        s.sendall(("Q %s\n.\n" % path).encode())
        data = b""
        while True:
            chunk = s.recv(4096)
            if not chunk:
                break
            data += chunk
        s.close()
        return data.decode().split("\n")[0]
    except OSError:
        return ""

class DriveLinuxBadges(GObject.GObject, Nautilus.InfoProvider):
    def update_file_info(self, file):
        if file.get_uri_scheme() != "file":
            return
        badge = ask(unquote(urlparse(file.get_uri()).path).rstrip("/") or "/")
        if badge:
            file.add_emblem("drivelinux_" + badge)
"#;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// The icon a badge is shown with ("synced" → "drivelinux_synced").
pub fn icon_name(badge: &str) -> String {
    format!("drivelinux_{badge}")
}

pub fn dolphin_plugin_path() -> PathBuf {
    home().join(".local/lib/qt6/plugins/kf6/overlayicon/drivelinux_overlay.so")
}

fn plasma_env_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("plasma-workspace/env/drive-linux.sh")
}

/// For desktops that start through systemd (read at login, like the Plasma file).
fn environment_d_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("environment.d/90-drive-linux.conf")
}

fn icon_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("icons/hicolor/scalable/emblems")
}

fn nautilus_extension_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("nautilus-python/extensions/drive_linux_badges.py")
}

/// Writes a file only when it changed, through a temporary file, so a running file manager that
/// has the old plugin loaded is never handed a half-written one.
fn write_if_changed(path: &Path, bytes: &[u8]) -> bool {
    if std::fs::read(path).map(|b| b == bytes).unwrap_or(false) {
        return false;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).is_ok() && std::fs::rename(&tmp, path).is_ok()
}

/// Installs (or removes) the status badges for Dolphin and GNOME Files. True when the Dolphin
/// plugin was newly put in place.
pub fn install(on: bool) -> bool {
    // Names without "-": KDE shortens unknown dashed names ("drive-linux-synced" → "drive") and
    // would find another icon in the person's theme before looking in hicolor.
    let files: Vec<PathBuf> = ICONS.iter().map(|(n, _)| icon_dir().join(format!("{}.svg", icon_name(n)))).collect();
    if !on {
        for p in files.iter().chain([dolphin_plugin_path(), plasma_env_path(), environment_d_path(), nautilus_extension_path()].iter()) {
            let _ = std::fs::remove_file(p);
        }
        return false;
    }
    for ((_, svg), p) in ICONS.iter().zip(&files) {
        write_if_changed(p, svg.as_bytes());
    }
    let mut new_plugin = false;
    if !DOLPHIN_PLUGIN.is_empty() {
        new_plugin = write_if_changed(&dolphin_plugin_path(), DOLPHIN_PLUGIN);
        // Dolphin looks for plugins on Qt's plugin path; Plasma reads this at login.
        let env = "#!/bin/sh\n# Added by Drive For Linux: lets Dolphin find its sync status plugin.\ncase \":$QT_PLUGIN_PATH:\" in\n  *\":$HOME/.local/lib/qt6/plugins:\"*) ;;\n  *) export QT_PLUGIN_PATH=\"$HOME/.local/lib/qt6/plugins${QT_PLUGIN_PATH:+:$QT_PLUGIN_PATH}\" ;;\nesac\n";
        write_if_changed(&plasma_env_path(), env.as_bytes());
        let envd = "# Added by Drive For Linux: lets Dolphin find its sync status plugin.\nQT_PLUGIN_PATH=${HOME}/.local/lib/qt6/plugins${QT_PLUGIN_PATH:+:$QT_PLUGIN_PATH}\n";
        write_if_changed(&environment_d_path(), envd.as_bytes());
    }
    write_if_changed(&nautilus_extension_path(), NAUTILUS_EXTENSION.as_bytes());
    new_plugin
}

/// Whether Dolphin running now can already see the plugin (it reads the plugin path at start).
pub fn dolphin_needs_relogin() -> bool {
    let want = home().join(".local/lib/qt6/plugins");
    !std::env::var("QT_PLUGIN_PATH").map(|v| v.split(':').any(|p| Path::new(p) == want)).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The original's badge identifiers, in its order (FinderSyncExtension 0x10154adc8…).
    const BADGES: &[&str] = &["pinned", "cloud", "mirror_root", "synced", "syncing", "blacklisted", "cse"];

    #[test]
    fn socket_protocol() {
        let dir = std::env::temp_dir().join(format!("badge-sock-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        let mut st = Settings::default();
        st.remote = "g".into();
        st.drive_mode = DriveMode::Stream;
        let root = PathBuf::from(st.stream_dir("g"));
        let mut sh = Shared::default();
        sh.accounts = vec!["g".into()];
        serve(Arc::new(Mutex::new(sh)), Arc::new(Mutex::new(st)));
        let mut c = UnixStream::connect(socket_path()).unwrap();
        let q = format!("Q {}\nQ /etc/hosts\n.\n", root.join("My Drive/a.pdf").display());
        c.write_all(q.as_bytes()).unwrap();
        let mut out = String::new();
        std::io::Read::read_to_string(&mut c, &mut out).unwrap();
        assert_eq!(out, "cloud\n\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_badge_has_an_icon() {
        for b in BADGES {
            assert!(ICONS.iter().any(|(n, _)| n == b), "{b}");
            assert!(!icon_name(b).contains('-'));
        }
    }

    /// Serves a streaming account for a while, for trying the Dolphin plugin by hand:
    /// `XDG_RUNTIME_DIR=<dir> cargo test serve_for_plugin -- --ignored`.
    #[test]
    #[ignore]
    fn serve_for_plugin() {
        let mut st = Settings::default();
        st.remote = "g".into();
        st.drive_mode = DriveMode::Stream;
        let root = PathBuf::from(st.stream_dir("g"));
        st.offline_pins = vec![root.join("My Drive/keep").to_string_lossy().into()];
        let mut sh = Shared::default();
        sh.accounts = vec!["g".into()];
        let shared = Arc::new(Mutex::new(sh));
        serve(shared.clone(), Arc::new(Mutex::new(st)));
        println!("ROOT {}", root.display());
        std::thread::sleep(Duration::from_secs(3));
        shared.lock().unwrap().job = Some("x".into()); // makes watchers hear CHANGED
        std::thread::sleep(Duration::from_secs(5));
    }

    /// Serves a mirror folder for trying Dolphin by hand: DEMO_MIRROR=<folder> with
    /// XDG_CONFIG_HOME holding drive-linux/mirror-state-g.json; `cargo test demo_mirror -- --ignored`.
    #[test]
    #[ignore]
    fn demo_mirror() {
        let mut st = Settings::default();
        st.remote = "g".into();
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = std::env::var("DEMO_MIRROR").unwrap();
        let mut sh = Shared::default();
        sh.accounts = vec!["g".into()];
        serve(Arc::new(Mutex::new(sh)), Arc::new(Mutex::new(st)));
        std::thread::sleep(Duration::from_secs(40));
    }

    #[test]
    fn ignore_globs() {
        assert!(glob("~$*", "~$report.docx"));
        assert!(glob("*.swp", ".notes.txt.swp"));
        assert!(glob(".~lock.*#", ".~lock.a.odt#"));
        assert!(!glob("*.swp", "swap.txt"));
        assert!(ignored("a/.DS_Store"));
        assert!(!ignored("a/b.txt"));
    }

    #[test]
    fn mirror_and_stream_badges() {
        let base = std::env::temp_dir().join(format!("badge-test-{}", std::process::id()));
        let dir = base.join("mirror");
        let _ = std::fs::create_dir_all(dir.join("sub"));
        std::fs::write(dir.join("sub/a.txt"), b"hello").unwrap();
        std::fs::write(dir.join("new.txt"), b"x").unwrap();
        std::fs::write(dir.join(".DS_Store"), b"x").unwrap();
        let e = file_entry(&dir.join("sub/a.txt")).unwrap();
        let s = State { remote: "g".into(), folder: dir.to_string_lossy().into(), files: [("sub/a.txt".to_string(), e)].into_iter().collect(), synced_at: Default::default(), ..Default::default() };
        std::fs::write(base.join("mirror-state-g.json"), serde_json::to_string(&s).unwrap()).unwrap();
        let mut st = Settings::default();
        st.remote = "g".into();
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = dir.to_string_lossy().into();
        let mut sh = Shared::default();
        sh.accounts = vec!["g".into()];
        let mut cache = Cache { state_dir: base.clone(), states: BTreeMap::new() };
        let q = |p: PathBuf, c: &mut Cache, st: &Settings, sh: &Shared| status(&p.to_string_lossy(), st, sh, c);
        assert_eq!(q(dir.clone(), &mut cache, &st, &sh), "mirror_root");
        assert_eq!(q(dir.join("sub/a.txt"), &mut cache, &st, &sh), "synced");
        assert_eq!(q(dir.join("new.txt"), &mut cache, &st, &sh), "syncing");
        assert_eq!(q(dir.join(".DS_Store"), &mut cache, &st, &sh), "blacklisted");
        assert_eq!(q(dir.join("sub"), &mut cache, &st, &sh), "synced");
        assert_eq!(q(PathBuf::from("/etc/hosts"), &mut cache, &st, &sh), "");
        // changed after the last sync
        std::fs::write(dir.join("sub/a.txt"), b"hello, changed").unwrap();
        assert_eq!(q(dir.join("sub/a.txt"), &mut cache, &st, &sh), "syncing");
        // streaming: pinned or online only
        st.drive_mode = DriveMode::Stream;
        let root = PathBuf::from(st.stream_dir("g"));
        st.offline_pins = vec![root.join("My Drive/keep").to_string_lossy().into()];
        assert_eq!(q(root.join("My Drive/keep/x.pdf"), &mut cache, &st, &sh), "pinned");
        assert_eq!(q(root.join("My Drive/other.pdf"), &mut cache, &st, &sh), "cloud");
        assert_eq!(q(root.join("My Drive"), &mut cache, &st, &sh), "");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn folders_show_syncing_while_a_file_below_them_moves() {
        let base = std::env::temp_dir().join(format!("badge-dir-{}", std::process::id()));
        let dir = base.join("mirror");
        let _ = std::fs::create_dir_all(dir.join("a"));
        let _ = std::fs::create_dir_all(dir.join("b"));
        std::fs::write(dir.join("a/big.iso"), b"x").unwrap();
        let mut st = Settings::default();
        st.remote = "g".into();
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = dir.to_string_lossy().into();
        let mut sh = Shared::default();
        sh.accounts = vec!["g".into()];
        sh.job = Some("Mirror files".into());
        sh.transfers.push(crate::engine::Activity {
            size: Some(1),
            name: "big.iso".into(),
            place: st.mirror_folder.clone(),
            state: crate::engine::ActState::Transfer { pct: 10.0, upload: Some(true) },
            at: SystemTime::now(),
        });
        let mut cache = Cache { state_dir: base.clone(), states: BTreeMap::new() };
        assert_eq!(status(&dir.join("a").to_string_lossy(), &st, &sh, &mut cache), "syncing");
        assert_eq!(status(&dir.join("b").to_string_lossy(), &st, &sh, &mut cache), "synced");
        assert_eq!(status(&dir.join("a/big.iso").to_string_lossy(), &st, &sh, &mut cache), "syncing");
        sh.transfers.clear();
        sh.job = None;
        assert_eq!(status(&dir.join("a").to_string_lossy(), &st, &sh, &mut cache), "synced");
        let _ = std::fs::remove_dir_all(&base);
    }
}
