//! Resolve ATC callsigns to map entities (FIR boundary, UIR, airport).
//!
//! Rules written from the structure of the VAT-Spy Data Project files
//! (see docs/DATA-SOURCES.md); they are validated by measuring the
//! resolution rate on live snapshots, not by observing other software.

use radarvector_data::StaticData;
use serde::Serialize;

/// Station type, from the callsign suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum StationKind {
    Delivery,
    Ground,
    Tower,
    Approach,
    Departure,
    Center,
    FlightService,
    Atis,
    Observer,
    /// No recognised suffix (supervisors, events, pilots in the ATC list...).
    Other,
}

impl StationKind {
    pub fn from_suffix(s: &str) -> Option<Self> {
        Some(match s {
            "DEL" => Self::Delivery,
            "GND" => Self::Ground,
            "TWR" => Self::Tower,
            "APP" => Self::Approach,
            "DEP" => Self::Departure,
            "CTR" => Self::Center,
            "FSS" => Self::FlightService,
            "ATIS" => Self::Atis,
            "OBS" => Self::Observer,
            _ => return None,
        })
    }

    /// Facility id of the v3 feed `facilities` table this kind corresponds to
    /// (0 OBS, 1 FSS, 2 DEL, 3 GND, 4 TWR, 5 APP, 6 CTR). ATIS has none.
    pub fn facility_id(self) -> Option<i64> {
        Some(match self {
            Self::Observer => 0,
            Self::FlightService => 1,
            Self::Delivery => 2,
            Self::Ground => 3,
            Self::Tower => 4,
            Self::Approach | Self::Departure => 5,
            Self::Center => 6,
            Self::Atis | Self::Other => return None,
        })
    }

    pub fn is_enroute(self) -> bool {
        matches!(self, Self::Center | Self::FlightService)
    }

    pub fn is_airport(self) -> bool {
        matches!(
            self,
            Self::Delivery | Self::Ground | Self::Tower | Self::Approach | Self::Departure | Self::Atis
        )
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Delivery => "DEL",
            Self::Ground => "GND",
            Self::Tower => "TWR",
            Self::Approach => "APP",
            Self::Departure => "DEP",
            Self::Center => "CTR",
            Self::FlightService => "FSS",
            Self::Atis => "ATIS",
            Self::Observer => "OBS",
            Self::Other => "",
        }
    }
}

/// Which variant of a boundary id to light when it has both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum Variant {
    /// Only one variant exists, or no preference: all features.
    All,
    Domestic,
    Oceanic,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct FirTarget {
    pub fir_icao: String,
    pub boundary_id: String,
    pub variant: Variant,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Resolved {
    /// One or more FIR boundaries (several when a prefix maps to several FIRs).
    Fir(Vec<FirTarget>),
    /// A UIR: every listed FIR lights up.
    Uir {
        id: String,
        firs: Vec<FirTarget>,
    },
    /// DEL, GND, TWR, ATIS at an airport.
    Airport {
        icao: String,
    },
    /// APP or DEP: drawn as a TRACON polygon when known, else a circle.
    Approach {
        icao: String,
        tracon: Option<String>,
    },
    Unresolved,
}

impl Resolved {
    pub fn is_resolved(&self) -> bool {
        !matches!(self, Self::Unresolved)
    }
    pub fn airport_icao(&self) -> Option<&str> {
        match self {
            Self::Airport { icao } | Self::Approach { icao, .. } => Some(icao),
            _ => None,
        }
    }
}

/// Split a callsign into upper-case segments (empty segments dropped, so
/// `EDDS__GND` works) and its station kind.
pub fn split(callsign: &str) -> (Vec<String>, StationKind) {
    let mut parts: Vec<String> = callsign
        .trim()
        .to_ascii_uppercase()
        .split(['_', '-'])
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let kind = parts.last().and_then(|s| StationKind::from_suffix(s));
    match kind {
        Some(k) if parts.len() > 1 => {
            parts.pop();
            (parts, k)
        }
        _ => (parts, StationKind::Other),
    }
}

/// Optional extension point for TRACON lookups (milestone J5).
pub trait TraconLookup {
    /// Return a TRACON id for the callsign key segments and kind.
    fn find(&self, segments: &[String], kind: StationKind) -> Option<String>;
}

impl TraconLookup for () {
    fn find(&self, _: &[String], _: StationKind) -> Option<String> {
        None
    }
}

fn targets_for(data: &StaticData, firs: Vec<&radarvector_data::Fir>, kind: StationKind) -> Vec<FirTarget> {
    let mut out: Vec<FirTarget> = Vec::new();
    for f in firs {
        let feats = data.boundaries.get(&f.boundary_id);
        let has_oceanic = feats.iter().any(|x| x.oceanic);
        let has_domestic = feats.iter().any(|x| !x.oceanic);
        let variant = if has_oceanic && has_domestic {
            if kind == StationKind::FlightService {
                Variant::Oceanic
            } else {
                Variant::Domestic
            }
        } else {
            Variant::All
        };
        let t = FirTarget {
            fir_icao: f.icao.clone(),
            boundary_id: f.boundary_id.clone(),
            variant,
        };
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// Resolve an enroute key (`LON_DK`, `EGTT_I`, `MTL`...). Tries the full
/// key then drops trailing segments.
fn resolve_enroute(data: &StaticData, segs: &[String], kind: StationKind) -> Resolved {
    for n in (1..=segs.len()).rev() {
        let key = segs[..n].join("_");
        let by_prefix: Vec<_> = data.firs_by_prefix(&key).collect();
        if !by_prefix.is_empty() {
            return Resolved::Fir(targets_for(data, by_prefix, kind));
        }
        // FIR ICAOs may contain '-' (e.g. `ADR-E`); try that spelling too.
        for k in [key.clone(), segs[..n].join("-")] {
            let by_icao: Vec<_> = data.firs_by_icao(&k).collect();
            if !by_icao.is_empty() {
                // One line per prefix share the same boundary; keep distinct ones.
                return Resolved::Fir(targets_for(data, by_icao, kind));
            }
        }
        // US ARTCC callsigns use the FAA id (`ZJX`) of ICAO `KZJX`.
        if key.len() == 3 && key.starts_with('Z') {
            let by_icao: Vec<_> = data.firs_by_icao(&format!("K{key}")).collect();
            if !by_icao.is_empty() {
                return Resolved::Fir(targets_for(data, by_icao, kind));
            }
        }
        if let Some(u) = data.uir(&key) {
            let firs = u.firs.iter().flat_map(|f| data.firs_by_icao(f).take(1)).collect();
            return Resolved::Uir {
                id: u.id.clone(),
                firs: targets_for(data, firs, kind),
            };
        }
    }
    Resolved::Unresolved
}

fn resolve_airport_code(data: &StaticData, code: &str) -> Option<String> {
    if let Some(a) = data.airport(code) {
        return Some(a.icao.clone());
    }
    let mut matches: Vec<_> = data.airports_by_iata(code).collect();
    matches.sort_by_key(|a| a.is_pseudo);
    matches.first().map(|a| a.icao.clone())
}

/// Resolve a callsign with no TRACON data.
pub fn resolve(data: &StaticData, callsign: &str) -> (StationKind, Resolved) {
    resolve_with(data, &(), callsign)
}

pub fn resolve_with(data: &StaticData, tracons: &dyn TraconLookup, callsign: &str) -> (StationKind, Resolved) {
    let (segs, kind) = split(callsign);
    if segs.is_empty() {
        return (kind, Resolved::Unresolved);
    }
    let r = match kind {
        StationKind::Center | StationKind::FlightService => resolve_enroute(data, &segs, kind),
        StationKind::Approach | StationKind::Departure => match resolve_airport_code(data, &segs[0]) {
            Some(icao) => Resolved::Approach {
                icao,
                tracon: tracons.find(&segs, kind),
            },
            None => Resolved::Unresolved,
        },
        StationKind::Delivery | StationKind::Ground | StationKind::Tower | StationKind::Atis => {
            match resolve_airport_code(data, &segs[0]) {
                Some(icao) => Resolved::Airport { icao },
                None => Resolved::Unresolved,
            }
        }
        StationKind::Observer | StationKind::Other => Resolved::Unresolved,
    };
    (kind, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn fixture() -> StaticData {
        let dat = "[Countries]\nUnited Kingdom|EG|\n[Airports]\n\
KJFK|New York-John F. Kennedy Intl NY|40.63975|-73.778925|JFK|KZNY|0\n\
EGLC|London City|51.505256|0.054369||EGTT|0\n\
EGLC|Thames|51.505256|0.054369|THAMES|EGTT|1\n\
CYYZ|Toronto-Pearson Intl|43.677222|-79.630556|TOR|CZYZ|0\n\
CYYZ|Toronto-Pearson Intl|43.677222|-79.630556|YYZ|CZYZ|1\n\
CYUL|Montreal|45.47|-73.74|YUL|CZUL|0\n\
[FIRs]\nEGTT|London|LON|EGTT\nEGTT|London||EGTT\nEGTT-D|London ACC (Dover)|LON_DK|EGTT-D\n\
CZUL|Montreal|MTL|CZUL\nCZUL|Montreal||CZUL\nKZNY|New York|NY|KZNY\nKZNY|New York||KZNY\n\
HAAA|Addis|ADD|HAAA\nKZJX|Jacksonville|JAX|KZJX\nHCSM|Mogadishu||HCSM\n\
[UIRs]\nAFRE_E|East Africa Control|HAAA,HCSM\n";
        let poly = |id: &str, oc: &str| {
            format!(
                r#"{{"type":"Feature","properties":{{"id":"{id}","oceanic":"{oc}"}},"geometry":{{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}}}}"#
            )
        };
        let geo = format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            [
                poly("EGTT", "0"),
                poly("EGTT-D", "0"),
                poly("CZUL", "0"),
                poly("KZNY", "0"),
                poly("KZNY", "1"),
                poly("HAAA", "0"),
                poly("KZJX", "0"),
                poly("HCSM", "0")
            ]
            .join(",")
        );
        StaticData::from_texts(dat, &geo).unwrap().0
    }

    fn fir_ids(r: &Resolved) -> Vec<(String, Variant)> {
        match r {
            Resolved::Fir(t) | Resolved::Uir { firs: t, .. } => {
                t.iter().map(|t| (t.boundary_id.clone(), t.variant)).collect()
            }
            _ => vec![],
        }
    }

    #[test]
    fn split_callsigns() {
        assert_eq!(split("EDDS__GND"), (vec!["EDDS".to_string()], StationKind::Ground));
        assert_eq!(split("lon_dk_ctr").1, StationKind::Center);
        assert_eq!(split("BAW12"), (vec!["BAW12".to_string()], StationKind::Other));
        assert_eq!(split("CTR").1, StationKind::Other);
    }

    #[test]
    fn enroute() {
        let d = fixture();
        assert_eq!(
            fir_ids(&resolve(&d, "LON_DK_CTR").1),
            vec![("EGTT-D".into(), Variant::All)]
        );
        assert_eq!(fir_ids(&resolve(&d, "LON_CTR").1), vec![("EGTT".into(), Variant::All)]);
        assert_eq!(
            fir_ids(&resolve(&d, "EGTT_I_CTR").1),
            vec![("EGTT".into(), Variant::All)]
        );
        assert_eq!(fir_ids(&resolve(&d, "MTL_CTR").1), vec![("CZUL".into(), Variant::All)]);
        assert_eq!(fir_ids(&resolve(&d, "CZUL_CTR").1), vec![("CZUL".into(), Variant::All)]);
        assert_eq!(
            fir_ids(&resolve(&d, "LON_DK_1_CTR").1),
            vec![("EGTT-D".into(), Variant::All)]
        );
        assert_eq!(
            fir_ids(&resolve(&d, "ZJX_T_CTR").1),
            vec![("KZJX".into(), Variant::All)]
        );
        assert_eq!(fir_ids(&resolve(&d, "JAX_CTR").1), vec![("KZJX".into(), Variant::All)]);
    }

    #[test]
    fn oceanic_variants() {
        let d = fixture();
        assert_eq!(
            fir_ids(&resolve(&d, "NY_CTR").1),
            vec![("KZNY".into(), Variant::Domestic)]
        );
        assert_eq!(
            fir_ids(&resolve(&d, "KZNY_FSS").1),
            vec![("KZNY".into(), Variant::Oceanic)]
        );
    }

    #[test]
    fn uir() {
        let d = fixture();
        let (_, r) = resolve(&d, "AFRE_E_CTR");
        assert!(matches!(&r, Resolved::Uir { id, .. } if id == "AFRE_E"));
        assert_eq!(fir_ids(&r).len(), 2);
    }

    #[test]
    fn airports() {
        let d = fixture();
        assert_eq!(resolve(&d, "JFK_TWR").1, Resolved::Airport { icao: "KJFK".into() });
        assert_eq!(resolve(&d, "KJFK_GND").1, Resolved::Airport { icao: "KJFK".into() });
        assert_eq!(
            resolve(&d, "THAMES_APP").1,
            Resolved::Approach {
                icao: "EGLC".into(),
                tracon: None
            }
        );
        assert_eq!(resolve(&d, "YYZ_ATIS").1, Resolved::Airport { icao: "CYYZ".into() });
        assert_eq!(
            resolve(&d, "TOR_N_DEP"),
            (
                StationKind::Departure,
                Resolved::Approach {
                    icao: "CYYZ".into(),
                    tracon: None
                }
            )
        );
    }

    #[test]
    fn unresolved() {
        let d = fixture();
        assert_eq!(resolve(&d, "ZZZZ_CTR").1, Resolved::Unresolved);
        assert_eq!(resolve(&d, "QQQ_TWR").1, Resolved::Unresolved);
        assert_eq!(resolve(&d, "JD_OBS"), (StationKind::Observer, Resolved::Unresolved));
        assert_eq!(resolve(&d, "").1, Resolved::Unresolved);
    }
}

#[cfg(test)]
pub(crate) mod tests_support {
    pub(crate) use super::tests::fixture;
}

/// TRACON lookup on SimAware data: tries the full key then drops trailing
/// segments (`LAX_U_APP` → `LAX_U`, then `LAX`). Returns the matched prefix.
impl TraconLookup for radarvector_data::tracon::Tracons {
    fn find(&self, segments: &[String], kind: StationKind) -> Option<String> {
        let dep = kind == StationKind::Departure;
        (1..=segments.len())
            .rev()
            .map(|n| segments[..n].join("_"))
            .find(|k| !radarvector_data::tracon::Tracons::find(self, k, dep).is_empty())
    }
}

#[cfg(test)]
mod tracon_tests {
    use super::*;

    #[test]
    fn approach_uses_tracon_prefixes() {
        let data = tests_support::fixture();
        let geo = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"id":"N90","prefix":["JFK","NY"],"name":"New York Approach"},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}},
          {"type":"Feature","properties":{"id":"N90","prefix":["JFK"],"suffix":"DEP","name":"New York Departure"},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}},
          {"type":"Feature","properties":{"id":"YYZ","prefix":["TOR_N"],"name":"Toronto North"},"geometry":{"type":"Polygon","coordinates":[[[0,0],[1,0],[1,1],[0,0]]]}}]}"#;
        let (tr, _) = radarvector_data::tracon::parse(geo).unwrap();
        let r = |cs: &str| resolve_with(&data, &tr, cs).1;
        assert_eq!(
            r("JFK_APP"),
            Resolved::Approach {
                icao: "KJFK".into(),
                tracon: Some("JFK".into())
            }
        );
        assert_eq!(
            r("JFK_U_DEP"),
            Resolved::Approach {
                icao: "KJFK".into(),
                tracon: Some("JFK".into())
            }
        );
        assert_eq!(
            r("TOR_N_APP"),
            Resolved::Approach {
                icao: "CYYZ".into(),
                tracon: Some("TOR_N".into())
            }
        );
        assert_eq!(
            r("TOR_S_APP"),
            Resolved::Approach {
                icao: "CYYZ".into(),
                tracon: None
            }
        );
        assert_eq!(
            r("YYZ_APP"),
            Resolved::Approach {
                icao: "CYYZ".into(),
                tracon: None
            }
        );
    }
}
