//! Logs and "Generate diagnostic info", following the original (apps/drive/fs/ui/diagnostic_util.cc,
//! function 0x10094ba20; feedback_recorder.cc, function 0x100512ce0):
//! - the report is built on one dedicated "diagnostic_info_generator_thread", one at a time;
//! - it is a zip named "Google Drive Diagnostics - %Y-%m-%dT%H.%M.%E2S%Ez.zip" (":" replaced by
//!   "."), first created in the temp folder; if the name is taken the time moves on 40 ms and it
//!   tries again, 16 attempts in all ("Can't create zip file in 16 attempts");
//! - it is then moved to the desktop (MaybeMoveDiagnosticFileToDesktop; copied and the temp file
//!   removed when a move isn't possible) and the full path is shown.
//!
//! Log files follow the flag defaults structured_log_max_file_size_kbytes = 5120 (0x103f52384) and
//! structured_log_max_file_count = 30 (0x103f523d0); the file is "structured_log.txt".
use crate::settings::config_dir;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const LOG_MAX_FILE_KB: u64 = 5120;
pub const LOG_MAX_FILES: usize = 30;
pub const ZIP_ATTEMPTS: u32 = 16;
pub const ZIP_RETRY_STEP_MS: i64 = 40;

/// "Enable verbose logging": rclone logs at debug level and every line is kept.
pub static VERBOSE: AtomicBool = AtomicBool::new(false);
static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn logs_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(|| PathBuf::from("/tmp")).join("drive-linux").join("Logs")
}

fn log_file(n: usize) -> PathBuf {
    if n == 0 {
        logs_dir().join("structured_log.txt")
    } else {
        logs_dir().join(format!("structured_log.{n}.txt"))
    }
}

/// rclone's verbosity flag for the current logging level.
pub fn rclone_verbosity() -> &'static str {
    if VERBOSE.load(Ordering::Relaxed) {
        "-vv"
    } else {
        "-v"
    }
}

/// Appends one line to the log, starting a new file past 5 MB and keeping the newest 30.
pub fn log_line(line: &str) {
    let _g = LOG_LOCK.lock();
    let _ = std::fs::create_dir_all(logs_dir());
    let cur = log_file(0);
    if std::fs::metadata(&cur).map(|m| m.len() > LOG_MAX_FILE_KB * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(log_file(LOG_MAX_FILES - 1));
        for n in (0..LOG_MAX_FILES - 1).rev() {
            let _ = std::fs::rename(log_file(n), log_file(n + 1));
        }
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&cur) {
        let _ = writeln!(f, "{}", line.trim_end());
    }
}

/// A line of the app's own (not rclone's) in the same JSON shape.
pub fn log_event(level: &str, msg: &str) {
    let t = std::process::Command::new("date").arg("--iso-8601=ns").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let v = serde_json::json!({ "time": t, "level": level, "msg": msg, "source": "drive-linux" });
    log_line(&v.to_string());
}

/// "Google Drive Diagnostics - 2026-11-10T21.30.05.12+07.00.zip" for a moment in unix ms.
pub fn zip_name(ms: i64) -> String {
    let out = std::process::Command::new("date").arg("-d").arg(format!("@{}.{:03}", ms.div_euclid(1000), ms.rem_euclid(1000))).arg("+%Y-%m-%dT%H:%M:%S.%2N%:z").env("LC_TIME", "C").output();
    let stamp = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    format!("Google Drive Diagnostics - {}.zip", stamp.replace(':', "."))
}

// ---------------------------------------------------------------- a small zip writer (stored)

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut c = !0u32;
    for b in data {
        c = table[((c ^ *b as u32) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

/// Writes `entries` (name, bytes) as an uncompressed zip.
pub fn write_zip(out: &mut impl Write, entries: &[(String, Vec<u8>)]) -> std::io::Result<()> {
    let mut central = Vec::new();
    let mut offset: u32 = 0;
    for (name, data) in entries {
        let crc = crc32(data);
        let n = name.as_bytes();
        let size = data.len() as u32;
        let mut h = Vec::new();
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes()); // version needed
        h.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
        h.extend_from_slice(&0u16.to_le_bytes()); // stored
        h.extend_from_slice(&0u16.to_le_bytes()); // time
        h.extend_from_slice(&0x21u16.to_le_bytes()); // date 1980-01-01
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&size.to_le_bytes());
        h.extend_from_slice(&(n.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(n);
        out.write_all(&h)?;
        out.write_all(data)?;
        let mut c = Vec::new();
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&0x0800u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&0x21u16.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&size.to_le_bytes());
        c.extend_from_slice(&size.to_le_bytes());
        c.extend_from_slice(&(n.len() as u16).to_le_bytes());
        c.extend_from_slice(&[0u8; 8]); // extra, comment, disk, internal attrs
        c.extend_from_slice(&0u32.to_le_bytes()); // external attrs
        c.extend_from_slice(&offset.to_le_bytes());
        c.extend_from_slice(n);
        central.extend_from_slice(&c);
        offset += h.len() as u32 + size;
    }
    let mut end = Vec::new();
    end.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    end.extend_from_slice(&[0u8; 4]);
    end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    end.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    end.extend_from_slice(&(central.len() as u32).to_le_bytes());
    end.extend_from_slice(&offset.to_le_bytes());
    end.extend_from_slice(&0u16.to_le_bytes());
    out.write_all(&central)?;
    out.write_all(&end)
}

// ---------------------------------------------------------------- the report

fn run(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd).args(args).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default()
}

/// What goes into the report. Sign-in tokens (rclone.conf) are never included.
fn collect(rclone: &str) -> Vec<(String, Vec<u8>)> {
    let mut e: Vec<(String, Vec<u8>)> = Vec::new();
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let about = format!(
        "Drive For Linux {}\nrclone: {}\nsystem: {}desktop: {} ({})\nverbose logging: {}\n",
        env!("CARGO_PKG_VERSION"),
        run(rclone, &["version"]).lines().next().unwrap_or(""),
        run("uname", &["-a"]),
        env("XDG_CURRENT_DESKTOP"),
        env("XDG_SESSION_TYPE"),
        VERBOSE.load(Ordering::Relaxed),
    );
    e.push(("about.txt".into(), about.into_bytes()));
    e.push(("connections.txt".into(), run(rclone, &["listremotes", "--long"]).into_bytes()));
    if let Ok(rd) = std::fs::read_dir(config_dir()) {
        let mut names: Vec<PathBuf> = rd.flatten().map(|d| d.path()).collect();
        names.sort();
        let mut states = String::new();
        for p in names {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name == "settings.json" || name == "history.json" || name.starts_with("ransomware-") {
                if let Ok(b) = std::fs::read(&p) {
                    e.push((format!("config/{name}"), b));
                }
            } else if name.starts_with("mirror-state") && name.ends_with(".json") {
                // Only a summary: the file lists can be very long.
                if let Some(v) = std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()) {
                    let files = v["files"].as_object().map(|o| o.len()).unwrap_or(0);
                    states.push_str(&format!("{name}: remote {} folder {} files {files}\n", v["remote"], v["folder"]));
                }
            }
        }
        e.push(("mirror-states.txt".into(), states.into_bytes()));
    }
    for n in (0..LOG_MAX_FILES).rev() {
        let p = log_file(n);
        if let Ok(b) = std::fs::read(&p) {
            e.push((format!("Logs/{}", p.file_name().unwrap().to_string_lossy()), b));
        }
    }
    e
}

/// Builds the report and returns where it was put (the desktop, else the home folder).
pub fn generate(rclone: &str) -> Result<PathBuf, String> {
    let entries = collect(rclone);
    let tmp_dir = std::env::temp_dir();
    let mut at = crate::ransom::now_ms();
    let mut made = None;
    for _ in 0..ZIP_ATTEMPTS {
        let p = tmp_dir.join(zip_name(at));
        if let Ok(f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            made = Some((p, f));
            break;
        }
        at += ZIP_RETRY_STEP_MS;
    }
    let (tmp, mut f) = made.ok_or_else(|| format!("Can't create zip file in {ZIP_ATTEMPTS} attempts"))?;
    write_zip(&mut f, &entries).map_err(|e| e.to_string())?;
    drop(f);
    let desk = dirs::desktop_dir().filter(|d| d.is_dir()).or_else(dirs::home_dir).ok_or("Can't acquire desktop directory")?;
    let dst = desk.join(tmp.file_name().unwrap());
    if std::fs::rename(&tmp, &dst).is_ok() {
        return Ok(dst);
    }
    match std::fs::copy(&tmp, &dst) {
        Ok(_) => {
            let _ = std::fs::remove_file(&tmp);
            Ok(dst)
        }
        Err(_) => Ok(tmp), // keep it where it is, like the original when the copy fails
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_known_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn zip_is_readable() {
        let mut buf = Vec::new();
        write_zip(&mut buf, &[("a.txt".into(), b"hello".to_vec()), ("Logs/b.txt".into(), b"world".to_vec())]).unwrap();
        assert_eq!(&buf[..4], &[0x50, 0x4b, 0x03, 0x04]);
        let tail = &buf[buf.len() - 22..];
        assert_eq!(&tail[..4], &[0x50, 0x4b, 0x05, 0x06]);
        assert_eq!(u16::from_le_bytes([tail[10], tail[11]]), 2);
    }

    #[test]
    fn name_like_the_original() {
        let n = zip_name(1_794_300_000_120);
        assert!(n.starts_with("Google Drive Diagnostics - 20"), "{n}");
        assert!(n.ends_with(".zip") && !n.contains(':'), "{n}");
    }
}

#[cfg(test)]
mod live {
    /// Run alone with HOME pointing at a scratch folder: `cargo test live_report -- --ignored`.
    #[test]
    #[ignore]
    fn live_report() {
        super::log_event("notice", "test line");
        let p = super::generate("rclone").expect("report");
        println!("REPORT {}", p.display());
    }
}

#[cfg(test)]
mod zip_layout {
    use super::*;

    /// The central directory must point back at each file's header, or zip tools can't open it.
    #[test]
    fn central_directory_points_at_each_entry() {
        let entries = vec![("about.txt".to_string(), b"one".to_vec()), ("Logs/structured_log.txt".to_string(), vec![7u8; 1000])];
        let mut buf = Vec::new();
        write_zip(&mut buf, &entries).unwrap();
        let end = buf.len() - 22;
        let u16_at = |i: usize| u16::from_le_bytes([buf[i], buf[i + 1]]) as usize;
        let u32_at = |i: usize| u32::from_le_bytes([buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]) as usize;
        assert_eq!(u32_at(end), 0x0605_4b50);
        let (count, cd_size, cd_off) = (u16_at(end + 10), u32_at(end + 12), u32_at(end + 16));
        assert_eq!(count, 2);
        assert_eq!(cd_off + cd_size, end);
        let mut at = cd_off;
        for (name, data) in &entries {
            assert_eq!(u32_at(at), 0x0201_4b50);
            assert_eq!(u32_at(at + 20), data.len());
            let n = u16_at(at + 28);
            assert_eq!(&buf[at + 46..at + 46 + n], name.as_bytes());
            let local = u32_at(at + 42);
            assert_eq!(u32_at(local), 0x0403_4b50);
            assert_eq!(u32_at(local + 14), crc32(data) as usize);
            at += 46 + n;
        }
        assert_eq!(rclone_verbosity(), "-v", "verbose logging is off by default");
    }
}
