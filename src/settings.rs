use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// Values from MECHANICS.md
pub const RATE_MIN: u64 = 1;
pub const RATE_MAX: u64 = 100_000_000;
pub const CACHE_MIN: u64 = 1;
pub const CACHE_MAX: u64 = 100_000_000;
/// Full re-check of a mirrored folder when nothing reported a change: the original's backup poll,
/// a PeriodicPoller with a fixed period of changelog_backup_poll_interval_ms = 11111000
/// (int64 field 61, 0x103f4bf79; poller built in function 0x100ab30e0, periodic_poller.cc
/// 0x100ab3480) — used while local changes are watched and Drive's change list answers.
pub const MIRROR_BACKUP_POLL_SECS: u64 = 11_111;
/// The same re-check when one of those isn't working (estimated).
pub const MIRROR_INTERVAL_SECS: u64 = 120;
/// Folders from this computer ("slurp roots" in the original) are walked again every
/// slurp_poll_path_frequency_min = 240 minutes (0x103f52a1d): SlurpSyncRoot::PollPath
/// (FUN_10015a020, slurp_sync_root.cc) → FSMedia::PollPath / PollPathRepeatedly (0x10013a470,
/// 0x10013b780, fs_media.cc). Changes in between come from the file watcher.
pub const FOLDER_POLL_SECS: u64 = 240 * 60;
/// The same walk when the folder can't be watched (estimated).
pub const FOLDER_INTERVAL_SECS: u64 = 300;
/// Activity items kept: default of item_events_buffer_size (0x103f49eed) = 50.
pub const ACTIVITY_MAX: usize = 50;
/// Original retries attaching the streaming folder up to 10 times, 2 s apart (function 0x1000518e0).
pub const MOUNT_MAX_ATTEMPTS: u32 = 10;
pub const MOUNT_RETRY_DELAY_SECS: u64 = 2;
/// Accounts at once: default of flag multi_account_max_accounts (0x103f529e6) = 4.
pub const MAX_ACCOUNTS: usize = 4;
/// How often Drive's change list is checked: changelog_polling_interval_ms (0x103f4b64e) = 10000.
pub const CHANGES_POLL_MS: u64 = 10_000;
/// A network drive that comes back is polled after reconnected_nas_polling_interval_seconds (0x103f528f1) = 60.
pub const NAS_RECONNECT_SECS: u64 = 60;
/// Writes stop when less is free: min_free_space_for_writes_kbytes (0x103f79dcd) = 512000.
pub const MIN_FREE_KB: u64 = 512_000;
/// Network storage is backed up every 6 hours (original string "Network-attached storage backed up every 6 hours").
pub const NAS_INTERVAL_SECS: u64 = 6 * 3600;
/// Wait after a local change before uploading: upload_idle_delay_msec (0x103f7b250) = 0
/// (the manager's 2-second loop still groups changes that arrive together).
pub const LOCAL_CHANGE_DEBOUNCE_SECS: u64 = 0;
/// Items kept available offline are looked at by the original's subscription task, throttled to
/// once per subscription_task_throttle_time_ms = 300000 (0x103f4d005; FUN_101761b10,
/// subscription_task.cc) ...
pub const PIN_CHECK_SECS: u64 = 300;
/// ... at most subscription_task_max_operations = 50 items per run (0x103f4d046; "Too many items
/// to re-subscribe, trimmed to …") ...
pub const PIN_MAX_PER_RUN: usize = 50;
/// ... and one item is tried again only after resubscribe_retry_interval_msec = 14400000
/// (0x103f4c219).
pub const PIN_RETRY_SECS: u64 = 14400;
/// Search results expire after file_provider_string_search_expiration_time_ms (0x103f4ff6b) = 300000.
pub const INDEX_REFRESH_SECS: u64 = 300;

/// Folder in My Drive that holds "Folders from your computer" (the original uses Drive's separate
/// "Computers" area, which other apps can't create).
pub const BACKUP_ROOT: &str = "Linux computers";

fn same_or_inside(a: &str, b: &str) -> bool {
    let a = std::path::Path::new(a);
    let b = std::path::Path::new(b);
    let ca = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let cb = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    ca.starts_with(&cb) || cb.starts_with(&ca)
}

/// Photo types the original backs up (default of flag 45371132, read in function 0x1009c22f0;
/// string at 0x1040a2936). The tail from "dng" on is the RAW list (0x1040a2983).
pub const PHOTO_EXTS: &str = "jpg,jpeg,jpe,png,gif,heif,heic,tif,tiff,webp,bmp,dib,ico,jp2,j2c,jpf,j2k,jpx";
pub const RAW_EXTS: &str = "dng,raw,arw,crw,cr2,cr3,dcr,dc2,erf,kc2,kdc,k25,mdc,mef,mos,mrw,nef,nrw,orf,pef,qtk,raf,rdc,rw2,srf,srw,sr2,x3f,3fr";
/// Video types (default of flag 45371155, function 0x1009c22f0; string at 0x1040a2a00).
pub const VIDEO_EXTS: &str = "3g2,3gp,asf,avi,divx,flv,m2t,m2ts,m4v,mkv,mmv,mng,mod,mov,mp4,mpe,mpg,mpeg,mts,ogv,qt,tod,webm,wmv";

/// Names the original never syncs (11 patterns built in function 0x1013684a0, ignore_patterns.cc),
/// written as rclone name patterns.
pub const IGNORE_NAMES: &[&str] = &["~$*", ".~lock.*#", "*.swp", ".DS_Store", "._*", ".apdisk", ".localized", "Icon\r", ".Trash", ".Spotlight-V100", ".fseventsd"];

/// rclone --exclude arguments for the ignore list (each name as a file and as a folder).
pub fn ignore_args() -> Vec<String> {
    let mut v = Vec::new();
    for n in IGNORE_NAMES {
        let n = n.to_string();
        v.push("--exclude".to_string());
        v.push(n.clone());
        v.push("--exclude".to_string());
        v.push(format!("{n}/**"));
    }
    v
}

/// rclone --include pattern for the photo/video backup.
pub fn media_include(upload_raw: bool) -> String {
    let mut exts = vec![PHOTO_EXTS];
    if upload_raw {
        exts.push(RAW_EXTS);
    }
    exts.push(VIDEO_EXTS);
    format!("*.{{{}}}", exts.join(","))
}

/// Result of checking a streaming folder, following CheckForValidDirMountPoint
/// (function 0x10152f980, path_util.cc).
#[derive(Debug, PartialEq)]
pub enum MountCheck {
    Ok,
    NotEmpty,
    NotAFolder,
    NoPermission,
}

pub fn check_mount_dir(path: &str) -> MountCheck {
    let p = std::path::Path::new(path);
    match std::fs::metadata(p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => MountCheck::Ok, // created later
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => MountCheck::NoPermission,
        Err(_) => MountCheck::NoPermission,
        Ok(m) if !m.is_dir() => MountCheck::NotAFolder,
        Ok(m) => {
            if m.permissions().readonly() {
                return MountCheck::NoPermission;
            }
            match std::fs::read_dir(p) {
                Err(_) => MountCheck::NoPermission,
                // Only hidden entries (names starting with '.') are allowed.
                Ok(rd) => {
                    for e in rd.flatten() {
                        let name = e.file_name();
                        let name = name.to_string_lossy();
                        if name.is_empty() || !name.starts_with('.') {
                            return MountCheck::NotEmpty;
                        }
                    }
                    MountCheck::Ok
                }
            }
        }
    }
}

/// The original's own wording for each refusal.
pub fn mount_check_message(c: &MountCheck) -> &'static str {
    match c {
        MountCheck::Ok => "",
        MountCheck::NotEmpty => "The selected folder must be empty.",
        MountCheck::NotAFolder => "The selected location is not a folder.",
        MountCheck::NoPermission => "The selected folder is missing read or write permission. You need both permissions for the streaming location.",
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum DriveMode {
    Stream,
    Mirror,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LocalFolder {
    pub path: String,
    pub to_drive: bool,
    pub to_photos: bool,
    /// Account it syncs to (empty: the selected account).
    #[serde(default)]
    pub account: String,
    /// A USB device: only synced while it is plugged in.
    #[serde(default)]
    pub device: bool,
    /// The device's own ID (filesystem UUID), so it is recognised wherever it gets mounted.
    #[serde(default)]
    pub media_id: String,
}

/// A device the person chose not to be asked about, like a row of the original's
/// `media (media_id, name, last_mount_point, …, ignored)` table (IgnoreMedia, 0x10093f270).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct IgnoredMedia {
    pub media_id: String,
    pub name: String,
    pub last_mount_point: String,
}

/// A mounted device's ID: its filesystem UUID (what the original keys devices by), else its
/// source device, else the mount path.
pub fn media_id(mount_point: &str) -> String {
    let out = std::process::Command::new("findmnt").args(["-no", "UUID,SOURCE", "--target", mount_point]).stderr(std::process::Stdio::null()).output();
    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        let mut parts = text.split_whitespace();
        let first = parts.next().unwrap_or("");
        let second = parts.next();
        // With no UUID findmnt prints only the source.
        if second.is_some() && !first.is_empty() {
            return format!("uuid:{first}");
        }
        if !first.is_empty() {
            return format!("dev:{first}");
        }
    }
    format!("path:{mount_point}")
}

/// Each account has its own My Drive syncing options, as in the original's account settings.
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AccountPrefs {
    pub drive_mode: DriveMode,
    pub stream_folder: String,
    pub mirror_folder: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    pub remote: String,
    pub photos_remote: String,
    pub drive_mode: DriveMode,
    pub stream_folder: String,
    pub mirror_folder: String,
    pub cache_folder: String,
    pub cache_limit_on: bool,
    pub cache_limit_gb: u64,
    pub download_limit_on: bool,
    pub download_kbps: u64,
    pub upload_limit_on: bool,
    pub upload_kbps: u64,
    pub autostart: bool,
    pub paused: bool,
    pub folders: Vec<LocalFolder>,
    pub mirror_initialized: bool,
    pub language: String,
    pub dark: bool,
    pub proxy_direct: bool,
    pub upload_raw: bool,
    pub account_prefs: std::collections::BTreeMap<String, AccountPrefs>,
    pub emails: std::collections::BTreeMap<String, String>,
    /// Display names of the accounts (from Google).
    pub names: std::collections::BTreeMap<String, String>,
    /// Files and folders inside a streaming folder kept available offline.
    pub offline_pins: Vec<String>,
    /// Older list of ignored device paths (moved into `ignored_media` on load).
    pub ignored_devices: Vec<String>,
    pub ignored_media: Vec<IgnoredMedia>,
    /// Notification types turned off with "Don’t show this again" (the original keeps a show flag
    /// per notification type and account: root_preference_db.cc, function 0x1009435b0).
    pub hidden_notices: Vec<String>,
    /// Closing the window keeps Drive running in the system tray, like the original (off: Close quits).
    pub close_keeps_running: bool,
    /// Google Drive entries in the file manager's right-click menu.
    pub file_manager_menu: bool,
    /// "Shared drive syncing": shared drive IDs mirrored per account (Mirror files only).
    pub synced_shared_drives: std::collections::BTreeMap<String, Vec<String>>,
    /// "Prompt me to back up devices" (original notification preference).
    pub prompt_devices: bool,
    /// "Enable verbose logging" (off by default).
    pub verbose_logging: bool,
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

impl Default for Settings {
    fn default() -> Self {
        let cache = dirs::cache_dir().unwrap_or_else(|| home().join(".cache")).join("drive-linux");
        Self {
            remote: "gdrive".into(),
            photos_remote: "gphotos".into(),
            drive_mode: DriveMode::Stream,
            stream_folder: String::new(), // empty = per-account default, see stream_dir()
            mirror_folder: home().join("Drive của tôi").to_string_lossy().into(),
            cache_folder: cache.to_string_lossy().into(),
            cache_limit_on: false,
            cache_limit_gb: 10,
            download_limit_on: false,
            download_kbps: 1000,
            upload_limit_on: false,
            upload_kbps: 1000,
            autostart: true, // gui_autostart_option (0x103f5224f) = true
            paused: false,
            folders: Vec::new(),
            mirror_initialized: false,
            language: "en".into(),
            dark: false,
            proxy_direct: false,
            upload_raw: false, // photos_upload_raw_files (0x103f77acc): bool with no default = off
            account_prefs: Default::default(),
            emails: Default::default(),
            names: Default::default(),
            offline_pins: Vec::new(),
            ignored_devices: Vec::new(),
            ignored_media: Vec::new(),
            prompt_devices: true,
            hidden_notices: Vec::new(),
            close_keeps_running: true,
            file_manager_menu: true,
            synced_shared_drives: Default::default(),
            verbose_logging: false,
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("drive-linux")
}

impl Settings {
    pub fn load() -> Self {
        let p = config_dir().join("settings.json");
        let mut st: Settings = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        for path in std::mem::take(&mut st.ignored_devices) {
            let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
            st.ignored_media.push(IgnoredMedia { media_id: format!("path:{path}"), name, last_mount_point: path });
        }
        st
    }

    /// Whether a device was set to be ignored (by its ID, or by an older path entry).
    pub fn is_ignored(&self, id: &str, mount_point: &str) -> bool {
        self.ignored_media.iter().any(|m| m.media_id == id || m.media_id == format!("path:{mount_point}"))
    }

    fn current_prefs(&self) -> AccountPrefs {
        AccountPrefs { drive_mode: self.drive_mode, stream_folder: self.stream_folder.clone(), mirror_folder: self.mirror_folder.clone() }
    }

    /// The syncing options of one account (the selected account's are the top-level fields).
    pub fn prefs_for(&self, account: &str) -> AccountPrefs {
        if account == self.remote {
            return self.current_prefs();
        }
        self.account_prefs.get(account).cloned().unwrap_or_else(|| AccountPrefs {
            drive_mode: DriveMode::Stream,
            stream_folder: String::new(),
            mirror_folder: home().join(format!("My Drive ({account})")).to_string_lossy().into(),
        })
    }

    /// These settings as seen by one account's sync.
    pub fn for_account(&self, account: &str) -> Settings {
        let p = self.prefs_for(account);
        let mut s = self.clone();
        s.remote = account.to_string();
        s.drive_mode = p.drive_mode;
        s.stream_folder = p.stream_folder;
        s.mirror_folder = p.mirror_folder;
        s
    }

    /// Make another account the one the settings screen shows and edits.
    pub fn select_account(&mut self, account: &str) {
        if account == self.remote {
            return;
        }
        let cur = self.current_prefs();
        self.account_prefs.insert(self.remote.clone(), cur);
        let p = self.prefs_for(account);
        self.remote = account.to_string();
        self.drive_mode = p.drive_mode;
        self.stream_folder = p.stream_folder;
        self.mirror_folder = p.mirror_folder;
    }

    /// Where mirrored shared drives go: a "Shared drives (<account>)" folder next to My Drive's.
    pub fn shared_drives_root(&self, account: &str) -> PathBuf {
        let mirror = PathBuf::from(&self.prefs_for(account).mirror_folder);
        let parent = mirror.parent().map(|p| p.to_path_buf()).unwrap_or_else(home);
        parent.join(format!("Shared drives ({})", self.account_label(account)))
    }

    pub fn shared_drive_folder(&self, account: &str, name: &str) -> PathBuf {
        let safe: String = name.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
        self.shared_drives_root(account).join(safe)
    }

    /// Display name of an account: its e-mail when known.
    pub fn account_label(&self, account: &str) -> String {
        self.emails.get(account).cloned().unwrap_or_else(|| account.to_string())
    }

    pub fn save(&self) {
        let mut me = self.clone();
        me.account_prefs.insert(me.remote.clone(), me.current_prefs());
        let dir = config_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(s) = serde_json::to_string_pretty(&me) {
            let tmp = dir.join("settings.json.tmp");
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(tmp, dir.join("settings.json"));
            }
        }
    }

    /// rclone --bwlimit "UP:DOWN" in KiB/s (original multiplies kB by 1024).
    pub fn bwlimit(&self) -> Option<String> {
        if !self.download_limit_on && !self.upload_limit_on {
            return None;
        }
        let up = if self.upload_limit_on { format!("{}k", self.upload_kbps.clamp(RATE_MIN, RATE_MAX)) } else { "off".into() };
        let down = if self.download_limit_on { format!("{}k", self.download_kbps.clamp(RATE_MIN, RATE_MAX)) } else { "off".into() };
        Some(format!("{up}:{down}"))
    }

    /// Streaming folder. The original uses <home>/Library/CloudStorage/GoogleDrive-<account>
    /// (string at 0x1040700fe, built in function 0x1000518e0); on Linux: <home>/GoogleDrive-<account>.
    pub fn stream_dir(&self, account: &str) -> String {
        let old_default = home().join("GoogleDrive").to_string_lossy().to_string();
        if self.stream_folder.is_empty() || self.stream_folder == old_default {
            home().join(format!("GoogleDrive-{account}")).to_string_lossy().into()
        } else {
            self.stream_folder.clone()
        }
    }

    /// The original refuses a synced folder inside (or holding) the Google Drive folder:
    /// "The folder cannot be within {MOUNT_POINT} which is being used by {EMAIL}".
    pub fn overlap_with_drive_folder(&self, path: &str, account: &str) -> Option<String> {
        let drive = match self.drive_mode {
            DriveMode::Stream => self.stream_dir(account),
            DriveMode::Mirror => self.mirror_folder.clone(),
        };
        if same_or_inside(path, &drive) {
            Some(drive)
        } else {
            None
        }
    }

    pub fn drive_folder(&self) -> String {
        match self.drive_mode {
            DriveMode::Stream => self.stream_dir(&self.remote),
            DriveMode::Mirror => self.mirror_folder.clone(),
        }
    }
}

pub fn set_autostart(on: bool) {
    let dir = dirs::config_dir().unwrap_or_else(|| home().join(".config")).join("autostart");
    let file = dir.join("drive-linux.desktop");
    if on {
        let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| "drive-linux".into());
        let _ = std::fs::create_dir_all(&dir);
        let icon = icon_path().to_string_lossy().to_string();
        let body = format!("[Desktop Entry]\nType=Application\nName=Drive For Linux\nIcon={icon}\nExec=\"{exe}\" --background\nX-GNOME-Autostart-enabled=true\n");
        let _ = std::fs::write(file, body);
    } else {
        let _ = std::fs::remove_file(file);
    }
}

/// Where the logo is written for the menu entries. They name it by its full path: by name
/// ("drivelinux") desktops can keep serving an old lookup from their icon cache, and an icon theme
/// with a generic "drive" icon used to win over the hyphenated "drive-linux".
pub fn icon_path() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| home().join(".local/share")).join("icons/hicolor/scalable/apps/drivelinux.svg")
}

/// The applications-menu entry and its icon, so the app can be started from the menu and its
/// window shows the logo in the taskbar (the window's class is "drive-linux", see
/// app::set_window_class). Rewritten only when it changed.
pub fn install_launcher() {
    let data = dirs::data_dir().unwrap_or_else(|| home().join(".local/share"));
    let icon = icon_path().to_string_lossy().to_string();
    let exe = std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| "drive-linux".into());
    let desktop = format!(
        "[Desktop Entry]\nType=Application\nName=Drive For Linux\nGenericName=Cloud file sync\nGenericName[vi]=Đồng bộ tệp trên đám mây\nComment=Sync your Google Drive files on Linux\nComment[vi]=Đồng bộ tệp Google Drive trên Linux\nIcon={icon}\nExec=\"{exe}\"\nTerminal=false\nCategories=Network;FileTransfer;\nStartupWMClass=drive-linux\nKeywords=drive;cloud;sync;backup;\n"
    );
    let write = |path: std::path::PathBuf, bytes: &[u8]| {
        if std::fs::read(&path).map(|b| b == bytes).unwrap_or(false) {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    };
    write(icon_path(), crate::theme::LOGO_SVG);
    write(data.join("applications/drive-linux.desktop"), desktop.as_bytes());
    // Google Docs/Sheets/Slides link files in the mirrored folder open in the browser.
    let opener = format!(
        "[Desktop Entry]\nType=Application\nName=Google Docs (Drive For Linux)\nName[vi]=Google Tài liệu (Drive For Linux)\nIcon={icon}\nExec=\"{exe}\" --open-doc %f\nTerminal=false\nNoDisplay=true\nMimeType={}\n",
        crate::gdocs::mime_types()
    );
    let mime = data.join("mime/packages/drive-linux.xml");
    let new_mime = !std::fs::read(&mime).map(|b| b == crate::gdocs::MIME_XML.as_bytes()).unwrap_or(false);
    write(mime, crate::gdocs::MIME_XML.as_bytes());
    let opener_path = data.join("applications/drive-linux-open-doc.desktop");
    let new_opener = !std::fs::read(&opener_path).map(|b| b == opener.as_bytes()).unwrap_or(false);
    write(opener_path, opener.as_bytes());
    if !(new_mime || new_opener) {
        return;
    }
    // The desktop's databases can take a while to rebuild (or wait on the session); never hold
    // the window up for them.
    let mime_dir = data.join("mime");
    let apps_dir = data.join("applications");
    std::thread::spawn(move || {
        let quiet = |cmd: &mut std::process::Command| {
            let _ = cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        };
        if new_mime {
            quiet(std::process::Command::new("update-mime-database").arg(&mime_dir));
        }
        quiet(std::process::Command::new("update-desktop-database").arg(&apps_dir));
        for t in crate::gdocs::mime_types().split(';').filter(|t| !t.is_empty()) {
            quiet(std::process::Command::new("xdg-mime").args(["default", "drive-linux-open-doc.desktop", t]));
        }
    });
}

pub fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string()).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "linux".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("drive-linux-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn an_old_or_empty_settings_file_gets_the_original_defaults() {
        let st: Settings = serde_json::from_str("{}").unwrap();
        assert!(st.autostart, "gui_autostart_option = true");
        assert!(!st.upload_raw, "photos_upload_raw_files has no default");
        assert!(st.close_keeps_running && st.file_manager_menu && st.prompt_devices);
        assert!(!st.verbose_logging && !st.paused);
        assert_eq!(st.language, "en");
        // a partial file keeps what it has and fills in the rest
        let st: Settings = serde_json::from_str(r#"{"remote":"g","drive_mode":"Mirror","mirror_folder":"/d"}"#).unwrap();
        assert_eq!((st.remote.as_str(), st.drive_mode, st.mirror_folder.as_str()), ("g", DriveMode::Mirror, "/d"));
        assert!(st.autostart);
    }

    #[test]
    fn each_account_keeps_its_own_drive_folder() {
        let mut st = Settings::default();
        st.remote = "a".into();
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = "/data/a".into();
        st.select_account("b");
        assert_eq!(st.remote, "b");
        assert_eq!(st.drive_mode, DriveMode::Stream, "a new account starts streaming");
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = "/data/b".into();
        st.select_account("a");
        assert_eq!((st.drive_mode, st.mirror_folder.as_str()), (DriveMode::Mirror, "/data/a"));
        assert_eq!(st.for_account("b").mirror_folder, "/data/b");
        assert_eq!(st.for_account("b").remote, "b");
    }

    #[test]
    fn synced_folders_may_not_overlap_the_drive_folder() {
        let mut st = Settings::default();
        st.remote = "a".into();
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = "/data/drive".into();
        assert!(st.overlap_with_drive_folder("/data/drive/photos", "a").is_some());
        assert!(st.overlap_with_drive_folder("/data", "a").is_some(), "holding the Drive folder");
        assert!(st.overlap_with_drive_folder("/data/drive", "a").is_some());
        assert!(st.overlap_with_drive_folder("/data/drive2", "a").is_none(), "a name that only starts the same");
        assert!(st.overlap_with_drive_folder("/home/u/Pictures", "a").is_none());
    }

    #[test]
    fn streaming_folder_must_be_empty_apart_from_hidden_files() {
        let d = tmp("mount");
        assert_eq!(check_mount_dir(&d.join("not-yet").to_string_lossy()), MountCheck::Ok);
        assert_eq!(check_mount_dir(&d.to_string_lossy()), MountCheck::Ok);
        std::fs::write(d.join(".hidden"), b"").unwrap();
        assert_eq!(check_mount_dir(&d.to_string_lossy()), MountCheck::Ok);
        std::fs::write(d.join("file.txt"), b"").unwrap();
        assert_eq!(check_mount_dir(&d.to_string_lossy()), MountCheck::NotEmpty);
        assert_eq!(check_mount_dir(&d.join("file.txt").to_string_lossy()), MountCheck::NotAFolder);
        assert_eq!(mount_check_message(&MountCheck::NotEmpty), "The selected folder must be empty.");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn photo_backup_takes_raw_files_only_when_asked() {
        let without = media_include(false);
        let with = media_include(true);
        assert!(without.starts_with("*.{") && without.ends_with('}'));
        assert!(without.contains("jpg") && without.contains("mp4") && !without.contains("cr2"));
        assert!(with.contains("cr2") && with.contains("dng"));
        assert_eq!(ignore_args().len(), IGNORE_NAMES.len() * 4);
    }

    #[test]
    fn device_ignore_list_and_labels() {
        let mut st = Settings::default();
        st.ignored_media.push(IgnoredMedia { media_id: "uuid:1234".into(), name: "USB".into(), last_mount_point: "/media/u/USB".into() });
        assert!(st.is_ignored("uuid:1234", "/somewhere/else"), "found by ID wherever it is mounted");
        assert!(!st.is_ignored("uuid:9999", "/media/u/USB"));
        st.emails.insert("g".into(), "person@example.com".into());
        assert_eq!(st.account_label("g"), "person@example.com");
        assert_eq!(st.account_label("other"), "other");
        assert!(st.shared_drive_folder("g", "Team/A").ends_with("Team_A"), "a slash can't be part of a folder name");
    }
}
