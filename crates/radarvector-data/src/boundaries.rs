//! Parser for `Boundaries.geojson` (VAT-Spy Data Project, CC-BY-SA-4.0).
//!
//! A GeoJSON FeatureCollection of `MultiPolygon` features with string
//! properties `id`, `oceanic`, `label_lon`, `label_lat`, `region`, `division`.
//! Ids are not unique (e.g. `KZNY` domestic + oceanic), so features are
//! grouped by id. Source: repository `schema.json`, see PROVENANCE.md.

use std::collections::HashMap;

use radarvector_geo::{BBox, Polygon};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct BoundaryFeature {
    pub id: String,
    pub oceanic: bool,
    /// Label position `(lat, lon)`, if present and numeric.
    pub label: Option<(f64, f64)>,
    pub region: Option<String>,
    pub division: Option<String>,
    /// Polygons in GeoJSON `[lon, lat]` order.
    #[serde(skip)]
    pub polygons: Vec<Polygon>,
    #[serde(skip)]
    pub bbox: BBox,
}

impl BoundaryFeature {
    /// True when the feature touches ±180 (split at the antimeridian).
    pub fn touches_antimeridian(&self) -> bool {
        self.bbox.min_lon <= -179.999 || self.bbox.max_lon >= 179.999
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BoundaryIssue {
    /// Index of the feature in the collection.
    pub index: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Boundaries {
    pub by_id: HashMap<String, Vec<BoundaryFeature>>,
    pub feature_count: usize,
}

impl Boundaries {
    pub fn get(&self, id: &str) -> &[BoundaryFeature] {
        self.by_id.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn contains(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }
    pub fn iter(&self) -> impl Iterator<Item = &BoundaryFeature> {
        self.by_id.values().flatten()
    }
}

#[derive(Debug, Default)]
pub struct BoundariesParse {
    pub boundaries: Boundaries,
    pub issues: Vec<BoundaryIssue>,
}

#[derive(Debug, thiserror::Error)]
pub enum BoundariesError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not a GeoJSON FeatureCollection")]
    NotFeatureCollection,
}

fn prop_str(props: &Value, key: &str) -> Option<String> {
    match props.get(key)? {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn prop_f64(props: &Value, key: &str) -> Option<f64> {
    match props.get(key)? {
        Value::String(s) => s.trim().parse().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
    .filter(|v: &f64| v.is_finite())
}

fn parse_ring(v: &Value) -> Option<Vec<[f64; 2]>> {
    v.as_array()?
        .iter()
        .map(|p| {
            let a = p.as_array()?;
            let lon = a.first()?.as_f64()?;
            let lat = a.get(1)?.as_f64()?;
            (lon.is_finite() && lat.is_finite()).then_some([lon, lat])
        })
        .collect()
}

fn parse_polygon(v: &Value) -> Option<Polygon> {
    v.as_array()?.iter().map(parse_ring).collect()
}

fn parse_geometry(g: &Value) -> Result<Vec<Polygon>, String> {
    let kind = g.get("type").and_then(Value::as_str).unwrap_or("");
    let coords = g.get("coordinates").ok_or("missing coordinates")?;
    match kind {
        "MultiPolygon" => coords
            .as_array()
            .ok_or("coordinates not an array")?
            .iter()
            .map(|p| parse_polygon(p).ok_or_else(|| "invalid polygon".to_string()))
            .collect(),
        "Polygon" => Ok(vec![parse_polygon(coords).ok_or("invalid polygon")?]),
        other => Err(format!("unsupported geometry type {other:?}")),
    }
}

/// Parse the GeoJSON text. Invalid features are reported and skipped.
pub fn parse(text: &str) -> Result<BoundariesParse, BoundariesError> {
    let root: Value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text))?;
    let features = root
        .get("features")
        .and_then(Value::as_array)
        .ok_or(BoundariesError::NotFeatureCollection)?;
    let mut out = BoundariesParse::default();
    for (index, f) in features.iter().enumerate() {
        let props = f.get("properties").unwrap_or(&Value::Null);
        let Some(id) = prop_str(props, "id") else {
            out.issues.push(BoundaryIssue {
                index,
                message: "missing id".into(),
            });
            continue;
        };
        let polygons = match f.get("geometry").map(parse_geometry) {
            Some(Ok(p)) if !p.is_empty() => p,
            Some(Ok(_)) => {
                out.issues.push(BoundaryIssue {
                    index,
                    message: format!("{id}: empty geometry"),
                });
                continue;
            }
            Some(Err(e)) => {
                out.issues.push(BoundaryIssue {
                    index,
                    message: format!("{id}: {e}"),
                });
                continue;
            }
            None => {
                out.issues.push(BoundaryIssue {
                    index,
                    message: format!("{id}: missing geometry"),
                });
                continue;
            }
        };
        let label = prop_f64(props, "label_lat").zip(prop_f64(props, "label_lon"));
        let feature = BoundaryFeature {
            oceanic: prop_str(props, "oceanic").is_some_and(|s| s == "1" || s == "true"),
            label,
            region: prop_str(props, "region"),
            division: prop_str(props, "division"),
            bbox: BBox::of_polygons(&polygons),
            polygons,
            id: id.clone(),
        };
        out.boundaries.by_id.entry(id).or_default().push(feature);
        out.boundaries.feature_count += 1;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"type":"FeatureCollection","name":"VATSIM Map","features":[
 {"type":"Feature","properties":{"id":"KZNY","oceanic":"0","label_lon":"-73","label_lat":"40","region":"AMAS","division":"VATUSA"},
  "geometry":{"type":"MultiPolygon","coordinates":[[[[-75,39],[-72,39],[-72,41],[-75,41],[-75,39]]]]}},
 {"type":"Feature","properties":{"id":"KZNY","oceanic":"1","label_lon":"-60","label_lat":"35","region":"AMAS","division":null},
  "geometry":{"type":"MultiPolygon","coordinates":[[[[-70,30],[-50,30],[-50,40],[-70,40],[-70,30]]]]}},
 {"type":"Feature","properties":{"id":"NZZO","oceanic":"0","label_lon":"x","label_lat":"-30"},
  "geometry":{"type":"MultiPolygon","coordinates":[[[[-180,-40],[-131,-40],[-131,-20],[-180,-20],[-180,-40]]],[[[163,-40],[180,-40],[180,-20],[163,-20],[163,-40]]]]}},
 {"type":"Feature","properties":{"oceanic":"0"},"geometry":null},
 {"type":"Feature","properties":{"id":"BROKEN"},"geometry":{"type":"MultiPolygon","coordinates":[[[["a",1]]]]}}
]}"#;

    #[test]
    fn parses_and_groups() {
        let p = parse(SAMPLE).unwrap();
        let b = &p.boundaries;
        assert_eq!(b.feature_count, 3);
        assert_eq!(b.get("KZNY").len(), 2);
        assert!(b.get("KZNY").iter().any(|f| f.oceanic));
        assert_eq!(b.get("KZNY")[0].label, Some((40.0, -73.0)));
        assert_eq!(b.get("KZNY")[1].division, None);
        let nzzo = &b.get("NZZO")[0];
        assert_eq!(nzzo.polygons.len(), 2);
        assert!(nzzo.touches_antimeridian());
        assert_eq!(nzzo.label, None);
        assert_eq!(p.issues.len(), 2);
    }

    #[test]
    fn rejects_non_collection() {
        assert!(matches!(parse("[]"), Err(BoundariesError::NotFeatureCollection)));
        assert!(parse("{").is_err());
    }
}
