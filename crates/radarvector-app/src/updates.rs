//! Build-time configuration of application updates.
//!
//! Release builds set `RADARVECTOR_UPDATE_URL` (URL of `latest.json`) and
//! `RADARVECTOR_UPDATE_PUBKEY` (minisign public key). Builds without them
//! cannot install updates.

use std::path::PathBuf;

pub fn manifest_url() -> Option<String> {
    option_env!("RADARVECTOR_UPDATE_URL")
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub fn public_key() -> Option<String> {
    option_env!("RADARVECTOR_UPDATE_PUBKEY")
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}

pub fn download_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
}

/// Open a downloaded installer with the system handler.
pub fn open(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut c = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut c = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut c = std::process::Command::new("xdg-open");
    c.arg(path).spawn().map(|_| ())
}
