//! Google Drive entries in the file manager's right-click menu, like the original's Finder menu
//! ("Copy link to clipboard", "Share with Google Drive", "Manage versions with Google Drive",
//! "Open with Google Drive", "Available offline", "Online only" — original string table).
//! Dolphin (KDE) gets a service menu, GNOME Files gets scripts. Each entry runs this app with
//! `--action <name> <path>`; a running copy receives it through its single-instance socket.
use crate::settings::*;
use std::path::{Path, PathBuf};

/// (action id, English label, Vietnamese label)
pub const ACTIONS: &[(&str, &str, &str)] = &[
    ("copy_link", "Copy link to clipboard", "Sao chép đường liên kết"),
    ("share", "Share with Google Drive", "Chia sẻ bằng Google Drive"),
    ("versions", "Manage versions with Google Drive", "Quản lý phiên bản bằng Google Drive"),
    ("open_web", "Open with Google Drive", "Mở bằng Google Drive"),
    ("offline", "Available offline", "Có thể dùng khi không có mạng"),
    ("online_only", "Online only", "Chỉ trực tuyến"),
];

fn exe() -> String {
    std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| "drive-linux".into())
}

fn data_home() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn dolphin_file() -> PathBuf {
    data_home().join("kio").join("servicemenus").join("drive-linux.desktop")
}

fn nautilus_dir() -> PathBuf {
    data_home().join("nautilus").join("scripts").join("Google Drive")
}

/// Writes (or removes) the menu entries.
/// True when the Dolphin sync-status plugin was newly put in place.
pub fn install(on: bool, vietnamese: bool) -> bool {
    // Sync status badges on files go with the menu (same setting).
    let new_plugin = crate::badges::install(on);
    let exe = exe();
    let dolphin = dolphin_file();
    let nautilus = nautilus_dir();
    if !on {
        let _ = std::fs::remove_file(&dolphin);
        if let Ok(rd) = std::fs::read_dir(&nautilus) {
            for e in rd.flatten() {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let _ = std::fs::remove_dir(&nautilus);
        return false;
    }
    let label = |en: &str, vi: &str| if vietnamese { vi.to_string() } else { en.to_string() };
    // Dolphin service menu: a "Google Drive" submenu.
    let mut desk = String::from("[Desktop Entry]\nType=Service\nMimeType=all/all;\nX-KDE-ServiceTypes=KonqPopupMenu/Plugin\nX-KDE-Submenu=Google Drive\nIcon=folder-cloud\n");
    desk.push_str(&format!("Actions={};\n\n", ACTIONS.iter().map(|a| a.0).collect::<Vec<_>>().join(";")));
    for (id, en, vi) in ACTIONS {
        desk.push_str(&format!("[Desktop Action {id}]\nName={}\nExec=\"{exe}\" --action {id} %f\n\n", label(en, vi)));
    }
    if let Some(dir) = dolphin.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&dolphin, desk).is_ok() {
        set_exec(&dolphin);
    }
    // GNOME Files scripts: one per entry, under a "Google Drive" folder.
    let _ = std::fs::create_dir_all(&nautilus);
    for (id, en, vi) in ACTIONS {
        let file = nautilus.join(label(en, vi));
        let script = format!("#!/bin/sh\nprintf '%s\\n' \"$NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\" | while IFS= read -r f; do [ -n \"$f\" ] && \"{exe}\" --action {id} \"$f\"; done\n");
        if std::fs::write(&file, script).is_ok() {
            set_exec(&file);
        }
    }
    new_plugin
}

fn set_exec(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
}

/// Where a local path lives in Drive.
pub enum Located {
    /// account, rclone path inside My Drive
    MyDrive(String, String),
    /// account, path below the streaming folder root ("Shared drives/…")
    Streamed(String, String),
    NotInDrive,
}

pub fn locate(st: &Settings, accounts: &[String], path: &str) -> Located {
    let p = Path::new(path);
    for acc in accounts {
        let s = st.for_account(acc);
        match s.drive_mode {
            DriveMode::Mirror => {
                if let Ok(rel) = p.strip_prefix(&s.mirror_folder) {
                    return Located::MyDrive(acc.clone(), rel.to_string_lossy().to_string());
                }
            }
            DriveMode::Stream => {
                let root = PathBuf::from(s.stream_dir(acc));
                if let Ok(rel) = p.strip_prefix(root.join("My Drive")) {
                    return Located::MyDrive(acc.clone(), rel.to_string_lossy().to_string());
                }
                if let Ok(rel) = p.strip_prefix(&root) {
                    return Located::Streamed(acc.clone(), rel.to_string_lossy().to_string());
                }
            }
        }
        // Folders from this computer are backed up under "Linux computers/<host>/<folder>".
        for f in &st.folders {
            if let Ok(rel) = p.strip_prefix(&f.path) {
                let base = Path::new(&f.path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let mut r = format!("{BACKUP_ROOT}/{}/{}", hostname(), base);
                if !rel.as_os_str().is_empty() {
                    r.push('/');
                    r.push_str(&rel.to_string_lossy());
                }
                let acc2 = if accounts.contains(&f.account) { f.account.clone() } else { acc.clone() };
                return Located::MyDrive(acc2, r);
            }
        }
    }
    Located::NotInDrive
}

/// Web addresses for an item ID.
pub fn web_url(action: &str, id: &str, is_dir: bool) -> String {
    match (action, is_dir) {
        (_, true) => format!("https://drive.google.com/drive/folders/{id}"),
        ("share", false) => format!("https://drive.google.com/file/d/{id}/view?usp=sharing"),
        _ => format!("https://drive.google.com/file/d/{id}/view"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locates_mirror_and_backup_paths() {
        let mut st = Settings::default();
        st.remote = "gdrive".into();
        st.drive_mode = DriveMode::Mirror;
        st.mirror_folder = "/data/drive".into();
        st.folders.push(LocalFolder { path: "/home/u/Photos".into(), to_drive: true, to_photos: false, account: "gdrive".into(), device: false, media_id: String::new() });
        let acc = vec!["gdrive".to_string()];
        match locate(&st, &acc, "/data/drive/a/b.txt") {
            Located::MyDrive(a, r) => assert_eq!((a.as_str(), r.as_str()), ("gdrive", "a/b.txt")),
            _ => panic!(),
        }
        match locate(&st, &acc, "/home/u/Photos/x.jpg") {
            Located::MyDrive(_, r) => assert!(r.starts_with("Linux computers/") && r.ends_with("/Photos/x.jpg")),
            _ => panic!(),
        }
        assert!(matches!(locate(&st, &acc, "/tmp/other"), Located::NotInDrive));
        assert_eq!(web_url("share", "ID", false), "https://drive.google.com/file/d/ID/view?usp=sharing");
    }

    #[test]
    fn locates_streamed_paths_and_builds_web_addresses() {
        let mut st = Settings::default();
        st.remote = "g".into();
        st.drive_mode = DriveMode::Stream;
        let root = st.stream_dir("g");
        let acc = vec!["g".to_string()];
        match locate(&st, &acc, &format!("{root}/My Drive/x/y.pdf")) {
            Located::MyDrive(a, r) => assert_eq!((a.as_str(), r.as_str()), ("g", "x/y.pdf")),
            _ => panic!("My Drive inside the streaming folder"),
        }
        match locate(&st, &acc, &format!("{root}/Shared drives/Team/z.doc")) {
            Located::Streamed(_, r) => assert_eq!(r, "Shared drives/Team/z.doc"),
            _ => panic!("shared drives inside the streaming folder"),
        }
        assert!(matches!(locate(&st, &[], &format!("{root}/My Drive/a")), Located::NotInDrive), "no account, nothing to locate");
        assert_eq!(web_url("open_web", "ID", true), "https://drive.google.com/drive/folders/ID");
        assert_eq!(web_url("versions", "ID", false), "https://drive.google.com/file/d/ID/view");
        assert_eq!(ACTIONS.len(), 6);
    }
}
