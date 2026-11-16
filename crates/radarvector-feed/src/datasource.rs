//! Download and install the VAT-Spy Data Project files.
//!
//! Source: GitHub releases of `vatsimnetwork/vatspy-data-project` (tag =
//! AIRAC cycle, assets `VATSpy.dat` and `Boundaries.geojson`), falling back
//! to the `master` branch. Files are validated before they replace the
//! installed copy; on failure the previous version stays in place.

use std::path::{Path, PathBuf};

use radarvector_data::StaticData;
use radarvector_data::static_data::{BOUNDARIES_FILE, DAT_FILE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::client::FeedError;

pub const REPO: &str = "vatsimnetwork/vatspy-data-project";
pub const MANIFEST_FILE: &str = "manifest.json";
const DATA_SUBDIR: &str = "vatspy-data";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// Release tag (AIRAC cycle) or `master`.
    pub tag: String,
    /// RFC 3339 install time.
    pub installed_at: String,
    pub dat_sha256: String,
    pub boundaries_sha256: String,
    pub source: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("{0}")]
    Feed(#[from] FeedError),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("downloaded data failed validation: {0}")]
    Invalid(String),
    #[error("no user data directory on this system")]
    NoDataDir,
}

#[derive(Debug)]
pub enum UpdateOutcome {
    Installed(Manifest),
    UpToDate(Manifest),
}

/// RadarVector's data directory for the VAT-Spy files.
pub fn default_data_dir() -> Result<PathBuf, UpdateError> {
    directories::ProjectDirs::from("", "", "RadarVector")
        .map(|d| d.data_dir().join(DATA_SUBDIR))
        .ok_or(UpdateError::NoDataDir)
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

async fn get(http: &reqwest::Client, url: &str, cancel: &CancellationToken) -> Result<Vec<u8>, UpdateError> {
    let fut = async {
        let r = http
            .get(url)
            .header("Accept", "application/vnd.github+json, */*")
            .send()
            .await?
            .error_for_status()?;
        Ok::<_, UpdateError>(r.bytes().await?.to_vec())
    };
    tokio::select! {
        r = fut => r,
        _ = cancel.cancelled() => Err(FeedError::Cancelled.into()),
    }
}

/// Locate the latest files: `(tag, dat_url, boundaries_url)`.
async fn latest_source(http: &reqwest::Client, cancel: &CancellationToken) -> (String, String, String) {
    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    if let Ok(bytes) = get(http, &api, cancel).await
        && let Ok(rel) = serde_json::from_slice::<Release>(&bytes)
    {
        let find = |n: &str| {
            rel.assets
                .iter()
                .find(|a| a.name == n)
                .map(|a| a.browser_download_url.clone())
        };
        if let (Some(d), Some(b)) = (find(DAT_FILE), find(BOUNDARIES_FILE)) {
            return (rel.tag_name, d, b);
        }
    }
    let raw = format!("https://raw.githubusercontent.com/{REPO}/master");
    (
        "master".into(),
        format!("{raw}/{DAT_FILE}"),
        format!("{raw}/{BOUNDARIES_FILE}"),
    )
}

/// Validate texts the same way the app loads them.
pub fn check(dat: &str, geojson: &str) -> Result<(), UpdateError> {
    let (_, report) = StaticData::from_texts(dat, geojson).map_err(|e| UpdateError::Invalid(e.to_string()))?;
    if report.dat.rejected_lines() > 0 {
        return Err(UpdateError::Invalid(format!(
            "{} rejected lines in {DAT_FILE}",
            report.dat.rejected_lines()
        )));
    }
    if !report.boundary_issues.is_empty() {
        return Err(UpdateError::Invalid(format!(
            "{} invalid boundary features",
            report.boundary_issues.len()
        )));
    }
    if let Some(e) = report.validation.errors().next() {
        return Err(UpdateError::Invalid(e.message.clone()));
    }
    Ok(())
}

/// Write both files and the manifest into `dir`, replacing the previous set
/// only once the new one is fully written.
pub fn install(dir: &Path, dat: &[u8], geojson: &[u8], tag: &str, source: &str) -> Result<Manifest, UpdateError> {
    let parent = dir.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::Builder::new().prefix(".rv-staging").tempdir_in(parent)?;
    let manifest = Manifest {
        tag: tag.to_string(),
        installed_at: now_rfc3339(),
        dat_sha256: sha256_hex(dat),
        boundaries_sha256: sha256_hex(geojson),
        source: source.to_string(),
    };
    std::fs::write(staging.path().join(DAT_FILE), dat)?;
    std::fs::write(staging.path().join(BOUNDARIES_FILE), geojson)?;
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| UpdateError::Invalid(e.to_string()))?;
    std::fs::write(staging.path().join(MANIFEST_FILE), json)?;

    let staged = staging.keep();
    let old = parent.join(".rv-previous");
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    if dir.exists() {
        std::fs::rename(dir, &old)?;
    }
    if let Err(e) = std::fs::rename(&staged, dir) {
        if old.exists() {
            let _ = std::fs::rename(&old, dir);
        }
        return Err(e.into());
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(manifest)
}

/// Check GitHub and install a newer data set into `dir` if one exists.
pub async fn update(
    http: &reqwest::Client,
    dir: &Path,
    force: bool,
    cancel: &CancellationToken,
) -> Result<UpdateOutcome, UpdateError> {
    let (tag, dat_url, geo_url) = latest_source(http, cancel).await;
    let current = read_manifest(dir);
    if !force
        && tag != "master"
        && let Some(m) = &current
        && m.tag == tag
        && dir.join(DAT_FILE).exists()
    {
        return Ok(UpdateOutcome::UpToDate(m.clone()));
    }
    let dat = get(http, &dat_url, cancel).await?;
    let geo = get(http, &geo_url, cancel).await?;
    if let Some(m) = &current
        && !force
        && m.dat_sha256 == sha256_hex(&dat)
        && m.boundaries_sha256 == sha256_hex(&geo)
    {
        return Ok(UpdateOutcome::UpToDate(m.clone()));
    }
    let dat_text = String::from_utf8_lossy(&dat);
    let geo_text = String::from_utf8_lossy(&geo);
    check(&dat_text, &geo_text)?;
    Ok(UpdateOutcome::Installed(install(dir, &dat, &geo, &tag, &dat_url)?))
}

/// Minimal UTC RFC 3339 formatter (avoids a date-time dependency).
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_unix(secs)
}

pub fn format_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant's public-domain algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix(1_794_787_200), "2026-11-16T00:00:00Z");
        assert_eq!(format_unix(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn install_replaces_and_writes_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("vatspy-data");
        let m1 = install(&dir, b"[Countries]\n", b"{}", "v1", "test").unwrap();
        assert_eq!(read_manifest(&dir), Some(m1));
        install(&dir, b"[Countries]\nFrance|LF|\n", b"{}", "v2", "test").unwrap();
        assert_eq!(read_manifest(&dir).map(|m| m.tag), Some("v2".into()));
        assert!(std::fs::read_to_string(dir.join(DAT_FILE)).unwrap().contains("France"));
        assert!(!tmp.path().join(".rv-previous").exists());
    }

    #[test]
    fn check_rejects_bad_data() {
        assert!(check("[Airports]\nXX|bad\n", r#"{"type":"FeatureCollection","features":[]}"#).is_err());
    }
}

/// SimAware TRACON Project (CC-BY-SA-4.0): same update flow, one file.
pub mod tracon {
    use super::*;

    pub const REPO: &str = "vatsimnetwork/simaware-tracon-project";
    pub const FILE: &str = "TRACONBoundaries.geojson";
    const SUBDIR: &str = "simaware-tracon";

    pub fn default_dir() -> Result<PathBuf, UpdateError> {
        directories::ProjectDirs::from("", "", "RadarVector")
            .map(|d| d.data_dir().join(SUBDIR))
            .ok_or(UpdateError::NoDataDir)
    }

    /// Valid when it parses and at most 1 % of features are rejected.
    pub fn check(text: &str) -> Result<usize, UpdateError> {
        let (t, skipped) = radarvector_data::tracon::parse(text).map_err(|e| UpdateError::Invalid(e.to_string()))?;
        if t.is_empty() || skipped * 100 > t.len() {
            return Err(UpdateError::Invalid(format!(
                "{} TRACON features, {skipped} rejected",
                t.len()
            )));
        }
        Ok(t.len())
    }

    pub fn install(dir: &Path, bytes: &[u8], tag: &str) -> Result<(), UpdateError> {
        std::fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, dir.join(FILE))?;
        let m = Manifest {
            tag: tag.into(),
            installed_at: now_rfc3339(),
            dat_sha256: String::new(),
            boundaries_sha256: sha256_hex(bytes),
            source: REPO.into(),
        };
        let json = serde_json::to_vec_pretty(&m).map_err(|e| UpdateError::Invalid(e.to_string()))?;
        std::fs::write(dir.join(MANIFEST_FILE), json)?;
        Ok(())
    }

    /// Download the latest release asset if its tag differs from the installed one.
    /// Returns the new tag when something was installed.
    pub async fn update(
        http: &reqwest::Client,
        dir: &Path,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, UpdateError> {
        let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let rel: Release =
            serde_json::from_slice(&get(http, &api, cancel).await?).map_err(|e| UpdateError::Invalid(e.to_string()))?;
        if read_manifest(dir).is_some_and(|m| m.tag == rel.tag_name) && dir.join(FILE).exists() {
            return Ok(None);
        }
        let url = rel
            .assets
            .iter()
            .find(|a| a.name == FILE)
            .map(|a| a.browser_download_url.clone())
            .ok_or_else(|| UpdateError::Invalid(format!("release {} has no {FILE}", rel.tag_name)))?;
        let bytes = get(http, &url, cancel).await?;
        check(&String::from_utf8_lossy(&bytes))?;
        install(dir, &bytes, &rel.tag_name)?;
        Ok(Some(rel.tag_name))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn check_and_install() {
            let good = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"SCT","prefix":["LAX"],"name":"SoCal"},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}}]}"#;
            assert_eq!(check(good).unwrap(), 1);
            assert!(check(r#"{"type":"FeatureCollection","features":[]}"#).is_err());
            let tmp = tempfile::tempdir().unwrap();
            install(tmp.path(), good.as_bytes(), "v1.2.14").unwrap();
            assert_eq!(read_manifest(tmp.path()).map(|m| m.tag), Some("v1.2.14".into()));
            assert!(tmp.path().join(FILE).exists());
        }
    }
}
