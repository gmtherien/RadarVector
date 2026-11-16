//! Tab contents (lists, detail sheets, METAR) and floating windows.

use egui::{RichText, Ui};
use egui_extras::{Column, TableBuilder};
use radarvector_core::callsign::Resolved;
use radarvector_core::{Filter, StationKind};
use radarvector_geo::point_in_multipolygon;

use crate::app::{AppState, ListState, Tab, parse_hex, to_hex};
use crate::i18n::{K, t};
use crate::map_view::{Selection, fmt_alt};
use crate::settings::{Favorite, Language, Theme};

/// Minutes since an RFC 3339 UTC timestamp like `2026-11-15T05:13:05.51Z`.
pub fn online_for(ts: &str) -> String {
    let parse = || -> Option<i64> {
        let y: i64 = ts.get(0..4)?.parse().ok()?;
        let mo: i64 = ts.get(5..7)?.parse().ok()?;
        let d: i64 = ts.get(8..10)?.parse().ok()?;
        let h: i64 = ts.get(11..13)?.parse().ok()?;
        let mi: i64 = ts.get(14..16)?.parse().ok()?;
        let s: i64 = ts.get(17..19)?.parse().ok()?;
        // Days from civil (Howard Hinnant).
        let y2 = if mo <= 2 { y - 1 } else { y };
        let era = y2.div_euclid(400);
        let yoe = y2 - era * 400;
        let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        Some(days * 86_400 + h * 3600 + mi * 60 + s)
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match parse() {
        Some(start) if now >= start => {
            let m = (now - start) / 60;
            format!("{}:{:02}", m / 60, m % 60)
        }
        _ => String::new(),
    }
}

fn list_header(ui: &mut Ui, st: &mut ListState, cols: &[&str]) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut st.filter)
                .hint_text(t(K::FilterText))
                .desired_width(180.0),
        );
        ui.menu_button(t(K::Columns), |ui| {
            for (i, c) in cols.iter().enumerate() {
                let mut shown = !st.hidden[i];
                if ui.checkbox(&mut shown, *c).changed() {
                    st.hidden[i] = !shown;
                }
            }
        });
    });
}

/// Generic sortable, filterable, virtualised table. `rows` are string cells;
/// returns the index of a clicked row and whether it was double-clicked.
fn table(
    ui: &mut Ui,
    id: &str,
    st: &mut ListState,
    cols: &[&str],
    rows: &[Vec<String>],
    numeric: &[bool],
) -> Option<(usize, bool)> {
    let q = st.filter.trim().to_ascii_uppercase();
    let mut idx: Vec<usize> = (0..rows.len())
        .filter(|&i| q.is_empty() || rows[i].iter().any(|c| c.to_ascii_uppercase().contains(&q)))
        .collect();
    let col = st.sort_col.min(cols.len().saturating_sub(1));
    idx.sort_by(|&a, &b| {
        let (x, y) = (&rows[a][col], &rows[b][col]);
        let o = if numeric.get(col).copied().unwrap_or(false) {
            let p = |s: &str| {
                s.trim_start_matches("FL")
                    .replace("ft", "")
                    .replace(':', ".")
                    .parse::<f64>()
                    .unwrap_or(f64::MIN)
            };
            p(x).total_cmp(&p(y))
        } else {
            x.cmp(y)
        };
        if st.sort_desc { o.reverse() } else { o }
    });
    let visible: Vec<usize> = (0..cols.len()).filter(|&i| !st.hidden[i]).collect();
    let mut clicked = None;
    ui.push_id(id, |ui| {
        let mut tb = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(egui::Sense::click())
            .auto_shrink([false, false]);
        for _ in &visible {
            tb = tb.column(Column::auto().at_least(40.0).clip(true));
        }
        tb.header(20.0, |mut h| {
            for &c in &visible {
                h.col(|ui| {
                    let arrow = if st.sort_col == c {
                        if st.sort_desc { " ▼" } else { " ▲" }
                    } else {
                        ""
                    };
                    if ui
                        .button(RichText::new(format!("{}{arrow}", cols[c])).strong())
                        .clicked()
                    {
                        if st.sort_col == c {
                            st.sort_desc = !st.sort_desc;
                        } else {
                            st.sort_col = c;
                            st.sort_desc = false;
                        }
                    }
                });
            }
        })
        .body(|body| {
            body.rows(18.0, idx.len(), |mut row| {
                let r = idx[row.index()];
                for &c in &visible {
                    row.col(|ui| {
                        ui.label(&rows[r][c]);
                    });
                }
                let resp = row.response();
                if resp.double_clicked() {
                    clicked = Some((r, true));
                } else if resp.clicked() {
                    clicked = Some((r, false));
                }
            });
        });
    });
    clicked
}

pub fn controllers(ui: &mut Ui, s: &mut AppState) {
    let Some(snap) = s.snapshot.clone() else {
        ui.spinner();
        return;
    };
    let cols = [
        t(K::Callsign),
        t(K::Name),
        t(K::Frequency),
        t(K::Rating),
        t(K::Online),
        "CID",
    ];
    let mut st = std::mem::take(&mut s.controllers_list);
    list_header(ui, &mut st, &cols);
    // ATC only: observers and suffix-less logins (facility 0) are excluded.
    let stations: Vec<_> = snap
        .stations
        .iter()
        .filter(|x| {
            x.kind != StationKind::Observer && (x.data.facility != 0 || x.is_atis || x.kind != StationKind::Other)
        })
        .collect();
    let rows: Vec<Vec<String>> = stations
        .iter()
        .map(|x| {
            vec![
                x.data.callsign.clone(),
                x.data.name.clone(),
                x.data.frequency.clone(),
                snap.rating_short(x.data.rating).to_string(),
                online_for(&x.data.logon_time),
                x.data.cid.to_string(),
            ]
        })
        .collect();
    if let Some((i, dbl)) = table(
        ui,
        "controllers",
        &mut st,
        &cols,
        &rows,
        &[false, false, true, false, true, true],
    ) {
        let sel = Selection::Station(stations[i].data.callsign.clone());
        if dbl {
            s.center_on(&sel);
        }
        s.select(sel, dbl);
    }
    s.controllers_list = st;
}

pub fn flights(ui: &mut Ui, s: &mut AppState) {
    let Some(snap) = s.snapshot.clone() else {
        ui.spinner();
        return;
    };
    let cols = [
        t(K::Callsign),
        t(K::Aircraft),
        t(K::Departure),
        t(K::Arrival),
        t(K::Altitude),
        t(K::Groundspeed),
        t(K::Rating),
        t(K::Name),
        "CID",
    ];
    let mut st = std::mem::take(&mut s.flights_list);
    list_header(ui, &mut st, &cols);
    let rows: Vec<Vec<String>> = snap
        .pilots
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let fp = p.flight_plan.as_ref();
            vec![
                p.callsign.clone(),
                snap.aircraft_types[i].clone(),
                fp.map(|f| f.departure.clone()).unwrap_or_default(),
                fp.map(|f| f.arrival.clone()).unwrap_or_default(),
                fmt_alt(p.altitude),
                p.groundspeed.to_string(),
                snap.pilot_rating_short(p.pilot_rating).to_string(),
                p.name.clone(),
                p.cid.to_string(),
            ]
        })
        .collect();
    if let Some((i, dbl)) = table(
        ui,
        "flights",
        &mut st,
        &cols,
        &rows,
        &[false, false, false, false, true, true, false, false, true],
    ) {
        let sel = Selection::Pilot(snap.pilots[i].callsign.clone());
        if dbl {
            s.center_on(&sel);
        }
        s.select(sel, dbl);
    }
    s.flights_list = st;
}

pub fn servers(ui: &mut Ui, s: &mut AppState) {
    let Some(snap) = s.snapshot.clone() else { return };
    egui::Grid::new("servers").striped(true).show(ui, |ui| {
        for h in ["Ident", t(K::Name), "Location", "Connections"] {
            ui.strong(h);
        }
        ui.end_row();
        for sv in &snap.servers {
            ui.label(&sv.ident);
            ui.label(&sv.name);
            ui.label(&sv.location);
            ui.label(if sv.client_connections_allowed { "✔" } else { "✖" });
            ui.end_row();
        }
    });
}

pub fn prefiles(ui: &mut Ui, s: &mut AppState) {
    let Some(snap) = s.snapshot.clone() else { return };
    egui::Grid::new("prefiles").striped(true).show(ui, |ui| {
        for h in [
            t(K::Callsign),
            t(K::Aircraft),
            t(K::Departure),
            t(K::Arrival),
            t(K::Route),
        ] {
            ui.strong(h);
        }
        ui.end_row();
        for p in &snap.prefiles {
            let fp = p.flight_plan.clone().unwrap_or_default();
            ui.label(&p.callsign);
            ui.label(radarvector_core::aircraft::aircraft_type(&fp));
            ui.label(&fp.departure);
            ui.label(&fp.arrival);
            ui.label(RichText::new(fp.route.chars().take(80).collect::<String>()).small());
            ui.end_row();
        }
    });
}

pub fn metar(ui: &mut Ui, s: &mut AppState) {
    ui.horizontal(|ui| {
        ui.label(t(K::Stations));
        let r = ui.add(
            egui::TextEdit::singleline(&mut s.metar.input)
                .hint_text("CYUL KJFK LFPG")
                .desired_width(260.0),
        );
        let go = ui.add_enabled(!s.metar.pending, egui::Button::new(t(K::Get))).clicked()
            || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if go {
            let stations: Vec<String> = s
                .metar
                .input
                .split([' ', ',', ';'])
                .filter(|x| !x.is_empty())
                .map(|x| x.to_ascii_uppercase())
                .collect();
            if !stations.is_empty() {
                s.metar.pending = true;
                s.metar.request = s.request_metar(stations);
            }
        }
        if s.metar.pending {
            ui.spinner();
        }
    });
    if let Some(e) = &s.metar.error {
        ui.colored_label(egui::Color32::LIGHT_RED, e);
    }
    for (_, line) in &s.metar.results {
        ui.label(RichText::new(line).monospace());
    }
}

fn favorite_button(ui: &mut Ui, s: &mut AppState, fav: Favorite) {
    let is_fav = s.settings.favorites.contains(&fav);
    if is_fav {
        if ui.button(format!("★ {}", t(K::RemoveFavorite))).clicked() {
            s.settings.favorites.retain(|f| f != &fav);
        }
    } else if ui.button(format!("☆ {}", t(K::AddFavorite))).clicked() {
        s.favorite_confirm = Some(fav);
    }
}

fn station_rows(ui: &mut Ui, s: &mut AppState, idxs: &[usize]) {
    let Some(snap) = s.snapshot.clone() else { return };
    if idxs.is_empty() {
        ui.label(RichText::new(t(K::NoAtc)).weak());
    }
    for &i in idxs {
        let st = &snap.stations[i];
        ui.horizontal(|ui| {
            if ui.link(&st.data.callsign).clicked() {
                s.select(Selection::Station(st.data.callsign.clone()), true);
            }
            ui.label(&st.data.frequency);
            ui.label(RichText::new(&st.data.name).weak());
        });
        if st.is_atis
            && let Some(lines) = &st.data.text_atis
        {
            ui.label(RichText::new(lines.join("\n")).monospace().small());
        }
    }
}

fn pilot_list(ui: &mut Ui, s: &mut AppState, label: &str, pilots: Vec<(String, String)>) {
    egui::CollapsingHeader::new(format!("{label} ({})", pilots.len()))
        .default_open(pilots.len() < 30)
        .show(ui, |ui| {
            for (cs, extra) in pilots {
                ui.horizontal(|ui| {
                    if ui.link(&cs).clicked() {
                        s.select(Selection::Pilot(cs.clone()), true);
                    }
                    ui.label(RichText::new(extra).weak());
                });
            }
        });
}

pub fn details(ui: &mut Ui, s: &mut AppState, sel: &Selection) {
    let data = s.data_full();
    let snap = s.snapshot.clone();
    ui.horizontal(|ui| {
        if ui.button(format!("⌖ {}", t(K::Center))).clicked() {
            s.center_on(sel);
            s.selection = Some(sel.clone());
        }
    });
    ui.separator();
    match sel {
        Selection::Pilot(cs) => {
            let Some(p) = snap.as_ref().and_then(|x| x.pilot_by_callsign(cs)).cloned() else {
                ui.label("—");
                return;
            };
            let snap = snap.as_ref().map(|x| x.as_ref());
            ui.heading(&p.callsign);
            ui.label(format!(
                "{}  ·  CID {}  ·  {}",
                p.name,
                p.cid,
                snap.map(|x| x.pilot_rating_short(p.pilot_rating)).unwrap_or("")
            ));
            egui::Grid::new("pilot").num_columns(2).show(ui, |ui| {
                ui.label(t(K::Position));
                ui.label(format!("{:.4} {:.4}", p.latitude, p.longitude));
                ui.end_row();
                ui.label(t(K::Altitude));
                ui.label(format!(
                    "{}  ·  {} kt  ·  {:03}°",
                    fmt_alt(p.altitude),
                    p.groundspeed,
                    p.heading
                ));
                ui.end_row();
                ui.label(t(K::Squawk));
                ui.label(&p.transponder);
                ui.end_row();
                ui.label("QNH");
                ui.label(format!("{} hPa / {:.2} inHg", p.qnh_mb, p.qnh_i_hg));
                ui.end_row();
                ui.label(t(K::Online));
                ui.label(online_for(&p.logon_time));
                ui.end_row();
                if let Some(fp) = &p.flight_plan {
                    let rows = [
                        (t(K::Rules), fp.flight_rules.clone()),
                        (t(K::Aircraft), fp.aircraft.clone()),
                        (
                            t(K::Departure),
                            format!(
                                "{}  {}",
                                fp.departure,
                                data.airport(&fp.departure).map(|a| a.name.as_str()).unwrap_or("")
                            ),
                        ),
                        (
                            t(K::Arrival),
                            format!(
                                "{}  {}",
                                fp.arrival,
                                data.airport(&fp.arrival).map(|a| a.name.as_str()).unwrap_or("")
                            ),
                        ),
                        (t(K::Alternate), fp.alternate.clone()),
                        (t(K::Cruise), format!("{}  ·  TAS {}", fp.altitude, fp.cruise_tas)),
                        (
                            "EOBT / EET / Fuel",
                            format!("{} / {} / {}", fp.deptime, fp.enroute_time, fp.fuel_time),
                        ),
                        (t(K::AssignedSquawk), fp.assigned_transponder.clone()),
                    ];
                    for (k, v) in rows {
                        ui.label(k);
                        ui.label(v);
                        ui.end_row();
                    }
                }
            });
            if let Some(fp) = &p.flight_plan {
                ui.label(RichText::new(t(K::Route)).strong());
                ui.label(RichText::new(&fp.route).monospace());
                ui.label(RichText::new(t(K::Remarks)).strong());
                ui.label(RichText::new(&fp.remarks).small());
            }
            ui.horizontal(|ui| {
                let followed = s.settings.followed_cids.contains(&p.cid);
                if ui
                    .selectable_label(followed, format!("🔔 {}", t(K::FollowedCids)))
                    .clicked()
                {
                    if followed {
                        s.settings.followed_cids.retain(|c| *c != p.cid);
                    } else {
                        s.settings.followed_cids.push(p.cid);
                    }
                }
            });
        }
        Selection::Station(cs) => {
            let Some(st) = snap.as_ref().and_then(|x| x.station_by_callsign(cs)).cloned() else {
                ui.label("—");
                return;
            };
            ui.heading(&st.data.callsign);
            ui.label(format!(
                "{}  ·  CID {}  ·  {}",
                st.data.name,
                st.data.cid,
                snap.as_ref().map(|x| x.rating_short(st.data.rating)).unwrap_or("")
            ));
            egui::Grid::new("station").num_columns(2).show(ui, |ui| {
                ui.label(t(K::Frequency));
                ui.label(&st.data.frequency);
                ui.end_row();
                ui.label(t(K::Logon));
                ui.label(format!(
                    "{}  ({})",
                    st.data.logon_time.get(0..16).unwrap_or("").replace('T', " "),
                    online_for(&st.data.logon_time)
                ));
                ui.end_row();
                ui.label("Type");
                ui.label(snap.as_ref().map(|x| x.facility_short(st.data.facility)).unwrap_or(""));
                ui.end_row();
            });
            if let Resolved::Approach {
                tracon: Some(prefix), ..
            } = &st.resolved
                && let Some(name) = s.tracons.name(prefix, st.kind)
            {
                ui.label(format!("TRACON: {name}"));
            }
            match &st.resolved {
                Resolved::Airport { icao } | Resolved::Approach { icao, .. } => {
                    if ui.link(icao).clicked() {
                        s.select(Selection::Airport(icao.clone()), true);
                    }
                }
                Resolved::Fir(tg) | Resolved::Uir { firs: tg, .. } => {
                    ui.horizontal_wrapped(|ui| {
                        for x in tg {
                            if ui.link(&x.boundary_id).clicked() {
                                s.select(Selection::Fir(x.boundary_id.clone()), true);
                            }
                        }
                    });
                }
                Resolved::Unresolved => {}
            }
            if let Some(lines) = &st.data.text_atis {
                ui.label(RichText::new(t(K::Info)).strong());
                ui.label(RichText::new(lines.join("\n")).monospace());
            }
        }
        Selection::Airport(icao) => {
            let Some(a) = data.airport(icao).cloned() else {
                ui.label("—");
                return;
            };
            ui.heading(format!("{}  {}", a.icao, a.name));
            ui.label(format!(
                "{}  ·  FIR {}  ·  {:.4} {:.4}",
                a.iata_lid.clone().unwrap_or_default(),
                a.fir,
                a.lat,
                a.lon
            ));
            favorite_button(ui, s, Favorite::Airport(a.icao.clone()));
            ui.label(RichText::new(t(K::Metar)).strong());
            match s.airport_metars.get(icao) {
                Some((_, Some(m))) => {
                    ui.label(RichText::new(m).monospace());
                }
                Some((_, None)) => {
                    ui.spinner();
                }
                None => {
                    let id = s.request_metar(vec![icao.clone()]);
                    s.airport_metars.insert(icao.clone(), (id, None));
                }
            }
            let stations = snap
                .as_ref()
                .and_then(|x| x.airports.get(icao))
                .map(|a| a.stations.clone())
                .unwrap_or_default();
            ui.label(RichText::new(t(K::Controllers)).strong());
            station_rows(ui, s, &stations);
            if let Some(sn) = &snap {
                let deps = sn
                    .departures(icao)
                    .map(|p| {
                        (
                            p.callsign.clone(),
                            p.flight_plan.as_ref().map(|f| f.arrival.clone()).unwrap_or_default(),
                        )
                    })
                    .collect();
                let arrs = sn
                    .arrivals(icao)
                    .map(|p| {
                        (
                            p.callsign.clone(),
                            p.flight_plan.as_ref().map(|f| f.departure.clone()).unwrap_or_default(),
                        )
                    })
                    .collect();
                pilot_list(ui, s, t(K::Departures), deps);
                pilot_list(ui, s, t(K::Arrivals), arrs);
            }
        }
        Selection::Fir(id) => {
            let boundary = data
                .firs_by_icao(id)
                .next()
                .map(|f| f.boundary_id.clone())
                .unwrap_or_else(|| id.clone());
            let icao = data
                .firs
                .iter()
                .find(|f| f.boundary_id == boundary)
                .map(|f| f.icao.clone())
                .unwrap_or_else(|| id.clone());
            ui.heading(format!("{boundary}  {}", data.fir_name(&boundary).unwrap_or("")));
            favorite_button(ui, s, Favorite::Fir(boundary.clone()));
            let stations = snap
                .as_ref()
                .and_then(|x| x.boundaries.get(&boundary))
                .map(|a| a.stations.clone())
                .unwrap_or_default();
            ui.label(RichText::new(t(K::Controllers)).strong());
            station_rows(ui, s, &stations);
            if let Some(sn) = &snap {
                let feats = data.boundaries.get(&boundary);
                let traffic: Vec<(String, String)> = sn
                    .pilots
                    .iter()
                    .filter(|p| {
                        feats.iter().any(|f| {
                            f.bbox.contains(p.longitude, p.latitude)
                                && point_in_multipolygon(p.longitude, p.latitude, &f.polygons)
                        })
                    })
                    .map(|p| (p.callsign.clone(), fmt_alt(p.altitude)))
                    .collect();
                pilot_list(ui, s, t(K::Traffic), traffic);
            }
            let airports: Vec<_> = data
                .airports
                .iter()
                .filter(|a| !a.is_pseudo && a.fir == icao)
                .map(|a| (a.icao.clone(), a.name.clone()))
                .collect();
            egui::CollapsingHeader::new(format!("{} ({})", t(K::Airports), airports.len()))
                .default_open(false)
                .show(ui, |ui| {
                    for (code, name) in airports {
                        ui.horizontal(|ui| {
                            let active = snap.as_ref().is_some_and(|x| x.airports.contains_key(&code));
                            if ui
                                .link(if active {
                                    RichText::new(&code).strong()
                                } else {
                                    RichText::new(&code)
                                })
                                .clicked()
                            {
                                s.select(Selection::Airport(code.clone()), true);
                            }
                            ui.label(RichText::new(name).weak());
                        });
                    }
                });
        }
        Selection::Country(prefix) => {
            let name = data
                .country_by_prefix(prefix)
                .map(|c| c.name.clone())
                .unwrap_or_default();
            ui.heading(format!("{prefix}  {name}"));
            favorite_button(ui, s, Favorite::Country(prefix.clone()));
            if let Some(sn) = &snap {
                let idx: Vec<usize> = sn
                    .stations
                    .iter()
                    .enumerate()
                    .filter(|(_, st)| match &st.resolved {
                        Resolved::Airport { icao } | Resolved::Approach { icao, .. } => {
                            icao.starts_with(prefix.as_str())
                        }
                        Resolved::Fir(tg) | Resolved::Uir { firs: tg, .. } => {
                            tg.iter().any(|x| x.fir_icao.starts_with(prefix.as_str()))
                        }
                        Resolved::Unresolved => false,
                    })
                    .map(|(i, _)| i)
                    .collect();
                station_rows(ui, s, &idx);
            }
        }
    }
}

pub fn filter_results(ui: &mut Ui, s: &mut AppState, i: usize) {
    let Some(f) = s.settings.filters.get(i).cloned() else {
        ui.label("—");
        return;
    };
    let Some(snap) = s.snapshot.clone() else { return };
    ui.heading(&f.name);
    let pilots: Vec<(String, String)> = snap
        .pilots
        .iter()
        .filter(|p| f.matches_pilot(p.cid, &p.callsign))
        .map(|p| {
            (
                p.callsign.clone(),
                format!(
                    "{}  {}",
                    p.name,
                    p.flight_plan
                        .as_ref()
                        .map(|x| format!("{}→{}", x.departure, x.arrival))
                        .unwrap_or_default()
                ),
            )
        })
        .collect();
    let stations: Vec<usize> = snap
        .stations
        .iter()
        .enumerate()
        .filter(|(_, x)| f.matches_controller(x.data.cid, &x.data.callsign))
        .map(|(i, _)| i)
        .collect();
    ui.label(RichText::new(format!("{} ({})", t(K::Controllers), stations.len())).strong());
    station_rows(ui, s, &stations);
    pilot_list(ui, s, t(K::Flights), pilots);
}

fn csv(v: &[String]) -> String {
    v.join(", ")
}
fn parse_csv(s: &str) -> Vec<String> {
    s.split([',', ' ', ';', '\n'])
        .map(|x| x.trim().to_ascii_uppercase())
        .filter(|x| !x.is_empty())
        .collect()
}

/// Floating windows: settings, filters editor, search, about, prompts.
pub fn windows(ctx: &egui::Context, s: &mut AppState) {
    // Settings (CFG-01).
    let mut open = s.settings_open;
    egui::Window::new(t(K::Settings))
        .open(&mut open)
        .default_width(420.0)
        .vscroll(true)
        .show(ctx, |ui| {
            let before_theme = (s.settings.theme, s.settings.font_scale, s.settings.language);
            egui::Grid::new("settings_general").num_columns(2).show(ui, |ui| {
                ui.label(t(K::Theme));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.settings.theme, Theme::Dark, t(K::Dark));
                    ui.radio_value(&mut s.settings.theme, Theme::Light, t(K::Light));
                });
                ui.end_row();
                ui.label(t(K::Language));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut s.settings.language, Language::System, t(K::System));
                    ui.radio_value(&mut s.settings.language, Language::English, "English");
                    ui.radio_value(&mut s.settings.language, Language::French, "Français");
                });
                ui.end_row();
                ui.label(t(K::FontScale));
                ui.add(egui::Slider::new(&mut s.settings.font_scale, 0.7..=2.0));
                ui.end_row();
                ui.label(t(K::ApproachRadius));
                ui.add(egui::Slider::new(&mut s.settings.approach_radius_nm, 5.0..=80.0));
                ui.end_row();
                ui.label(t(K::Aircraft));
                ui.add(egui::Slider::new(&mut s.settings.aircraft_size, 3.0..=16.0));
                ui.end_row();
            });
            ui.checkbox(&mut s.settings.invert_mouse_buttons, t(K::InvertMouse));
            ui.checkbox(&mut s.settings.hide_unstaffed, t(K::HideUnstaffed));
            ui.checkbox(&mut s.settings.show_ground_traffic, "Ground traffic / Trafic au sol");
            ui.checkbox(&mut s.settings.show_idl, "IDL");
            ui.horizontal(|ui| {
                ui.checkbox(&mut s.settings.label_altitude, t(K::Altitude));
                ui.checkbox(&mut s.settings.label_speed, t(K::Groundspeed));
            });
            ui.separator();
            ui.checkbox(&mut s.settings.notifications, t(K::Notifications));
            ui.checkbox(&mut s.settings.auto_check_updates, t(K::AutoCheckUpdates));
            ui.checkbox(&mut s.settings.alert_followed_cids, t(K::FollowedCids));
            ui.checkbox(&mut s.settings.alert_favorite_atc, t(K::Favorites));
            let mut cids = s
                .settings
                .followed_cids
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            if ui
                .add(egui::TextEdit::singleline(&mut cids).hint_text("CID, CID"))
                .changed()
            {
                s.settings.followed_cids = cids.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
            }
            ui.separator();
            egui::CollapsingHeader::new(t(K::Colors)).show(ui, |ui| {
                let c = s.settings.colors_mut();
                let edit = |ui: &mut Ui, label: &str, v: &mut [u8; 4]| {
                    ui.horizontal(|ui| {
                        ui.color_edit_button_srgba_unmultiplied(v);
                        ui.label(label);
                    });
                };
                edit(ui, "Background", &mut c.background);
                edit(ui, "Land", &mut c.land);
                edit(ui, "Coastline", &mut c.coastline);
                edit(ui, "Borders", &mut c.borders);
                edit(ui, "FIR (inactive)", &mut c.fir_inactive);
                edit(ui, "FIR (active)", &mut c.fir_active);
                edit(ui, "FIR fill (active)", &mut c.fir_active_fill);
                edit(ui, "UIR (active)", &mut c.uir_active);
                edit(ui, "Aircraft", &mut c.aircraft);
                edit(ui, "Aircraft (selected)", &mut c.aircraft_selected);
                edit(ui, "Aircraft label", &mut c.aircraft_label);
                edit(ui, "DEL", &mut c.airport_delivery);
                edit(ui, "GND", &mut c.airport_ground);
                edit(ui, "TWR", &mut c.airport_tower);
                edit(ui, "APP", &mut c.airport_approach);
                edit(ui, "ATIS", &mut c.airport_atis);
                edit(ui, "Airport label", &mut c.airport_label);
                edit(ui, "FIR label", &mut c.fir_label);
                edit(ui, "Route", &mut c.route);
                edit(ui, "Tooltip background", &mut c.tooltip_background);
                edit(ui, "Tooltip text", &mut c.tooltip_text);
            });
            ui.separator();
            ui.label(t(K::DataDir));
            let mut dir = s
                .settings
                .data_dir_override
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            if ui.text_edit_singleline(&mut dir).changed() {
                s.settings.data_dir_override = if dir.trim().is_empty() {
                    None
                } else {
                    Some(dir.trim().into())
                };
            }
            if ui.button(t(K::UpdateData)).clicked() {
                s.backend.send(crate::backend::Command::UpdateData { force: true });
            }
            ui.separator();
            if ui.button(t(K::RestoreDefaults)).clicked() {
                s.settings.restore_defaults();
            }
            if before_theme != (s.settings.theme, s.settings.font_scale, s.settings.language) {
                crate::i18n::set_language(s.settings.language);
                crate::app::apply_style(ctx, &s.settings);
            }
        });
    s.settings_open = open;

    // Filters editor (FLT-01).
    let mut open = s.filters_open;
    egui::Window::new(t(K::Filters))
        .open(&mut open)
        .default_width(460.0)
        .vscroll(true)
        .show(ctx, |ui| {
            let mut remove = None;
            let mut show = None;
            for (i, f) in s.settings.filters.iter_mut().enumerate() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut f.name);
                        let mut has_color = f.color.is_some();
                        if ui.checkbox(&mut has_color, "").changed() {
                            f.color = has_color.then(|| "#FF6F00".into());
                        }
                        if let Some(c) = f.color.as_mut() {
                            let mut rgba = parse_hex(c).unwrap_or([255, 111, 0, 255]);
                            if ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed() {
                                *c = to_hex(rgba);
                            }
                        }
                        if ui.button(t(K::Results)).clicked() {
                            show = Some(i);
                        }
                        if ui.button(t(K::Delete)).clicked() {
                            remove = Some(i);
                        }
                    });
                    let mut cids = f.cids.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(", ");
                    ui.horizontal(|ui| {
                        ui.label("CID");
                        if ui.text_edit_singleline(&mut cids).changed() {
                            f.cids = cids.split([',', ' ']).filter_map(|x| x.trim().parse().ok()).collect();
                        }
                    });
                    let mut fl = csv(&f.flight_patterns);
                    ui.horizontal(|ui| {
                        ui.label(t(K::Flights));
                        if ui
                            .add(egui::TextEdit::singleline(&mut fl).hint_text("ACA*, *123"))
                            .changed()
                        {
                            f.flight_patterns = parse_csv(&fl);
                        }
                    });
                    let mut atc = csv(&f.atc_patterns);
                    ui.horizontal(|ui| {
                        ui.label("ATC");
                        if ui
                            .add(egui::TextEdit::singleline(&mut atc).hint_text("CZUL*, *_CTR"))
                            .changed()
                        {
                            f.atc_patterns = parse_csv(&atc);
                        }
                    });
                });
            }
            if let Some(i) = remove {
                s.settings.filters.remove(i);
            }
            if let Some(i) = show {
                s.open_tab(Tab::FilterResults(i));
            }
            if ui.button(format!("+ {}", t(K::NewFilter))).clicked() {
                s.settings.filters.push(Filter {
                    name: t(K::NewFilter).into(),
                    ..Default::default()
                });
            }
        });
    s.filters_open = open;

    // Unified search (JMP-02).
    if s.search_open {
        let mut open = true;
        let mut close = false;
        egui::Window::new(t(K::Search))
            .open(&mut open)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 60.0])
            .default_width(480.0)
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut s.search_text)
                        .hint_text("callsign, CID, name, ICAO, IATA")
                        .desired_width(f32::INFINITY),
                );
                r.request_focus();
                let data = s.data_full();
                let results = crate::search::search(&data, s.snapshot.as_deref(), &s.search_text, 30);
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                for (k, (sel, label)) in results.iter().enumerate() {
                    if ui.selectable_label(false, label).clicked() || (enter && k == 0) {
                        s.center_on(sel);
                        s.select(sel.clone(), true);
                        close = true;
                    }
                }
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
        s.search_open = open && !close;
        if !s.search_open {
            s.search_text.clear();
        }
    }

    // Bookmark name prompt.
    if let Some(mut name) = s.bookmark_prompt.take() {
        let mut keep = true;
        egui::Window::new(t(K::AddBookmark)).collapsible(false).show(ctx, |ui| {
            ui.text_edit_singleline(&mut name);
            ui.horizontal(|ui| {
                if ui.button(t(K::Save)).clicked() && !name.trim().is_empty() {
                    s.settings.bookmarks.push(crate::settings::Bookmark {
                        name: name.trim().into(),
                        camera: s.map.camera,
                    });
                    keep = false;
                }
                if ui.button(t(K::Cancel)).clicked() {
                    keep = false;
                }
            });
        });
        if keep {
            s.bookmark_prompt = Some(name);
        }
    }

    // Favourite confirmation (FAV-01).
    if let Some(fav) = s.favorite_confirm.clone() {
        egui::Window::new(t(K::AddFavorite))
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!("{}  {}", t(K::ConfirmAddFavorite), fav.code()));
                ui.horizontal(|ui| {
                    if ui.button(t(K::Add)).clicked() {
                        if !s.settings.favorites.contains(&fav) {
                            s.settings.favorites.push(fav.clone());
                        }
                        s.favorite_confirm = None;
                    }
                    if ui.button(t(K::Cancel)).clicked() {
                        s.favorite_confirm = None;
                    }
                });
            });
    }

    // About.
    let mut open = s.about_open;
    egui::Window::new(t(K::About))
        .open(&mut open)
        .default_width(460.0)
        .show(ctx, |ui| {
            ui.heading(format!("RadarVector {}", env!("CARGO_PKG_VERSION")));
            ui.label("A native VATSIM traffic viewer. Code licensed MIT OR Apache-2.0.");
            ui.label("Not affiliated with VATSIM.");
            ui.separator();
            ui.label(RichText::new("Data").strong());
            ui.horizontal_wrapped(|ui| {
                ui.label("Upstream boundary and airport data:");
                ui.hyperlink_to(
                    "community boundary data project",
                    "https://github.com/vatsimnetwork/vatspy-data-project",
                );
                ui.label(format!("({}) © its contributors, CC-BY-SA-4.0.", s.data_tag));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("TRACON boundaries:");
                ui.hyperlink_to(
                    "SimAware TRACON Project",
                    "https://github.com/vatsimnetwork/simaware-tracon-project",
                );
                ui.label("© its contributors, CC-BY-SA-4.0.");
            });
            ui.hyperlink_to("CC-BY-SA-4.0", "https://creativecommons.org/licenses/by-sa/4.0/");
            ui.label("Made with Natural Earth (public domain).");
            ui.label("Live data: VATSIM public data feeds.");
            ui.separator();
            ui.label(
                RichText::new("Third-party Rust crates: see THIRD_PARTY_LICENSES.html shipped with the app.").small(),
            );
            if !s.log.is_empty() {
                egui::CollapsingHeader::new("Log").show(ui, |ui| {
                    for l in s.log.iter().rev().take(30) {
                        ui.label(RichText::new(l).small().monospace());
                    }
                });
            }
        });
    s.about_open = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn online_duration() {
        assert_eq!(online_for("garbage"), "");
        let s = online_for("2000-01-01T00:00:00Z");
        assert!(s.contains(':'));
    }

    #[test]
    fn csv_parsing() {
        assert_eq!(parse_csv("aca*, *_ctr ;x"), vec!["ACA*", "*_CTR", "X"]);
    }
}
