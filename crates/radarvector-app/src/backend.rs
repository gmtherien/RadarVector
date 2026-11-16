//! Background networking: a small tokio runtime that refreshes the feed,
//! updates the static data and fetches METARs. The UI thread never does I/O.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use radarvector_core::{NetworkSnapshot, SnapshotDiff};
use radarvector_data::StaticData;
use radarvector_feed::backoff::{Backoff, clamp_interval};
use radarvector_feed::datasource::{self, UpdateOutcome};
use radarvector_feed::{CancellationToken, FeedClient, FetchOutcome};
use tokio::sync::mpsc;

pub const STATUS_PAGE: &str = "https://status.vatsim.net/";
const DATA_UPDATE_EVERY: Duration = Duration::from_secs(24 * 3600);

pub enum Command {
    RefreshNow,
    Stop,
    SetAuto { enabled: bool, interval: Duration },
    FetchMetar { request: u64, stations: Vec<String> },
    UpdateData { force: bool },
    CheckAppUpdate { manual: bool },
    InstallAppUpdate(radarvector_feed::appupdate::Available),
}

pub enum Event {
    RefreshStarted,
    /// New snapshot and diff against the previous one.
    Snapshot(Arc<NetworkSnapshot>, Box<SnapshotDiff>),
    RefreshDone,
    RefreshFailed {
        error: String,
        retry_in: Duration,
    },
    Metar {
        request: u64,
        result: Result<Vec<(String, String)>, String>,
    },
    StaticDataUpdated {
        tag: String,
    },
    TraconUpdated(Arc<crate::tracon::TraconSet>),
    AppUpdate {
        available: Option<radarvector_feed::appupdate::Available>,
        manual: bool,
    },
    AppUpdateFailed(String),
    AppUpdateDownloaded(PathBuf),
    Log(String),
}

pub struct Backend {
    tx: mpsc::UnboundedSender<Command>,
    pub events: std::sync::mpsc::Receiver<Event>,
}

impl Backend {
    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }

    /// Start the background thread. `notify` is called after each event (to
    /// wake the UI).
    pub fn start(
        static_data: Arc<ArcSwap<StaticData>>,
        tracons: Arc<ArcSwap<radarvector_data::tracon::Tracons>>,
        data_dir: PathBuf,
        auto: bool,
        interval: Duration,
        notify: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (etx, erx) = std::sync::mpsc::channel();
        let notify = Arc::new(notify);
        std::thread::Builder::new()
            .name("radarvector-net".into())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = etx.send(Event::Log(format!("cannot start network runtime: {e}")));
                        return;
                    }
                };
                rt.block_on(async move {
                    let send = |e: Event| {
                        let _ = etx.send(e);
                        notify();
                    };
                    let client = match FeedClient::new() {
                        Ok(c) => Arc::new(c),
                        Err(e) => {
                            send(Event::Log(format!("HTTP client error: {e}")));
                            return;
                        }
                    };
                    let mut auto = auto;
                    let mut interval = clamp_interval(interval);
                    let mut backoff = Backoff::new();
                    let mut next_refresh = Instant::now();
                    // First refresh before the (slower) data update check.
                    let mut next_data_update = Instant::now() + Duration::from_secs(5);
                    let current_cancel = Arc::new(std::sync::Mutex::new(CancellationToken::new()));
                    // Stop must act while a fetch is in flight, so commands are
                    // read by their own task.
                    let (itx, mut irx) = mpsc::unbounded_channel::<Command>();
                    {
                        let cc = current_cancel.clone();
                        tokio::spawn(async move {
                            while let Some(cmd) = rx.recv().await {
                                match cmd {
                                    Command::Stop => {
                                        if let Ok(t) = cc.lock() {
                                            t.cancel();
                                        }
                                    }
                                    other => {
                                        if itx.send(other).is_err() {
                                            break;
                                        }
                                    }
                                }
                            }
                        });
                    }
                    let mut last_snapshot: Option<Arc<NetworkSnapshot>> = None;
                    let mut force_refresh = true;

                    loop {
                        let now = Instant::now();
                        if now >= next_data_update {
                            next_data_update = now + DATA_UPDATE_EVERY;
                            let c = client.clone();
                            let dir = data_dir.clone();
                            let sd = static_data.clone();
                            match datasource::update(c.http(), &dir, false, &CancellationToken::new()).await {
                                Ok(UpdateOutcome::Installed(m)) => match StaticData::load_dir(&dir) {
                                    Ok((d, _)) => {
                                        sd.store(Arc::new(d));
                                        send(Event::StaticDataUpdated { tag: m.tag });
                                        force_refresh = true;
                                    }
                                    Err(e) => send(Event::Log(format!("data reload failed: {e}"))),
                                },
                                Ok(UpdateOutcome::UpToDate(m)) => send(Event::Log(format!("VAT-Spy data up to date ({})", m.tag))),
                                Err(e) => send(Event::Log(format!("VAT-Spy data update failed: {e}"))),
                            }
                            if let Ok(tdir) = datasource::tracon::default_dir() {
                                match datasource::tracon::update(client.http(), &tdir, &CancellationToken::new()).await {
                                    Ok(Some(tag)) => {
                                        if let Some(set) = crate::tracon::TraconSet::load_dir(&tdir) {
                                            tracons.store(Arc::new(set.tracons.clone()));
                                            send(Event::TraconUpdated(Arc::new(set)));
                                            send(Event::Log(format!("TRACON data {tag} installed")));
                                            force_refresh = true;
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(e) => send(Event::Log(format!("TRACON update failed: {e}"))),
                                }
                            }
                        }
                        if force_refresh || (auto && now >= next_refresh) {
                            force_refresh = false;
                            send(Event::RefreshStarted);
                            let cancel = CancellationToken::new();
                            if let Ok(mut t) = current_cancel.lock() {
                                *t = cancel.clone();
                            }
                            let result = client.fetch_data(&cancel).await;
                            match result {
                                Ok(FetchOutcome::Updated(feed)) => {
                                    backoff.succeed();
                                    let feed = *feed;
                                    let sd = static_data.load_full();
                                    let tr = tracons.load_full();
                                    let built = tokio::task::spawn_blocking(move || NetworkSnapshot::build_with(&sd, &*tr, feed)).await;
                                    if let Ok(snap) = built {
                                        let snap = Arc::new(snap);
                                        let diff = last_snapshot
                                            .as_ref()
                                            .map(|old| SnapshotDiff::between(old, &snap))
                                            .unwrap_or_default();
                                        last_snapshot = Some(snap.clone());
                                        send(Event::Snapshot(snap, Box::new(diff)));
                                    }
                                    send(Event::RefreshDone);
                                    next_refresh = Instant::now() + interval;
                                }
                                Ok(FetchOutcome::Unchanged) => {
                                    send(Event::RefreshDone);
                                    next_refresh = Instant::now() + interval;
                                }
                                Err(radarvector_feed::FeedError::Cancelled) => {
                                    send(Event::RefreshDone);
                                    next_refresh = Instant::now() + interval;
                                }
                                Err(e) => {
                                    let retry_in = backoff.fail();
                                    next_refresh = Instant::now() + retry_in;
                                    send(Event::RefreshFailed { error: e.to_string(), retry_in });
                                }
                            }
                        }

                        let wait = if auto { next_refresh.saturating_duration_since(Instant::now()) } else { Duration::from_secs(3600) }
                            .min(next_data_update.saturating_duration_since(Instant::now()));
                        tokio::select! {
                            cmd = irx.recv() => match cmd {
                                None => break,
                                Some(Command::RefreshNow) => {
                                    client.reset_unchanged_check();
                                    force_refresh = true;
                                }
                                Some(Command::Stop) => {}
                                Some(Command::SetAuto { enabled, interval: i }) => {
                                    auto = enabled;
                                    interval = clamp_interval(i);
                                    if auto { next_refresh = next_refresh.min(Instant::now() + interval); }
                                }
                                Some(Command::FetchMetar { request, stations }) => {
                                    let c = client.clone();
                                    let etx2 = etx.clone();
                                    let n = notify.clone();
                                    tokio::spawn(async move {
                                        let r = c.fetch_metars(&stations, &CancellationToken::new()).await.map_err(|e| e.to_string());
                                        let _ = etx2.send(Event::Metar { request, result: r });
                                        n();
                                    });
                                }
                                Some(Command::CheckAppUpdate { manual }) => {
                                    let c = client.clone();
                                    let etx2 = etx.clone();
                                    let n = notify.clone();
                                    tokio::spawn(async move {
                                        let ev = match crate::updates::manifest_url() {
                                            None => Event::AppUpdateFailed("updates are not configured in this build".into()),
                                            Some(url) => match radarvector_feed::appupdate::check(c.http(), &url, env!("CARGO_PKG_VERSION"), &CancellationToken::new()).await {
                                                Ok(available) => Event::AppUpdate { available, manual },
                                                Err(e) => Event::AppUpdateFailed(e.to_string()),
                                            },
                                        };
                                        if manual || !matches!(ev, Event::AppUpdateFailed(_)) {
                                            let _ = etx2.send(ev);
                                            n();
                                        }
                                    });
                                }
                                Some(Command::InstallAppUpdate(update)) => {
                                    let c = client.clone();
                                    let etx2 = etx.clone();
                                    let n = notify.clone();
                                    tokio::spawn(async move {
                                        let ev = match crate::updates::public_key() {
                                            None => Event::AppUpdateFailed("no update signing key in this build".into()),
                                            Some(pk) => {
                                                let dir = crate::updates::download_dir();
                                                match radarvector_feed::appupdate::download_verified(c.http(), &update, &pk, &dir, &CancellationToken::new()).await {
                                                    Ok(path) => Event::AppUpdateDownloaded(path),
                                                    Err(e) => Event::AppUpdateFailed(e.to_string()),
                                                }
                                            }
                                        };
                                        let _ = etx2.send(ev);
                                        n();
                                    });
                                }
                                Some(Command::UpdateData { force }) => {
                                    if force {
                                        let dir = data_dir.clone();
                                        match datasource::update(client.http(), &dir, true, &CancellationToken::new()).await {
                                            Ok(UpdateOutcome::Installed(m)) | Ok(UpdateOutcome::UpToDate(m)) => {
                                                if let Ok((d, _)) = StaticData::load_dir(&dir) {
                                                    static_data.store(Arc::new(d));
                                                    send(Event::StaticDataUpdated { tag: m.tag });
                                                    force_refresh = true;
                                                }
                                            }
                                            Err(e) => send(Event::Log(format!("VAT-Spy data update failed: {e}"))),
                                        }
                                    } else {
                                        next_data_update = Instant::now();
                                    }
                                }
                            },
                            _ = tokio::time::sleep(wait) => {}
                        }
                    }
                });
            })
            .ok();
        Self { tx, events: erx }
    }
}
