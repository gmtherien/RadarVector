//! Main application state and top-level UI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use egui::{Color32, RichText};
use egui_dock::{DockArea, DockState, NodeIndex};
use radarvector_core::callsign::Resolved;
use radarvector_core::{NetworkSnapshot, SnapshotDiff};
use radarvector_data::StaticData;
use radarvector_geo::{BBox, LatLon};
use radarvector_render::Basemap;

use crate::backend::{Backend, Command, Event, STATUS_PAGE};
use crate::i18n::{K, set_language, t};
use crate::map_view::{MapContext, MapView, MeasureState, Prepared, Selection};
use crate::search;
use crate::settings::{Bookmark, Favorite, Rgba, Settings, Theme};
use crate::tracon::TraconSet;

/// Dockable tabs.
#[derive(Debug, Clone, PartialEq)]
pub enum Tab {
    Map,
    Controllers,
    Flights,
    Servers,
    Prefiles,
    Metar,
    Details(Selection),
    FilterResults(usize),
}

pub struct ListState {
    pub filter: String,
    pub sort_col: usize,
    pub sort_desc: bool,
    pub hidden: Vec<bool>,
}

impl Default for ListState {
    fn default() -> Self {
        Self {
            filter: String::new(),
            sort_col: 0,
            sort_desc: false,
            hidden: vec![false; 12],
        }
    }
}

#[derive(Default)]
pub struct MetarState {
    pub input: String,
    pub request: u64,
    pub pending: bool,
    pub results: Vec<(String, String)>,
    pub error: Option<String>,
}

pub struct AppState {
    pub settings: Settings,
    saved_settings: String,
    settings_changed_at: Option<Instant>,
    pub static_data: Arc<ArcSwap<StaticData>>,
    pub data_tag: String,
    pub tracons: Arc<TraconSet>,
    pub snapshot: Option<Arc<NetworkSnapshot>>,
    pub backend: Backend,
    pub map: MapView,
    prepared_rx: Option<std::sync::mpsc::Receiver<Prepared>>,
    pub selection: Option<Selection>,
    pub tracks: HashMap<String, Vec<LatLon>>,
    pub refreshing: bool,
    pub last_success: Option<Instant>,
    pub last_error: Option<String>,
    pub log: Vec<String>,
    pub controllers_list: ListState,
    pub flights_list: ListState,
    pub metar: MetarState,
    pub airport_metars: HashMap<String, (u64, Option<String>)>,
    next_request: u64,
    pub jump_text: String,
    pub search_open: bool,
    pub search_text: String,
    pub settings_open: bool,
    pub about_open: bool,
    pub filters_open: bool,
    pub bookmark_prompt: Option<String>,
    pub favorite_confirm: Option<Favorite>,
    pub open_tabs: Vec<Tab>,
    pub focus_tab: Option<Tab>,
    pub highlights: HashMap<String, Rgba>,
    highlights_hash: u64,
    was_minimized: bool,
    pub map_cursor: Option<LatLon>,
    pub frame_times: std::collections::VecDeque<f32>,
    pub persist: bool,
    pending_preview: Option<Selection>,
    preview_tab: Option<Selection>,
    pub app_update: Option<radarvector_feed::appupdate::Available>,
    pub app_update_msg: Option<String>,
}

pub struct RadarVectorApp {
    dock: DockState<Tab>,
    state: AppState,
    dev: crate::devmode::DevMode,
}

const EMBEDDED_DAT: &str = include_str!("../../../assets/vatspy-data/VATSpy.dat");
const EMBEDDED_GEOJSON: &str = include_str!("../../../assets/vatspy-data/Boundaries.geojson");

pub fn data_dir(settings: &Settings) -> PathBuf {
    settings
        .data_dir_override
        .clone()
        .or_else(|| radarvector_feed::datasource::default_data_dir().ok())
        .unwrap_or_else(|| PathBuf::from("vatspy-data"))
}

fn load_static(settings: &Settings) -> (StaticData, String) {
    let dir = data_dir(settings);
    if let Ok((d, report)) = StaticData::load_dir(&dir)
        && report.dat.rejected_lines() == 0
        && report.validation.is_ok()
    {
        let tag = radarvector_feed::datasource::read_manifest(&dir)
            .map(|m| m.tag)
            .unwrap_or_else(|| "local".into());
        return (d, tag);
    }
    let d = StaticData::from_texts(EMBEDDED_DAT, EMBEDDED_GEOJSON)
        .map(|(d, _)| d)
        .unwrap_or_default();
    (d, "v2611.2 (embedded)".into())
}

pub fn apply_style(ctx: &egui::Context, s: &Settings) {
    let mut visuals = match s.theme {
        Theme::Dark => egui::Visuals::dark(),
        Theme::Light => egui::Visuals::light(),
    };
    let c = s.colors();
    visuals.extreme_bg_color = Color32::from_rgb(c.background[0], c.background[1], c.background[2]);
    ctx.set_visuals(visuals);
    ctx.set_zoom_factor(s.font_scale.clamp(0.6, 2.5));
}

impl RadarVectorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, msaa: u32) -> Self {
        Self::with_settings(cc, msaa, Settings::load(), true)
    }

    /// `persist = false` never writes settings (tests).
    pub fn with_settings(cc: &eframe::CreationContext<'_>, msaa: u32, settings: Settings, persist: bool) -> Self {
        set_language(settings.language);
        apply_style(&cc.egui_ctx, &settings);
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            radarvector_render::install(rs, msaa);
        }
        let (data, tag) = load_static(&settings);
        let static_data = Arc::new(ArcSwap::from_pointee(data));
        let tracons = Arc::new(TraconSet::load());

        // Build GPU geometry off the UI thread.
        let (ptx, prx) = std::sync::mpsc::channel();
        {
            let sd = static_data.load_full();
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let p = Prepared::build(&sd, Basemap::embedded());
                let _ = ptx.send(p);
                ctx.request_repaint();
            });
        }

        let ctx = cc.egui_ctx.clone();
        let backend = Backend::start(
            static_data.clone(),
            Arc::new(ArcSwap::from_pointee(tracons.tracons.clone())),
            data_dir(&settings),
            settings.auto_refresh,
            Duration::from_secs(settings.refresh_interval_s),
            move || ctx.request_repaint(),
        );

        let dev = crate::devmode::DevMode::from_env();
        let mut camera = settings.last_camera.unwrap_or_default();
        camera.projection = settings.projection;
        if let Some((lat, lon, span)) = dev.view {
            camera.set_viewport(1000.0, 700.0);
            camera.look_at(LatLon::new(lat, lon), span);
        }
        let mut dock = DockState::new(vec![Tab::Map]);
        let surface = dock.main_surface_mut();
        surface.split_right(NodeIndex::root(), 0.72, vec![Tab::Controllers, Tab::Flights]);

        if settings.auto_check_updates && persist {
            backend.send(Command::CheckAppUpdate { manual: false });
        }
        let saved = settings.to_toml();
        Self {
            dock,
            dev,
            state: AppState {
                settings,
                saved_settings: saved,
                settings_changed_at: None,
                static_data,
                data_tag: tag,
                tracons,
                snapshot: None,
                backend,
                map: MapView::new(camera),
                prepared_rx: Some(prx),
                selection: None,
                tracks: HashMap::new(),
                refreshing: false,
                last_success: None,
                last_error: None,
                log: Vec::new(),
                controllers_list: ListState::default(),
                flights_list: ListState::default(),
                metar: MetarState::default(),
                airport_metars: HashMap::new(),
                next_request: 1,
                jump_text: String::new(),
                search_open: false,
                search_text: String::new(),
                settings_open: false,
                about_open: false,
                filters_open: false,
                bookmark_prompt: None,
                favorite_confirm: None,
                open_tabs: Vec::new(),
                focus_tab: None,
                highlights: HashMap::new(),
                highlights_hash: 0,
                was_minimized: false,
                map_cursor: None,
                frame_times: Default::default(),
                persist,
                pending_preview: None,
                preview_tab: None,
                app_update: None,
                app_update_msg: None,
            },
        }
    }
}

impl AppState {
    pub fn data(&self) -> arc_swap::Guard<Arc<StaticData>> {
        self.static_data.load()
    }

    pub fn request_id(&mut self) -> u64 {
        self.next_request += 1;
        self.next_request
    }

    pub fn request_metar(&mut self, stations: Vec<String>) -> u64 {
        let id = self.request_id();
        self.backend.send(Command::FetchMetar { request: id, stations });
        id
    }

    pub fn open_tab(&mut self, tab: Tab) {
        self.open_tabs.push(tab.clone());
        self.focus_tab = Some(tab);
    }

    /// Single click: show `sel` in the reusable preview details tab.
    pub fn preview(&mut self, sel: Selection) {
        self.selection = Some(sel.clone());
        if let Selection::Airport(icao) = &sel
            && !self.airport_metars.contains_key(icao)
        {
            let id = self.request_metar(vec![icao.clone()]);
            self.airport_metars.insert(icao.clone(), (id, None));
        }
        self.pending_preview = Some(sel);
    }

    pub fn select(&mut self, sel: Selection, open_details: bool) {
        self.selection = Some(sel.clone());
        if open_details {
            if let Selection::Airport(icao) = &sel {
                let id = self.request_metar(vec![icao.clone()]);
                self.airport_metars.insert(icao.clone(), (id, None));
            }
            self.open_tab(Tab::Details(sel));
        }
    }

    /// Geographic position of a selection (centre).
    pub fn position_of(&self, sel: &Selection) -> Option<(LatLon, f64)> {
        let data = self.data();
        match sel {
            Selection::Pilot(cs) => self
                .snapshot
                .as_ref()?
                .pilot_by_callsign(cs)
                .map(|p| (LatLon::new(p.latitude, p.longitude), 6.0)),
            Selection::Airport(icao) => data.airport(icao).map(|a| (LatLon::new(a.lat, a.lon), 3.0)),
            Selection::Fir(id) => {
                let id = data
                    .firs_by_icao(id)
                    .next()
                    .map(|f| f.boundary_id.clone())
                    .unwrap_or_else(|| id.clone());
                let feats = data.boundaries.get(&id);
                let mut b = BBox::empty();
                for f in feats {
                    b.extend(f.bbox.min_lon, f.bbox.min_lat);
                    b.extend(f.bbox.max_lon, f.bbox.max_lat);
                }
                b.is_valid().then(|| {
                    let span = (b.max_lon - b.min_lon).max(b.max_lat - b.min_lat).clamp(2.0, 120.0);
                    (
                        LatLon::new((b.min_lat + b.max_lat) / 2.0, (b.min_lon + b.max_lon) / 2.0),
                        span * 1.3,
                    )
                })
            }
            Selection::Country(prefix) => {
                let mut b = BBox::empty();
                for a in data
                    .airports
                    .iter()
                    .filter(|a| !a.is_pseudo && a.icao.starts_with(prefix.as_str()))
                {
                    b.extend(a.lon, a.lat);
                }
                b.is_valid().then(|| {
                    let span = (b.max_lon - b.min_lon).max(b.max_lat - b.min_lat).clamp(3.0, 120.0);
                    (
                        LatLon::new((b.min_lat + b.max_lat) / 2.0, (b.min_lon + b.max_lon) / 2.0),
                        span * 1.2,
                    )
                })
            }
            Selection::Station(cs) => {
                let s = self.snapshot.as_ref()?.station_by_callsign(cs)?;
                match &s.resolved {
                    Resolved::Airport { icao } | Resolved::Approach { icao, .. } => {
                        self.position_of(&Selection::Airport(icao.clone()))
                    }
                    Resolved::Fir(t) => t
                        .first()
                        .and_then(|t| self.position_of(&Selection::Fir(t.boundary_id.clone()))),
                    Resolved::Uir { firs, .. } => firs
                        .first()
                        .and_then(|t| self.position_of(&Selection::Fir(t.boundary_id.clone()))),
                    Resolved::Unresolved => None,
                }
            }
        }
    }

    pub fn center_on(&mut self, sel: &Selection) {
        if let Some((p, span)) = self.position_of(sel) {
            self.map.camera.look_at(p, span);
            self.focus_tab = Some(Tab::Map);
        }
    }

    pub fn favorite_selection(f: &Favorite) -> Selection {
        match f {
            Favorite::Country(c) => Selection::Country(c.clone()),
            Favorite::Fir(c) => Selection::Fir(c.clone()),
            Favorite::Airport(c) => Selection::Airport(c.clone()),
        }
    }

    fn recompute_highlights(&mut self) {
        let mut h = HashMap::new();
        if let Some(s) = &self.snapshot {
            for f in self.settings.filters.iter() {
                let Some(color) = f.color.as_deref().and_then(parse_hex) else {
                    continue;
                };
                for p in &s.pilots {
                    if f.matches_pilot(p.cid, &p.callsign) {
                        h.insert(p.callsign.clone(), color);
                    }
                }
                for st in &s.stations {
                    if f.matches_controller(st.data.cid, &st.data.callsign) {
                        h.insert(st.data.callsign.clone(), color);
                    }
                }
            }
        }
        let mut keys: Vec<_> = h.iter().map(|(k, v)| (k.clone(), *v)).collect();
        keys.sort();
        self.highlights_hash = {
            use std::hash::{Hash, Hasher};
            let mut hs = std::collections::hash_map::DefaultHasher::new();
            keys.hash(&mut hs);
            hs.finish()
        };
        self.highlights = h;
    }

    fn on_snapshot(&mut self, snap: Arc<NetworkSnapshot>, diff: &SnapshotDiff) {
        // Track history (memory only).
        let alive: std::collections::HashSet<&str> = snap.pilots.iter().map(|p| p.callsign.as_str()).collect();
        self.tracks.retain(|k, _| alive.contains(k.as_str()));
        for p in &snap.pilots {
            let t = self.tracks.entry(p.callsign.clone()).or_default();
            let pos = LatLon::new(p.latitude, p.longitude);
            if t.last() != Some(&pos) {
                t.push(pos);
                if t.len() > 2000 {
                    t.remove(0);
                }
            }
        }
        crate::alerts::check(&self.settings, &self.data(), &snap, diff);
        self.snapshot = Some(snap);
        self.recompute_highlights();
    }

    fn handle_events(&mut self) {
        while let Ok(ev) = self.backend.events.try_recv() {
            match ev {
                Event::RefreshStarted => self.refreshing = true,
                Event::RefreshDone => {
                    self.refreshing = false;
                    self.last_error = None;
                    self.last_success = Some(Instant::now());
                }
                Event::RefreshFailed { error, retry_in } => {
                    self.refreshing = false;
                    self.last_error = Some(format!("{error} (retry in {} s)", retry_in.as_secs()));
                }
                Event::Snapshot(s, diff) => self.on_snapshot(s, &diff),
                Event::Metar { request, result } => {
                    if request == self.metar.request {
                        self.metar.pending = false;
                        match result {
                            Ok(r) => {
                                self.metar.results = r;
                                self.metar.error = None;
                            }
                            Err(e) => self.metar.error = Some(e),
                        }
                    } else if let Ok(r) = result {
                        for (icao, (id, text)) in self.airport_metars.iter_mut() {
                            if *id == request {
                                *text = Some(
                                    r.iter()
                                        .find(|(s, _)| s == icao)
                                        .map(|x| x.1.clone())
                                        .unwrap_or_else(|| "—".into()),
                                );
                            }
                        }
                    }
                }
                Event::StaticDataUpdated { tag } => {
                    self.data_tag = tag.clone();
                    self.log.push(format!("VAT-Spy data {tag} installed"));
                    let sd = self.static_data.load_full();
                    let p = Prepared::build(&sd, Basemap::embedded());
                    self.map.prepared = Some(p);
                }
                Event::AppUpdate { available, manual } => {
                    if available.is_none() && manual {
                        self.app_update_msg = Some(t(K::UpToDate).into());
                    }
                    self.app_update = available;
                }
                Event::AppUpdateFailed(e) => self.app_update_msg = Some(e),
                Event::AppUpdateDownloaded(path) => {
                    self.app_update = None;
                    self.app_update_msg = Some(format!("{}", path.display()));
                    if let Err(e) = crate::updates::open(&path) {
                        self.log.push(format!("cannot open installer: {e}"));
                    }
                }
                Event::TraconUpdated(set) => self.tracons = set,
                Event::Log(l) => {
                    tracing::info!("{l}");
                    self.log.push(l);
                    if self.log.len() > 200 {
                        self.log.remove(0);
                    }
                }
            }
        }
    }

    fn autosave(&mut self) {
        if !self.persist {
            return;
        }
        if std::env::var_os("RADARVECTOR_SCREENSHOT").is_some() || std::env::var_os("RADARVECTOR_BENCH").is_some() {
            return;
        }
        self.settings.last_camera = Some(self.map.camera);
        self.settings.projection = self.map.camera.projection;
        let now = self.settings.to_toml();
        if now != self.saved_settings {
            let changed = *self.settings_changed_at.get_or_insert_with(Instant::now);
            if changed.elapsed() > Duration::from_secs(2) {
                if let Err(e) = self.settings.save() {
                    self.log.push(format!("cannot save settings: {e}"));
                }
                self.saved_settings = now;
                self.settings_changed_at = None;
            }
        }
    }

    pub fn jump_to(&mut self, text: &str) -> bool {
        let q = text.trim().to_ascii_uppercase();
        if q.is_empty() {
            return false;
        }
        let data = self.data();
        let sel = if data.airport(&q).is_some() {
            Some(Selection::Airport(q.clone()))
        } else if data.firs_by_icao(&q).next().is_some() || data.boundaries.contains(&q) {
            Some(Selection::Fir(q.clone()))
        } else if q.len() == 2 && data.country_by_prefix(&q).is_some() {
            Some(Selection::Country(q.clone()))
        } else {
            data.airports_by_iata(&q)
                .find(|a| !a.is_pseudo)
                .map(|a| Selection::Airport(a.icao.clone()))
        };
        drop(data);
        match sel {
            Some(s) => {
                self.center_on(&s);
                self.selection = Some(s);
                true
            }
            None => false,
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let keys = [
            egui::Key::Num0,
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ];
        ctx.input(|i| {
            for (n, k) in keys.iter().enumerate() {
                if i.key_pressed(*k) && i.modifiers.command {
                    if i.modifiers.alt {
                        self.settings.hotkey_bookmarks.insert(
                            n.to_string(),
                            Bookmark {
                                name: format!("{n}"),
                                camera: self.map.camera,
                            },
                        );
                        self.log.push(format!("bookmark {n} set"));
                    } else if let Some(b) = self.settings.hotkey_bookmarks.get(&n.to_string()) {
                        self.map.camera.center = b.camera.center;
                        self.map.camera.scale = b.camera.scale;
                        self.focus_tab = Some(Tab::Map);
                    }
                }
            }
            if i.modifiers.command && i.key_pressed(egui::Key::K) {
                self.search_open = true;
            }
            if i.modifiers.command && i.key_pressed(egui::Key::R) {
                self.backend.send(Command::RefreshNow);
            }
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.refreshing, egui::Button::new(format!("⟳ {}", t(K::Refresh))))
                .clicked()
            {
                self.backend.send(Command::RefreshNow);
            }
            if ui
                .add_enabled(self.refreshing, egui::Button::new(format!("■ {}", t(K::Stop))))
                .clicked()
            {
                self.backend.send(Command::Stop);
            }
            let mut auto = self.settings.auto_refresh;
            let mut secs = self.settings.refresh_interval_s;
            ui.checkbox(&mut auto, t(K::AutoRefresh));
            ui.add(egui::DragValue::new(&mut secs).range(15..=3600).suffix(" s"));
            if auto != self.settings.auto_refresh || secs != self.settings.refresh_interval_s {
                self.settings.auto_refresh = auto;
                self.settings.refresh_interval_s = secs.max(15);
                self.backend.send(Command::SetAuto {
                    enabled: auto,
                    interval: Duration::from_secs(self.settings.refresh_interval_s),
                });
            }
            ui.separator();

            // Jump box with suggestions.
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.jump_text)
                    .hint_text(t(K::Jump))
                    .desired_width(110.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter {
                let txt = self.jump_text.clone();
                if self.jump_to(&txt) {
                    self.jump_text.clear();
                }
            }
            if resp.has_focus() && !self.jump_text.trim().is_empty() {
                let sugg = search::jump_suggestions(&self.data(), &self.jump_text, 8);
                let popup_id = ui.make_persistent_id("jump_suggestions");
                egui::Area::new(popup_id)
                    .fixed_pos(resp.rect.left_bottom())
                    .order(egui::Order::Foreground)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            for (code, label) in sugg {
                                if ui.selectable_label(false, format!("{code}  {label}")).clicked() {
                                    self.jump_to(&code);
                                    self.jump_text.clear();
                                }
                            }
                        });
                    });
            }
            if ui.button("🔍").on_hover_text(t(K::Search)).clicked() {
                self.search_open = true;
            }
            ui.separator();

            let measuring = self.map.measure != MeasureState::Off;
            if ui
                .selectable_label(measuring, format!("📏 {}", t(K::Measure)))
                .clicked()
            {
                self.map.measure = if measuring {
                    MeasureState::Off
                } else {
                    MeasureState::Armed
                };
            }

            // Bookmarks.
            ui.menu_button(format!("🔖 {}", t(K::Bookmarks)), |ui| {
                let mut go = None;
                let mut del = None;
                for (i, b) in self.settings.bookmarks.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.button(&b.name).clicked() {
                            go = Some(b.camera);
                            ui.close();
                        }
                        if ui.small_button("✕").clicked() {
                            del = Some(i);
                        }
                    });
                }
                for (n, b) in self.settings.hotkey_bookmarks.iter() {
                    if ui.button(format!("Ctrl+{n}  {}", b.name)).clicked() {
                        go = Some(b.camera);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t(K::AddBookmark)).clicked() {
                    self.bookmark_prompt = Some(String::new());
                    ui.close();
                }
                if let Some(c) = go {
                    self.map.camera.center = c.center;
                    self.map.camera.scale = c.scale;
                }
                if let Some(i) = del {
                    self.settings.bookmarks.remove(i);
                }
            });

            // Favourites.
            ui.menu_button(format!("★ {}", t(K::Favorites)), |ui| {
                let mut go = None;
                let mut del = None;
                for (i, f) in self.settings.favorites.iter().enumerate() {
                    let label = match f {
                        Favorite::Country(c) => format!("{}: {c}", t(K::Country)),
                        Favorite::Fir(c) => format!("{}: {c}", t(K::Fir)),
                        Favorite::Airport(c) => format!("{}: {c}", t(K::Airport)),
                    };
                    ui.horizontal(|ui| {
                        if ui.button(label).clicked() {
                            go = Some(Self::favorite_selection(f));
                            ui.close();
                        }
                        if ui.small_button("✕").on_hover_text(t(K::RemoveFavorite)).clicked() {
                            del = Some(i);
                        }
                    });
                }
                if let Some(s) = go {
                    self.center_on(&s);
                }
                if let Some(i) = del {
                    self.settings.favorites.remove(i);
                }
            });

            // Filters.
            ui.menu_button(format!("⚑ {}", t(K::Filters)), |ui| {
                let mut open = None;
                for (i, f) in self.settings.filters.iter().enumerate() {
                    if ui.button(&f.name).clicked() {
                        open = Some(i);
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button(t(K::EditFilters)).clicked() {
                    self.filters_open = true;
                    ui.close();
                }
                if let Some(i) = open {
                    self.open_tab(Tab::FilterResults(i));
                }
            });
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(s) = &self.snapshot {
                let ts = s.general.update_timestamp.get(11..19).unwrap_or("");
                ui.label(format!("{ts}Z"));
                ui.separator();
                ui.label(format!("{} {}", s.general.connected_clients, t(K::Clients)));
                ui.label(format!("✈ {}", s.pilots.len()));
                ui.label(format!("📡 {}", s.stations.len()));
                let unresolved: Vec<_> = s.unresolved().map(|st| st.data.callsign.clone()).collect();
                if !unresolved.is_empty() {
                    ui.label(RichText::new(format!("{} {}", unresolved.len(), t(K::Unresolved))).weak())
                        .on_hover_text(unresolved.join("\n"));
                }
            }
            if self.refreshing {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let avg = if self.frame_times.is_empty() {
                    0.0
                } else {
                    self.frame_times.iter().sum::<f32>() / self.frame_times.len() as f32
                };
                ui.label(RichText::new(format!("{:.1} ms", avg)).weak().small());
                ui.label(RichText::new(format!("VAT-Spy data {}", self.data_tag)).weak().small());
                if let Some(c) = self.map_cursor {
                    ui.label(RichText::new(format!("{:.4} {:.4}", c.lat, c.lon)).weak().small());
                }
            });
        });
    }
}

impl AppState {
    fn banner(&mut self, ui: &mut egui::Ui) {
        if let Some(u) = self.app_update.clone() {
            egui::Frame::new()
                .fill(Color32::from_rgb(20, 70, 110))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            Color32::WHITE,
                            format!("⬆ {}: RadarVector {}", t(K::UpdateAvailable), u.version),
                        );
                        if ui.button(t(K::Install)).clicked() {
                            self.backend.send(Command::InstallAppUpdate(u.clone()));
                            self.app_update = None;
                        }
                        if ui.small_button("✕").clicked() {
                            self.app_update = None;
                        }
                    });
                });
        }
        if let Some(msg) = self.app_update_msg.clone() {
            ui.horizontal(|ui| {
                ui.label(msg);
                if ui.small_button("✕").clicked() {
                    self.app_update_msg = None;
                }
            });
        }
        if let Some(err) = self.last_error.clone() {
            egui::Frame::new()
                .fill(Color32::from_rgb(120, 40, 20))
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.colored_label(Color32::WHITE, format!("⚠ {}: {err}", t(K::DownloadError)));
                        if let Some(at) = self.last_success {
                            let min = at.elapsed().as_secs() / 60;
                            ui.colored_label(Color32::WHITE, t(K::StaleData).replace("%N", &min.to_string()));
                        }
                        ui.hyperlink_to(t(K::NetworkStatus), STATUS_PAGE);
                        if ui.small_button("✕").clicked() {
                            self.last_error = None;
                        }
                    });
                });
        }
    }
}

/// Map cursor position (shown in the status bar).
impl AppState {
    #[allow(clippy::needless_lifetimes)]
    fn map_ctx<'a>(&'a self, data: &'a StaticData) -> MapContext<'a> {
        MapContext {
            data,
            snapshot: self.snapshot.as_ref(),
            settings: &self.settings,
            selection: self.selection.as_ref(),
            tracks: &self.tracks,
            tracons: Some(&self.tracons),
            highlights: &self.highlights,
            highlights_hash: self.highlights_hash,
        }
    }
}

pub fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255])
}

pub fn to_hex(c: Rgba) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

struct Viewer<'a> {
    s: &'a mut AppState,
}

impl egui_dock::TabViewer for Viewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> egui::Id {
        egui::Id::new(format!("{tab:?}"))
    }

    fn title(&mut self, tab: &mut Tab) -> egui::WidgetText {
        match tab {
            Tab::Map => t(K::Map).into(),
            Tab::Controllers => t(K::Controllers).into(),
            Tab::Flights => t(K::Flights).into(),
            Tab::Servers => t(K::Servers).into(),
            Tab::Prefiles => t(K::Prefiles).into(),
            Tab::Metar => t(K::Metar).into(),
            Tab::Details(sel) => match sel {
                Selection::Pilot(c)
                | Selection::Station(c)
                | Selection::Airport(c)
                | Selection::Fir(c)
                | Selection::Country(c) => c.clone().into(),
            },
            Tab::FilterResults(i) => self
                .s
                .settings
                .filters
                .get(*i)
                .map(|f| format!("⚑ {}", f.name))
                .unwrap_or_default()
                .into(),
        }
    }

    fn is_closeable(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Map)
    }

    fn scroll_bars(&self, tab: &Tab) -> [bool; 2] {
        match tab {
            Tab::Map | Tab::Controllers | Tab::Flights => [false, false],
            _ => [false, true],
        }
    }

    fn clear_background(&self, tab: &Tab) -> bool {
        !matches!(tab, Tab::Map)
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Tab) {
        match tab {
            Tab::Map => {
                let data = self.s.data_full();
                let mut map = std::mem::replace(&mut self.s.map, MapView::new(Default::default()));
                let resp = {
                    let ctx = self.s.map_ctx(&data);
                    map.show(ui, &ctx)
                };
                self.s.map = map;
                self.s.map_cursor = resp.cursor;
                if let Some(sel) = resp.clicked {
                    self.s.preview(sel);
                }
                if let Some(sel) = resp.double_clicked {
                    self.s.select(sel, true);
                }
                if let Some(sel) = resp.context_menu {
                    self.s.select(sel, true);
                }
            }
            Tab::Controllers => crate::tabs::controllers(ui, self.s),
            Tab::Flights => crate::tabs::flights(ui, self.s),
            Tab::Servers => crate::tabs::servers(ui, self.s),
            Tab::Prefiles => crate::tabs::prefiles(ui, self.s),
            Tab::Metar => crate::tabs::metar(ui, self.s),
            Tab::Details(sel) => crate::tabs::details(ui, self.s, sel),
            Tab::FilterResults(i) => crate::tabs::filter_results(ui, self.s, *i),
        }
    }
}

impl AppState {
    pub fn data_full(&self) -> Arc<StaticData> {
        self.static_data.load_full()
    }
}

impl eframe::App for RadarVectorApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        let t0 = Instant::now();
        let s = &mut self.state;
        if let Some(rx) = &s.prepared_rx
            && let Ok(p) = rx.try_recv()
        {
            s.map.prepared = Some(p);
            s.prepared_rx = None;
        }
        s.handle_events();
        s.handle_shortcuts(ctx);
        if self.dev.active() {
            dev_frame(&mut self.dev, s, ctx);
        }

        // REF-03: refresh when restored from minimised.
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        if s.was_minimized && !minimized {
            s.backend.send(Command::RefreshNow);
        }
        s.was_minimized = minimized;

        egui::Panel::top("menu").show(root, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button(t(K::File), |ui| {
                    if ui.button(t(K::Settings)).clicked() {
                        s.settings_open = true;
                        ui.close();
                    }
                    if ui.button(t(K::UpdateData)).clicked() {
                        s.backend.send(Command::UpdateData { force: true });
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t(K::Quit)).clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button(t(K::View), |ui| {
                    for tab in [
                        Tab::Map,
                        Tab::Controllers,
                        Tab::Flights,
                        Tab::Servers,
                        Tab::Prefiles,
                        Tab::Metar,
                    ] {
                        let name = match tab {
                            Tab::Map => t(K::Map),
                            Tab::Controllers => t(K::Controllers),
                            Tab::Flights => t(K::Flights),
                            Tab::Servers => t(K::Servers),
                            Tab::Prefiles => t(K::Prefiles),
                            _ => t(K::Metar),
                        };
                        if ui.button(name).clicked() {
                            s.open_tab(tab);
                            ui.close();
                        }
                    }
                    ui.separator();
                    let mut dark = s.settings.theme == Theme::Dark;
                    if ui.checkbox(&mut dark, t(K::Dark)).changed() {
                        s.settings.theme = if dark { Theme::Dark } else { Theme::Light };
                        apply_style(ctx, &s.settings);
                    }
                    ui.checkbox(&mut s.settings.hide_unstaffed, t(K::HideUnstaffed));
                    ui.checkbox(&mut s.settings.show_aircraft, t(K::Flights));
                    ui.checkbox(
                        &mut s.settings.show_aircraft_labels,
                        format!("{} ({})", t(K::Labels), t(K::Flights)),
                    );
                    ui.checkbox(&mut s.settings.show_fir_labels, format!("{} (FIR)", t(K::Labels)));
                    ui.checkbox(
                        &mut s.settings.show_airport_labels,
                        format!("{} ({})", t(K::Labels), t(K::Airports)),
                    );
                    ui.checkbox(&mut s.settings.show_tracon_polygons, "TRACON");
                    ui.separator();
                    let mut merc = s.map.camera.projection == radarvector_geo::Projection::Mercator;
                    if ui.checkbox(&mut merc, "Mercator").changed() {
                        s.map.camera.set_projection(if merc {
                            radarvector_geo::Projection::Mercator
                        } else {
                            radarvector_geo::Projection::Equirectangular
                        });
                    }
                });
                ui.menu_button(t(K::Tools), |ui| {
                    if ui.button(t(K::Measure)).clicked() {
                        s.map.measure = MeasureState::Armed;
                        ui.close();
                    }
                    if ui.button(t(K::Metar)).clicked() {
                        s.open_tab(Tab::Metar);
                        ui.close();
                    }
                    if ui.button(t(K::Search)).clicked() {
                        s.search_open = true;
                        ui.close();
                    }
                });
                ui.menu_button(t(K::Help), |ui| {
                    if ui.button(t(K::CheckUpdates)).clicked() {
                        s.backend.send(Command::CheckAppUpdate { manual: true });
                        ui.close();
                    }
                    if ui.button(t(K::About)).clicked() {
                        s.about_open = true;
                        ui.close();
                    }
                    ui.hyperlink_to(t(K::NetworkStatus), STATUS_PAGE);
                });
            });
            s.toolbar(ui);
        });
        egui::Panel::bottom("status").show(root, |ui| s.status_bar(ui));

        // Open requested tabs (avoid duplicates; focus existing).
        if let Some(sel) = s.pending_preview.take() {
            let tab = Tab::Details(sel.clone());
            if self.dock.find_tab(&tab).is_none() {
                let old = s
                    .preview_tab
                    .clone()
                    .map(Tab::Details)
                    .and_then(|t| self.dock.find_tab(&t));
                match old {
                    Some(path) => {
                        if let Some(leaf) = self.dock[path.surface][path.node].get_leaf_mut()
                            && let Some(slot) = leaf.tabs.get_mut(path.tab.0)
                        {
                            *slot = tab.clone();
                        }
                    }
                    None => add_tab(&mut self.dock, tab.clone()),
                }
            }
            s.preview_tab = Some(sel);
            s.focus_tab = Some(tab);
        }
        for tab in std::mem::take(&mut s.open_tabs) {
            if self.dock.find_tab(&tab).is_none() {
                add_tab(&mut self.dock, tab);
            }
        }
        if let Some(tab) = s.focus_tab.take()
            && let Some(path) = self.dock.find_tab(&tab)
        {
            let _ = self.dock.set_active_tab(path);
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(root, |ui| {
                s.banner(ui);
                DockArea::new(&mut self.dock)
                    .show_leaf_collapse_buttons(false)
                    .show_leaf_close_all_buttons(false)
                    .show_inside(ui, &mut Viewer { s });
            });

        crate::tabs::windows(ctx, s);
        s.autosave();
        s.frame_times.push_back(t0.elapsed().as_secs_f32() * 1000.0);
        if s.frame_times.len() > 60 {
            s.frame_times.pop_front();
        }
    }

    fn on_exit(&mut self) {
        if self.dev.active() || !self.state.persist {
            return;
        }
        self.state.settings.last_camera = Some(self.state.map.camera);
        let _ = self.state.settings.save();
    }
}

fn dev_frame(dev: &mut crate::devmode::DevMode, s: &mut AppState, ctx: &egui::Context) {
    ctx.request_repaint();
    if dev.start.elapsed() < Duration::from_millis(500) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }
    if let Some(n) = dev.bench {
        if s.map.prepared.is_none() {
            return;
        }
        if dev.dts.is_empty() && !dev.requested {
            dev.requested = true;
            dev.start = Instant::now();
            let data = s.data_full();
            s.snapshot = Some(Arc::new(NetworkSnapshot::build(
                &data,
                crate::devmode::synthetic_feed(n),
            )));
            s.settings.show_aircraft_labels = true;
        }
        let tsec = dev.bench_elapsed().as_secs_f64();
        // Pan around the world and zoom in/out.
        s.map.camera.center = [(tsec * 36.0) % 360.0 - 180.0, 20.0 * (tsec * 0.6).sin()];
        let span = 60.0 + 280.0 * (0.5 + 0.5 * (tsec * 0.8).cos());
        s.map.camera.set_span(span);
        dev.dts.push(ctx.input(|i| i.unstable_dt) * 1000.0);
        if tsec > 10.0 {
            let mut v = dev.dts[10..].to_vec();
            v.sort_by(f32::total_cmp);
            let avg = v.iter().sum::<f32>() / v.len() as f32;
            let p95 = v[(v.len() as f32 * 0.95) as usize];
            let cpu = s.frame_times.iter().sum::<f32>() / s.frame_times.len().max(1) as f32;
            println!(
                "BENCH aircraft={n} frames={} avg_frame_ms={avg:.2} p95_ms={p95:.2} fps={:.1} ui_cpu_ms={cpu:.2}",
                v.len(),
                1000.0 / avg
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        return;
    }
    if let Some(path) = dev.screenshot.clone() {
        let ready = s.map.prepared.is_some() && s.snapshot.is_some();
        if ready && !dev.requested && dev.start.elapsed() > Duration::from_secs(2) {
            dev.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let img = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = img {
            match crate::devmode::save_png(&path, &img) {
                Ok(()) => println!("SCREENSHOT {}", path.display()),
                Err(e) => eprintln!("screenshot failed: {e}"),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if dev.start.elapsed() > Duration::from_secs(60) {
            eprintln!(
                "screenshot timeout: prepared={} snapshot={} requested={}",
                s.map.prepared.is_some(),
                s.snapshot.is_some(),
                dev.requested
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[cfg(test)]
impl RadarVectorApp {
    pub fn state(&mut self) -> &mut AppState {
        &mut self.state
    }

    pub fn details_tabs(&self) -> usize {
        self.dock
            .iter_all_tabs()
            .filter(|(_, t)| matches!(t, Tab::Details(_)))
            .count()
    }
}

/// Add a tab next to the lists (never on top of the map); recreate a side
/// leaf if the user closed every other tab.
fn add_tab(dock: &mut DockState<Tab>, tab: Tab) {
    let target = dock
        .iter_all_tabs()
        .find(|(_, t)| !matches!(t, Tab::Map))
        .map(|(p, _)| p);
    if let Some(p) = target
        && let Some(leaf) = dock[p.surface][p.node].get_leaf_mut()
    {
        leaf.append_tab(tab);
        return;
    }
    dock.main_surface_mut().split_right(NodeIndex::root(), 0.72, vec![tab]);
}
