//! Two-way "Mirror files" sync. rclone 1.60's bisync stops on deletions and on empty folders, so
//! the app compares both sides with the list it saved after the previous run and decides per file:
//! new here → copy there, deleted here (was in the saved list) → delete there (Drive keeps it in
//! the trash for 30 days), changed on one side → copy that side's version. Changed on both sides
//! (or different files under the same name on the first run) is a conflict: like the original,
//! the Drive version is put in place and the computer's version is never overwritten — it is moved
//! to a "Lost and Found" folder first (MoveNewFileIntoPlace 0x100bfc6a0 refuses to overwrite
//! "Conflicting file exists at …"; DeconflictCopy 0x100671870 copies the item to lost and found).
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub size: i64,
    pub mtime: i64, // unix seconds
}

pub type Listing = BTreeMap<String, Entry>;

#[derive(Default, Serialize, Deserialize)]
pub struct State {
    pub remote: String,
    pub folder: String,
    pub files: Listing,
    /// When each file was last synced (unix seconds), like the original's
    /// last_item_event_time(item_id, event_id, timestamp) table (RecordItemEvent, 0x100849940).
    #[serde(default)]
    pub synced_at: BTreeMap<String, i64>,
    /// Link files written for Google Docs/Sheets/Slides (see gdocs.rs), so a deleted document's
    /// link file can be removed.
    #[serde(default)]
    pub links: BTreeSet<String>,
}

/// Carries the last-synced times forward: files moved by this run get `now`, files still present
/// keep their time, files gone are dropped (the original deletes an item's rows when it goes).
pub fn next_synced_at(prior: &BTreeMap<String, i64>, plan: &Plan, next: &Listing, now: i64) -> BTreeMap<String, i64> {
    let moved: BTreeSet<&String> = plan.upload.iter().chain(plan.download.iter()).collect();
    next.keys().filter_map(|k| if moved.contains(k) { Some((k.clone(), now)) } else { prior.get(k).map(|t| (k.clone(), *t)) }).collect()
}

#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub upload: Vec<String>,
    pub download: Vec<String>,
    pub delete_remote: Vec<String>,
    pub delete_local: Vec<String>,
    /// Both sides changed: keep the computer's copy in Lost and Found, then download Drive's.
    pub conflicts: Vec<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.upload.is_empty() && self.download.is_empty() && self.delete_remote.is_empty() && self.delete_local.is_empty() && self.conflicts.is_empty()
    }
}

/// Same file when the sizes match and the times differ by at most a second
/// (Drive keeps milliseconds, local disks keep nanoseconds).
pub fn same(a: &Entry, b: &Entry) -> bool {
    a.size == b.size && (a.mtime - b.mtime).abs() <= 1
}

pub fn plan(local: &Listing, remote: &Listing, prior: &Listing) -> Plan {
    let mut p = Plan::default();
    let names: BTreeSet<&String> = local.keys().chain(remote.keys()).collect();
    for name in names {
        let (l, r, s) = (local.get(name), remote.get(name), prior.get(name));
        match (l, r) {
            (Some(l), None) => match s {
                // removed on Drive but edited here since: keep the edit (upload it again)
                Some(s) if !same(l, s) => p.upload.push(name.clone()),
                Some(_) => p.delete_local.push(name.clone()), // removed on Drive
                None => p.upload.push(name.clone()),          // new on this computer
            },
            (None, Some(r)) => match s {
                // removed here but edited on Drive since: keep the edit (download it again)
                Some(s) if !same(r, s) => p.download.push(name.clone()),
                Some(_) => p.delete_remote.push(name.clone()), // removed on this computer
                None => p.download.push(name.clone()),         // new on Drive
            },
            (Some(l), Some(r)) => {
                if same(l, r) {
                    continue;
                }
                let l_changed = s.map(|s| !same(l, s)).unwrap_or(true);
                let r_changed = s.map(|s| !same(r, s)).unwrap_or(true);
                if l_changed && !r_changed {
                    p.upload.push(name.clone());
                } else if r_changed && !l_changed {
                    p.download.push(name.clone());
                } else {
                    p.conflicts.push(name.clone());
                    p.download.push(name.clone());
                }
            }
            (None, None) => {}
        }
    }
    p
}

/// The list both sides agree on once the plan has been carried out.
pub fn next_state(local: &Listing, remote: &Listing, plan: &Plan) -> Listing {
    let mut out = Listing::new();
    let deleted: BTreeSet<&String> = plan.delete_local.iter().chain(plan.delete_remote.iter()).collect();
    let downloaded: BTreeSet<&String> = plan.download.iter().collect();
    for (k, v) in local {
        if !deleted.contains(k) && !downloaded.contains(k) {
            out.insert(k.clone(), *v);
        }
    }
    for k in &plan.download {
        if let Some(v) = remote.get(k) {
            out.insert(k.clone(), *v);
        }
    }
    out
}

/// Parses rclone's RFC 3339 times ("2026-11-09T09:38:58.944Z", "...+07:00") to unix seconds.
pub fn parse_time(t: &str) -> Option<i64> {
    let b = t.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |a: usize, z: usize| t.get(a..z)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    // days from civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut secs = days * 86400 + h * 3600 + mi * 60 + s;
    let rest = &t[19..];
    let rest = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
    if let Some(sign) = rest.chars().next() {
        if sign == '+' || sign == '-' {
            let digits: String = rest[1..].chars().filter(|c| c.is_ascii_digit()).collect();
            if digits.len() >= 4 {
                let oh: i64 = digits[0..2].parse().ok()?;
                let om: i64 = digits[2..4].parse().ok()?;
                let off = oh * 3600 + om * 60;
                secs -= if sign == '+' { off } else { -off };
            }
        }
    }
    Some(secs)
}

/// Reads `rclone lsjson -R --files-only` output.
pub fn parse_listing(json: &[u8]) -> Result<Listing, String> {
    let v: serde_json::Value = serde_json::from_slice(json).map_err(|e| e.to_string())?;
    let mut out = Listing::new();
    for item in v.as_array().ok_or("not a list")? {
        if item["IsDir"].as_bool().unwrap_or(false) {
            continue;
        }
        let size = item["Size"].as_i64().unwrap_or(-1);
        if size < 0 {
            continue; // Google Docs/Sheets/Slides have no file to copy
        }
        let path = item["Path"].as_str().unwrap_or("").to_string();
        let mtime = item["ModTime"].as_str().and_then(parse_time).unwrap_or(0);
        if !path.is_empty() {
            out.insert(path, Entry { size, mtime });
        }
    }
    Ok(out)
}

/// Reads a listing saved by rclone's old two-way sync ("path2" = this computer's side), so the
/// first run after the switch still knows which files were deleted locally.
pub fn parse_bisync_listing(text: &str) -> Listing {
    let mut out = Listing::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let Some(q) = line.find('"') else { continue };
        let name = line[q + 1..].trim_end().trim_end_matches('"').to_string();
        let fields: Vec<&str> = line[..q].split_whitespace().collect();
        // "-", size, hash, id, time
        if fields.len() < 5 {
            continue;
        }
        let size = fields[1].parse().unwrap_or(-1);
        let mtime = parse_time(fields[4]).unwrap_or(0);
        out.insert(name, Entry { size, mtime });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(items: &[(&str, i64, i64)]) -> Listing {
        items.iter().map(|(n, s, t)| (n.to_string(), Entry { size: *s, mtime: *t })).collect()
    }

    #[test]
    fn deleting_the_only_local_file_deletes_it_on_drive() {
        let prior = l(&[("app-windows.jpg", 175714, 100)]);
        let p = plan(&Listing::new(), &prior.clone(), &prior);
        assert_eq!(p.delete_remote, vec!["app-windows.jpg".to_string()]);
        assert!(p.upload.is_empty() && p.download.is_empty() && p.delete_local.is_empty());
        assert!(next_state(&Listing::new(), &prior, &p).is_empty());
    }

    #[test]
    fn first_run_is_a_union_and_conflicts_keep_both() {
        let p = plan(&l(&[("a", 1, 10), ("c", 1, 90)]), &l(&[("b", 2, 10), ("c", 2, 60)]), &Listing::new());
        assert_eq!(p.upload, vec!["a".to_string()]);
        assert_eq!(p.download, vec!["b".to_string(), "c".to_string()]);
        // even though this computer's "c" is newer, it is not uploaded over Drive's: it goes to Lost and Found
        assert_eq!(p.conflicts, vec!["c".to_string()]);
        assert!(p.delete_local.is_empty() && p.delete_remote.is_empty());
    }

    #[test]
    fn remote_delete_and_one_sided_edits() {
        let prior = l(&[("gone", 1, 1), ("edit_here", 1, 1), ("edit_there", 1, 1)]);
        let local = l(&[("gone", 1, 1), ("edit_here", 5, 9), ("edit_there", 1, 1)]);
        let remote = l(&[("edit_here", 1, 1), ("edit_there", 7, 9)]);
        let p = plan(&local, &remote, &prior);
        assert_eq!(p.delete_local, vec!["gone".to_string()]);
        assert_eq!(p.upload, vec!["edit_here".to_string()]);
        assert_eq!(p.download, vec!["edit_there".to_string()]);
    }

    #[test]
    fn old_listing_is_understood() {
        let t = "# bisync listing v1\n-   175714 md5:03c1784739ae85a0b547f58e512bd0ce - 2026-11-09T09:38:58.944000000+0000 \"app-windows.jpg\"\n";
        let l = parse_bisync_listing(t);
        assert_eq!(l.get("app-windows.jpg").map(|e| e.size), Some(175714));
        assert_eq!(l["app-windows.jpg"].mtime, parse_time("2026-11-09T09:38:58Z").unwrap());
    }

    #[test]
    fn synced_times_follow_the_original_rules() {
        let prior: BTreeMap<String, i64> = [("keep".to_string(), 5), ("gone".to_string(), 6)].into_iter().collect();
        let plan = Plan { upload: vec!["new".into()], ..Default::default() };
        let next = l(&[("keep", 1, 1), ("new", 1, 1)]);
        let out = next_synced_at(&prior, &plan, &next, 100);
        assert_eq!(out.get("keep"), Some(&5));
        assert_eq!(out.get("new"), Some(&100));
        assert!(!out.contains_key("gone"));
    }

    #[test]
    fn an_edit_wins_over_a_delete_on_the_other_side() {
        let prior = l(&[("a.txt", 5, 100), ("b.txt", 5, 100)]);
        // a.txt: deleted here, edited on Drive → comes back; b.txt: deleted on Drive, edited here → goes back up
        let local = l(&[("b.txt", 9, 200)]);
        let remote = l(&[("a.txt", 7, 300)]);
        let p = plan(&local, &remote, &prior);
        assert_eq!(p.download, vec!["a.txt".to_string()]);
        assert_eq!(p.upload, vec!["b.txt".to_string()]);
        assert!(p.delete_local.is_empty() && p.delete_remote.is_empty());
        let next = next_state(&local, &remote, &p);
        assert_eq!(next.len(), 2);
    }

    #[test]
    fn changes_on_both_sides_and_quiet_runs() {
        let prior = l(&[("a.txt", 5, 100)]);
        let p = plan(&l(&[("a.txt", 6, 200)]), &l(&[("a.txt", 7, 300)]), &prior);
        assert_eq!(p.conflicts, vec!["a.txt".to_string()], "this computer's copy goes to Lost and Found");
        assert_eq!(p.download, vec!["a.txt".to_string()], "Drive's copy is put in place");
        // nothing changed anywhere: nothing to do, and a 1 s clock difference is not a change
        let same_both = l(&[("a.txt", 5, 100)]);
        assert!(plan(&same_both, &l(&[("a.txt", 5, 101)]), &prior).is_empty());
        // gone on both sides: nothing to do
        assert!(plan(&Listing::new(), &Listing::new(), &prior).is_empty());
    }

    #[test]
    fn listing_skips_folders_and_google_docs() {
        let json = br#"[{"Path":"d","IsDir":true,"Size":-1},{"Path":"d/a.txt","IsDir":false,"Size":3,"ModTime":"2026-11-09T09:38:58.944Z"},{"Path":"Doc","IsDir":false,"Size":-1}]"#;
        let got = parse_listing(json).unwrap();
        assert_eq!(got.keys().collect::<Vec<_>>(), vec!["d/a.txt"]);
        assert!(parse_listing(b"{}").is_err(), "a broken listing is an error, never 'everything was deleted'");
    }

    #[test]
    fn times_parse_with_offsets() {
        let a = parse_time("2026-11-09T09:38:58.944Z").unwrap();
        let b = parse_time("2026-11-09T16:38:58.1234+07:00").unwrap();
        assert_eq!(a, b);
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), Some(0));
    }
}
