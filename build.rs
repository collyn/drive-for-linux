//! Builds the Dolphin sync-status plugin (fm-plugins/dolphin) with the system's Qt 6 and KF6 KIO,
//! so the app can install it. When the tools or libraries are missing the app still builds; it
//! just has no Dolphin plugin to install (an empty file is embedded instead).
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let so = out.join("drivelinux_overlay.so");
    let src = Path::new("fm-plugins/dolphin");
    for f in ["drivelinux_overlay.cpp", "drivelinux_overlay.json", "koverlayiconplugin_decl.h"] {
        println!("cargo:rerun-if-changed={}", src.join(f).display());
    }
    println!("cargo:rerun-if-changed=build.rs");
    if !build_plugin(src, &out, &so) {
        println!("cargo:warning=Dolphin sync-status plugin not built (needs g++, Qt 6 development files and KF6 KIO); file badges in Dolphin will be unavailable");
        let _ = std::fs::write(&so, b"");
    }
}

fn find<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    candidates.iter().copied().find(|p| Path::new(p).exists())
}

fn build_plugin(src: &Path, out: &Path, so: &Path) -> bool {
    let Some(moc) = find(&["/usr/lib/qt6/libexec/moc", "/usr/lib/x86_64-linux-gnu/qt6/libexec/moc", "/usr/lib64/qt6/libexec/moc"]) else { return false };
    let Some(qt) = find(&["/usr/include/x86_64-linux-gnu/qt6", "/usr/include/qt6"]) else { return false };
    let Some(kio) = find(&["/usr/lib/x86_64-linux-gnu/libKF6KIOCore.so.6", "/usr/lib64/libKF6KIOCore.so.6", "/usr/lib/libKF6KIOCore.so.6"]) else { return false };
    let cpp = src.join("drivelinux_overlay.cpp");
    let moc_ok = Command::new(moc).arg(&cpp).arg("-o").arg(out.join("drivelinux_overlay.moc")).status().map(|s| s.success()).unwrap_or(false);
    if !moc_ok {
        return false;
    }
    Command::new("g++")
        .args(["-shared", "-fPIC", "-O2", "-std=c++17"])
        .arg(format!("-I{qt}"))
        .arg(format!("-I{qt}/QtCore"))
        .arg(format!("-I{qt}/QtNetwork"))
        .arg(format!("-I{}", src.display()))
        .arg(format!("-I{}", out.display()))
        .arg(&cpp)
        .arg("-o")
        .arg(so)
        .args(["-lQt6Core", "-lQt6Network"])
        .arg(kio)
        .arg("-Wl,--no-undefined")
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
