//! Typed models of the public VATSIM data feeds.
//!
//! Schemas checked against live responses of `status.json`, the v3 data
//! feed and the transceivers feed on 2026-11-15 (see PROVENANCE.md).
//! Decoding is tolerant: every field has a default, unknown fields are
//! ignored, and each array element is decoded on its own so one bad record
//! never invalidates the batch.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `https://status.vatsim.net/status.json`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Status {
    pub data: StatusData,
    pub user: Vec<String>,
    pub metar: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StatusData {
    pub v3: Vec<String>,
    pub transceivers: Vec<String>,
    pub servers: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct General {
    pub version: i64,
    pub update_timestamp: String,
    pub connected_clients: i64,
    pub unique_users: i64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct FlightPlan {
    pub flight_rules: String,
    pub aircraft: String,
    pub aircraft_faa: String,
    pub aircraft_short: String,
    pub departure: String,
    pub arrival: String,
    pub alternate: String,
    pub cruise_tas: String,
    pub altitude: String,
    pub deptime: String,
    pub enroute_time: String,
    pub fuel_time: String,
    pub remarks: String,
    pub route: String,
    pub revision_id: i64,
    pub assigned_transponder: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Pilot {
    pub cid: i64,
    pub name: String,
    pub callsign: String,
    pub server: String,
    pub pilot_rating: i64,
    pub military_rating: i64,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: i64,
    pub groundspeed: i64,
    pub transponder: String,
    pub heading: i64,
    pub qnh_i_hg: f64,
    pub qnh_mb: i64,
    pub logon_time: String,
    pub last_updated: String,
    pub flight_plan: Option<FlightPlan>,
}

/// Controllers and ATIS share this shape (ATIS adds `atis_code`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Controller {
    pub cid: i64,
    pub name: String,
    pub callsign: String,
    pub frequency: String,
    pub facility: i64,
    pub rating: i64,
    pub server: String,
    pub visual_range: i64,
    pub atis_code: Option<String>,
    pub text_atis: Option<Vec<String>>,
    pub logon_time: String,
    pub last_updated: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Server {
    pub ident: String,
    pub hostname_or_ip: String,
    pub location: String,
    pub name: String,
    pub client_connections_allowed: bool,
    pub is_sweatbox: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Prefile {
    pub cid: i64,
    pub name: String,
    pub callsign: String,
    pub flight_plan: Option<FlightPlan>,
    pub last_updated: String,
}

/// Entry of `facilities` / `ratings` (`short`, `long`) or
/// `pilot_ratings` / `military_ratings` (`short_name`, `long_name`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct RefEntry {
    pub id: i64,
    #[serde(alias = "short_name")]
    pub short: String,
    #[serde(alias = "long_name")]
    pub long: String,
}

/// The decoded v3 data feed.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DataFeed {
    pub general: General,
    pub pilots: Vec<Pilot>,
    pub controllers: Vec<Controller>,
    pub atis: Vec<Controller>,
    pub servers: Vec<Server>,
    pub prefiles: Vec<Prefile>,
    pub facilities: Vec<RefEntry>,
    pub ratings: Vec<RefEntry>,
    pub pilot_ratings: Vec<RefEntry>,
    pub military_ratings: Vec<RefEntry>,
    /// Records that could not be decoded, per array name.
    pub skipped: Vec<(String, usize)>,
}

impl DataFeed {
    pub fn facility_short(&self, id: i64) -> Option<&str> {
        self.facilities.iter().find(|f| f.id == id).map(|f| f.short.as_str())
    }
    pub fn rating_short(&self, id: i64) -> Option<&str> {
        self.ratings.iter().find(|f| f.id == id).map(|f| f.short.as_str())
    }
    pub fn pilot_rating_short(&self, id: i64) -> Option<&str> {
        self.pilot_ratings.iter().find(|f| f.id == id).map(|f| f.short.as_str())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Transceiver {
    pub id: i64,
    pub frequency: i64,
    #[serde(rename = "latDeg")]
    pub lat: f64,
    #[serde(rename = "lonDeg")]
    pub lon: f64,
    #[serde(rename = "heightMslM")]
    pub height_msl_m: f64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct StationTransceivers {
    pub callsign: String,
    pub transceivers: Vec<Transceiver>,
}

fn decode_array<T: DeserializeOwned>(root: &Value, key: &str, skipped: &mut Vec<(String, usize)>) -> Vec<T> {
    let Some(arr) = root.get(key).and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut bad = 0;
    let out = arr
        .iter()
        .filter_map(|v| {
            let r = T::deserialize(v).ok();
            if r.is_none() {
                bad += 1;
            }
            r
        })
        .collect();
    if bad > 0 {
        skipped.push((key.to_string(), bad));
    }
    out
}

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("JSON root is not an object")]
    NotObject,
}

/// Decode a v3 data feed body.
pub fn decode_data_feed(bytes: &[u8]) -> Result<DataFeed, DecodeError> {
    let root: Value = serde_json::from_slice(bytes)?;
    if !root.is_object() {
        return Err(DecodeError::NotObject);
    }
    let mut skipped = Vec::new();
    let general = root
        .get("general")
        .and_then(|g| General::deserialize(g).ok())
        .unwrap_or_default();
    Ok(DataFeed {
        general,
        pilots: decode_array(&root, "pilots", &mut skipped),
        controllers: decode_array(&root, "controllers", &mut skipped),
        atis: decode_array(&root, "atis", &mut skipped),
        servers: decode_array(&root, "servers", &mut skipped),
        prefiles: decode_array(&root, "prefiles", &mut skipped),
        facilities: decode_array(&root, "facilities", &mut skipped),
        ratings: decode_array(&root, "ratings", &mut skipped),
        pilot_ratings: decode_array(&root, "pilot_ratings", &mut skipped),
        military_ratings: decode_array(&root, "military_ratings", &mut skipped),
        skipped,
    })
}

/// Decode the transceivers feed (a top-level array).
pub fn decode_transceivers(bytes: &[u8]) -> Result<Vec<StationTransceivers>, DecodeError> {
    let root: Value = serde_json::from_slice(bytes)?;
    let arr = root.as_array().ok_or(DecodeError::NotObject)?;
    Ok(arr
        .iter()
        .filter_map(|v| StationTransceivers::deserialize(v).ok())
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_decoding() {
        let body = br#"{
          "general": {"version": 3, "update_timestamp": "2026-11-15T22:01:44Z", "connected_clients": 2, "extra": 1},
          "pilots": [
            {"cid": 1, "callsign": "ACA123", "latitude": 45.5, "longitude": -73.6, "altitude": 35000, "heading": 90,
             "flight_plan": {"aircraft_short": "A333", "departure": "CYUL", "arrival": "LFPG", "revision_id": 2}},
            {"cid": "not a number", "callsign": "BAD"},
            {"cid": 2, "callsign": "NOFP", "flight_plan": null}
          ],
          "controllers": [{"cid": 3, "callsign": "MTL_CTR", "frequency": "132.450", "facility": 6, "text_atis": null}],
          "atis": [{"cid": 4, "callsign": "CYUL_ATIS", "atis_code": "B", "text_atis": ["line 1", "line 2"]}],
          "facilities": [{"id": 6, "short": "CTR", "long": "Enroute"}],
          "pilot_ratings": [{"id": 1, "short_name": "PPL", "long_name": "Private Pilot License"}],
          "unknown_array": [1, 2, 3]
        }"#;
        let f = decode_data_feed(body).unwrap();
        assert_eq!(f.general.connected_clients, 2);
        assert_eq!(f.pilots.len(), 2);
        assert_eq!(f.skipped, vec![("pilots".to_string(), 1)]);
        assert_eq!(
            f.pilots[0].flight_plan.as_ref().map(|p| p.arrival.as_str()),
            Some("LFPG")
        );
        assert!(f.pilots[1].flight_plan.is_none());
        assert_eq!(f.atis[0].text_atis.as_ref().map(Vec::len), Some(2));
        assert_eq!(f.facility_short(6), Some("CTR"));
        assert_eq!(f.pilot_rating_short(1), Some("PPL"));
    }

    #[test]
    fn invalid_root() {
        assert!(decode_data_feed(b"[]").is_err());
        assert!(decode_data_feed(b"{").is_err());
        assert!(decode_data_feed(b"{}").is_ok());
    }

    #[test]
    fn transceivers() {
        let t = decode_transceivers(
            br#"[{"callsign":"BAW15","transceivers":[{"id":1,"frequency":122800013,"latDeg":50.3,"lonDeg":7.3,"heightMslM":9655.7}]}]"#,
        )
        .unwrap();
        assert_eq!(t[0].transceivers[0].lat, 50.3);
    }
}
