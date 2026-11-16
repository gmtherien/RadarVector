//! HTTP client for VATSIM's public feeds.
//!
//! Feed URLs are read from `status.json` instead of being hard-coded, as
//! VATSIM asks; a random URL is picked from each list on every request.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;

use crate::models::{self, DataFeed, StationTransceivers, Status};

pub const STATUS_URL: &str = "https://status.vatsim.net/status.json";
pub const STATUS_TTL: Duration = Duration::from_secs(24 * 3600);
pub const METAR_TTL: Duration = Duration::from_secs(5 * 60);
pub const TIMEOUT: Duration = Duration::from_secs(20);
pub const USER_AGENT: &str = concat!("RadarVector/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{0}")]
    Decode(#[from] models::DecodeError),
    #[error("status.json lists no URL for {0}")]
    NoUrl(&'static str),
    #[error("cancelled")]
    Cancelled,
}

/// Outcome of a data feed fetch.
#[derive(Debug)]
pub enum FetchOutcome {
    /// New data.
    Updated(Box<DataFeed>),
    /// `update_timestamp` did not change since the last fetch.
    Unchanged,
}

pub struct FeedClient {
    http: reqwest::Client,
    status_url: String,
    status: Mutex<Option<(Instant, Status)>>,
    last_update: Mutex<Option<String>>,
    metar_cache: Mutex<HashMap<String, (Instant, String)>>,
}

fn pick(urls: &[String]) -> Option<&String> {
    if urls.is_empty() {
        return None;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0);
    urls.get(nanos % urls.len())
}

impl FeedClient {
    pub fn new() -> Result<Self, FeedError> {
        Self::with_status_url(STATUS_URL)
    }

    pub fn with_status_url(url: &str) -> Result<Self, FeedError> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(TIMEOUT)
            .build()?;
        Ok(Self {
            http,
            status_url: url.to_string(),
            status: Mutex::new(None),
            last_update: Mutex::new(None),
            metar_cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    async fn get_bytes(&self, url: &str, cancel: &CancellationToken) -> Result<Vec<u8>, FeedError> {
        let fut = async {
            let resp = self.http.get(url).send().await?.error_for_status()?;
            Ok::<_, FeedError>(resp.bytes().await?.to_vec())
        };
        tokio::select! {
            r = fut => r,
            _ = cancel.cancelled() => Err(FeedError::Cancelled),
        }
    }

    /// `status.json`, cached for 24 h.
    pub async fn status(&self, cancel: &CancellationToken) -> Result<Status, FeedError> {
        if let Ok(g) = self.status.lock()
            && let Some((at, s)) = g.as_ref()
            && at.elapsed() < STATUS_TTL
        {
            return Ok(s.clone());
        }
        let bytes = self.get_bytes(&self.status_url, cancel).await?;
        let status: Status = serde_json::from_slice(&bytes).map_err(models::DecodeError::from)?;
        if let Ok(mut g) = self.status.lock() {
            *g = Some((Instant::now(), status.clone()));
        }
        Ok(status)
    }

    /// Fetch the v3 data feed. Returns [`FetchOutcome::Unchanged`] when the
    /// server-side `update_timestamp` equals the previous one.
    pub async fn fetch_data(&self, cancel: &CancellationToken) -> Result<FetchOutcome, FeedError> {
        let status = self.status(cancel).await?;
        let url = pick(&status.data.v3).ok_or(FeedError::NoUrl("v3"))?;
        let bytes = self.get_bytes(url, cancel).await?;
        let feed = models::decode_data_feed(&bytes)?;
        let mut last = self.last_update.lock().unwrap_or_else(|e| e.into_inner());
        if !feed.general.update_timestamp.is_empty() && last.as_deref() == Some(feed.general.update_timestamp.as_str())
        {
            return Ok(FetchOutcome::Unchanged);
        }
        *last = Some(feed.general.update_timestamp.clone());
        Ok(FetchOutcome::Updated(Box::new(feed)))
    }

    /// Forget the last `update_timestamp` so the next fetch always returns data.
    pub fn reset_unchanged_check(&self) {
        if let Ok(mut g) = self.last_update.lock() {
            *g = None;
        }
    }

    pub async fn fetch_transceivers(&self, cancel: &CancellationToken) -> Result<Vec<StationTransceivers>, FeedError> {
        let status = self.status(cancel).await?;
        let url = pick(&status.data.transceivers).ok_or(FeedError::NoUrl("transceivers"))?;
        Ok(models::decode_transceivers(&self.get_bytes(url, cancel).await?)?)
    }

    /// METARs for several stations; returns `(icao, raw text)` in the order
    /// received. Results are cached 5 minutes per station.
    pub async fn fetch_metars(
        &self,
        icaos: &[String],
        cancel: &CancellationToken,
    ) -> Result<Vec<(String, String)>, FeedError> {
        let wanted: Vec<String> = icaos
            .iter()
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric()))
            .collect();
        let mut out = Vec::new();
        let mut missing = Vec::new();
        {
            let cache = self.metar_cache.lock().unwrap_or_else(|e| e.into_inner());
            for icao in &wanted {
                match cache.get(icao) {
                    Some((at, text)) if at.elapsed() < METAR_TTL => out.push((icao.clone(), text.clone())),
                    _ => missing.push(icao.clone()),
                }
            }
        }
        if !missing.is_empty() {
            let status = self.status(cancel).await?;
            let base = pick(&status.metar).ok_or(FeedError::NoUrl("metar"))?;
            let fut = async {
                let resp = self
                    .http
                    .get(base)
                    .query(&[("id", missing.join(","))])
                    .send()
                    .await?
                    .error_for_status()?;
                Ok::<_, FeedError>(resp.text().await?)
            };
            let body = tokio::select! {
                r = fut => r?,
                _ = cancel.cancelled() => return Err(FeedError::Cancelled),
            };
            let mut cache = self.metar_cache.lock().unwrap_or_else(|e| e.into_inner());
            for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
                let station = line.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
                if missing.contains(&station) {
                    cache.insert(station.clone(), (Instant::now(), line.to_string()));
                    out.push((station, line.to_string()));
                }
            }
        }
        out.sort_by_key(|(icao, _)| wanted.iter().position(|w| w == icao));
        Ok(out)
    }
}
