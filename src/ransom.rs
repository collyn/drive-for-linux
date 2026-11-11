//! Ransomware protection: pause syncing for an account when many synced files are rewritten or
//! removed on this computer at once, so the damage never reaches Drive.
//!
//! The original gets the alert from Google's servers (SyncingStatus flag, checked by
//! CheckForRansomwareAlert in apps/drive/fs/ui/client.cc, function 0x1002e0ea0). What it does with
//! it is local and is followed here:
//! - the alert is kept in a "ransomware-alert" sentinel file holding the account and the alert
//!   time, so syncing stays paused after a restart (has_ransomware_alert / ransomware_alert_timestamp_ms);
//! - a "ransomware-cleared" or "ransomware-false" sentinel written when the person dealt with an
//!   earlier alert takes precedence when it is no older than one hour (0xe10 = 3600 s) before the
//!   new alert's time ("ExistingRansomwareClearTakesPrecedence"); an older one is removed;
//! - signing out handles the alert (HandleRansomwareAlertOnSignOut).
//!
//! The servers' detection isn't reachable through rclone or the public Drive API, so this app
//! decides from the mirror plan itself (thresholds are estimates).
use crate::mirror::{Listing, Plan};
use crate::settings::config_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Fewest synced files rewritten or removed in one pass that count as an attack (estimated).
pub const MIN_FILES: usize = 50;
/// ...and they must be at least this share of the folder's synced files (estimated) ...
pub const MIN_PERCENT: usize = 10;
/// ...unless there are at least this many (estimated).
pub const ALWAYS_FILES: usize = 500;
/// A clear made no earlier than this before an alert wins over it (function 0x1002e0ea0, 0xe10).
pub const CLEAR_PRECEDENCE_MS: i64 = 3_600_000;

/// One mirrored folder whose changes were held back.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeldFolder {
    /// rclone location on Drive ("gdrive:" or a shared drive).
    pub spec: String,
    pub folder: String,
    /// Synced files rewritten on this computer.
    pub changed: Vec<String>,
    /// Files new on this computer (ransom notes, encrypted copies...).
    pub added: Vec<String>,
    /// Synced files removed from this computer.
    pub deleted: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Alert {
    pub account: String,
    /// Unix milliseconds, like ransomware_alert_timestamp_ms.
    pub at_ms: i64,
    pub folders: Vec<HeldFolder>,
    /// Local date and time of the alert, as shown in "paused on {0} at {1}".
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub time: String,
}

impl Alert {
    pub fn file_count(&self) -> usize {
        self.folders.iter().map(|f| f.changed.len() + f.deleted.len()).sum()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Clear {
    pub account: String,
    pub at_ms: i64,
}

fn safe(acc: &str) -> String {
    acc.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

pub fn alert_path(acc: &str) -> PathBuf {
    config_dir().join(format!("ransomware-alert-{}.json", safe(acc)))
}

/// "ransomware-false" when the person said they recognise the activity, else "ransomware-cleared".
pub fn clear_path(acc: &str, false_positive: bool) -> PathBuf {
    let kind = if false_positive { "false" } else { "cleared" };
    config_dir().join(format!("ransomware-{kind}-{}.json", safe(acc)))
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Local date and time of a moment, for "paused on {0} at {1}" ("10 Nov 2026", "14:05").
pub fn local_date_time(ms: i64) -> (String, String) {
    let out = std::process::Command::new("date").arg("-d").arg(format!("@{}", ms / 1000)).arg("+%-d %b %Y|%H:%M").env("LC_TIME", "C").output();
    let text = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    match text.split_once('|') {
        Some((d, t)) => (d.to_string(), t.to_string()),
        None => (String::new(), String::new()),
    }
}

pub fn load_alert(acc: &str) -> Option<Alert> {
    std::fs::read_to_string(alert_path(acc)).ok().and_then(|t| serde_json::from_str(&t).ok())
}

fn write_atomic(path: &PathBuf, text: String) -> bool {
    let _ = std::fs::create_dir_all(config_dir());
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).is_ok() && std::fs::rename(tmp, path).is_ok()
}

pub fn save_alert(a: &Alert) -> bool {
    serde_json::to_string(a).map(|t| write_atomic(&alert_path(&a.account), t)).unwrap_or(false)
}

/// Ends the alert: the alert file goes and a clear sentinel records when (and how) it was dealt with.
pub fn clear_alert(acc: &str, false_positive: bool) {
    let _ = std::fs::remove_file(alert_path(acc));
    let c = Clear { account: acc.to_string(), at_ms: now_ms() };
    if let Ok(t) = serde_json::to_string(&c) {
        let _ = write_atomic(&clear_path(acc, false_positive), t);
    }
}

/// True when an earlier clear still covers an alert raised at `alert_ms`; a stale clear is removed.
pub fn clear_takes_precedence(acc: &str, alert_ms: i64) -> bool {
    for fp in [false, true] {
        let p = clear_path(acc, fp);
        let Some(c) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Clear>(&t).ok()) else { continue };
        if c.account == acc && covers(c.at_ms, alert_ms) {
            return true;
        }
        let _ = std::fs::remove_file(&p);
    }
    false
}

pub fn covers(clear_ms: i64, alert_ms: i64) -> bool {
    clear_ms >= alert_ms - CLEAR_PRECEDENCE_MS
}

/// The files a mirror pass would push to Drive that look like an attack, or None.
pub fn suspicious(plan: &Plan, prior: &Listing, spec: &str, folder: &str) -> Option<HeldFolder> {
    let changed: Vec<String> = plan.upload.iter().chain(plan.conflicts.iter()).filter(|n| prior.contains_key(*n)).cloned().collect();
    let added: Vec<String> = plan.upload.iter().filter(|n| !prior.contains_key(*n)).cloned().collect();
    let deleted = plan.delete_remote.clone();
    let hit = changed.len() + deleted.len();
    let share_ok = hit * 100 >= prior.len() * MIN_PERCENT;
    if hit >= ALWAYS_FILES || (hit >= MIN_FILES && share_ok) {
        let mut changed = changed;
        changed.sort();
        changed.dedup();
        Some(HeldFolder { spec: spec.into(), folder: folder.into(), changed, added, deleted })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mirror::Entry;

    fn listing(n: usize) -> Listing {
        (0..n).map(|i| (format!("f{i}.doc"), Entry { size: 10, mtime: 1 })).collect()
    }

    #[test]
    fn mass_rewrite_is_held_back() {
        let prior = listing(200);
        let mut plan = Plan::default();
        for i in 0..40 {
            plan.upload.push(format!("f{i}.doc"));
        }
        for i in 40..60 {
            plan.delete_remote.push(format!("f{i}.doc"));
        }
        plan.upload.push("README_DECRYPT.txt".into());
        let h = suspicious(&plan, &prior, "gdrive:", "/x").expect("60 of 200 synced files touched");
        assert_eq!(h.changed.len(), 40);
        assert_eq!(h.deleted.len(), 20);
        assert_eq!(h.added, vec!["README_DECRYPT.txt".to_string()]);
    }

    #[test]
    fn ordinary_work_is_not() {
        let prior = listing(200);
        let mut plan = Plan::default();
        // many new files (a photo import) are fine
        for i in 0..1000 {
            plan.upload.push(format!("new{i}.jpg"));
        }
        for i in 0..20 {
            plan.upload.push(format!("f{i}.doc"));
        }
        assert!(suspicious(&plan, &prior, "gdrive:", "/x").is_none());
        // 60 edits in a folder of 10 000 synced files is under the share
        let big = listing(10_000);
        let mut p2 = Plan::default();
        for i in 0..60 {
            p2.upload.push(format!("f{i}.doc"));
        }
        assert!(suspicious(&p2, &big, "gdrive:", "/x").is_none());
        // ...but 500 always is
        for i in 60..500 {
            p2.upload.push(format!("f{i}.doc"));
        }
        assert!(suspicious(&p2, &big, "gdrive:", "/x").is_some());
    }

    #[test]
    fn clear_precedence_window() {
        let alert = 10_000_000;
        assert!(covers(alert, alert));
        assert!(covers(alert - 3_600_000, alert));
        assert!(!covers(alert - 3_600_001, alert));
    }

    #[test]
    fn conflicts_count_and_small_folders_never_trigger() {
        let prior = listing(100);
        let mut plan = Plan::default();
        for i in 0..30 {
            plan.conflicts.push(format!("f{i}.doc"));
            plan.download.push(format!("f{i}.doc"));
        }
        for i in 30..50 {
            plan.delete_remote.push(format!("f{i}.doc"));
        }
        let h = suspicious(&plan, &prior, "g:", "/x").expect("50 of 100 synced files");
        let alert = Alert { account: "g".into(), at_ms: 0, folders: vec![h], date: String::new(), time: String::new() };
        assert_eq!(alert.file_count(), 50);
        // a folder with only 40 synced files can never reach 50
        let small = listing(40);
        let mut all = Plan::default();
        all.delete_remote = small.keys().cloned().collect();
        assert!(suspicious(&all, &small, "g:", "/x").is_none());
        assert_eq!(super::safe("a/b c"), "a_b_c");
    }
}
