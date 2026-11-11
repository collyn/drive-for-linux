//! Talks to Google Drive through rclone (its Google Drive and Google Photos connections).
//! One manager thread runs every connected account (up to 4, like the original).
use crate::settings::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ActState {
    Synced,
    Deleted,
    Transfer { pct: f32, upload: Option<bool> },
    Error(String),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Activity {
    pub size: Option<u64>,
    pub name: String,
    pub place: String,
    pub state: ActState,
    pub at: SystemTime,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub at: SystemTime,
    pub read: bool,
}

#[derive(Clone, Debug)]
pub struct IndexItem {
    pub path: String,
    pub name: String,
    pub id: String,
    pub is_dir: bool,
}

#[derive(Default)]
pub struct Shared {
    pub rclone_ok: Option<bool>,
    pub rclone_version: String,
    /// The selected account, when it is connected.
    pub account: Option<String>,
    /// Every connected account.
    pub accounts: Vec<String>,
    pub photos_account: Option<String>,
    pub checked_account: bool,
    pub connecting: bool,
    pub connecting_photos: bool,
    pub activity: VecDeque<Activity>,
    pub notices: Vec<Notice>,
    pub transfers: Vec<Activity>,
    pub pending: u64,
    /// Quota of the selected account.
    pub quota: Option<(u64, u64)>,
    pub quotas: BTreeMap<String, (u64, u64)>,
    pub mounted: bool,
    pub mounted_set: BTreeSet<String>,
    pub shared_drives: BTreeMap<String, Vec<String>>,
    /// (id, name) of every shared drive per account, for "Shared drive syncing".
    pub drive_list: BTreeMap<String, Vec<(String, String)>>,
    pub job: Option<String>,
    pub last_done: Option<SystemTime>,
    pub sizes: std::collections::HashMap<String, u64>,
    pub shared_items: Option<Vec<RemoteItem>>,
    pub shared_loading: bool,
    pub offline_items: Option<Vec<(String, u64)>>,
    pub last_error: Option<String>,
    /// File lists per account used by search.
    pub index: BTreeMap<String, Vec<IndexItem>>,
    /// A USB device that was plugged in and isn't known yet: ask whether to back it up.
    pub new_device: Option<String>,
    pub updating_rclone: bool,
    pub pins_busy: bool,
    /// Activity or notifications changed since they were last written to disk.
    pub history_dirty: bool,
    /// Notification types the person turned off ("Don’t show this again").
    pub hidden_notices: BTreeSet<String>,
    /// Accounts whose syncing is held back because of a ransomware alert.
    pub ransomware: BTreeMap<String, crate::ransom::Alert>,
}

/// Activity and notifications kept between runs.
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct History {
    activity: VecDeque<Activity>,
    notices: Vec<Notice>,
}

fn history_path() -> PathBuf {
    config_dir().join("history.json")
}

#[derive(Clone, Debug)]
pub struct RemoteItem {
    pub name: String,
    pub size: i64,
    pub is_dir: bool,
    pub id: String,
    pub mod_time: String,
}

pub static PROXY_DIRECT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Shared {
    fn push_activity(&mut self, a: Activity) {
        self.activity.retain(|x| !(x.name == a.name && x.place == a.place));
        self.activity.push_front(a);
        self.activity.truncate(ACTIVITY_MAX);
        self.history_dirty = true;
    }

    fn load_history(&mut self) {
        if let Some(h) = std::fs::read_to_string(history_path()).ok().and_then(|t| serde_json::from_str::<History>(&t).ok()) {
            self.activity = h.activity;
            self.notices = h.notices;
            // The same notice twice (from earlier versions) is shown once, newest kept.
            let before = self.notices.len();
            let mut seen = std::collections::HashSet::new();
            self.notices.retain(|n| seen.insert((n.title.clone(), n.body.clone())));
            if self.notices.len() != before {
                self.history_dirty = true;
            }
        }
        if self.activity.is_empty() {
            self.seed_activity_from_mirror_state();
        }
    }

    /// No saved activity yet (earlier versions didn't keep it): rebuild the list from the files
    /// each mirror folder last synced, newest first, so Sync activity isn't empty.
    fn seed_activity_from_mirror_state(&mut self) {
        let mut rows: Vec<Activity> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(config_dir()) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !(name.starts_with("mirror-state") && name.ends_with(".json")) {
                    continue;
                }
                let Some(state) = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<crate::mirror::State>(&t).ok()) else { continue };
                for (path, entry) in &state.files {
                    // The recorded sync time when there is one, else the file's own time.
                    let secs = state.synced_at.get(path).copied().unwrap_or(entry.mtime);
                    // Like the original, skip items no longer on this computer.
                    if !Path::new(&state.folder).join(path).exists() {
                        continue;
                    }
                    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(secs.max(0) as u64);
                    rows.push(Activity { size: Some(entry.size.max(0) as u64), name: short_name(path), place: state.folder.clone(), state: ActState::Synced, at });
                }
            }
        }
        rows.sort_by_key(|r| std::cmp::Reverse(r.at));
        rows.dedup_by(|a, b| a.name == b.name && a.place == b.place);
        rows.truncate(ACTIVITY_MAX);
        if !rows.is_empty() {
            self.activity = rows.into();
            self.history_dirty = true;
        }
    }

    /// Writes activity and notifications to disk so they survive a restart.
    pub fn save_history(&mut self) {
        if !self.history_dirty {
            return;
        }
        self.history_dirty = false;
        let h = History { activity: self.activity.clone(), notices: self.notices.clone() };
        if let Ok(t) = serde_json::to_string(&h) {
            let _ = std::fs::create_dir_all(config_dir());
            let tmp = history_path().with_extension("tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(tmp, history_path());
            }
        }
    }
    pub fn notify(&mut self, title: &str, body: &str) {
        if self.hidden_notices.contains(title) {
            return;
        }
        if self.notices.iter().any(|n| n.title == title && n.body == body && !n.read) {
            return;
        }
        crate::diag::log_event("notice", &format!("{title}: {body}"));
        self.notices.insert(0, Notice { title: title.into(), body: body.into(), at: SystemTime::now(), read: false });
        self.notices.truncate(50);
        self.history_dirty = true;
    }
}

pub enum Cmd {
    /// Add an account (a new browser sign-in).
    Connect,
    ConnectPhotos,
    Disconnect(String),
    SyncNow,
    Reconfigure,
    Refresh,
    PinsChanged,
    /// "I recognize it" → "Allow sync": the alert was a false positive; push the changes.
    RansomwareAllow(String),
    /// Move the changed files aside, put Drive's copies back and resume syncing.
    RansomwareRestore(String),
    Quit,
}

/// Longest wait for the browser sign-in: browser_auth_wait_for_client_timeout_ms (0x103f52044) = 300000.
pub const CONNECT_TIMEOUT_SECS: u64 = 300;
/// Photo upload attempts: photos_max_upload_retries_offline (0x103f4cd76) = 3.
pub const PHOTOS_UPLOAD_RETRIES: u32 = 3;
/// A photo or video is backed up once it has gone unchanged this long; every change restarts
/// its wait: photos_debounce_edit_time_ms (0x103f4cdee) = 300000, used by the Photos debouncer
/// (FUN_100c7fbe0 records each change time, FUN_100c804f0 "ScheduleAction", debouncer.cc,
/// fires at earliest change + delay).
pub const PHOTOS_SETTLE_SECS: u64 = 300;

pub struct Engine {
    pub shared: Arc<Mutex<Shared>>,
    pub settings: Arc<Mutex<Settings>>,
    job: Arc<Mutex<Option<Child>>>,
    connect_child: Arc<Mutex<Option<Child>>>,
    tx: Sender<Cmd>,
    ctx: eframe::egui::Context,
}

/// rclone downloaded by the app (newer than the system's) when present, else the system one.
pub fn own_rclone_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("drive-linux").join("bin").join("rclone")
}

/// The rclone program the app runs (its own copy when downloaded, else the system's).
pub fn rclone_program() -> String {
    let own = own_rclone_path();
    if own.is_file() {
        own.to_string_lossy().to_string()
    } else {
        "rclone".into()
    }
}

fn rclone() -> Command {
    let own = own_rclone_path();
    let mut c = Command::new(if own.is_file() { own.to_string_lossy().to_string() } else { "rclone".into() });
    c.stdin(Stdio::null());
    if PROXY_DIRECT.load(std::sync::atomic::Ordering::Relaxed) {
        for v in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy"] {
            c.env_remove(v);
        }
    }
    c
}

/// Name of the connection that shows My Drive and the shared drives side by side.
pub fn combined_name(account: &str) -> String {
    format!("{account}__all")
}

impl Engine {
    pub fn new(ctx: eframe::egui::Context, settings: Settings, run: bool) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        crate::diag::VERBOSE.store(settings.verbose_logging, std::sync::atomic::Ordering::Relaxed);
        {
            let mut sh = shared.lock().unwrap();
            sh.load_history();
            sh.hidden_notices = settings.hidden_notices.iter().cloned().collect();
            sh.activity.truncate(ACTIVITY_MAX);
        }
        let settings = Arc::new(Mutex::new(settings));
        let job = Arc::new(Mutex::new(None));
        let connect_child = Arc::new(Mutex::new(None));
        let (tx, rx) = channel();
        let w = Worker {
            once: !run,
            shared: shared.clone(),
            settings: settings.clone(),
            job: job.clone(),
            connect_child: connect_child.clone(),
            ctx: ctx.clone(),
            tx: tx.clone(),
            runs: BTreeMap::new(),
            next_folder: BTreeMap::new(),
            photos_settle: BTreeMap::new(),
            next_account: Instant::now(),
            next_pins: Instant::now() + Duration::from_secs(20),
            next_devices: Instant::now(),
            seen_devices: BTreeSet::new(),
            watcher: None,
            watched: BTreeSet::new(),
            changes: None,
            nas_offline: BTreeSet::new(),
            ransom_checked: BTreeSet::new(),
            watch_ok: false,
            pin_last: Arc::default(),
        };
        std::thread::spawn(move || w.run(rx));
        if run {
            // Once at start, like the original's NotifyIfDeprecated (see update.rs).
            crate::update::spawn_check(shared.clone());
        }
        Self { shared, settings, job, connect_child, tx, ctx }
    }

    /// Stop a sign-in that is waiting on the browser.
    pub fn cancel_connect(&self) {
        if let Some(c) = self.connect_child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.tx.send(c);
    }

    /// When the system logs out or shuts down (SIGTERM/SIGHUP/SIGINT), quit without asking —
    /// the original's confirm_quit_during_system_logout default is false (0x103f5321b) — but
    /// still release streaming folders and save the history first.
    pub fn quit_on_system_signals(&self) {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        let Ok(mut signals) = signal_hook::iterator::Signals::new([SIGTERM, SIGHUP, SIGINT]) else { return };
        let tx = self.tx.clone();
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            if signals.forever().next().is_some() {
                let _ = tx.send(Cmd::Quit);
                std::thread::sleep(Duration::from_millis(1500));
                if let Ok(mut sh) = shared.lock() {
                    sh.history_dirty = true;
                    sh.save_history();
                }
                std::process::exit(0);
            }
        });
    }

    /// "Add shortcut to Google Drive" for an item shared with the account, placed in the folder
    /// the person picked. Follows the original (client.cc, function 0x10036aae0): the place must be
    /// inside the account's Drive folder ("Selected new shortcut item path … is not within mount
    /// point …" → "A shortcut cannot be added to this location"); picking a folder names the
    /// shortcut after the item; a failure shows "Error adding shortcut" / "Failed to create shortcut".
    pub fn add_shortcut(&self, target_id: String, target_name: String, dest: PathBuf) {
        let shared = self.shared.clone();
        let settings = self.settings.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let st = settings.lock().unwrap().clone();
            let accounts = shared.lock().unwrap().accounts.clone();
            let fail = |body: &str| {
                desktop_notify("Error adding shortcut", body);
                shared.lock().unwrap().notify("Error adding shortcut", body);
                ctx.request_repaint();
            };
            if target_id.is_empty() {
                return fail("Failed to create shortcut");
            }
            let (acc, rel) = match crate::filemenu::locate(&st, &accounts, &dest.to_string_lossy()) {
                crate::filemenu::Located::MyDrive(a, r) => (a, r),
                _ => return fail("A shortcut cannot be added to this location"),
            };
            let parent = if rel.is_empty() { Some("root".to_string()) } else { item_id(&format!("{acc}:{rel}")).filter(|(_, dir)| *dir).map(|(id, _)| id) };
            let Some(parent) = parent else { return fail("A shortcut cannot be added to this location") };
            shared.lock().unwrap().job = Some("Creating shortcut".into());
            ctx.request_repaint();
            let body = serde_json::json!({
                "name": target_name,
                "mimeType": "application/vnd.google-apps.shortcut",
                "shortcutDetails": { "targetId": target_id },
                "parents": [parent],
            });
            let r = drive_api_post(&acc, "https://www.googleapis.com/drive/v3/files?supportsAllDrives=true&fields=id,name", &body);
            shared.lock().unwrap().job = None;
            match r {
                Ok(v) => {
                    let name = v["name"].as_str().unwrap_or(&target_name).to_string();
                    let at = dest.join(&name);
                    desktop_notify("Created a shortcut", &format!("Shortcut added at {}", at.display()));
                    shared.lock().unwrap().notify("Created a shortcut", &format!("Shortcut added at {}", at.display()));
                    let _ = tx.send(Cmd::SyncNow); // the mirror brings it down; streaming shows it on the next listing
                }
                Err(e) => fail(&format!("Failed to create shortcut ({e})")),
            }
            ctx.request_repaint();
        });
    }

    /// Lists items others shared with the selected account (Drive's "Shared with me").
    pub fn load_shared(&self) {
        let shared = self.shared.clone();
        let remote = match shared.lock().unwrap().account.clone() {
            Some(r) => r,
            None => return,
        };
        shared.lock().unwrap().shared_loading = true;
        std::thread::spawn(move || {
            let out = rclone().args(["lsjson", &format!("{remote}:"), "--drive-shared-with-me", "--max-depth", "1"]).stderr(Stdio::null()).output();
            let mut items = Vec::new();
            if let Ok(o) = out {
                if let Ok(serde_json::Value::Array(a)) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
                    for v in a {
                        items.push(RemoteItem {
                            name: v["Name"].as_str().unwrap_or("").to_string(),
                            size: v["Size"].as_i64().unwrap_or(-1),
                            is_dir: v["IsDir"].as_bool().unwrap_or(false),
                            id: v["ID"].as_str().unwrap_or("").to_string(),
                            mod_time: v["ModTime"].as_str().unwrap_or("").chars().take(10).collect(),
                        });
                    }
                }
            }
            items.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            let mut sh = shared.lock().unwrap();
            sh.shared_items = Some(items);
            sh.shared_loading = false;
        });
    }

    /// Files kept on this computer: the stream cache, or the whole mirror folder.
    pub fn load_offline(&self) {
        let shared = self.shared.clone();
        let st = self.settings.lock().unwrap().clone();
        std::thread::spawn(move || {
            let remote = st.remote.clone();
            let root = match st.drive_mode {
                DriveMode::Stream => PathBuf::from(&st.cache_folder).join("vfs").join(combined_name(&remote)),
                DriveMode::Mirror => PathBuf::from(&st.mirror_folder),
            };
            let mut out = Vec::new();
            let mut stack = vec![root.clone()];
            while let Some(d) = stack.pop() {
                if out.len() > 5000 {
                    break;
                }
                if let Ok(rd) = std::fs::read_dir(&d) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.is_dir() {
                            stack.push(p);
                        } else if let Ok(m) = e.metadata() {
                            let rel = p.strip_prefix(&root).map(|r| r.to_string_lossy().to_string()).unwrap_or_default();
                            out.push((rel, m.len()));
                        }
                    }
                }
            }
            out.sort();
            shared.lock().unwrap().offline_items = Some(out);
        });
    }

    pub fn set_paused(&self, paused: bool) {
        {
            let mut s = self.settings.lock().unwrap();
            s.paused = paused;
            s.save();
        }
        if paused {
            if let Some(c) = self.job.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
            self.shared.lock().unwrap().transfers.clear();
        } else {
            self.send(Cmd::SyncNow);
        }
    }

    /// Download the newest rclone from rclone.org (checked against its published SHA-256) into
    /// the app's own folder. Newer rclone uses Google Photos' current access, which 1.60 lacks.
    pub fn update_rclone(&self) {
        let shared = self.shared.clone();
        let ctx = self.ctx.clone();
        let tx = self.tx.clone();
        {
            let mut sh = shared.lock().unwrap();
            if sh.updating_rclone {
                return;
            }
            sh.updating_rclone = true;
        }
        std::thread::spawn(move || {
            let res = download_rclone();
            let mut sh = shared.lock().unwrap();
            sh.updating_rclone = false;
            match res {
                Ok(v) => {
                    sh.rclone_version = v.clone();
                    sh.notify("rclone updated", &format!("Now using {v}. Streaming restarts with it."));
                }
                Err(e) => sh.notify("Couldn’t update rclone", &e),
            }
            drop(sh);
            let _ = tx.send(Cmd::Reconfigure);
            ctx.request_repaint();
        });
    }
}

fn download_rclone() -> Result<String, String> {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        "arm" => "arm-v7",
        a => return Err(format!("No rclone build for {a}")),
    };
    let curl = |url: &str, out: &Path| -> Result<(), String> {
        let ok = Command::new("curl").args(["-fsSL", "--retry", "2", "-o"]).arg(out).arg(url).status().map_err(|e| e.to_string())?;
        if ok.success() {
            Ok(())
        } else {
            Err(format!("Download failed: {url}"))
        }
    };
    let tmp = std::env::temp_dir().join(format!("drive-linux-rclone-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    curl("https://downloads.rclone.org/version.txt", &tmp.join("version.txt"))?;
    let ver_line = std::fs::read_to_string(tmp.join("version.txt")).map_err(|e| e.to_string())?;
    let ver = ver_line.trim().trim_start_matches("rclone ").to_string(); // "v1.71.0"
    if !ver.starts_with('v') {
        return Err("Unexpected version file".into());
    }
    let zip = format!("rclone-{ver}-linux-{arch}.zip");
    curl(&format!("https://downloads.rclone.org/{ver}/{zip}"), &tmp.join(&zip))?;
    curl(&format!("https://downloads.rclone.org/{ver}/SHA256SUMS"), &tmp.join("SHA256SUMS"))?;
    let sums = std::fs::read_to_string(tmp.join("SHA256SUMS")).map_err(|e| e.to_string())?;
    let want = sums.lines().find(|l| l.trim_end().ends_with(&zip)).and_then(|l| l.split_whitespace().next()).ok_or("Checksum not listed")?.to_string();
    let got = Command::new("sha256sum").arg(tmp.join(&zip)).output().map_err(|e| e.to_string())?;
    let got = String::from_utf8_lossy(&got.stdout).split_whitespace().next().unwrap_or("").to_string();
    if got != want {
        return Err("Checksum didn’t match; nothing was installed".into());
    }
    let inner = format!("rclone-{ver}-linux-{arch}/rclone");
    let bin = Command::new("unzip").arg("-p").arg(tmp.join(&zip)).arg(&inner).output().map_err(|e| e.to_string())?;
    if !bin.status.success() || bin.stdout.len() < 1_000_000 {
        return Err("Couldn’t unpack rclone".into());
    }
    let dest = own_rclone_path();
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let part = dest.with_extension("part");
    std::fs::write(&part, &bin.stdout).map_err(|e| e.to_string())?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &dest).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(tmp.join(&zip));
    Ok(rclone_version())
}

/// What the manager keeps for each connected account.
#[derive(Default)]
struct AcctRun {
    mount: Option<Child>,
    mount_dir: String,
    mount_started: Option<Instant>,
    mount_attempts: u32,
    next_mirror: Option<Instant>,
    next_quota: Option<Instant>,
    next_index: Option<Instant>,
    next_drives: Option<Instant>,
    next_drive_list: Option<Instant>,
    /// Drive change list position (changes API page token) and when to look again.
    page_token: Option<String>,
    next_changes: Option<Instant>,
    /// Something changed on Drive since the search list was read.
    index_stale: bool,
    /// Change list requests that failed in a row (for the back-off).
    changes_failures: u32,
}

struct Worker {
    once: bool,
    connect_child: Arc<Mutex<Option<Child>>>,
    shared: Arc<Mutex<Shared>>,
    settings: Arc<Mutex<Settings>>,
    job: Arc<Mutex<Option<Child>>>,
    ctx: eframe::egui::Context,
    tx: Sender<Cmd>,
    runs: BTreeMap<String, AcctRun>,
    next_folder: BTreeMap<String, Instant>,
    /// Folders backed up to Google Photos with a change still settling: a second pass is due then.
    photos_settle: BTreeMap<String, Instant>,
    next_account: Instant,
    next_pins: Instant,
    next_devices: Instant,
    seen_devices: BTreeSet<String>,
    watcher: Option<notify::RecommendedWatcher>,
    watched: BTreeSet<String>,
    changes: Option<Receiver<PathBuf>>,
    nas_offline: BTreeSet<String>,
    ransom_checked: BTreeSet<String>,
    /// Every folder in `watched` is really being watched (adding a watch can fail, e.g. when the
    /// system's inotify watch limit is reached).
    watch_ok: bool,
    /// When each offline file was last read into the cache.
    pin_last: Arc<Mutex<std::collections::HashMap<PathBuf, Instant>>>,
}

impl Worker {
    fn run(mut self, rx: Receiver<Cmd>) {
        let ok = rclone().arg("version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok();
        {
            let mut sh = self.shared.lock().unwrap();
            sh.rclone_ok = Some(ok);
            sh.rclone_version = if ok { rclone_version() } else { String::new() };
        }
        self.ctx.request_repaint();
        if !ok {
            return;
        }
        if self.once {
            // screenshot mode: only read which accounts are connected, start nothing
            self.detect_accounts();
            let accounts = self.shared.lock().unwrap().accounts.clone();
            for acc in accounts {
                if let Some(a) = crate::ransom::load_alert(&acc) {
                    self.shared.lock().unwrap().ransomware.insert(acc, a);
                }
            }
            self.ctx.request_repaint();
            return;
        }
        loop {
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(Cmd::Quit) | Err(RecvTimeoutError::Disconnected) => {
                    self.stop_all_mounts();
                    self.shared.lock().unwrap().save_history();
                    return;
                }
                // Sign-in runs on its own thread so syncing keeps going while the browser is open.
                Ok(Cmd::Connect) => self.spawn_connect(false),
                Ok(Cmd::ConnectPhotos) => self.spawn_connect(true),
                Ok(Cmd::Refresh) => {
                    self.next_account = Instant::now();
                    for r in self.runs.values_mut() {
                        r.next_quota = None;
                    }
                }
                Ok(Cmd::Disconnect(acc)) => self.disconnect(&acc),
                Ok(Cmd::SyncNow) => {
                    for r in self.runs.values_mut() {
                        r.next_mirror = None;
                    }
                    self.next_folder.clear();
                }
                Ok(Cmd::Reconfigure) => {
                    self.stop_all_mounts();
                    for r in self.runs.values_mut() {
                        r.mount_attempts = 0;
                        r.next_mirror = None;
                        r.next_drives = None;
                    }
                }
                Ok(Cmd::PinsChanged) => self.next_pins = Instant::now(),
                Ok(Cmd::RansomwareAllow(acc)) => self.ransomware_resolve(&acc, false),
                Ok(Cmd::RansomwareRestore(acc)) => self.ransomware_resolve(&acc, true),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.take_local_changes();
            self.tick();
            self.shared.lock().unwrap().save_history();
            self.ctx.request_repaint();
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if now >= self.next_account {
            self.detect_accounts();
            self.next_account = now + Duration::from_secs(30);
        }
        if now >= self.next_devices {
            self.scan_devices();
            self.next_devices = now + Duration::from_secs(5);
        }
        let accounts = self.shared.lock().unwrap().accounts.clone();
        // Stop what belongs to accounts that were disconnected.
        let gone: Vec<String> = self.runs.keys().filter(|a| !accounts.contains(a)).cloned().collect();
        for a in gone {
            if let Some(mut r) = self.runs.remove(&a) {
                stop_mount(&mut r, &self.shared, &a);
            }
        }
        if accounts.is_empty() {
            return;
        }
        let st_all = self.settings.lock().unwrap().clone();
        self.update_watches(&st_all, &accounts);
        for acc in &accounts {
            let st = st_all.for_account(acc);
            let mut run = self.runs.remove(acc).unwrap_or_default();
            // A ransomware alert survives restarts (the original's "ransomware-alert" sentinel).
            if self.ransom_checked.insert(acc.clone()) {
                if let Some(a) = crate::ransom::load_alert(acc) {
                    self.shared.lock().unwrap().ransomware.insert(acc.clone(), a);
                }
            }
            let held = self.shared.lock().unwrap().ransomware.contains_key(acc);
            if run.next_quota.map(|t| Instant::now() >= t).unwrap_or(true) {
                self.quota_and_email(acc);
                run.next_quota = Some(Instant::now() + Duration::from_secs(300));
            }
            // Drive's change list, every 10 s like the original (changelog_polling_interval_ms).
            if run.next_changes.map(|t| Instant::now() >= t).unwrap_or(true) {
                let result = drive_changes(acc, run.page_token.as_deref());
                run.changes_failures = if result.is_some() { 0 } else { run.changes_failures.saturating_add(1) };
                run.next_changes = Some(Instant::now() + changes_poll_delay(run.changes_failures));
                match result {
                    Some(c) => {
                        if c.files {
                            run.next_mirror = Some(Instant::now());
                            run.index_stale = true;
                        }
                        if c.drives {
                            // Shared drive roots are updated from the change list's drive changes
                            // (ProcessSharedDriveChanges, 0x1017c53b0), not from file changes.
                            run.next_drive_list = None;
                            run.next_drives = None; // the streaming folder's "Shared drives"
                        }
                        run.page_token = Some(c.token);
                    }
                    None => run.index_stale = true, // can't tell: fall back to the timers
                }
            }
            let index_due = run.next_index.map(|t| Instant::now() >= t).unwrap_or(true);
            if run.next_index.is_none() || (index_due && run.index_stale) {
                self.refresh_index(acc);
                run.next_index = Some(Instant::now() + Duration::from_secs(INDEX_REFRESH_SECS));
                run.index_stale = false;
            }
            // Shared drives the account can open: read at start (fetch_shared_drive_roots_on_init =
            // true), then when the change list reports a drive change; the original has no timer
            // for it, so the safety re-read follows its backup poll (changelog_backup_poll_interval_ms).
            if run.next_drive_list.map(|t| Instant::now() >= t).unwrap_or(true) {
                if let Some(list) = list_shared_drives(acc) {
                    self.prune_shared_drives(acc, &list);
                    self.shared.lock().unwrap().drive_list.insert(acc.clone(), list);
                }
                run.next_drive_list = Some(Instant::now() + Duration::from_secs(MIRROR_BACKUP_POLL_SECS));
            }
            // Streaming keeps a live folder even while paused (opened files stay readable).
            match st.drive_mode {
                DriveMode::Stream => self.ensure_mount(&mut run, acc, &st),
                DriveMode::Mirror => stop_mount(&mut run, &self.shared, acc),
            }
            if !st.paused && !held && st.drive_mode == DriveMode::Mirror && run.next_mirror.map(|t| Instant::now() >= t).unwrap_or(true) {
                self.mirror(acc, &st);
                // Local changes come from the watcher and Drive's from the change list, so the full
                // re-check is only the original's backup poll — unless one of them isn't working.
                let watched = self.watch_ok && self.watched.contains(&st.mirror_folder);
                let every = if watched && run.page_token.is_some() && run.changes_failures == 0 { MIRROR_BACKUP_POLL_SECS } else { MIRROR_INTERVAL_SECS };
                run.next_mirror = Some(Instant::now() + Duration::from_secs(every));
            }
            self.runs.insert(acc.clone(), run);
        }
        {
            let selected = st_all.remote.clone();
            let mut sh = self.shared.lock().unwrap();
            sh.quota = sh.quotas.get(&selected).copied();
            sh.mounted = sh.mounted_set.contains(&selected);
        }
        if st_all.paused {
            return;
        }
        for f in &st_all.folders {
            if self.settings.lock().unwrap().paused {
                break;
            }
            let present = Path::new(&f.path).is_dir();
            let network = !f.device && is_network_path(&f.path);
            if network && !present {
                self.nas_offline.insert(f.path.clone());
                continue;
            }
            if network && self.nas_offline.remove(&f.path) {
                // Came back: poll it after reconnected_nas_polling_interval_seconds.
                self.next_folder.insert(f.path.clone(), Instant::now() + Duration::from_secs(NAS_RECONNECT_SECS));
                continue;
            }
            if f.device && !present {
                // USB devices are backed up when connected; sync right away when it comes back.
                self.next_folder.remove(&f.path);
                continue;
            }
            if self.photos_settle.get(&f.path).is_some_and(|t| Instant::now() >= *t) {
                // The photos changed here have now settled: back them up.
                self.photos_settle.remove(&f.path);
                self.next_folder.remove(&f.path);
            }
            if self.next_folder.get(&f.path).map(|t| Instant::now() < *t).unwrap_or(false) {
                continue;
            }
            let acc = if accounts.contains(&f.account) { f.account.clone() } else { st_all.remote.clone() };
            let acc = if accounts.contains(&acc) { acc } else { accounts[0].clone() };
            if self.shared.lock().unwrap().ransomware.contains_key(&acc) {
                continue; // syncing is held for this account
            }
            let st = st_all.for_account(&acc);
            self.backup_folder(&acc, &st, f);
            let every = if is_network_path(&f.path) {
                NAS_INTERVAL_SECS
            } else if self.watch_ok && self.watched.contains(&f.path) {
                FOLDER_POLL_SECS
            } else {
                FOLDER_INTERVAL_SECS
            };
            self.next_folder.insert(f.path.clone(), Instant::now() + Duration::from_secs(every));
        }
        if Instant::now() >= self.next_pins {
            self.refresh_pins(&st_all);
            self.next_pins = Instant::now() + Duration::from_secs(PIN_CHECK_SECS);
        }
    }

    // ------------------------------------------------------------------ accounts

    fn detect_accounts(&mut self) {
        let out = rclone().args(["listremotes", "--long"]).stderr(Stdio::null()).output();
        let text = out.map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        let mut drives = Vec::new();
        let mut photos = Vec::new();
        for line in text.lines() {
            if let Some((name, kind)) = line.split_once(':') {
                let kind = kind.trim();
                let name = name.trim().to_string();
                if (kind == "drive" || kind == "google photos") && !has_token(&name) {
                    continue; // half-finished sign-in
                }
                if kind == "drive" {
                    drives.push(name);
                } else if kind == "google photos" {
                    photos.push(name);
                }
            }
        }
        drives.truncate(MAX_ACCOUNTS);
        let mut st = self.settings.lock().unwrap();
        let selected = if drives.contains(&st.remote) { Some(st.remote.clone()) } else { drives.first().cloned() };
        if let Some(a) = &selected {
            if *a != st.remote {
                st.select_account(a);
                st.save();
            }
        }
        let ph = if photos.contains(&st.photos_remote) { Some(st.photos_remote.clone()) } else { photos.first().cloned() };
        if let Some(p) = &ph {
            st.photos_remote = p.clone();
        }
        drop(st);
        let mut sh = self.shared.lock().unwrap();
        sh.account = selected;
        sh.accounts = drives;
        sh.photos_account = ph;
        sh.checked_account = true;
    }

    /// Shared drives that are switched on but no longer listed (hidden, left or deleted) stop
    /// syncing, like "Removing hidden, unsynced or no longer existing shared drive"
    /// (function 0x10064bb90). Files already on this computer are kept.
    fn prune_shared_drives(&mut self, acc: &str, list: &[(String, String)]) {
        let mut st = self.settings.lock().unwrap();
        let Some(on) = st.synced_shared_drives.get_mut(acc) else { return };
        let before = on.len();
        on.retain(|id| list.iter().any(|(i, _)| i == id));
        if on.len() != before {
            st.save();
            drop(st);
            self.shared.lock().unwrap().notify(
                "Shared drive sync settings updated",
                "Shared drive syncing disabled for shared drives that are hidden, that you left or that no longer exist. Their files stay on this computer.",
            );
        }
    }

    fn quota_and_email(&mut self, remote: &str) {
        let out = rclone().args(["about", &format!("{remote}:"), "--json"]).stderr(Stdio::null()).output();
        if let Ok(o) = out {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&o.stdout) {
                let used = v["used"].as_u64().unwrap_or(0);
                let total = v["total"].as_u64().unwrap_or(0);
                self.shared.lock().unwrap().quotas.insert(remote.to_string(), (used, total));
            }
        }
        let known = {
            let st = self.settings.lock().unwrap();
            st.emails.contains_key(remote) && st.names.contains_key(remote)
        };
        let avatar_ok = image::image_dimensions(avatar_path(remote)).map(|(w, _)| w == AVATAR_PX).unwrap_or(false);
        if !known || !avatar_ok {
            if let Some((email, name, photo)) = account_profile(remote) {
                {
                    let mut st = self.settings.lock().unwrap();
                    st.emails.insert(remote.to_string(), email);
                    if !name.is_empty() {
                        st.names.insert(remote.to_string(), name);
                    }
                    st.save();
                }
                if !photo.is_empty() {
                    save_avatar(remote, &photo);
                }
            }
        }
    }

    fn spawn_connect(&mut self, photos: bool) {
        let name = if photos {
            self.settings.lock().unwrap().photos_remote.clone()
        } else {
            let existing = self.shared.lock().unwrap().accounts.clone();
            if existing.len() >= MAX_ACCOUNTS {
                self.shared.lock().unwrap().notify("Maximum number of accounts reached", "Disconnect an account to add another");
                return;
            }
            let all = remote_names();
            match (1..=MAX_ACCOUNTS + 4).map(|i| if i == 1 { "gdrive".to_string() } else { format!("gdrive{i}") }).find(|n| !existing.contains(n) && (!all.contains(n) || !has_token(n))) {
                Some(n) => n,
                None => return,
            }
        };
        {
            let mut sh = self.shared.lock().unwrap();
            if sh.connecting {
                return;
            }
            sh.connecting = true;
            sh.connecting_photos = photos;
        }
        self.ctx.request_repaint();
        let (shared, ctx, child_slot, tx, settings) = (self.shared.clone(), self.ctx.clone(), self.connect_child.clone(), self.tx.clone(), self.settings.clone());
        std::thread::spawn(move || {
            let result = run_sign_in(&name, photos, &child_slot);
            let mut sh = shared.lock().unwrap();
            sh.connecting = false;
            sh.connecting_photos = false;
            match result {
                Ok(()) if photos => sh.notify("Google Photos connected", "Photos and videos can now be backed up to Google Photos."),
                Ok(()) => {
                    sh.notify("Success!", "You’ve signed into Google Drive.");
                    let mut st = settings.lock().unwrap();
                    st.select_account(&name);
                    st.save();
                }
                Err(msg) if photos => {
                    let v = rclone_version();
                    sh.notify(
                        "Can’t connect Google Photos",
                        &format!("Google blocked or didn’t finish the sign-in. Since March 2025 Google only lets apps add photos through its newer Photos access. Using {v}; if it is older than v1.70, use “Get the latest rclone” in Settings and try again. Details: {msg}"),
                    )
                }
                Err(msg) => sh.notify("Sign in Failed", &msg),
            }
            drop(sh);
            let _ = tx.send(Cmd::Refresh);
            ctx.request_repaint();
        });
    }

    fn disconnect(&mut self, remote: &str) {
        if let Some(mut r) = self.runs.remove(remote) {
            stop_mount(&mut r, &self.shared, remote);
        }
        let _ = rclone().args(["config", "delete", remote]).output();
        let _ = rclone().args(["config", "delete", &combined_name(remote)]).output();
        let _ = rclone().args(["config", "delete", &format!("{remote}__shared")]).output();
        {
            let mut sh = self.shared.lock().unwrap();
            sh.accounts.retain(|a| a != remote);
            sh.quotas.remove(remote);
            sh.index.remove(remote);
            if sh.account.as_deref() == Some(remote) {
                sh.account = None;
                sh.quota = None;
            }
            sh.transfers.clear();
            if sh.ransomware.remove(remote).is_some() || crate::ransom::alert_path(remote).exists() {
                crate::ransom::clear_alert(remote, false);
            }
            sh.notify("You’ve been signed out.", &format!("{remote} was disconnected from this computer."));
        }
        let mut st = self.settings.lock().unwrap();
        st.emails.remove(remote);
        st.names.remove(remote);
        let _ = std::fs::remove_file(avatar_path(remote));
        st.account_prefs.remove(remote);
        let _ = std::fs::remove_file(config_dir().join(format!("mirror-state-{remote}.json")));
        if let Ok(rd) = std::fs::read_dir(config_dir()) {
            let prefix = format!("mirror-state-{remote}-sd-");
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().starts_with(&prefix) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        st.save();
        drop(st);
        self.next_account = Instant::now();
    }

    // ------------------------------------------------------------------ streaming

    /// Builds the connection that shows "My Drive" and "Shared drives" in one folder, like the
    /// original's streaming drive.
    fn prepare_combined(&mut self, remote: &str, run: &mut AcctRun) -> String {
        let name = combined_name(remote);
        if run.next_drives.map(|t| Instant::now() < t).unwrap_or(false) && remote_names().contains(&name) {
            return name;
        }
        // Rebuilt when the change list reports a shared drive change; otherwise on the backup poll.
        run.next_drives = Some(Instant::now() + Duration::from_secs(MIRROR_BACKUP_POLL_SECS));
        let drives = list_shared_drives(remote).unwrap_or_default();
        // rclone 1.60's combine can't nest folders in one level, so shared drives get their own
        // combined connection that appears as the "Shared drives" folder.
        let shared_name = format!("{remote}__shared");
        let quiet = |args: &[&str]| {
            let _ = rclone().args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
        };
        quiet(&["config", "delete", &name]);
        quiet(&["config", "delete", &shared_name]);
        let shared_up = if drives.is_empty() {
            let empty = config_dir().join("empty");
            let _ = std::fs::create_dir_all(&empty);
            format!("\"Shared drives=:local:{}\"", empty.to_string_lossy())
        } else {
            let ups: Vec<String> = drives
                .iter()
                .map(|(id, title)| {
                    let safe: String = title.chars().map(|c| if c == '/' || c == '"' || c == '\\' || c == '=' { '_' } else { c }).collect();
                    format!("\"{safe}={remote},team_drive={id}:\"")
                })
                .collect();
            quiet(&["config", "create", &shared_name, "combine", "upstreams", &ups.join(" ")]);
            format!("\"Shared drives={shared_name}:\"")
        };
        let ups = format!("\"My Drive={remote}:\" {shared_up}");
        quiet(&["config", "create", &name, "combine", "upstreams", &ups]);
        self.shared.lock().unwrap().shared_drives.insert(remote.to_string(), drives.into_iter().map(|(_, t)| t).collect());
        name
    }

    fn ensure_mount(&mut self, run: &mut AcctRun, remote: &str, st: &Settings) {
        if let Some(c) = run.mount.as_mut() {
            match c.try_wait() {
                Ok(None) => return,
                _ => {
                    run.mount = None;
                    self.shared.lock().unwrap().mounted_set.remove(remote);
                    let _ = unmount(&run.mount_dir);
                    // An early exit counts as a failed attempt (original: domain_controller.mm,
                    // function 0x1000518e0 retries attaching up to 10 times, 2 s apart).
                    if run.mount_started.map(|t| t.elapsed() < Duration::from_secs(20)).unwrap_or(false) {
                        if run.mount_attempts >= MOUNT_MAX_ATTEMPTS {
                            self.shared.lock().unwrap().notify("Unable to mount to selected folder.", "Google Drive could not be streamed. Make sure the folder is empty and FUSE is installed.");
                        }
                    } else {
                        run.mount_attempts = 0;
                    }
                }
            }
        }
        if run.mount_attempts >= MOUNT_MAX_ATTEMPTS {
            return; // gave up until settings change or the app restarts
        }
        if run.mount_started.map(|t| t.elapsed() < Duration::from_secs(MOUNT_RETRY_DELAY_SECS)).unwrap_or(false) {
            return;
        }
        let dir = st.stream_dir(remote);
        // A mount still attached from an earlier run looks like a full folder; release it first.
        let _ = unmount(&dir);
        let check = check_mount_dir(&dir);
        if check != MountCheck::Ok {
            run.mount_attempts = MOUNT_MAX_ATTEMPTS; // like the original: refuse, don't retry
            self.shared.lock().unwrap().notify("Unable to mount to selected folder.", mount_check_message(&check));
            return;
        }
        run.mount_attempts += 1;
        let source = self.prepare_combined(remote, run);
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::create_dir_all(&st.cache_folder);
        let mut c = rclone();
        c.args(["mount", &format!("{source}:"), &dir, "--vfs-cache-mode", "full", "--cache-dir", &st.cache_folder])
            // Cached files stay until space is needed (the original keeps them; rclone's default drops them after 1 h).
            .args(["--vfs-cache-max-age", "8760h", "--dir-cache-time", "1m", "--poll-interval", &format!("{}s", CHANGES_POLL_MS / 1000)])
            .args(["--volname", &format!("Google Drive ({})", st.account_label(remote))])
            .args(["--use-json-log", crate::diag::rclone_verbosity(), "--stats", "2s", "--stats-log-level", "NOTICE"]);
        if st.cache_limit_on {
            c.args(["--vfs-cache-max-size", &format!("{}G", st.cache_limit_gb.clamp(CACHE_MIN, CACHE_MAX))]);
        }
        if let Some(bw) = st.bwlimit() {
            c.args(["--bwlimit", &bw]);
        }
        c.stdout(Stdio::null()).stderr(Stdio::piped());
        run.mount_started = Some(Instant::now());
        match c.spawn() {
            Ok(mut child) => {
                if let Some(err) = child.stderr.take() {
                    let shared = self.shared.clone();
                    let ctx = self.ctx.clone();
                    std::thread::spawn(move || read_log(err, &shared, &ctx, "Google Drive", Some(true), None));
                }
                run.mount = Some(child);
                run.mount_dir = dir;
                self.shared.lock().unwrap().mounted_set.insert(remote.to_string());
            }
            Err(e) => self.shared.lock().unwrap().notify("Unable to mount to selected folder.", &e.to_string()),
        }
    }

    fn stop_all_mounts(&mut self) {
        let keys: Vec<String> = self.runs.keys().cloned().collect();
        for k in keys {
            if let Some(r) = self.runs.get_mut(&k) {
                stop_mount(r, &self.shared, &k);
            }
        }
    }

    /// Files kept "available offline": read them through the streaming folder so the cache holds them.
    fn refresh_pins(&mut self, st: &Settings) {
        if st.offline_pins.is_empty() || self.shared.lock().unwrap().pins_busy {
            return;
        }
        let pins = st.offline_pins.clone();
        let shared = self.shared.clone();
        let last = self.pin_last.clone();
        shared.lock().unwrap().pins_busy = true;
        std::thread::spawn(move || {
            let mut files = Vec::new();
            for p in &pins {
                collect_files(Path::new(p), &mut files, 20000);
            }
            let due = pins_due(&files, &last.lock().unwrap(), Instant::now());
            for f in due {
                if let Ok(mut fh) = std::fs::File::open(&f) {
                    if std::io::copy(&mut fh, &mut std::io::sink()).is_ok() {
                        last.lock().unwrap().insert(f, Instant::now());
                    }
                }
            }
            last.lock().unwrap().retain(|p, _| files.contains(p));
            shared.lock().unwrap().pins_busy = false;
        });
    }

    fn refresh_index(&mut self, remote: &str) {
        let shared = self.shared.clone();
        let remote = remote.to_string();
        std::thread::spawn(move || {
            let out = rclone()
                .args(["lsjson", &format!("{remote}:"), "-R", "--fast-list", "--no-mimetype", "--no-modtime"])
                .args(["--exclude", &format!("/{BACKUP_ROOT}/**")])
                .stderr(Stdio::null())
                .output();
            let Ok(o) = out else { return };
            let Ok(serde_json::Value::Array(a)) = serde_json::from_slice::<serde_json::Value>(&o.stdout) else { return };
            let items: Vec<IndexItem> = a
                .iter()
                .map(|v| IndexItem {
                    path: v["Path"].as_str().unwrap_or("").to_string(),
                    name: v["Name"].as_str().unwrap_or("").to_string(),
                    id: v["ID"].as_str().unwrap_or("").to_string(),
                    is_dir: v["IsDir"].as_bool().unwrap_or(false),
                })
                .collect();
            shared.lock().unwrap().index.insert(remote, items);
        });
    }

    // ------------------------------------------------------------------ devices & local changes

    /// USB devices: a newly plugged-in device that isn't known yet is offered for backup.
    fn scan_devices(&mut self) {
        let user = std::env::var("USER").unwrap_or_default();
        let st = self.settings.lock().unwrap().clone();
        let mut present = BTreeSet::new();
        for base in [format!("/run/media/{user}"), format!("/media/{user}")] {
            if let Ok(rd) = std::fs::read_dir(&base) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        present.insert(e.path().to_string_lossy().to_string());
                    }
                }
            }
        }
        let mut moved = Vec::new();
        for p in &present {
            let id = media_id(p);
            // A device already backed up that came back at a different place: follow it.
            if let Some(f) = st.folders.iter().find(|f| f.device && !f.media_id.is_empty() && f.media_id == id && f.path != *p) {
                moved.push((f.path.clone(), p.clone()));
                continue;
            }
            let known = st.folders.iter().any(|f| f.path == *p || (!f.media_id.is_empty() && f.media_id == id)) || st.is_ignored(&id, p) || self.seen_devices.contains(p);
            if !known {
                self.seen_devices.insert(p.clone());
                let mut sh = self.shared.lock().unwrap();
                if sh.new_device.is_none() {
                    sh.new_device = Some(p.clone());
                }
            }
        }
        if !moved.is_empty() {
            let mut st = self.settings.lock().unwrap();
            for (old, new) in moved {
                if let Some(f) = st.folders.iter_mut().find(|f| f.path == old) {
                    f.path = new;
                }
            }
            st.save();
        }
        // Forget devices that were unplugged so they are asked about again next time.
        self.seen_devices.retain(|d| present.contains(d));
    }

    /// Watch the mirror folders and connected folders so a change syncs within seconds.
    fn update_watches(&mut self, st: &Settings, accounts: &[String]) {
        use notify::Watcher;
        let mut want = BTreeSet::new();
        for a in accounts {
            let s = st.for_account(a);
            if s.drive_mode == DriveMode::Mirror {
                want.insert(s.mirror_folder.clone());
                let drives = self.shared.lock().unwrap().drive_list.get(a).cloned().unwrap_or_default();
                for (id, name) in &drives {
                    if s.synced_shared_drives.get(a).map(|v| v.contains(id)).unwrap_or(false) {
                        let f = s.shared_drive_folder(a, name);
                        let _ = std::fs::create_dir_all(&f);
                        want.insert(f.to_string_lossy().to_string());
                    }
                }
            }
        }
        for f in &st.folders {
            if Path::new(&f.path).is_dir() {
                want.insert(f.path.clone());
            }
        }
        if want == self.watched && self.watcher.is_some() {
            return;
        }
        let (tx, rx) = channel::<PathBuf>();
        let ctx = self.ctx.clone();
        let w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return;
                }
                for p in ev.paths {
                    let _ = tx.send(p);
                }
                ctx.request_repaint();
            }
        });
        let Ok(mut w) = w else {
            self.watch_ok = false;
            return;
        };
        let mut ok = true;
        for p in &want {
            if let Err(e) = w.watch(Path::new(p), notify::RecursiveMode::Recursive) {
                ok = false;
                crate::diag::log_event("error", &format!("Can't watch {p} for changes: {e}"));
            }
        }
        self.watch_ok = ok;
        self.watcher = Some(w);
        self.changes = Some(rx);
        self.watched = want;
    }

    fn take_local_changes(&mut self) {
        let Some(rx) = &self.changes else { return };
        let changed: Vec<PathBuf> = rx.try_iter().collect();
        if changed.is_empty() {
            return;
        }
        let st = self.settings.lock().unwrap().clone();
        let soon = Instant::now() + Duration::from_secs(LOCAL_CHANGE_DEBOUNCE_SECS);
        let accounts: Vec<String> = self.runs.keys().cloned().collect();
        for p in changed {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.ends_with(".partial") || name.starts_with(".~lock") || crate::gdocs::is_link(&name) {
                continue;
            }
            for a in &accounts {
                let s = st.for_account(a);
                let in_shared = s.synced_shared_drives.contains_key(a) && p.starts_with(s.shared_drives_root(a));
                if s.drive_mode == DriveMode::Mirror && (p.starts_with(&s.mirror_folder) || in_shared) {
                    if let Some(r) = self.runs.get_mut(a) {
                        r.next_mirror = Some(soon);
                    }
                }
            }
            for f in &st.folders {
                if p.starts_with(&f.path) {
                    self.next_folder.insert(f.path.clone(), soon);
                    if f.to_photos {
                        self.photos_settle.insert(f.path.clone(), Instant::now() + Duration::from_secs(PHOTOS_SETTLE_SECS + 1));
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ mirror & folder backup

    /// Mirror files: My Drive, then every shared drive switched on in "Shared drive syncing".
    fn mirror(&mut self, remote: &str, st: &Settings) {
        let folder = st.mirror_folder.clone();
        if self.mirror_pair(remote, &format!("{remote}:"), &folder, &format!("mirror-state-{remote}.json"), true, st) {
            return;
        }
        let enabled = st.synced_shared_drives.get(remote).cloned().unwrap_or_default();
        if enabled.is_empty() {
            return;
        }
        let drives = self.shared.lock().unwrap().drive_list.get(remote).cloned().unwrap_or_default();
        for (id, name) in drives.iter().filter(|(id, _)| enabled.contains(id)) {
            if self.settings.lock().unwrap().paused {
                break;
            }
            let local = st.shared_drive_folder(remote, name);
            if self.mirror_pair(remote, &format!("{remote},team_drive={id}:"), &local.to_string_lossy(), &format!("mirror-state-{remote}-sd-{id}.json"), false, st) {
                break;
            }
        }
    }

    /// Two-way sync of one Drive location (`spec`) with one local folder. True when it found what
    /// looks like ransomware and held syncing back.
    fn mirror_pair(&mut self, remote: &str, spec: &str, folder: &str, state_name: &str, my_drive: bool, st: &Settings) -> bool {
        use crate::mirror::*;
        let folder = folder.to_string();
        let _ = std::fs::create_dir_all(&folder);
        let mut filters = ignore_args();
        if my_drive {
            // The original's "Computers" backups live outside My Drive, so mirroring never pulls them down.
            filters.extend(["--exclude".to_string(), format!("/{BACKUP_ROOT}/**")]);
        }
        let state_key = if my_drive { remote.to_string() } else { spec.to_string() };
        let list = |target: &str, extra: &[String]| -> Result<Vec<u8>, String> {
            let out = rclone().args(["lsjson", target, "-R", "--files-only"]).args(extra).args(&filters).stdout(Stdio::piped()).stderr(Stdio::piped()).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(last_line(&String::from_utf8_lossy(&out.stderr)).to_string());
            }
            Ok(out.stdout)
        };
        self.shared.lock().unwrap().job = Some("Mirror files".into());
        self.ctx.request_repaint();
        // Google Docs/Sheets/Slides are listed too (no size): they become link files here, and the
        // link files on this computer are left out of its file list so they are never uploaded.
        let mut docs = Vec::new();
        let sides = list(&folder, &crate::gdocs::exclude_args()).and_then(|b| parse_listing(&b)).and_then(|l| {
            // Every Google document under its own type, also those rclone can't export (Forms, My Maps…).
            let raw = list(spec, &["--drive-show-all-gdocs".to_string()])?;
            docs = crate::gdocs::parse_docs(&raw);
            parse_listing(&raw).map(|r| (l, r))
        });
        let (local, remote_files) = match sides {
            Ok(v) => v,
            Err(e) => {
                // Never treat a failed listing as "everything was deleted".
                let mut sh = self.shared.lock().unwrap();
                sh.job = None;
                sh.notify("Can’t sync folder to Drive", &format!("Mirror files: couldn’t read the file list of “{folder}” ({e}). It will be tried again."));
                return false;
            }
        };
        let state_path = config_dir().join(state_name);
        let read_state = |p: &Path| -> State { std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default() };
        let mut saved = read_state(&state_path);
        if saved.remote.is_empty() && my_drive {
            saved = read_state(&config_dir().join("mirror-state.json")); // before several accounts
        }
        let prior = if saved.remote == state_key && saved.folder == folder {
            saved.files.clone()
        } else if my_drive {
            old_bisync_listing(remote, &folder).unwrap_or_default()
        } else {
            Default::default()
        };
        let plan = plan(&local, &remote_files, &prior);
        let prior_links = if saved.remote == state_key && saved.folder == folder { saved.links.clone() } else { Default::default() };
        if !self.settings.lock().unwrap().paused {
            let email = st.emails.get(remote).cloned().unwrap_or_default();
            let added = crate::gdocs::write_links(&folder, &docs, &email, &prior_links);
            if added > 0 {
                let mut sh = self.shared.lock().unwrap();
                for d in docs.iter().filter(|d| !prior_links.contains(&d.path)).take(added) {
                    sh.push_activity(Activity { size: None, name: short_name(&d.path), place: folder.clone(), state: ActState::Synced, at: SystemTime::now() });
                }
            }
        }
        let links: std::collections::BTreeSet<String> = docs.iter().map(|d| d.path.clone()).collect();
        // Many synced files rewritten or removed at once: hold everything back before any of it
        // reaches Drive, unless the person dealt with an alert less than an hour ago.
        if let Some(held) = crate::ransom::suspicious(&plan, &prior, spec, &folder) {
            let at = crate::ransom::now_ms();
            if !crate::ransom::clear_takes_precedence(remote, at) {
                self.raise_ransomware(remote, held, at);
                let mut sh = self.shared.lock().unwrap();
                sh.job = None;
                return true;
            }
        }
        let mut ok = true;
        let list_file = |name: &str, items: &[String]| -> String {
            let p = config_dir().join(format!("{}-{name}.txt", state_name.trim_end_matches(".json")));
            let _ = std::fs::write(&p, items.join("\n"));
            p.to_string_lossy().to_string()
        };
        let place = folder.clone();
        if !plan.upload.is_empty() && !self.settings.lock().unwrap().paused {
            let f = list_file("upload", &plan.upload);
            let args = vec!["copy".into(), folder.clone(), spec.to_string(), "--files-from-raw".into(), f, "--no-traverse".into()];
            ok &= self.run_job("Mirror files", &place, Some(true), args, st, Some(folder.clone()));
        }
        // Conflicts: move this computer's copy to Lost and Found before Drive's copy is put in place.
        let mut moved_to_lost_and_found = 0;
        for n in &plan.conflicts {
            let src = Path::new(&folder).join(n);
            let dst = lost_and_found_dir().join(n);
            if let Some(parent) = dst.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let dst = unique_path(dst);
            if std::fs::rename(&src, &dst).is_ok() || (std::fs::copy(&src, &dst).is_ok() && std::fs::remove_file(&src).is_ok()) {
                moved_to_lost_and_found += 1;
            } else {
                ok = false;
            }
        }
        if moved_to_lost_and_found > 0 {
            let dir = lost_and_found_dir().to_string_lossy().to_string();
            self.shared.lock().unwrap().notify("Can’t sync file, moved it to “Lost and Found” folder.", &dir);
        }
        // The original stops writing when less than min_free_space_for_writes_kbytes is free.
        let need_kb: u64 = plan.download.iter().filter_map(|n| remote_files.get(n)).map(|e| (e.size.max(0) as u64) / 1024).sum();
        let free = free_kb(&folder);
        let room = free.map(|f| f >= need_kb + MIN_FREE_KB).unwrap_or(true);
        if !plan.download.is_empty() && !room {
            ok = false;
            self.shared.lock().unwrap().notify("Not enough disk space", &format!("You don’t have enough disk space to sync some files ({folder})"));
        }
        if !plan.download.is_empty() && room && !self.settings.lock().unwrap().paused {
            let f = list_file("download", &plan.download);
            let args = vec!["copy".into(), spec.to_string(), folder.clone(), "--files-from-raw".into(), f, "--no-traverse".into()];
            ok &= self.run_job("Mirror files", &place, Some(false), args, st, Some(folder.clone()));
        }
        if !plan.delete_remote.is_empty() && !self.settings.lock().unwrap().paused {
            // Drive moves deleted files to its trash (rclone's default for Google Drive).
            let f = list_file("delete", &plan.delete_remote);
            let args = vec!["delete".into(), spec.to_string(), "--files-from-raw".into(), f];
            ok &= self.run_job("Mirror files", &place, None, args, st, None);
            let mut sh = self.shared.lock().unwrap();
            for n in &plan.delete_remote {
                sh.push_activity(Activity { size: prior.get(n).map(|e| e.size as u64), name: short_name(n), place: "Google Drive".into(), state: ActState::Deleted, at: SystemTime::now() });
            }
        }
        if !plan.delete_local.is_empty() && !self.settings.lock().unwrap().paused {
            for n in &plan.delete_local {
                let path = Path::new(&folder).join(n);
                // Put it in the desktop trash when possible, so it can be restored.
                let trashed = Command::new("gio").arg("trash").arg(&path).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
                if !trashed && std::fs::remove_file(&path).is_err() {
                    ok = false;
                    continue;
                }
                self.shared.lock().unwrap().push_activity(Activity {
                    size: local.get(n).map(|e| e.size as u64),
                    name: short_name(n),
                    place: place.clone(),
                    state: ActState::Deleted,
                    at: SystemTime::now(),
                });
            }
        }
        if ok && !self.settings.lock().unwrap().paused {
            let files = next_state(&local, &remote_files, &plan);
            let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            let synced_at = next_synced_at(&saved.synced_at, &plan, &files, now);
            let state = State { remote: state_key.clone(), folder: folder.clone(), files, synced_at, links };
            if let Ok(t) = serde_json::to_string(&state) {
                let tmp = state_path.with_extension("tmp");
                if std::fs::write(&tmp, t).is_ok() {
                    let _ = std::fs::rename(tmp, &state_path);
                }
            }
            self.shared.lock().unwrap().last_done = Some(SystemTime::now());
        }
        let mut sh = self.shared.lock().unwrap();
        sh.job = None;
        sh.transfers.clear();
        if plan.is_empty() {
            sh.last_done = Some(SystemTime::now());
        }
        false
    }

    /// Holds syncing for the account and tells the person, with the original's wording
    /// ("Syncing paused: Ransomware detected on {0}").
    fn raise_ransomware(&mut self, remote: &str, held: crate::ransom::HeldFolder, at_ms: i64) {
        let mut alert = crate::ransom::load_alert(remote).unwrap_or_else(|| {
            let (date, time) = crate::ransom::local_date_time(at_ms);
            crate::ransom::Alert { account: remote.to_string(), at_ms, folders: Vec::new(), date, time }
        });
        alert.folders.retain(|f| f.folder != held.folder);
        alert.folders.push(held);
        crate::ransom::save_alert(&alert);
        let who = self.settings.lock().unwrap().account_label(remote);
        let (date, time) = (alert.date.clone(), alert.time.clone());
        let title = format!("Syncing paused: Ransomware detected on {who}");
        let body = format!("Multiple files were modified, and syncing was paused on {date} at {time}. If you don‘t recognize this activity, action needs to be taken to prevent further corruption.");
        desktop_notify(&title, &body);
        let mut sh = self.shared.lock().unwrap();
        sh.notify(&title, &body);
        sh.ransomware.insert(remote.to_string(), alert);
        self.ctx.request_repaint();
    }

    /// Ends a ransomware hold. `restore`: move what changed on this computer to a new folder
    /// and put Drive's copies (which were never overwritten) back; otherwise the person
    /// recognised the activity and the changes are synced as they are.
    fn ransomware_resolve(&mut self, remote: &str, restore: bool) {
        let Some(alert) = crate::ransom::load_alert(remote).or_else(|| self.shared.lock().unwrap().ransomware.get(remote).cloned()) else {
            self.shared.lock().unwrap().ransomware.remove(remote);
            return;
        };
        let st = self.settings.lock().unwrap().for_account(remote);
        let mut ok = true;
        let mut aside_dir = None;
        if restore {
            let (date, time) = crate::ransom::local_date_time(crate::ransom::now_ms());
            let base = unique_path(lost_and_found_dir().join(format!("Changed files {date} {}", time.replace(':', "."))));
            for f in &alert.folders {
                let name = Path::new(&f.folder).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "folder".into());
                for n in f.changed.iter().chain(f.added.iter()) {
                    let src = Path::new(&f.folder).join(n);
                    if !src.exists() {
                        continue;
                    }
                    let dst = base.join(&name).join(n);
                    if let Some(parent) = dst.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    if std::fs::rename(&src, &dst).is_err() && !(std::fs::copy(&src, &dst).is_ok() && std::fs::remove_file(&src).is_ok()) {
                        ok = false;
                    }
                }
                let back: Vec<String> = f.changed.iter().chain(f.deleted.iter()).cloned().collect();
                if !back.is_empty() {
                    let list = config_dir().join(format!("ransomware-restore-{}.txt", std::process::id()));
                    let _ = std::fs::write(&list, back.join("\n"));
                    let args = vec!["copy".into(), f.spec.clone(), f.folder.clone(), "--files-from-raw".into(), list.to_string_lossy().to_string(), "--no-traverse".into()];
                    ok &= self.run_job("Mirror files", &f.folder, Some(false), args, &st, Some(f.folder.clone()));
                    let _ = std::fs::remove_file(&list);
                }
            }
            aside_dir = Some(base);
        }
        if restore && !ok {
            // Keep the hold: some files couldn't be put back; the person can try again.
            self.shared.lock().unwrap().notify("Can’t clear sync restriction", "Google Drive was unable to clear the sync restriction. Check your internet connection and try again later.");
            return;
        }
        crate::ransom::clear_alert(remote, !restore);
        let who = self.settings.lock().unwrap().account_label(remote);
        let mut sh = self.shared.lock().unwrap();
        sh.ransomware.remove(remote);
        sh.notify("Cleared sync restriction", &format!("Google Drive cleared the sync restriction on {who}."));
        if let Some(dir) = aside_dir.filter(|d| d.exists()) {
            sh.notify("Changed files moved", &format!("The files changed on this computer were moved to {}", dir.display()));
        }
        drop(sh);
        if let Some(r) = self.runs.get_mut(remote) {
            r.next_mirror = None;
        }
        self.next_folder.clear();
        self.ctx.request_repaint();
    }

    fn backup_folder(&mut self, remote: &str, st: &Settings, f: &LocalFolder) {
        if let Some(used_by) = st.overlap_with_drive_folder(&f.path, remote) {
            self.shared.lock().unwrap().notify("Can’t sync folder to Drive", &format!("The folder cannot be within {used_by} which is being used by {}", st.account_label(remote)));
            return;
        }
        if !Path::new(&f.path).is_dir() {
            self.shared.lock().unwrap().notify("Can’t sync folder to Drive", &format!("The selected folder could not be found: “{}”", f.path));
            return;
        }
        let base = Path::new(&f.path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Folder".into());
        if f.to_drive {
            let dest = format!("{remote}:{BACKUP_ROOT}/{}/{}", hostname(), base);
            // Make the folder appear in Drive at once, even while it is empty.
            let _ = rclone().args(["mkdir", &dest]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            let mut args = vec!["sync".into(), f.path.clone(), dest, "--create-empty-src-dirs".into()];
            args.extend(ignore_args());
            self.run_job("Sync with Google Drive", &base, Some(true), args, st, Some(f.path.clone()));
        }
        let photos = self.shared.lock().unwrap().photos_account.clone();
        if f.to_photos {
            if let Some(p) = photos {
                let args = vec![
                    "copy".into(),
                    f.path.clone(),
                    format!("{p}:upload"),
                    "--ignore-case".into(),
                    "--include".into(),
                    media_include(st.upload_raw),
                    "--retries".into(),
                    PHOTOS_UPLOAD_RETRIES.to_string(),
                    // Files still being edited wait until they settle (a later pass takes them).
                    "--min-age".into(),
                    format!("{PHOTOS_SETTLE_SECS}s"),
                ];
                self.run_job("Back up to Google Photos", &base, Some(true), args, st, Some(f.path.clone()));
            }
        }
    }

    fn run_job(&mut self, label: &str, place: &str, upload: Option<bool>, args: Vec<String>, st: &Settings, root: Option<String>) -> bool {
        let mut c = rclone();
        c.args(&args).args(["--use-json-log", crate::diag::rclone_verbosity(), "--stats", "1s", "--stats-log-level", "NOTICE"]);
        if let Some(bw) = st.bwlimit() {
            c.args(["--bwlimit", &bw]);
        }
        c.stdout(Stdio::null()).stderr(Stdio::piped());
        let Ok(mut child) = c.spawn() else { return false };
        let err = child.stderr.take();
        *self.job.lock().unwrap() = Some(child);
        {
            let mut sh = self.shared.lock().unwrap();
            sh.job = Some(label.to_string());
            sh.last_error = None;
        }
        self.ctx.request_repaint();
        if let Some(err) = err {
            read_log(err, &self.shared, &self.ctx, place, upload, root);
        }
        let status = self.job.lock().unwrap().take().and_then(|mut c| c.wait().ok());
        let ok = status.map(|s| s.success()).unwrap_or(false);
        let mut sh = self.shared.lock().unwrap();
        sh.job = None;
        sh.transfers.clear();
        sh.pending = 0;
        if ok {
            sh.last_done = Some(SystemTime::now());
        } else if !self.settings.lock().unwrap().paused {
            let why = sh.last_error.clone().unwrap_or_else(|| "rclone stopped with an error".into());
            sh.notify("Can’t sync folder to Drive", &format!("{label}: “{place}” didn’t finish ({why}). It will be tried again."));
        }
        ok
    }
}

fn stop_mount(run: &mut AcctRun, shared: &Arc<Mutex<Shared>>, remote: &str) {
    if let Some(mut c) = run.mount.take() {
        let _ = unmount(&run.mount_dir);
        std::thread::sleep(Duration::from_millis(300));
        let _ = c.kill();
        let _ = c.wait();
        shared.lock().unwrap().mounted_set.remove(remote);
        run.mount_started = None;
    }
}

fn collect_files(p: &Path, out: &mut Vec<PathBuf>, max: usize) {
    if out.len() >= max {
        return;
    }
    if p.is_file() {
        out.push(p.to_path_buf());
    } else if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            collect_files(&e.path(), out, max);
        }
    }
}

/// Folders on network storage (NFS, SMB, SSHFS…) are backed up every 6 hours, like the original's NAS backup.
pub fn is_network_path(path: &str) -> bool {
    let Ok(mounts) = std::fs::read_to_string("/proc/mounts") else { return false };
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 {
            continue;
        }
        let mp = f[1].replace("\\040", " ");
        if Path::new(path).starts_with(&mp) && best.as_ref().map(|(l, _)| mp.len() > *l).unwrap_or(true) {
            best = Some((mp.len(), f[2].to_string()));
        }
    }
    matches!(best.map(|(_, t)| t).as_deref(), Some("nfs" | "nfs4" | "cifs" | "smb3" | "smbfs" | "fuse.sshfs" | "davfs" | "fuse.davfs" | "9p"))
}

fn remote_names() -> Vec<String> {
    rclone().args(["listremotes"]).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().trim_end_matches(':').to_string()).collect()).unwrap_or_default()
}

/// (id, name) of every shared drive the account can open; None when the list couldn't be read
/// (then nothing is removed, like the original's "Unable to … list shared drives").
fn list_shared_drives(remote: &str) -> Option<Vec<(String, String)>> {
    let o = rclone().args(["backend", "drives", &format!("{remote}:")]).stderr(Stdio::null()).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let serde_json::Value::Array(a) = serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()? else { return None };
    Some(a.iter().filter_map(|v| Some((v["id"].as_str()?.to_string(), v["name"].as_str()?.to_string()))).collect())
}

/// The account's e-mail, display name and profile picture address, read from Google with the
/// connection's own sign-in.
fn account_profile(remote: &str) -> Option<(String, String, String)> {
    let j = drive_api(remote, "https://www.googleapis.com/drive/v3/about?fields=user(emailAddress,displayName,photoLink)")?;
    let u = &j["user"];
    Some((u["emailAddress"].as_str()?.to_string(), u["displayName"].as_str().unwrap_or("").to_string(), u["photoLink"].as_str().unwrap_or("").to_string()))
}

/// Where an account's profile picture is kept (a 256 px PNG).
pub fn avatar_path(remote: &str) -> PathBuf {
    dirs::cache_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("drive-linux").join("avatars").join(format!("{remote}.png"))
}

/// The profile picture address the original asks for (function 0x1003d5d60, home_dialog.cc):
/// no options → add "=s256-c"; with options → keep those not starting with c, h, s or w (crop,
/// height, size, width) after "=s256-c".
pub fn avatar_url(link: &str) -> String {
    match link.find('=') {
        None => format!("{link}=s256-c"),
        Some(i) => {
            let mut url = format!("{}=s256-c", &link[..i]);
            for opt in link[i + 1..].split('-') {
                match opt.chars().next() {
                    Some('c' | 'h' | 's' | 'w') | None => {}
                    Some(_) => {
                        url.push('-');
                        url.push_str(opt);
                    }
                }
            }
            url
        }
    }
}

/// Size the original requests profile pictures at (the "s256" in 0x1003d5d60).
pub const AVATAR_PX: u32 = 256;

fn save_avatar(remote: &str, link: &str) {
    let url = avatar_url(link);
    let Ok(out) = Command::new("curl").args(["-fsSL", "--max-time", "20", &url]).stderr(Stdio::null()).output() else { return };
    if !out.status.success() {
        return;
    }
    let Ok(img) = image::load_from_memory(&out.stdout) else { return };
    let img = img.resize_to_fill(AVATAR_PX, AVATAR_PX, image::imageops::FilterType::Lanczos3);
    let path = avatar_path(remote);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tmp = path.with_extension("tmp.png");
    if img.save(&tmp).is_ok() {
        let _ = std::fs::rename(tmp, path);
    }
}

/// Drive ID of an item (`remote:path`) and whether it is a folder.
pub fn item_id(spec: &str) -> Option<(String, bool)> {
    let out = rclone().args(["lsjson", "--stat", spec]).stderr(Stdio::null()).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let id = v["ID"].as_str()?.to_string();
    Some((id, v["IsDir"].as_bool().unwrap_or(false)))
}

/// The item's own web address from Drive (`webViewLink`, the v2 `alternateLink` the original's
/// core hands to CopyToClipboard, function 0x100580500).
pub fn web_view_link(remote: &str, id: &str) -> Option<String> {
    let v = drive_api(remote, &format!("https://www.googleapis.com/drive/v3/files/{id}?fields=webViewLink&supportsAllDrives=true"))?;
    v["webViewLink"].as_str().map(|s| s.to_string())
}

/// POST a JSON body to a Drive API URL with the connection's own sign-in (token on curl's stdin).
/// Returns Drive's answer, or its error message.
fn drive_api_post(remote: &str, url: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    let access = access_token(remote).ok_or("not signed in")?;
    let mut child = Command::new("curl")
        .args(["-sS", "-X", "POST", "-H", "@-", "-H", "Content-Type: application/json", "--data-binary"])
        .arg(body.to_string())
        .arg(url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or("curl")?;
        let _ = writeln!(stdin, "Authorization: Bearer {access}");
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|_| last_line(&String::from_utf8_lossy(&out.stderr)).to_string())?;
    if let Some(msg) = v["error"]["message"].as_str() {
        return Err(msg.to_string());
    }
    Ok(v)
}

/// A desktop notification, so file manager actions show a result while the window is hidden.
pub fn desktop_notify(title: &str, body: &str) {
    let _ = Command::new("notify-send").args(["-a", crate::APP_NAME, "-i", "drive-linux", title, body]).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

/// Free space (KiB) on the disk holding `path`.
fn free_kb(path: &str) -> Option<u64> {
    let out = Command::new("df").args(["-Pk", path]).stderr(Stdio::null()).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().nth(1)?.split_whitespace().nth(3)?.parse().ok()
}

fn access_token(remote: &str) -> Option<String> {
    let dump = rclone().args(["config", "dump"]).stderr(Stdio::null()).output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&dump.stdout).ok()?;
    let token: serde_json::Value = serde_json::from_str(v[remote]["token"].as_str()?).ok()?;
    token["access_token"].as_str().map(|s| s.to_string())
}

/// GET a Drive API URL with the connection's own sign-in (token passed to curl on stdin).
fn drive_api(remote: &str, url: &str) -> Option<serde_json::Value> {
    let access = access_token(remote)?;
    let mut child = Command::new("curl").args(["-fsS", "-H", "@-", url]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take()?;
        let _ = writeln!(stdin, "Authorization: Bearer {access}");
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice(&out.stdout).ok()
}

/// Asks Drive's change list whether anything changed since `token`; returns (changed, next token).
/// The first call only fetches a starting point.
/// What one read of Drive's change list found.
#[derive(Debug, Default, PartialEq)]
struct Changes {
    /// Files or folders changed.
    files: bool,
    /// A shared drive was added, renamed, hidden or removed (changeType "drive").
    drives: bool,
    /// Where to continue next time.
    token: String,
}

/// Sorts one page of the change list into file changes and shared drive changes.
fn classify_changes(v: &serde_json::Value, c: &mut Changes) {
    for ch in v["changes"].as_array().into_iter().flatten() {
        if ch["changeType"].as_str() == Some("drive") {
            c.drives = true;
        } else {
            c.files = true;
        }
    }
}

fn drive_changes(remote: &str, token: Option<&str>) -> Option<Changes> {
    let Some(t) = token else {
        let v = drive_api(remote, "https://www.googleapis.com/drive/v3/changes/startPageToken?supportsAllDrives=true")?;
        return Some(Changes { token: v["startPageToken"].as_str()?.to_string(), ..Default::default() });
    };
    let mut page = t.to_string();
    let mut c = Changes::default();
    for _ in 0..20 {
        let url = format!(
            "https://www.googleapis.com/drive/v3/changes?pageToken={page}&pageSize=1000&supportsAllDrives=true&includeItemsFromAllDrives=true&fields=nextPageToken,newStartPageToken,changes(changeType,fileId,driveId)"
        );
        let v = drive_api(remote, &url)?;
        classify_changes(&v, &mut c);
        if let Some(n) = v["newStartPageToken"].as_str() {
            c.token = n.to_string();
            return Some(c);
        }
        page = v["nextPageToken"].as_str()?.to_string();
    }
    c.token = page;
    Some(c)
}

/// Where conflicting copies from this computer are kept (the original's "Lost and Found").
pub fn lost_and_found_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("drive-linux").join("Lost and Found")
}

/// "name.ext" → "name (1).ext" … when the name is already taken.
fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = p.parent().map(|x| x.to_path_buf()).unwrap_or_default();
    (1..).map(|i| parent.join(format!("{stem} ({i}){ext}"))).find(|c| !c.exists()).unwrap()
}

/// The last list saved by the earlier rclone two-way sync for this folder, if any.
fn old_bisync_listing(remote: &str, folder: &str) -> Option<crate::mirror::Listing> {
    let session = format!("{remote}_..{}", folder.trim_start_matches('/').replace('/', "_"));
    let dir = config_dir().join("bisync");
    for suffix in ["path2.lst", "path2.lst-err"] {
        if let Ok(t) = std::fs::read_to_string(dir.join(format!("{session}.{suffix}"))) {
            return Some(crate::mirror::parse_bisync_listing(&t));
        }
    }
    None
}

/// True when a connection has a saved sign-in (a half-finished sign-in leaves none).
fn has_token(name: &str) -> bool {
    rclone().args(["config", "show", name]).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim_start().starts_with("token"))).unwrap_or(false)
}

pub fn rclone_version() -> String {
    rclone().arg("version").stderr(Stdio::null()).output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.trim().to_string())).unwrap_or_else(|| "rclone".into())
}

/// Runs rclone's browser sign-in, giving up after CONNECT_TIMEOUT_SECS or when cancelled.
fn run_sign_in(name: &str, photos: bool, slot: &Arc<Mutex<Option<Child>>>) -> Result<(), String> {
    // A connection left without a sign-in by an earlier attempt is removed first.
    if !has_token(name) {
        let _ = rclone().args(["config", "delete", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let args: &[&str] = if photos { &["google photos", "read_only=false"] } else { &["drive", "scope=drive"] };
    let mut child = rclone().args(["config", "create", name]).args(args).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let err = child.stderr.take();
    let err_text = Arc::new(Mutex::new(String::new()));
    if let Some(mut e) = err {
        let t = err_text.clone();
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = std::io::Read::read_to_string(&mut e, &mut buf);
            *t.lock().unwrap() = buf;
        });
    }
    *slot.lock().unwrap() = Some(child);
    let started = Instant::now();
    let status = loop {
        std::thread::sleep(Duration::from_millis(500));
        let mut g = slot.lock().unwrap();
        let Some(c) = g.as_mut() else { break None };
        match c.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if started.elapsed() > Duration::from_secs(CONNECT_TIMEOUT_SECS) => {
                let _ = c.kill();
                let _ = c.wait();
                break None;
            }
            Ok(None) => {}
            Err(_) => break None,
        }
    };
    *slot.lock().unwrap() = None;
    std::thread::sleep(Duration::from_millis(200));
    let ok = status.map(|s| s.success()).unwrap_or(false) && has_token(name);
    if ok {
        return Ok(());
    }
    // Don't leave a connection without a sign-in behind: it would look connected.
    let _ = rclone().args(["config", "delete", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    let text = err_text.lock().unwrap().clone();
    Err(if status.is_none() { "The sign-in was cancelled or took too long.".to_string() } else { last_line(&text).to_string() })
}

fn unmount(dir: &str) -> std::io::Result<()> {
    if dir.is_empty() {
        return Ok(());
    }
    let ok = Command::new("fusermount3").args(["-u", "-z", dir]).stderr(Stdio::null()).status();
    if ok.map(|s| s.success()).unwrap_or(false) {
        return Ok(());
    }
    Command::new("fusermount").args(["-u", "-z", dir]).stderr(Stdio::null()).status().map(|_| ())
}

fn last_line(s: &str) -> &str {
    s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("An error occurred, please try again later")
}

/// When to ask Drive's change list again: every changelog_polling_interval_ms; after failures the
/// original's PeriodicPoller backs off between 2× and 4× that period (function 0x100ab32a0 builds
/// it from the int32 field 7 with period×2 and period×4).
fn changes_poll_delay(failures: u32) -> Duration {
    let base = Duration::from_millis(CHANGES_POLL_MS);
    match failures {
        0 => base,
        1 => base * 2,
        _ => base * 4,
    }
}

/// Which offline files to read this run: never read first, then the longest ago, skipping those
/// read within PIN_RETRY_SECS, at most PIN_MAX_PER_RUN.
fn pins_due(files: &[PathBuf], last: &std::collections::HashMap<PathBuf, Instant>, now: Instant) -> Vec<PathBuf> {
    let mut due: Vec<(Option<Instant>, &PathBuf)> =
        files.iter().map(|f| (last.get(f).copied(), f)).filter(|(t, _)| t.map(|t| now.duration_since(t) >= Duration::from_secs(PIN_RETRY_SECS)).unwrap_or(true)).collect();
    due.sort_by_key(|(t, _)| *t); // None (never read) sorts first
    due.into_iter().take(PIN_MAX_PER_RUN).map(|(_, f)| f.clone()).collect()
}

fn short_name(path: &str) -> String {
    Path::new(path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

/// Reads rclone's JSON log lines and turns them into activity rows.
fn read_log(err: impl std::io::Read, shared: &Arc<Mutex<Shared>>, ctx: &eframe::egui::Context, place: &str, upload: Option<bool>, root: Option<String>) {
    let mut last_save = Instant::now();
    for line in BufReader::new(err).lines().map_while(Result::ok) {
        // Everything rclone says goes to the log; progress lines only with verbose logging.
        if crate::diag::VERBOSE.load(std::sync::atomic::Ordering::Relaxed) || !line.contains("\"stats\"") {
            crate::diag::log_line(&line);
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
        let msg = v["msg"].as_str().unwrap_or("");
        let object = v["object"].as_str().unwrap_or("");
        let level = v["level"].as_str().unwrap_or("");
        let mut sh = shared.lock().unwrap();
        if let Some(stats) = v.get("stats") {
            let total = stats["totalTransfers"].as_u64().unwrap_or(0);
            let done = stats["transfers"].as_u64().unwrap_or(0);
            sh.pending = total.saturating_sub(done);
            sh.transfers = stats["transferring"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|t| Activity {
                            size: t["size"].as_u64(),
                            name: short_name(t["name"].as_str().unwrap_or("")),
                            place: place.into(),
                            state: ActState::Transfer { pct: t["percentage"].as_f64().unwrap_or(0.0) as f32, upload },
                            at: SystemTime::now(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let pairs: Vec<(String, u64)> = sh.transfers.iter().filter_map(|a| a.size.map(|z| (a.name.clone(), z))).collect();
            for (k, z) in pairs {
                sh.sizes.insert(k, z);
            }
            if sh.sizes.len() > 2000 {
                sh.sizes.clear();
            }
        } else if !object.is_empty() && (msg.starts_with("Copied") || msg.contains("upload succeeded")) {
            let name = short_name(object);
            let size = sh.sizes.get(&name).copied().or_else(|| root.as_ref().and_then(|r| std::fs::metadata(Path::new(r).join(object)).ok()).map(|m| m.len()));
            sh.push_activity(Activity { size, name, place: place.into(), state: ActState::Synced, at: SystemTime::now() });
        } else if !object.is_empty() && msg.starts_with("Deleted") {
            sh.push_activity(Activity { size: None, name: short_name(object), place: place.into(), state: ActState::Deleted, at: SystemTime::now() });
        } else if level == "error" {
            sh.last_error = Some(msg.trim().to_string());
            let name = if object.is_empty() { place.to_string() } else { short_name(object) };
            sh.push_activity(Activity { size: None, name, place: place.into(), state: ActState::Error(msg.to_string()), at: SystemTime::now() });
        }
        if sh.history_dirty && last_save.elapsed() > Duration::from_secs(10) {
            sh.save_history();
            last_save = Instant::now();
        }
        drop(sh);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rclone_log_becomes_activity() {
        let log = concat!(
            r#"{"level":"notice","msg":"x","stats":{"totalTransfers":3,"transfers":1,"transferring":[{"name":"dir/photo.jpg","percentage":40,"size":300003}]}}"#,
            "\n",
            r#"{"level":"info","msg":"Copied (new)","object":"dir/photo.jpg"}"#,
            "\n",
            r#"{"level":"info","msg":"Deleted","object":"old.txt"}"#,
            "\n",
            r#"{"level":"error","msg":"permission denied","object":"secret.bin"}"#,
            "\n",
        );
        let shared = Arc::new(Mutex::new(Shared::default()));
        let ctx = eframe::egui::Context::default();
        read_log(log.as_bytes(), &shared, &ctx, "Docs", Some(true), None);
        let sh = shared.lock().unwrap();
        assert_eq!(sh.pending, 2);
        assert_eq!(sh.activity.len(), 3);
        assert_eq!(sh.activity[2].name, "photo.jpg");
        assert_eq!(sh.activity[2].size, Some(300003));
        assert_eq!(sh.activity[2].state, ActState::Synced);
        assert_eq!(sh.activity[1].state, ActState::Deleted);
        assert!(matches!(sh.activity[0].state, ActState::Error(_)));
    }

    #[test]
    fn stream_folder_follows_original_naming() {
        let mut s = Settings::default();
        let home = dirs::home_dir().unwrap();
        assert_eq!(s.stream_dir("gdrive"), home.join("GoogleDrive-gdrive").to_string_lossy());
        s.stream_folder = "/data/drive".into();
        assert_eq!(s.stream_dir("gdrive"), "/data/drive");
        assert_eq!(MOUNT_MAX_ATTEMPTS, 10);
        assert_eq!(crate::app::cache_percent(5, 10), 50);
        assert_eq!(crate::app::cache_percent(1, 3), 33);
    }

    #[test]
    fn original_lists_and_folder_rules() {
        assert!(media_include(true).contains("cr3") && media_include(true).contains("mp4"));
        assert!(!media_include(false).contains("cr3"));
        assert_eq!(IGNORE_NAMES.len(), 11);
        let dir = std::env::temp_dir().join(format!("dl-check-{}", std::process::id()));
        let d = dir.to_string_lossy().to_string();
        assert_eq!(check_mount_dir(&d), MountCheck::Ok); // missing is fine
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".hidden"), "x").unwrap();
        assert_eq!(check_mount_dir(&d), MountCheck::Ok); // hidden entries allowed
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(check_mount_dir(&d), MountCheck::NotEmpty);
        assert_eq!(check_mount_dir(&dir.join("a.txt").to_string_lossy()), MountCheck::NotAFolder);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn history_survives_a_round_trip() {
        let mut h = History::default();
        h.activity.push_back(Activity { size: Some(5), name: "a.txt".into(), place: "/x".into(), state: ActState::Synced, at: SystemTime::now() });
        h.notices.push(Notice { title: "t".into(), body: "b".into(), at: SystemTime::now(), read: true });
        let text = serde_json::to_string(&h).unwrap();
        let back: History = serde_json::from_str(&text).unwrap();
        assert_eq!(back.activity.len(), 1);
        assert_eq!(back.activity[0].name, "a.txt");
        assert!(back.notices[0].read);
    }

    #[test]
    fn ignored_devices_match_by_id_or_old_path() {
        let mut st = Settings::default();
        st.ignored_media.push(IgnoredMedia { media_id: "uuid:ABC".into(), name: "USB".into(), last_mount_point: "/run/media/u/USB".into() });
        st.ignored_media.push(IgnoredMedia { media_id: "path:/media/u/OLD".into(), name: "OLD".into(), last_mount_point: "/media/u/OLD".into() });
        assert!(st.is_ignored("uuid:ABC", "/run/media/u/ELSEWHERE"));
        assert!(st.is_ignored("uuid:ZZZ", "/media/u/OLD"));
        assert!(!st.is_ignored("uuid:ZZZ", "/run/media/u/NEW"));
    }

    #[test]
    fn shared_drives_mirror_next_to_my_drive() {
        let mut st = Settings::default();
        st.remote = "gdrive".into();
        st.mirror_folder = "/mnt/DATA2/testdrive".into();
        st.emails.insert("gdrive".into(), "me@example.com".into());
        assert_eq!(st.shared_drive_folder("gdrive", "Team/A").to_string_lossy(), "/mnt/DATA2/Shared drives (me@example.com)/Team_A");
    }

    #[test]
    fn avatar_url_follows_original_rule() {
        assert_eq!(avatar_url("https://lh3.googleusercontent.com/a/abc"), "https://lh3.googleusercontent.com/a/abc=s256-c");
        assert_eq!(avatar_url("https://lh3.googleusercontent.com/a/abc=s64"), "https://lh3.googleusercontent.com/a/abc=s256-c");
        assert_eq!(avatar_url("https://lh3.googleusercontent.com/a/abc=s96-c-k-no"), "https://lh3.googleusercontent.com/a/abc=s256-c-k-no");
    }

    #[test]
    fn bandwidth_flag_matches_original_units() {
        let mut s = Settings::default();
        assert_eq!(s.bwlimit(), None);
        s.download_limit_on = true;
        s.download_kbps = 500;
        assert_eq!(s.bwlimit().as_deref(), Some("off:500k"));
        s.upload_limit_on = true;
        s.upload_kbps = 999_999_999; // clamped to the original's max
        assert_eq!(s.bwlimit().as_deref(), Some("100000000k:500k"));
    }
}

#[cfg(test)]
mod live_api {
    /// Uses the real connection; creates nothing (Drive refuses an empty target).
    /// `cargo test refused_shortcut -- --ignored`
    #[test]
    #[ignore]
    fn refused_shortcut() {
        let body = serde_json::json!({"name": "x", "mimeType": "application/vnd.google-apps.shortcut", "shortcutDetails": {"targetId": ""}, "parents": ["root"]});
        let r = super::drive_api_post("gdrive", "https://www.googleapis.com/drive/v3/files?supportsAllDrives=true&fields=id,name", &body);
        println!("RESULT {r:?}");
        assert!(r.is_err());
    }
}

#[cfg(test)]
mod pin_tests {
    use super::*;

    #[test]
    fn only_drive_changes_reread_the_shared_drive_list() {
        let mut c = Changes::default();
        classify_changes(&serde_json::json!({"changes": [{"changeType": "file", "fileId": "a"}]}), &mut c);
        assert!(c.files && !c.drives);
        let mut c = Changes::default();
        classify_changes(&serde_json::json!({"changes": [{"changeType": "drive", "driveId": "0A"}]}), &mut c);
        assert!(!c.files && c.drives);
        let mut c = Changes::default();
        classify_changes(&serde_json::json!({"changes": []}), &mut c);
        assert_eq!(c, Changes::default());
    }

    #[test]
    fn change_list_backs_off_like_the_original() {
        assert_eq!(changes_poll_delay(0), Duration::from_secs(10));
        assert_eq!(changes_poll_delay(1), Duration::from_secs(20));
        assert_eq!(changes_poll_delay(2), Duration::from_secs(40));
        assert_eq!(changes_poll_delay(50), Duration::from_secs(40), "never longer than 4×");
        assert_eq!(MIRROR_BACKUP_POLL_SECS * 1000, 11_111_000);
        assert_eq!(FOLDER_POLL_SECS, 240 * 60, "slurp_poll_path_frequency_min = 240");
        assert_eq!(PHOTOS_SETTLE_SECS * 1000, 300_000, "photos_debounce_edit_time_ms = 300000");
    }

    #[test]
    fn offline_files_follow_the_subscription_limits() {
        let files: Vec<PathBuf> = (0..80).map(|i| PathBuf::from(format!("/m/f{i}"))).collect();
        let now = Instant::now() + Duration::from_secs(PIN_RETRY_SECS * 2);
        let mut last = std::collections::HashMap::new();
        // never read: 50 at most per run
        assert_eq!(pins_due(&files, &last, now).len(), PIN_MAX_PER_RUN);
        // read just now: not again until the retry interval has passed
        for f in &files {
            last.insert(f.clone(), now);
        }
        assert!(pins_due(&files, &last, now).is_empty());
        // one read long ago comes back
        last.insert(files[3].clone(), now - Duration::from_secs(PIN_RETRY_SECS));
        assert_eq!(pins_due(&files, &last, now), vec![files[3].clone()]);
    }
}
