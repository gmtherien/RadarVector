//! Application self-update (UPD-03).
//!
//! The release workflow publishes `latest.json` next to the installers, in
//! the format of cargo-packager's updater:
//!
//! ```json
//! { "version": "0.2.0", "notes": "...", "pub_date": "2026-12-01T00:00:00Z",
//!   "platforms": { "darwin-aarch64": { "url": "https://…/RadarVector.dmg",
//!                                      "signature": "<base64 of the .sig file>" } } }
//! ```
//!
//! Every download is verified against the embedded minisign public key
//! before it is written to disk; an unsigned or tampered package is refused.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct PlatformAsset {
    pub url: String,
    pub signature: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub pub_date: String,
    pub platforms: HashMap<String, PlatformAsset>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Available {
    pub version: String,
    pub notes: String,
    pub asset: PlatformAsset,
}

#[derive(Debug, thiserror::Error)]
pub enum AppUpdateError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("no package for this platform ({0})")]
    NoPlatform(String),
    #[error("signature check failed: {0}")]
    Signature(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cancelled")]
    Cancelled,
}

/// cargo-packager / Tauri platform key, e.g. `darwin-aarch64`.
pub fn platform_key() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

fn parse_version(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v.trim().trim_start_matches('v')).ok()
}

/// True when `candidate` is strictly newer than `current`, following SemVer
/// precedence (so `0.1.0` is newer than `0.1.0-beta.1`). Unparsable versions
/// are never considered newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// Decide whether `manifest` offers an update for this platform.
pub fn evaluate(manifest: &Manifest, current: &str, platform: &str) -> Result<Option<Available>, AppUpdateError> {
    if !is_newer(&manifest.version, current) {
        return Ok(None);
    }
    let asset = manifest
        .platforms
        .get(platform)
        .ok_or_else(|| AppUpdateError::NoPlatform(platform.to_string()))?;
    Ok(Some(Available {
        version: manifest.version.clone(),
        notes: manifest.notes.clone(),
        asset: asset.clone(),
    }))
}

fn decode_b64_text(s: &str) -> Option<String> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
}

/// Accepts a raw minisign public key (`RW…`) or the base64 of a whole
/// minisign public key file (as printed by `cargo packager signer generate`).
pub fn parse_public_key(key: &str) -> Result<minisign_verify::PublicKey, AppUpdateError> {
    let key = key.trim();
    let raw = match decode_b64_text(key) {
        Some(text) if text.contains("untrusted comment") => text.lines().nth(1).unwrap_or("").trim().to_string(),
        _ => key.to_string(),
    };
    minisign_verify::PublicKey::from_base64(&raw).map_err(|e| AppUpdateError::Signature(format!("bad public key: {e}")))
}

/// Verify `data` against a signature given as base64 of the `.sig` file
/// (manifest format) or as the `.sig` text itself.
pub fn verify(data: &[u8], signature: &str, public_key: &str) -> Result<(), AppUpdateError> {
    let pk = parse_public_key(public_key)?;
    let text = if signature.contains("untrusted comment") {
        signature.to_string()
    } else {
        decode_b64_text(signature).unwrap_or_default()
    };
    let sig = minisign_verify::Signature::decode(&text).map_err(|e| AppUpdateError::Signature(e.to_string()))?;
    pk.verify(data, &sig, false)
        .map_err(|e| AppUpdateError::Signature(e.to_string()))
}

async fn get(http: &reqwest::Client, url: &str, cancel: &CancellationToken) -> Result<Vec<u8>, AppUpdateError> {
    let fut = async {
        let r = http.get(url).send().await?.error_for_status()?;
        Ok::<_, AppUpdateError>(r.bytes().await?.to_vec())
    };
    tokio::select! {
        r = fut => r,
        _ = cancel.cancelled() => Err(AppUpdateError::Cancelled),
    }
}

/// Fetch the manifest and return the update for this platform, if any.
pub async fn check(
    http: &reqwest::Client,
    manifest_url: &str,
    current: &str,
    cancel: &CancellationToken,
) -> Result<Option<Available>, AppUpdateError> {
    let bytes = get(http, manifest_url, cancel).await?;
    let m: Manifest = serde_json::from_slice(&bytes).map_err(|e| AppUpdateError::Manifest(e.to_string()))?;
    evaluate(&m, current, &platform_key())
}

/// Download the package, verify its signature, and save it in `dir`.
/// Returns the saved path; nothing is written if verification fails.
pub async fn download_verified(
    http: &reqwest::Client,
    update: &Available,
    public_key: &str,
    dir: &Path,
    cancel: &CancellationToken,
) -> Result<PathBuf, AppUpdateError> {
    let data = get(http, &update.asset.url, cancel).await?;
    verify(&data, &update.asset.signature, public_key)?;
    let name = update
        .asset
        .url
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty())
        .unwrap_or("RadarVector-update");
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
        .collect();
    std::fs::create_dir_all(dir)?;
    let path = dir.join(name);
    std::fs::write(&path, data)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keypair() -> (String, minisign::SecretKey) {
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        (kp.pk.to_base64(), kp.sk)
    }

    fn sign(sk: &minisign::SecretKey, data: &[u8]) -> String {
        let sb = minisign::sign(None, sk, std::io::Cursor::new(data), Some("RadarVector test"), None).unwrap();
        base64::engine::general_purpose::STANDARD.encode(sb.into_string())
    }

    #[test]
    fn versions() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("0.1.0", "0.1.0-beta.1"));
        assert!(is_newer("0.1.0-beta.2", "0.1.0-beta.1"));
        assert!(is_newer("0.1.0-beta.10", "0.1.0-beta.9"));
        assert!(!is_newer("0.1.0-beta.1", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn signature_roundtrip_and_tamper() {
        let (pk, sk) = keypair();
        let data = b"installer bytes";
        let sig = sign(&sk, data);
        verify(data, &sig, &pk).unwrap();
        assert!(verify(b"tampered bytes!", &sig, &pk).is_err());
        let (other_pk, _) = keypair();
        assert!(verify(data, &sig, &other_pk).is_err());
        // Public key given as base64 of a key file.
        let file = format!("untrusted comment: minisign public key\n{pk}\n");
        let pk_file_b64 = base64::engine::general_purpose::STANDARD.encode(file);
        verify(data, &sig, &pk_file_b64).unwrap();
    }

    #[test]
    fn manifest_evaluation() {
        let m: Manifest = serde_json::from_str(
            r#"{"version":"0.3.0","notes":"n","platforms":{"darwin-aarch64":{"url":"https://x/RadarVector.dmg","signature":"s"}}}"#,
        )
        .unwrap();
        assert!(evaluate(&m, "0.3.0", "darwin-aarch64").unwrap().is_none());
        let a = evaluate(&m, "0.2.0", "darwin-aarch64").unwrap().unwrap();
        assert_eq!(a.asset.url, "https://x/RadarVector.dmg");
        assert!(matches!(
            evaluate(&m, "0.2.0", "linux-x86_64"),
            Err(AppUpdateError::NoPlatform(_))
        ));
        assert!(platform_key().contains('-'));
    }
}
