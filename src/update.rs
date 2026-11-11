//! "Is this version still current?", checked once when the app starts, like the original's
//! NotifyIfDeprecated (apps/drive/fs/ui/client.cc, in function 0x10031f830: "Verifying
//! deprecation status to notify the user."). The original asks Google's server for a status
//! (0 fine, 1 unsupported, 2 deprecated, 3 OS unsupported, 4 deprecated and unsupported, 5 OS
//! unsupported soon, 6-8 the 32-bit cases, 9 Photos backup) and tells the person to "upgrade to a
//! newer version". This app has no such server: it compares its version with the newest release
//! of the GitHub project it was built from (the release workflow records it in APP_HOMEPAGE), so
//! builds made elsewhere never check. Pre-releases are not offered.
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// The project the release workflow built this copy from ("https://github.com/<owner>/<repo>").
const HOMEPAGE: Option<&str> = option_env!("APP_HOMEPAGE");

fn repo(homepage: &str) -> Option<(String, String)> {
    let rest = homepage.trim_end_matches('/').strip_prefix("https://github.com/")?;
    let (owner, name) = rest.split_once('/')?;
    let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    (ok(owner) && ok(name) && !name.contains('/')).then(|| (owner.to_string(), name.to_string()))
}

/// "v0.2.0" → [0, 2, 0]; anything after a dash or plus is left out.
fn numbers(v: &str) -> Option<Vec<u64>> {
    let v = v.trim().trim_start_matches('v');
    let core = v.split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `latest` is a newer release than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (numbers(latest), numbers(current)) {
        (Some(mut a), Some(mut b)) => {
            let n = a.len().max(b.len());
            a.resize(n, 0);
            b.resize(n, 0);
            a > b
        }
        _ => false,
    }
}

/// Looks for a newer release in the background and, when there is one, adds a notification.
pub fn spawn_check(shared: Arc<Mutex<crate::engine::Shared>>) {
    let Some((owner, name)) = HOMEPAGE.and_then(repo) else { return };
    std::thread::spawn(move || {
        let url = format!("https://api.github.com/repos/{owner}/{name}/releases/latest");
        let mut c = Command::new("curl");
        c.args(["-fsSL", "--max-time", "20", "-H", "Accept: application/vnd.github+json", "-A", "drive-for-linux", &url]);
        if crate::engine::PROXY_DIRECT.load(std::sync::atomic::Ordering::Relaxed) {
            for v in ["HTTP_PROXY", "HTTPS_PROXY", "http_proxy", "https_proxy", "ALL_PROXY", "all_proxy"] {
                c.env_remove(v);
            }
        }
        let Ok(out) = c.stdin(Stdio::null()).stderr(Stdio::null()).output() else { return };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return };
        let (Some(tag), Some(page)) = (v["tag_name"].as_str(), v["html_url"].as_str()) else { return };
        crate::diag::log_event("info", &format!("Latest release: {tag}; this copy: {}", crate::VERSION));
        if !is_newer(tag, crate::VERSION) {
            return;
        }
        let title = crate::i18n::tr("A newer version of Drive For Linux is available");
        let body = format!("{} {} → {}: {page}", crate::i18n::tr("Please update to a newer version."), crate::VERSION, tag.trim_start_matches('v'));
        let mut sh = shared.lock().unwrap();
        let shown = !sh.hidden_notices.contains(title);
        sh.notify(title, &body);
        drop(sh);
        if shown {
            crate::engine::desktop_notify(title, &body);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0-beta.1"));
        assert!(!is_newer("0.2.0-beta.1", "0.2.0"));
        assert!(!is_newer("nightly", "0.1.0"));
    }

    #[test]
    fn only_github_projects_are_checked() {
        assert_eq!(repo("https://github.com/someone/drive-for-linux"), Some(("someone".into(), "drive-for-linux".into())));
        assert_eq!(repo("https://github.com/"), None);
        assert_eq!(repo("https://example.com/a/b"), None);
        assert_eq!(repo("https://github.com/a/b/c"), None);
    }
}
