//! The map: GPU layers, interaction, picking, labels and tooltips.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use radarvector_core::NetworkSnapshot;
use radarvector_core::callsign::Resolved;
use radarvector_data::StaticData;
use radarvector_geo::{Camera, LatLon, distance_nm, initial_bearing_true, point_in_multipolygon};
use radarvector_render::geometry::{FirGeometry, Mesh, circle_segments, tessellate};
use radarvector_render::{AircraftInstance, Basemap, Layer, MapCallback, MapFrame, SegmentInstance};

use crate::settings::{Colors, Rgba, Settings};
use crate::tracon::TraconSet;

/// Something the user can select on the map or in a list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Selection {
    Pilot(String),
    Station(String),
    Airport(String),
    Fir(String),
    Country(String),
}

/// Prepared static geometry (built off the UI thread).
pub struct Prepared {
    pub land: Layer<Mesh>,
    pub base_lines_src: Basemap,
    pub fir: Arc<FirGeometry>,
    pub static_ptr: usize,
}

impl Prepared {
    pub fn build(data: &StaticData, basemap: Basemap) -> Self {
        let mut land = Mesh::default();
        tessellate(&basemap.land, [255, 255, 255, 255], &mut land);
        Self {
            land: Layer::new(land),
            base_lines_src: basemap,
            fir: Arc::new(radarvector_render::geometry::build_fir_geometry(data)),
            static_ptr: data as *const StaticData as usize,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum MeasureState {
    #[default]
    Off,
    Armed,
    From(LatLon),
    Done(LatLon, LatLon),
}

/// Inputs that, when changed, require a layer rebuild.
#[derive(Debug, Clone, PartialEq, Default)]
struct LayerKey {
    snapshot: usize,
    static_ptr: usize,
    style: u64,
    selection: Option<Selection>,
    measure: Option<(u64, u64, u64, u64)>,
    tracks: usize,
}

pub struct MapView {
    pub camera: Camera,
    pub prepared: Option<Prepared>,
    pub measure: MeasureState,
    base_lines: Layer<Vec<SegmentInstance>>,
    fir_fill: Layer<Mesh>,
    fir_lines: Layer<Vec<SegmentInstance>>,
    overlay: Layer<Vec<SegmentInstance>>,
    aircraft: Layer<Vec<AircraftInstance>>,
    key_fir: LayerKey,
    key_aircraft: LayerKey,
    key_overlay: LayerKey,
    key_base: u64,
    pub last_frame_ms: f32,
}

fn c32(c: Rgba) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn hash_of(v: &impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn style_hash(s: &Settings, filters_hash: u64) -> u64 {
    hash_of(&(
        format!("{:?}", s.colors()),
        s.hide_unstaffed,
        s.show_idl,
        s.aircraft_size.to_bits(),
        s.show_aircraft,
        s.show_ground_traffic,
        (s.approach_radius_nm * 10.0) as i64,
        s.show_tracon_polygons,
        filters_hash,
    ))
}

/// Per-frame context the map needs from the app.
pub struct MapContext<'a> {
    pub data: &'a StaticData,
    pub snapshot: Option<&'a Arc<NetworkSnapshot>>,
    pub settings: &'a Settings,
    pub selection: Option<&'a Selection>,
    pub tracks: &'a HashMap<String, Vec<LatLon>>,
    pub tracons: Option<&'a TraconSet>,
    /// Highlight colour for each pilot / station callsign matched by a filter.
    pub highlights: &'a HashMap<String, Rgba>,
    pub highlights_hash: u64,
}

pub struct MapResponse {
    pub clicked: Option<Selection>,
    pub double_clicked: Option<Selection>,
    pub context_menu: Option<Selection>,
    pub cursor: Option<LatLon>,
}

impl MapView {
    pub fn new(camera: Camera) -> Self {
        Self {
            camera,
            prepared: None,
            measure: MeasureState::Off,
            base_lines: Layer::default(),
            fir_fill: Layer::default(),
            fir_lines: Layer::default(),
            overlay: Layer::default(),
            aircraft: Layer::default(),
            key_fir: LayerKey::default(),
            key_aircraft: LayerKey::default(),
            key_overlay: LayerKey::default(),
            key_base: 0,
            last_frame_ms: 0.0,
        }
    }

    fn rebuild_layers(&mut self, ctx: &MapContext) {
        let Some(prep) = &self.prepared else { return };
        let colors = ctx.settings.colors();
        let style = style_hash(ctx.settings, ctx.highlights_hash);
        let snap_ptr = ctx.snapshot.map_or(0, |s| Arc::as_ptr(s) as usize);

        // Coastlines / borders / land colour.
        let base_key = hash_of(&(
            format!("{:?}{:?}{:?}", colors.coastline, colors.borders, colors.land),
            ctx.settings.show_idl,
        ));
        if base_key != self.key_base {
            self.key_base = base_key;
            let mut v = Vec::new();
            for (a, b) in &prep.base_lines_src.borders {
                v.push(SegmentInstance {
                    a: *a,
                    b: *b,
                    color: colors.borders,
                    width: 0.7,
                });
            }
            for (a, b) in &prep.base_lines_src.coastlines {
                v.push(SegmentInstance {
                    a: *a,
                    b: *b,
                    color: colors.coastline,
                    width: 1.0,
                });
            }
            if ctx.settings.show_idl {
                for w in ctx.data.idl.windows(2) {
                    v.push(SegmentInstance {
                        a: [w[0].lon as f32, w[0].lat as f32],
                        b: [w[1].lon as f32, w[1].lat as f32],
                        color: colors.idl,
                        width: 1.0,
                    });
                }
            }
            self.base_lines = Layer::new(v);
            let mut land = (*prep.land.data).clone();
            land.vertices.iter_mut().for_each(|x| x.color = colors.land);
            self.prepared_land_recolor(land);
        }
        let Some(prep) = &self.prepared else { return };

        // FIR fills and outlines.
        let key = LayerKey {
            snapshot: snap_ptr,
            static_ptr: prep.static_ptr,
            style,
            ..Default::default()
        };
        if key != self.key_fir {
            self.key_fir = key.clone();
            let mut fill = prep.fir.fill.clone();
            let mut inactive = Vec::new();
            let mut active = Vec::new();
            for f in &prep.fir.features {
                let act = ctx
                    .snapshot
                    .and_then(|s| s.boundaries.get(&f.boundary_id))
                    .filter(|a| a.lights(f.oceanic));
                let (fill_c, line_c, width) = match act {
                    Some(a) if a.via_uir_only => (colors.uir_active_fill, colors.uir_active, 1.6),
                    Some(_) => (colors.fir_active_fill, colors.fir_active, 1.8),
                    None => ([0, 0, 0, 0], colors.fir_inactive, 0.8),
                };
                for v in &mut fill.vertices[f.fill_vertices.clone()] {
                    v.color = fill_c;
                }
                let hidden = act.is_none() && (ctx.settings.hide_unstaffed || f.subsector);
                if hidden {
                    continue;
                }
                let target = if act.is_some() { &mut active } else { &mut inactive };
                for (a, b) in &f.segments {
                    target.push(SegmentInstance {
                        a: *a,
                        b: *b,
                        color: line_c,
                        width,
                    });
                }
            }
            inactive.extend(active);
            self.fir_fill = Layer::new(fill);
            self.fir_lines = Layer::new(inactive);
        }

        // Aircraft.
        let key = LayerKey {
            snapshot: snap_ptr,
            style,
            selection: ctx.selection.cloned(),
            ..Default::default()
        };
        if key != self.key_aircraft {
            self.key_aircraft = key.clone();
            let mut v = Vec::new();
            if let Some(s) = ctx.snapshot
                && ctx.settings.show_aircraft
            {
                for p in &s.pilots {
                    let on_ground = p.groundspeed < 40 && p.altitude < 15_000;
                    if on_ground && !ctx.settings.show_ground_traffic {
                        continue;
                    }
                    let selected = matches!(ctx.selection, Some(Selection::Pilot(c)) if c == &p.callsign);
                    let color = if selected {
                        colors.aircraft_selected
                    } else {
                        ctx.highlights.get(&p.callsign).copied().unwrap_or(colors.aircraft)
                    };
                    let size = ctx.settings.aircraft_size
                        * if selected {
                            1.4
                        } else if on_ground {
                            0.7
                        } else {
                            1.0
                        };
                    v.push(AircraftInstance {
                        pos: [p.longitude as f32, p.latitude as f32],
                        heading: p.heading as f32,
                        size,
                        color,
                    });
                }
            }
            self.aircraft = Layer::new(v);
        }

        // Overlay: approach circles / TRACONs, selected flight route + track, measurement.
        let measure_key = match &self.measure {
            MeasureState::From(a) => Some((a.lat.to_bits(), a.lon.to_bits(), 0, 0)),
            MeasureState::Done(a, b) => Some((a.lat.to_bits(), a.lon.to_bits(), b.lat.to_bits(), b.lon.to_bits())),
            _ => None,
        };
        let tracks_len = match ctx.selection {
            Some(Selection::Pilot(c)) => ctx.tracks.get(c).map_or(0, Vec::len),
            _ => 0,
        };
        let key = LayerKey {
            snapshot: snap_ptr,
            static_ptr: prep.static_ptr,
            style,
            selection: ctx.selection.cloned(),
            measure: measure_key,
            tracks: tracks_len,
        };
        if key != self.key_overlay {
            self.key_overlay = key;
            let mut v = Vec::new();
            if let Some(s) = ctx.snapshot {
                for st in &s.stations {
                    if let Resolved::Approach { icao, tracon } = &st.resolved {
                        let drawn = ctx.settings.show_tracon_polygons
                            && tracon.as_ref().is_some_and(|t| {
                                ctx.tracons.is_some_and(|ts| {
                                    let segs = ts.segments(t, st.kind);
                                    let any = !segs.is_empty();
                                    v.extend(segs.into_iter().map(|(a, b)| SegmentInstance {
                                        a,
                                        b,
                                        color: colors.airport_approach,
                                        width: 1.5,
                                    }));
                                    any
                                })
                            });
                        if !drawn && let Some(a) = ctx.data.airport(icao) {
                            for (p, q) in circle_segments(a.lat, a.lon, ctx.settings.approach_radius_nm, 64) {
                                v.push(SegmentInstance {
                                    a: p,
                                    b: q,
                                    color: colors.airport_approach,
                                    width: 1.5,
                                });
                            }
                        }
                    }
                }
                if let Some(Selection::Pilot(cs)) = ctx.selection
                    && let Some(p) = s.pilot_by_callsign(cs)
                {
                    let pos = LatLon::new(p.latitude, p.longitude);
                    if let Some(fp) = &p.flight_plan {
                        let mut route = |from: LatLon, to: LatLon, alpha: u8| {
                            let pts = radarvector_geo::great_circle_points(from, to, 64);
                            let mut c = colors.route;
                            c[3] = alpha;
                            for w in pts.windows(2) {
                                v.push(SegmentInstance {
                                    a: [w[0].lon as f32, w[0].lat as f32],
                                    b: [w[1].lon as f32, w[1].lat as f32],
                                    color: c,
                                    width: 1.5,
                                });
                            }
                        };
                        if let Some(d) = ctx.data.airport(&fp.departure) {
                            route(LatLon::new(d.lat, d.lon), pos, 120);
                        }
                        if let Some(a) = ctx.data.airport(&fp.arrival) {
                            route(pos, LatLon::new(a.lat, a.lon), 230);
                        }
                    }
                    if let Some(track) = ctx.tracks.get(cs) {
                        for w in track.windows(2) {
                            let mut b = w[1].lon;
                            while b - w[0].lon > 180.0 {
                                b -= 360.0;
                            }
                            while b - w[0].lon < -180.0 {
                                b += 360.0;
                            }
                            v.push(SegmentInstance {
                                a: [w[0].lon as f32, w[0].lat as f32],
                                b: [b as f32, w[1].lat as f32],
                                color: colors.aircraft_selected,
                                width: 2.0,
                            });
                        }
                    }
                }
            }
            if let MeasureState::Done(a, b) = &self.measure {
                let pts = radarvector_geo::great_circle_points(*a, *b, 64);
                for w in pts.windows(2) {
                    v.push(SegmentInstance {
                        a: [w[0].lon as f32, w[0].lat as f32],
                        b: [w[1].lon as f32, w[1].lat as f32],
                        color: colors.measure,
                        width: 1.5,
                    });
                }
            }
            self.overlay = Layer::new(v);
        }
    }

    fn prepared_land_recolor(&mut self, land: Mesh) {
        if let Some(p) = &mut self.prepared {
            p.land = Layer::new(land);
        }
    }

    /// Rebuild layers if needed and return the GPU frame for the current camera.
    pub fn frame(&mut self, ctx: &MapContext) -> Option<MapFrame> {
        self.rebuild_layers(ctx);
        let prep = self.prepared.as_ref()?;
        Some(MapFrame {
            camera: self.camera,
            land: prep.land.clone(),
            fir_fill: self.fir_fill.clone(),
            base_lines: self.base_lines.clone(),
            fir_lines: self.fir_lines.clone(),
            overlay_lines: self.overlay.clone(),
            aircraft: self.aircraft.clone(),
        })
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &MapContext) -> MapResponse {
        let t0 = std::time::Instant::now();
        let colors = ctx.settings.colors().clone();
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.camera.set_viewport(rect.width() as f64, rect.height() as f64);
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, c32(colors.background));

        // Input: pan with the primary button (secondary if inverted), zoom with the wheel.
        let pan_button = if ctx.settings.invert_mouse_buttons {
            egui::PointerButton::Secondary
        } else {
            egui::PointerButton::Primary
        };
        if response.dragged_by(pan_button) {
            let d = response.drag_delta();
            self.camera.pan([d.x as f64, d.y as f64]);
        }
        let hover_pos = response.hover_pos();
        if let Some(hp) = hover_pos {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = (scroll as f64 / 200.0).exp() * zoom as f64;
            if (factor - 1.0).abs() > 1e-4 {
                let local = hp - rect.min;
                self.camera.zoom_at([local.x as f64, local.y as f64], factor);
            }
        }
        let to_screen = |cam: &Camera, p: LatLon| -> Pos2 {
            let s = cam.to_screen(p);
            rect.min + Vec2::new(s[0] as f32, s[1] as f32)
        };
        let cursor = hover_pos.map(|hp| {
            let l = hp - rect.min;
            self.camera.from_screen([l.x as f64, l.y as f64])
        });

        if let Some(frame) = self.frame(ctx) {
            painter.add(egui_wgpu::Callback::new_paint_callback(rect, MapCallback(frame)));
        } else {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "…",
                FontId::proportional(24.0),
                c32(colors.fir_label),
            );
        }

        // Labels, airport symbols.
        let mut placer = LabelPlacer::new(rect);
        let font = FontId::proportional(11.0 * ctx.settings.font_scale);
        let small = FontId::proportional(10.0 * ctx.settings.font_scale);
        let cam = self.camera;
        let in_view = |p: Pos2| rect.expand(20.0).contains(p);
        let mut airport_hits: Vec<(Pos2, String)> = Vec::new();

        if let Some(s) = ctx.snapshot {
            // Selected element label first.
            if let Some(Selection::Pilot(cs)) = ctx.selection
                && let Some(p) = s.pilot_by_callsign(cs)
            {
                let pos = to_screen(&cam, LatLon::new(p.latitude, p.longitude));
                placer.place(
                    &painter,
                    pos + Vec2::new(8.0, -8.0),
                    Align2::LEFT_BOTTOM,
                    cs,
                    &font,
                    c32(colors.aircraft_selected),
                    true,
                );
            }
            // Active airports.
            let mut airports: Vec<_> = s.airports.iter().collect();
            airports.sort_by_key(|(icao, _)| icao.as_str());
            for (icao, act) in airports {
                let Some(a) = ctx.data.airport(icao) else { continue };
                let p = to_screen(&cam, LatLon::new(a.lat, a.lon));
                if !in_view(p) {
                    continue;
                }
                draw_airport_symbol(&painter, p, act, &colors);
                airport_hits.push((p, icao.clone()));
                if ctx.settings.show_airport_labels {
                    placer.place(
                        &painter,
                        p + Vec2::new(0.0, 7.0),
                        Align2::CENTER_TOP,
                        icao,
                        &small,
                        c32(colors.airport_label),
                        false,
                    );
                }
            }
            // FIR labels.
            if ctx.settings.show_fir_labels
                && let Some(prep) = &self.prepared
            {
                let mut seen = std::collections::HashSet::new();
                for f in &prep.fir.features {
                    let Some(act) = s.boundaries.get(&f.boundary_id) else {
                        continue;
                    };
                    if !act.lights(f.oceanic) || !seen.insert((&f.boundary_id, f.oceanic)) {
                        continue;
                    }
                    let Some(feat) = ctx
                        .data
                        .boundaries
                        .get(&f.boundary_id)
                        .iter()
                        .find(|x| x.oceanic == f.oceanic)
                    else {
                        continue;
                    };
                    let Some((lat, lon)) = feat.label else { continue };
                    let p = to_screen(&cam, LatLon::new(lat, lon));
                    if !in_view(p) {
                        continue;
                    }
                    let c = if act.via_uir_only {
                        colors.uir_active
                    } else {
                        colors.fir_label
                    };
                    placer.place(&painter, p, Align2::CENTER_CENTER, &f.boundary_id, &font, c32(c), false);
                }
            }
            // Aircraft labels when zoomed in enough or highlighted.
            if ctx.settings.show_aircraft_labels && ctx.settings.show_aircraft {
                let zoomed = cam.scale > 12.0;
                for p in &s.pilots {
                    let hl = ctx.highlights.get(&p.callsign);
                    if !zoomed && hl.is_none() {
                        continue;
                    }
                    if p.groundspeed < 40
                        && p.altitude < 15_000
                        && (!ctx.settings.show_ground_traffic || cam.scale < 200.0)
                    {
                        continue;
                    }
                    let pos = to_screen(&cam, LatLon::new(p.latitude, p.longitude));
                    if !rect.contains(pos) {
                        continue;
                    }
                    let mut text = p.callsign.clone();
                    if ctx.settings.label_altitude {
                        text.push_str(&format!(" {}", fmt_alt(p.altitude)));
                    }
                    if ctx.settings.label_speed {
                        text.push_str(&format!(" {}kt", p.groundspeed));
                    }
                    let c = hl.copied().unwrap_or(colors.aircraft_label);
                    placer.place(
                        &painter,
                        pos + Vec2::new(7.0, -6.0),
                        Align2::LEFT_BOTTOM,
                        &text,
                        &small,
                        c32(c),
                        hl.is_some(),
                    );
                }
            }
        }

        // Measurement.
        let primary_clicked = response.clicked_by(if ctx.settings.invert_mouse_buttons {
            egui::PointerButton::Secondary
        } else {
            egui::PointerButton::Primary
        });
        let mut measuring_click = false;
        match (&self.measure, primary_clicked, cursor) {
            (MeasureState::Armed, true, Some(c)) => {
                self.measure = MeasureState::From(c);
                measuring_click = true;
            }
            (MeasureState::From(a), true, Some(c)) => {
                self.measure = MeasureState::Done(*a, c);
                measuring_click = true;
            }
            (MeasureState::Done(..), true, Some(c)) => {
                self.measure = MeasureState::From(c);
                measuring_click = true;
            }
            _ => {}
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.measure = MeasureState::Off;
        }
        let measure_text = match (&self.measure, cursor) {
            (MeasureState::From(a), Some(c)) => {
                let pa = to_screen(&cam, *a);
                let pc = to_screen(&cam, c);
                painter.line_segment([pa, pc], Stroke::new(1.5, c32(colors.measure)));
                Some((pc, *a, c))
            }
            (MeasureState::Done(a, b), _) => Some((to_screen(&cam, *b), *a, *b)),
            _ => None,
        };
        if let Some((pos, a, b)) = measure_text {
            let text = format!("{:.1} NM  {:03.0}°T", distance_nm(a, b), initial_bearing_true(a, b));
            let r = painter.text(
                pos + Vec2::new(10.0, 10.0),
                Align2::LEFT_TOP,
                &text,
                font.clone(),
                c32(colors.measure),
            );
            painter.rect_filled(r.expand(3.0), 3.0, c32(colors.tooltip_background));
            painter.text(
                pos + Vec2::new(10.0, 10.0),
                Align2::LEFT_TOP,
                &text,
                font.clone(),
                c32(colors.measure),
            );
        }

        // Picking.
        let pick = |pos: Pos2| -> Option<Selection> {
            let s = ctx.snapshot?;
            let lp = cam.from_screen([(pos.x - rect.min.x) as f64, (pos.y - rect.min.y) as f64]);
            let deg = 8.0 / cam.scale;
            let mut best: Option<(f32, Selection)> = None;
            for lon_shift in [0.0, -360.0, 360.0] {
                let env = rstar::AABB::from_corners(
                    [lp.lon - deg + lon_shift, lp.lat - deg * 2.0],
                    [lp.lon + deg + lon_shift, lp.lat + deg * 2.0],
                );
                for item in s.pilot_tree.locate_in_envelope(env) {
                    let p = &s.pilots[item.data];
                    let sp = to_screen(&cam, LatLon::new(p.latitude, p.longitude));
                    let d = sp.distance(pos);
                    if d < 10.0 && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                        best = Some((d, Selection::Pilot(p.callsign.clone())));
                    }
                }
            }
            if best.is_some() {
                return best.map(|b| b.1);
            }
            if let Some((_, icao)) = airport_hits
                .iter()
                .filter(|(p, _)| p.distance(pos) < 9.0)
                .min_by(|a, b| a.0.distance(pos).total_cmp(&b.0.distance(pos)))
            {
                return Some(Selection::Airport(icao.clone()));
            }
            // FIR under the cursor: staffed first, then any.
            let feats: Vec<_> = ctx
                .data
                .boundaries
                .iter()
                .filter(|f| point_in_multipolygon(lp.lon, lp.lat, &f.polygons))
                .collect();
            let staffed = feats
                .iter()
                .find(|f| s.boundaries.get(&f.id).is_some_and(|a| a.lights(f.oceanic)));
            staffed
                .or(feats.iter().find(|f| !f.id.contains('-')))
                .or(feats.first())
                .map(|f| Selection::Fir(f.id.clone()))
        };

        let mut out = MapResponse {
            clicked: None,
            double_clicked: None,
            context_menu: None,
            cursor,
        };
        if let Some(hp) = hover_pos
            && !response.dragged()
            && let Some(sel) = pick(hp)
            && let Some(s) = ctx.snapshot
        {
            let text = tooltip_text(&sel, s, ctx.data);
            if !text.is_empty() {
                draw_tooltip(&painter, hp + Vec2::new(14.0, 14.0), &text, &font, &colors, rect);
            }
        }
        if primary_clicked && !measuring_click {
            out.clicked = response.interact_pointer_pos().and_then(pick);
        }
        if response.double_clicked() {
            out.double_clicked = response.interact_pointer_pos().and_then(pick);
        }
        if response.secondary_clicked() && !ctx.settings.invert_mouse_buttons {
            out.context_menu = response.interact_pointer_pos().and_then(pick);
        }
        self.last_frame_ms = t0.elapsed().as_secs_f32() * 1000.0;
        out
    }
}

pub fn fmt_alt(ft: i64) -> String {
    if ft >= 18_000 {
        format!("FL{:03}", (ft + 50) / 100)
    } else {
        format!("{ft}ft")
    }
}

fn draw_airport_symbol(p: &egui::Painter, pos: Pos2, a: &radarvector_core::snapshot::AirportActivity, c: &Colors) {
    // Concentric design: outer ring = approach, filled disc = tower, inner dot = ground,
    // small square = delivery; grey ring = ATIS only.
    if a.approach {
        p.circle_stroke(pos, 7.0, Stroke::new(2.0, c32(c.airport_approach)));
    }
    if a.tower {
        p.circle_filled(pos, 4.5, c32(c.airport_tower));
    } else if a.any_controlled() || a.atis {
        p.circle_stroke(
            pos,
            4.5,
            Stroke::new(
                1.5,
                c32(if a.atis && !a.any_controlled() {
                    c.airport_atis
                } else {
                    c.airport_label
                }),
            ),
        );
    }
    if a.ground {
        p.circle_filled(pos, 2.0, c32(c.airport_ground));
    }
    if a.delivery {
        p.rect_filled(
            Rect::from_center_size(pos + Vec2::new(5.5, -5.5), Vec2::splat(3.0)),
            0.0,
            c32(c.airport_delivery),
        );
    }
}

fn draw_tooltip(p: &egui::Painter, pos: Pos2, text: &str, font: &FontId, c: &Colors, bounds: Rect) {
    let galley = p.layout_no_wrap(text.to_string(), font.clone(), c32(c.tooltip_text));
    let mut r = Rect::from_min_size(pos, galley.size() + Vec2::splat(10.0));
    if r.right() > bounds.right() {
        r = r.translate(Vec2::new(-(r.width() + 28.0), 0.0));
    }
    if r.bottom() > bounds.bottom() {
        r = r.translate(Vec2::new(0.0, -(r.height() + 28.0)));
    }
    p.rect_filled(r, 4.0, c32(c.tooltip_background));
    p.galley(r.min + Vec2::splat(5.0), galley, c32(c.tooltip_text));
}

pub fn tooltip_text(sel: &Selection, s: &NetworkSnapshot, data: &StaticData) -> String {
    match sel {
        Selection::Pilot(cs) => s
            .pilot_by_callsign(cs)
            .map(|p| {
                let idx = s.pilots.iter().position(|x| x.callsign == p.callsign).unwrap_or(0);
                let ty = s.aircraft_types.get(idx).cloned().unwrap_or_default();
                let route = p
                    .flight_plan
                    .as_ref()
                    .map(|f| format!("{} → {}", f.departure, f.arrival))
                    .unwrap_or_default();
                format!(
                    "{}  {}\n{}\n{}  {} kt  {:03}°",
                    p.callsign,
                    ty,
                    route,
                    fmt_alt(p.altitude),
                    p.groundspeed,
                    p.heading
                )
            })
            .unwrap_or_default(),
        Selection::Airport(icao) => {
            let name = data.airport(icao).map(|a| a.name.clone()).unwrap_or_default();
            let mut t = format!("{icao}  {name}");
            if let Some(act) = s.airports.get(icao) {
                for &i in &act.stations {
                    let st = &s.stations[i];
                    t.push_str(&format!("\n{}  {}", st.data.callsign, st.data.frequency));
                }
            }
            t.push_str(&format!(
                "\n↑{}  ↓{}",
                s.departures(icao).count(),
                s.arrivals(icao).count()
            ));
            t
        }
        Selection::Fir(id) => {
            let name = data.fir_name(id).unwrap_or("");
            let mut t = format!("{id}  {name}");
            if let Some(act) = s.boundaries.get(id) {
                for &i in &act.stations {
                    let st = &s.stations[i];
                    t.push_str(&format!("\n{}  {}", st.data.callsign, st.data.frequency));
                }
            }
            t
        }
        Selection::Station(cs) => s
            .station_by_callsign(cs)
            .map(|st| format!("{}  {}\n{}", st.data.callsign, st.data.frequency, st.data.name))
            .unwrap_or_default(),
        Selection::Country(p) => data.country_by_prefix(p).map(|c| c.name.clone()).unwrap_or_default(),
    }
}

/// Greedy label placement with a coarse grid to avoid overlaps.
struct LabelPlacer {
    cells: HashMap<(i32, i32), Vec<Rect>>,
    bounds: Rect,
}

const CELL: f32 = 64.0;

impl LabelPlacer {
    fn new(bounds: Rect) -> Self {
        Self {
            cells: HashMap::new(),
            bounds,
        }
    }

    fn cells_of(r: Rect) -> impl Iterator<Item = (i32, i32)> {
        let (x0, x1) = ((r.min.x / CELL).floor() as i32, (r.max.x / CELL).floor() as i32);
        let (y0, y1) = ((r.min.y / CELL).floor() as i32, (r.max.y / CELL).floor() as i32);
        (x0..=x1).flat_map(move |x| (y0..=y1).map(move |y| (x, y)))
    }

    /// Draw `text` if it doesn't overlap a placed label (always when `force`).
    #[allow(clippy::too_many_arguments)]
    fn place(
        &mut self,
        p: &egui::Painter,
        pos: Pos2,
        align: Align2,
        text: &str,
        font: &FontId,
        color: Color32,
        force: bool,
    ) -> bool {
        let galley = p.layout_no_wrap(text.to_string(), font.clone(), color);
        let r = align.anchor_size(pos, galley.size()).expand(1.0);
        if !self.bounds.intersects(r) {
            return false;
        }
        if !force && Self::cells_of(r).any(|c| self.cells.get(&c).is_some_and(|v| v.iter().any(|o| o.intersects(r)))) {
            return false;
        }
        for c in Self::cells_of(r) {
            self.cells.entry(c).or_default().push(r);
        }
        p.galley(r.min + Vec2::splat(1.0), galley, color);
        true
    }
}

#[cfg(test)]
mod visual_tests {
    //! Offscreen renders of fixed views (needs a GPU; run with `--ignored`).
    //! PNGs go to `target/visual/`; references live in `tests/visual/` and are
    //! created on first run, then compared with a pixel tolerance.

    use super::*;
    use radarvector_render::offscreen::Offscreen;

    fn snapshot(data: &StaticData, tr: &crate::tracon::TraconSet) -> NetworkSnapshot {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/radarvector-app/tests/visual/feed.json");
        let feed = std::fs::read(&root)
            .ok()
            .and_then(|b| radarvector_feed::models::decode_data_feed(&b).ok())
            .unwrap_or_else(|| crate::devmode::synthetic_feed(3000));
        NetworkSnapshot::build_with(data, &tr.tracons, feed)
    }

    fn read_png(p: &std::path::Path) -> Option<Vec<u8>> {
        let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(p).ok()?));
        let mut r = dec.read_info().ok()?;
        let mut buf = vec![0; r.output_buffer_size()?];
        let info = r.next_frame(&mut buf).ok()?;
        buf.truncate(info.buffer_size());
        Some(buf)
    }

    fn mean_abs_diff(a: &[u8], b: &[u8]) -> f64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (*x as i32 - *y as i32).unsigned_abs() as f64)
            .sum::<f64>()
            / a.len() as f64
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn reference_views() {
        let (data, _) = StaticData::from_texts(
            include_str!("../../../assets/vatspy-data/VATSpy.dat"),
            include_str!("../../../assets/vatspy-data/Boundaries.geojson"),
        )
        .unwrap();
        let tr = crate::tracon::TraconSet::embedded();
        let snap = Arc::new(snapshot(&data, &tr));
        let settings = Settings::default();
        let mut view = MapView::new(Camera::default());
        view.prepared = Some(Prepared::build(&data, Basemap::embedded()));
        let mut off = Offscreen::new().expect("GPU");
        let out_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/visual");
        let ref_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual");
        std::fs::create_dir_all(&out_dir).unwrap();
        std::fs::create_dir_all(&ref_dir).unwrap();
        let tracks = HashMap::new();
        let highlights = HashMap::new();
        let views = [
            ("world", 20.0, 0.0, 360.0),
            ("europe", 50.0, 10.0, 40.0),
            ("north_america_east", 42.0, -75.0, 30.0),
            ("antimeridian_nzzo", -30.0, 180.0, 90.0),
        ];
        let (w, h) = (1200u32, 700u32);
        for (name, lat, lon, span) in views {
            view.camera.set_viewport(w as f64, h as f64);
            view.camera.look_at(LatLon::new(lat, lon), span);
            let ctx = MapContext {
                data: &data,
                snapshot: Some(&snap),
                settings: &settings,
                selection: None,
                tracks: &tracks,
                tracons: Some(&tr),
                highlights: &highlights,
                highlights_hash: 0,
            };
            let frame = view.frame(&ctx).unwrap();
            let px = off.render(frame, w, h, settings.colors().background).unwrap();
            let distinct: std::collections::HashSet<[u8; 4]> = px.chunks(4).map(|c| [c[0], c[1], c[2], c[3]]).collect();
            assert!(distinct.len() > 50, "{name}: render looks empty");
            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &px);
            crate::devmode::save_png(&out_dir.join(format!("{name}.png")), &img).unwrap();
            let reference = ref_dir.join(format!("{name}.png"));
            match read_png(&reference) {
                Some(r) if r.len() == px.len() => {
                    let d = mean_abs_diff(&r, &px);
                    assert!(d < 2.0, "{name}: mean pixel difference {d:.2} vs reference");
                }
                _ => crate::devmode::save_png(&reference, &img).unwrap(),
            }
        }
    }
}

#[cfg(test)]
mod bench {
    //! NF-03 headless benchmark: full map frame = egui CPU pass (labels,
    //! picking, tessellation) + GPU render, while panning and zooming.
    //! `cargo test -p radarvector-app --release -- --ignored --nocapture bench_map`

    use super::*;
    use radarvector_render::offscreen::Offscreen;

    #[test]
    #[ignore = "needs a GPU; benchmark"]
    fn bench_map() {
        let (data, _) = StaticData::from_texts(
            include_str!("../../../assets/vatspy-data/VATSpy.dat"),
            include_str!("../../../assets/vatspy-data/Boundaries.geojson"),
        )
        .unwrap();
        let tr = crate::tracon::TraconSet::embedded();
        let t = std::time::Instant::now();
        let prepared = Prepared::build(&data, Basemap::embedded());
        println!("prepare geometry: {:.0} ms", t.elapsed().as_secs_f64() * 1000.0);
        let mut off = Offscreen::new().expect("GPU");
        let (w, h) = (1600u32, 900u32);
        for n in [3000usize, 5000] {
            let mut feed = crate::devmode::synthetic_feed(n);
            if let Ok(b) =
                std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/visual/feed.json"))
                && let Ok(real) = radarvector_feed::models::decode_data_feed(&b)
            {
                feed.controllers = real.controllers;
                feed.atis = real.atis;
            }
            let snap = Arc::new(NetworkSnapshot::build_with(&data, &tr.tracons, feed));
            let settings = Settings {
                show_aircraft_labels: true,
                ..Default::default()
            };
            let mut view = MapView::new(Camera::default());
            view.prepared = Some(Prepared {
                land: prepared.land.clone(),
                base_lines_src: prepared.base_lines_src.clone(),
                fir: prepared.fir.clone(),
                static_ptr: prepared.static_ptr,
            });
            let egui_ctx = egui::Context::default();
            let tracks = HashMap::new();
            let highlights = HashMap::new();
            let mut cpu = Vec::new();
            let mut gpu = Vec::new();
            for i in 0..240 {
                let tsec = i as f64 / 60.0;
                let ctx = MapContext {
                    data: &data,
                    snapshot: Some(&snap),
                    settings: &settings,
                    selection: None,
                    tracks: &tracks,
                    tracons: Some(&tr),
                    highlights: &highlights,
                    highlights_hash: 0,
                };
                let t0 = std::time::Instant::now();
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(w as f32, h as f32))),
                    ..Default::default()
                };
                let mut frame = None;
                let out = egui_ctx.run_ui(input, |ui| {
                    view.camera.center = [(tsec * 36.0) % 360.0 - 180.0, 20.0 * (tsec * 0.6).sin()];
                    view.camera.scale = w as f64 / (40.0 + 300.0 * (0.5 + 0.5 * (tsec * 0.8).cos()));
                    view.show(ui, &ctx);
                    frame = view.frame(&ctx);
                });
                let _prims = egui_ctx.tessellate(out.shapes, 1.0);
                cpu.push(t0.elapsed().as_secs_f64() * 1000.0);
                let t1 = std::time::Instant::now();
                off.render_no_readback(frame.unwrap(), w, h, [0, 0, 0, 255]).unwrap();
                gpu.push(t1.elapsed().as_secs_f64() * 1000.0);
            }
            let stats = |v: &mut Vec<f64>| {
                let v = &mut v[20..];
                v.sort_by(f64::total_cmp);
                (
                    v.iter().sum::<f64>() / v.len() as f64,
                    v[(v.len() as f64 * 0.95) as usize],
                )
            };
            let (ca, cp) = stats(&mut cpu);
            let (ga, gp) = stats(&mut gpu);
            println!(
                "BENCH {n} aircraft {w}x{h}: CPU avg {ca:.2} ms p95 {cp:.2} ms | GPU avg {ga:.2} ms p95 {gp:.2} ms | total p95 {:.2} ms",
                cp + gp
            );
            assert!(cp + gp < 16.7, "frame budget exceeded");
        }
    }
}
