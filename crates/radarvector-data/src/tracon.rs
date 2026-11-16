//! SimAware TRACON Project boundaries (`TRACONBoundaries.geojson`, CC-BY-SA-4.0).
//!
//! Each feature has `id`, `prefix` (array of callsign prefixes), optional
//! `suffix` (`DEP`; absent = applies to APP), `name`, optional
//! `label_lat`/`label_lon` numbers. Source: repository `schema-single.json`
//! (https://github.com/vatsimnetwork/simaware-tracon-project), read 2026-11-15.

use std::collections::HashMap;

use radarvector_geo::{BBox, Polygon};
use serde_json::Value;

use crate::boundaries::BoundariesError;

#[derive(Debug, Clone)]
pub struct TraconFeature {
    pub id: String,
    pub name: String,
    pub prefixes: Vec<String>,
    /// `Some("DEP")` for departure-only boundaries.
    pub suffix: Option<String>,
    pub label: Option<(f64, f64)>,
    pub polygons: Vec<Polygon>,
    pub bbox: BBox,
}

#[derive(Debug, Clone, Default)]
pub struct Tracons {
    pub features: Vec<TraconFeature>,
    by_prefix: HashMap<String, Vec<usize>>,
}

impl Tracons {
    /// Features for an exact callsign prefix (`LAX`, `LAX_U`...) and station
    /// kind: departure stations prefer `DEP` boundaries, then generic ones;
    /// approach stations use generic and `APP` boundaries.
    pub fn find(&self, prefix: &str, departure: bool) -> Vec<&TraconFeature> {
        let all: Vec<&TraconFeature> = self
            .by_prefix
            .get(&prefix.to_ascii_uppercase())
            .into_iter()
            .flatten()
            .map(|&i| &self.features[i])
            .collect();
        let dep: Vec<_> = all
            .iter()
            .copied()
            .filter(|f| f.suffix.as_deref() == Some("DEP"))
            .collect();
        if departure && !dep.is_empty() {
            return dep;
        }
        all.into_iter()
            .filter(|f| f.suffix.as_deref().is_none_or(|s| s != "DEP"))
            .collect()
    }

    pub fn len(&self) -> usize {
        self.features.len()
    }

    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }
}

fn polygons(g: &Value) -> Option<Vec<Polygon>> {
    let ring = |r: &Value| -> Option<Vec<[f64; 2]>> {
        r.as_array()?
            .iter()
            .map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
            .collect()
    };
    let poly = |p: &Value| -> Option<Polygon> { p.as_array()?.iter().map(ring).collect() };
    let c = g.get("coordinates")?;
    match g.get("type")?.as_str()? {
        "Polygon" => Some(vec![poly(c)?]),
        "MultiPolygon" => c.as_array()?.iter().map(poly).collect(),
        _ => None,
    }
}

/// Parse the TRACON GeoJSON; invalid features are skipped (count returned).
pub fn parse(text: &str) -> Result<(Tracons, usize), BoundariesError> {
    let root: Value = serde_json::from_str(text.strip_prefix('\u{feff}').unwrap_or(text))?;
    let feats = root
        .get("features")
        .and_then(Value::as_array)
        .ok_or(BoundariesError::NotFeatureCollection)?;
    let mut t = Tracons::default();
    let mut skipped = 0;
    for f in feats {
        let p = f.get("properties").unwrap_or(&Value::Null);
        let id = p.get("id").and_then(Value::as_str).map(str::to_string);
        let prefixes: Vec<String> = p
            .get("prefix")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.trim().to_ascii_uppercase())
                    .collect()
            })
            .unwrap_or_default();
        let polys = f.get("geometry").and_then(polygons);
        let (Some(id), Some(polys)) = (id, polys) else {
            skipped += 1;
            continue;
        };
        if prefixes.is_empty() || polys.is_empty() {
            skipped += 1;
            continue;
        }
        let label = p
            .get("label_lat")
            .and_then(Value::as_f64)
            .zip(p.get("label_lon").and_then(Value::as_f64));
        let idx = t.features.len();
        for pre in &prefixes {
            t.by_prefix.entry(pre.clone()).or_default().push(idx);
        }
        t.features.push(TraconFeature {
            id,
            name: p.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
            prefixes,
            suffix: p
                .get("suffix")
                .and_then(Value::as_str)
                .map(|s| s.trim().to_ascii_uppercase())
                .filter(|s| !s.is_empty()),
            label,
            bbox: BBox::of_polygons(&polys),
            polygons: polys,
        });
    }
    Ok((t, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"id":"SCT","prefix":["LAX"],"name":"SoCal Approach"},"geometry":{"type":"MultiPolygon","coordinates":[[[[-119,33],[-117,33],[-117,35],[-119,33]]]]}},
      {"type":"Feature","properties":{"id":"SCT","prefix":["LAX"],"suffix":"DEP","name":"SoCal Departure"},"geometry":{"type":"Polygon","coordinates":[[[-119,33],[-118,33],[-118,34],[-119,33]]]}},
      {"type":"Feature","properties":{"id":"A80","prefix":["ATL","AHN"],"name":"Atlanta Approach","label_lat":33.6,"label_lon":-84.4},"geometry":{"type":"Polygon","coordinates":[[[-85,33],[-84,33],[-84,34],[-85,33]]]}},
      {"type":"Feature","properties":{"id":"BAD","prefix":[]},"geometry":null}
    ]}"#;

    #[test]
    fn parse_and_find() {
        let (t, skipped) = parse(SAMPLE).unwrap();
        assert_eq!(t.len(), 3);
        assert_eq!(skipped, 1);
        assert_eq!(t.find("lax", false).len(), 1);
        assert_eq!(t.find("LAX", false)[0].name, "SoCal Approach");
        assert_eq!(t.find("LAX", true)[0].name, "SoCal Departure");
        assert_eq!(t.find("AHN", false)[0].id, "A80");
        assert_eq!(t.find("AHN", false)[0].label, Some((33.6, -84.4)));
        assert!(t.find("ATL", true).iter().all(|f| f.id == "A80"));
        assert!(t.find("JFK", false).is_empty());
    }
}
