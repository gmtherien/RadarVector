//! Immutable network snapshot assembled from static data and the v3 feed.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use radarvector_data::StaticData;
use radarvector_feed::models::{DataFeed, General, Prefile, RefEntry, Server};
use radarvector_feed::{Controller, Pilot};
use rstar::RTree;
use rstar::primitives::GeomWithData;
use serde::Serialize;

use crate::aircraft::aircraft_type;
use crate::callsign::{self, FirTarget, Resolved, StationKind, TraconLookup, Variant};

/// An ATC station (controller or ATIS) with its resolution.
#[derive(Debug, Clone, Serialize)]
pub struct Station {
    pub data: Controller,
    pub kind: StationKind,
    pub resolved: Resolved,
    pub is_atis: bool,
}

/// Which airport services are open.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AirportActivity {
    pub delivery: bool,
    pub ground: bool,
    pub tower: bool,
    pub approach: bool,
    pub atis: bool,
    /// Indices into [`NetworkSnapshot::stations`].
    pub stations: Vec<usize>,
}

impl AirportActivity {
    pub fn any_controlled(&self) -> bool {
        self.delivery || self.ground || self.tower || self.approach
    }
}

/// Activity on one boundary id.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BoundaryActivity {
    /// Which variants are staffed.
    pub variants: BTreeSet<VariantKey>,
    pub stations: Vec<usize>,
    /// True when lit only through a UIR.
    pub via_uir_only: bool,
}

/// Orderable mirror of [`Variant`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum VariantKey {
    All,
    Domestic,
    Oceanic,
}

impl From<Variant> for VariantKey {
    fn from(v: Variant) -> Self {
        match v {
            Variant::All => Self::All,
            Variant::Domestic => Self::Domestic,
            Variant::Oceanic => Self::Oceanic,
        }
    }
}

impl BoundaryActivity {
    /// Should a feature with this oceanic flag be drawn as active?
    pub fn lights(&self, oceanic: bool) -> bool {
        self.variants.contains(&VariantKey::All)
            || self.variants.contains(&if oceanic {
                VariantKey::Oceanic
            } else {
                VariantKey::Domestic
            })
    }
}

/// R-tree entry for a pilot: `[lon, lat]` + index into `pilots`.
pub type PilotPoint = GeomWithData<[f64; 2], usize>;

#[derive(Debug, Default)]
pub struct NetworkSnapshot {
    pub general: General,
    pub pilots: Vec<Pilot>,
    /// Aircraft type per pilot (same index), without equipment codes.
    pub aircraft_types: Vec<String>,
    pub stations: Vec<Station>,
    pub servers: Vec<Server>,
    pub prefiles: Vec<Prefile>,
    pub facilities: Vec<RefEntry>,
    pub ratings: Vec<RefEntry>,
    pub pilot_ratings: Vec<RefEntry>,
    pub military_ratings: Vec<RefEntry>,
    pub airports: HashMap<String, AirportActivity>,
    pub boundaries: HashMap<String, BoundaryActivity>,
    pub pilot_tree: RTree<PilotPoint>,
}

impl NetworkSnapshot {
    pub fn build(data: &StaticData, feed: DataFeed) -> Self {
        Self::build_with(data, &(), feed)
    }

    pub fn build_with(data: &StaticData, tracons: &dyn TraconLookup, feed: DataFeed) -> Self {
        let mut s = NetworkSnapshot {
            general: feed.general,
            aircraft_types: feed
                .pilots
                .iter()
                .map(|p| p.flight_plan.as_ref().map(aircraft_type).unwrap_or_default())
                .collect(),
            servers: feed.servers,
            prefiles: feed.prefiles,
            facilities: feed.facilities,
            ratings: feed.ratings,
            pilot_ratings: feed.pilot_ratings,
            military_ratings: feed.military_ratings,
            ..Default::default()
        };
        let stations = feed
            .controllers
            .into_iter()
            .map(|c| (c, false))
            .chain(feed.atis.into_iter().map(|c| (c, true)));
        for (c, is_atis) in stations {
            let (kind, resolved) = callsign::resolve_with(data, tracons, &c.callsign);
            let idx = s.stations.len();
            match &resolved {
                Resolved::Airport { icao } | Resolved::Approach { icao, .. } => {
                    let a = s.airports.entry(icao.clone()).or_default();
                    match kind {
                        StationKind::Delivery => a.delivery = true,
                        StationKind::Ground => a.ground = true,
                        StationKind::Tower => a.tower = true,
                        StationKind::Approach | StationKind::Departure => a.approach = true,
                        StationKind::Atis => a.atis = true,
                        _ => {}
                    }
                    a.stations.push(idx);
                }
                Resolved::Fir(targets) => mark(&mut s.boundaries, targets, idx, false),
                Resolved::Uir { firs, .. } => mark(&mut s.boundaries, firs, idx, true),
                Resolved::Unresolved => {}
            }
            s.stations.push(Station {
                data: c,
                kind,
                resolved,
                is_atis,
            });
        }
        s.pilots = feed.pilots;
        s.pilot_tree = RTree::bulk_load(
            s.pilots
                .iter()
                .enumerate()
                .filter(|(_, p)| p.latitude.is_finite() && p.longitude.is_finite())
                .map(|(i, p)| PilotPoint::new([p.longitude, p.latitude], i))
                .collect(),
        );
        s
    }

    /// Stations with a real ATC suffix that could not be placed on the map.
    pub fn unresolved(&self) -> impl Iterator<Item = &Station> {
        self.stations
            .iter()
            .filter(|s| !s.resolved.is_resolved() && !matches!(s.kind, StationKind::Observer | StationKind::Other))
    }

    /// Stations that should be placed on the map (ATC suffix, not OBS).
    pub fn placeable(&self) -> impl Iterator<Item = &Station> {
        self.stations
            .iter()
            .filter(|s| !matches!(s.kind, StationKind::Observer | StationKind::Other))
    }

    pub fn resolution_rate(&self) -> f64 {
        let total = self.placeable().count();
        if total == 0 {
            return 1.0;
        }
        1.0 - self.unresolved().count() as f64 / total as f64
    }

    pub fn pilot_by_callsign(&self, callsign: &str) -> Option<&Pilot> {
        self.pilots.iter().find(|p| p.callsign.eq_ignore_ascii_case(callsign))
    }

    pub fn station_by_callsign(&self, callsign: &str) -> Option<&Station> {
        self.stations
            .iter()
            .find(|s| s.data.callsign.eq_ignore_ascii_case(callsign))
    }

    /// Pilots whose flight plan departs from / arrives at `icao`.
    pub fn departures(&self, icao: &str) -> impl Iterator<Item = &Pilot> {
        self.pilots.iter().filter(move |p| {
            p.flight_plan
                .as_ref()
                .is_some_and(|f| f.departure.eq_ignore_ascii_case(icao))
        })
    }
    pub fn arrivals(&self, icao: &str) -> impl Iterator<Item = &Pilot> {
        self.pilots.iter().filter(move |p| {
            p.flight_plan
                .as_ref()
                .is_some_and(|f| f.arrival.eq_ignore_ascii_case(icao))
        })
    }

    pub fn facility_short(&self, id: i64) -> &str {
        self.facilities
            .iter()
            .find(|f| f.id == id)
            .map_or("", |f| f.short.as_str())
    }
    pub fn rating_short(&self, id: i64) -> &str {
        self.ratings
            .iter()
            .find(|f| f.id == id)
            .map_or("", |f| f.short.as_str())
    }
    pub fn pilot_rating_short(&self, id: i64) -> &str {
        self.pilot_ratings
            .iter()
            .find(|f| f.id == id)
            .map_or("", |f| f.short.as_str())
    }
}

fn mark(map: &mut HashMap<String, BoundaryActivity>, targets: &[FirTarget], idx: usize, via_uir: bool) {
    for t in targets {
        let e = map.entry(t.boundary_id.clone()).or_insert_with(|| BoundaryActivity {
            via_uir_only: true,
            ..Default::default()
        });
        e.variants.insert(t.variant.into());
        if !e.stations.contains(&idx) {
            e.stations.push(idx);
        }
        if !via_uir {
            e.via_uir_only = false;
        }
    }
}

/// Changes between two snapshots (for alerts and track history).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SnapshotDiff {
    pub pilots_connected: Vec<(i64, String)>,
    pub pilots_disconnected: Vec<(i64, String)>,
    pub stations_opened: Vec<(i64, String)>,
    pub stations_closed: Vec<(i64, String)>,
}

impl SnapshotDiff {
    pub fn between(old: &NetworkSnapshot, new: &NetworkSnapshot) -> Self {
        let set = |it: Vec<(i64, String)>| it.into_iter().collect::<BTreeSet<_>>();
        let p_old = set(old.pilots.iter().map(|p| (p.cid, p.callsign.clone())).collect());
        let p_new = set(new.pilots.iter().map(|p| (p.cid, p.callsign.clone())).collect());
        let s_old = set(old
            .stations
            .iter()
            .map(|s| (s.data.cid, s.data.callsign.clone()))
            .collect());
        let s_new = set(new
            .stations
            .iter()
            .map(|s| (s.data.cid, s.data.callsign.clone()))
            .collect());
        Self {
            pilots_connected: p_new.difference(&p_old).cloned().collect(),
            pilots_disconnected: p_old.difference(&p_new).cloned().collect(),
            stations_opened: s_new.difference(&s_old).cloned().collect(),
            stations_closed: s_old.difference(&s_new).cloned().collect(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.pilots_connected.is_empty()
            && self.pilots_disconnected.is_empty()
            && self.stations_opened.is_empty()
            && self.stations_closed.is_empty()
    }
}

/// Counts of unresolved callsigns across several snapshots, for diagnostics.
pub fn unresolved_histogram<'a>(snaps: impl IntoIterator<Item = &'a NetworkSnapshot>) -> BTreeMap<String, usize> {
    let mut h = BTreeMap::new();
    for s in snaps {
        for st in s.unresolved() {
            *h.entry(st.data.callsign.to_ascii_uppercase()).or_insert(0) += 1;
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use radarvector_feed::models::decode_data_feed;

    fn feed(controllers: &str, pilots: &str) -> DataFeed {
        let body = format!(
            r#"{{"general":{{"update_timestamp":"t"}},"controllers":[{controllers}],"atis":[],"pilots":[{pilots}]}}"#
        );
        decode_data_feed(body.as_bytes()).unwrap()
    }

    #[test]
    fn builds_activity() {
        let d = crate::callsign::tests_support::fixture();
        let f = feed(
            r#"{"cid":1,"callsign":"JFK_TWR"},{"cid":2,"callsign":"JFK_GND"},{"cid":3,"callsign":"NY_CTR"},
               {"cid":4,"callsign":"ZZZ_CTR"},{"cid":5,"callsign":"AB_OBS"},{"cid":6,"callsign":"AFRE_E_CTR"}"#,
            r#"{"cid":10,"callsign":"ACA1","latitude":45.0,"longitude":-73.0,"flight_plan":{"aircraft":"H/B744/L","departure":"CYUL","arrival":"KJFK"}}"#,
        );
        let s = NetworkSnapshot::build(&d, f);
        let jfk = &s.airports["KJFK"];
        assert!(jfk.tower && jfk.ground && !jfk.approach);
        assert!(s.boundaries["KZNY"].lights(false));
        assert!(!s.boundaries["KZNY"].lights(true));
        assert!(s.boundaries["HAAA"].via_uir_only);
        assert_eq!(s.unresolved().count(), 1);
        assert_eq!(s.placeable().count(), 5);
        assert!((s.resolution_rate() - 0.8).abs() < 1e-9);
        assert_eq!(s.aircraft_types[0], "B744");
        assert_eq!(s.arrivals("kjfk").count(), 1);
        assert_eq!(s.pilot_tree.size(), 1);
    }

    #[test]
    fn diff() {
        let d = crate::callsign::tests_support::fixture();
        let a = NetworkSnapshot::build(
            &d,
            feed(r#"{"cid":1,"callsign":"JFK_TWR"}"#, r#"{"cid":10,"callsign":"ACA1"}"#),
        );
        let b = NetworkSnapshot::build(
            &d,
            feed(
                r#"{"cid":2,"callsign":"MTL_CTR"}"#,
                r#"{"cid":10,"callsign":"ACA1"},{"cid":11,"callsign":"WJA2"}"#,
            ),
        );
        let df = SnapshotDiff::between(&a, &b);
        assert_eq!(df.pilots_connected, vec![(11, "WJA2".to_string())]);
        assert!(df.pilots_disconnected.is_empty());
        assert_eq!(df.stations_opened, vec![(2, "MTL_CTR".to_string())]);
        assert_eq!(df.stations_closed, vec![(1, "JFK_TWR".to_string())]);
        assert!(SnapshotDiff::between(&a, &a).is_empty());
    }
}
