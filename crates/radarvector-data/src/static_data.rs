//! Indexed static data assembled from `VATSpy.dat` and `Boundaries.geojson`.

use std::collections::HashMap;
use std::path::Path;

use rstar::{RTree, primitives::GeomWithData};
use serde::Serialize;

use crate::boundaries::{self, Boundaries, BoundariesError, BoundaryIssue};
use crate::dat::{self, Airport, Country, DatParse, Fir, IdlPoint, Uir};
use crate::validate::{self, ValidationReport};

pub const DAT_FILE: &str = "VATSpy.dat";
pub const BOUNDARIES_FILE: &str = "Boundaries.geojson";

/// R-tree entry: `[lon, lat]` point carrying an index into `airports`.
pub type AirportPoint = GeomWithData<[f64; 2], usize>;

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("{0}")]
    Boundaries(#[from] BoundariesError),
}

/// Everything the loader reports besides the data itself.
#[derive(Debug, Default, Serialize)]
pub struct LoadReport {
    pub dat: DatParse,
    pub boundary_issues: Vec<BoundaryIssue>,
    pub validation: ValidationReport,
}

impl LoadReport {
    pub fn is_clean(&self) -> bool {
        self.dat.rejected_lines() == 0 && self.boundary_issues.is_empty() && self.validation.is_ok()
    }
}

#[derive(Debug, Default)]
pub struct StaticData {
    pub countries: Vec<Country>,
    pub airports: Vec<Airport>,
    pub firs: Vec<Fir>,
    pub uirs: Vec<Uir>,
    pub idl: Vec<IdlPoint>,
    pub boundaries: Boundaries,

    country_by_prefix: HashMap<String, usize>,
    airport_by_icao: HashMap<String, Vec<usize>>,
    airport_by_iata: HashMap<String, Vec<usize>>,
    fir_by_icao: HashMap<String, Vec<usize>>,
    fir_by_prefix: HashMap<String, Vec<usize>>,
    uir_by_id: HashMap<String, usize>,
    airport_tree: RTree<AirportPoint>,
}

impl StaticData {
    /// Build indices from parsed parts.
    pub fn build(dat: dat::DatFile, boundaries: Boundaries) -> Self {
        let mut s = StaticData {
            countries: dat.countries,
            airports: dat.airports,
            firs: dat.firs,
            uirs: dat.uirs,
            idl: dat.idl,
            boundaries,
            ..Default::default()
        };
        for (i, c) in s.countries.iter().enumerate() {
            s.country_by_prefix.entry(c.prefix.to_ascii_uppercase()).or_insert(i);
        }
        let mut points = Vec::new();
        for (i, a) in s.airports.iter().enumerate() {
            s.airport_by_icao
                .entry(a.icao.to_ascii_uppercase())
                .or_default()
                .push(i);
            if let Some(iata) = &a.iata_lid {
                s.airport_by_iata.entry(iata.to_ascii_uppercase()).or_default().push(i);
            }
            if !a.is_pseudo {
                points.push(AirportPoint::new([a.lon, a.lat], i));
            }
        }
        s.airport_tree = RTree::bulk_load(points);
        for (i, f) in s.firs.iter().enumerate() {
            s.fir_by_icao.entry(f.icao.to_ascii_uppercase()).or_default().push(i);
            if let Some(p) = &f.callsign_prefix {
                s.fir_by_prefix.entry(p.to_ascii_uppercase()).or_default().push(i);
            }
        }
        for (i, u) in s.uirs.iter().enumerate() {
            s.uir_by_id.entry(u.id.to_ascii_uppercase()).or_insert(i);
        }
        s
    }

    /// Parse, validate and index both files from text.
    pub fn from_texts(dat_text: &str, geojson_text: &str) -> Result<(Self, LoadReport), LoadError> {
        let dat = dat::parse(dat_text);
        let b = boundaries::parse(geojson_text)?;
        let validation = validate::validate(&dat.data, &b.boundaries);
        let data = Self::build(dat.data.clone(), b.boundaries);
        let report = LoadReport {
            dat: DatParse {
                data: Default::default(),
                ..dat
            },
            boundary_issues: b.issues,
            validation,
        };
        Ok((data, report))
    }

    /// Load `VATSpy.dat` and `Boundaries.geojson` from a directory.
    pub fn load_dir(dir: &Path) -> Result<(Self, LoadReport), LoadError> {
        let read = |name: &str| {
            let p = dir.join(name);
            std::fs::read_to_string(&p).map_err(|source| LoadError::Io {
                path: p.display().to_string(),
                source,
            })
        };
        Self::from_texts(&read(DAT_FILE)?, &read(BOUNDARIES_FILE)?)
    }

    pub fn country_by_prefix(&self, prefix: &str) -> Option<&Country> {
        self.country_by_prefix
            .get(&prefix.to_ascii_uppercase())
            .map(|&i| &self.countries[i])
    }

    /// All entries (real and pseudo) with this ICAO.
    pub fn airports_by_icao(&self, icao: &str) -> impl Iterator<Item = &Airport> {
        self.airport_by_icao
            .get(&icao.to_ascii_uppercase())
            .into_iter()
            .flatten()
            .map(|&i| &self.airports[i])
    }

    /// The real (non-pseudo) airport for an ICAO, or the first pseudo entry.
    pub fn airport(&self, icao: &str) -> Option<&Airport> {
        let mut it = self.airports_by_icao(icao).peekable();
        let first = *it.peek()?;
        Some(self.airports_by_icao(icao).find(|a| !a.is_pseudo).unwrap_or(first))
    }

    pub fn airports_by_iata(&self, code: &str) -> impl Iterator<Item = &Airport> {
        self.airport_by_iata
            .get(&code.to_ascii_uppercase())
            .into_iter()
            .flatten()
            .map(|&i| &self.airports[i])
    }

    pub fn firs_by_icao(&self, icao: &str) -> impl Iterator<Item = &Fir> {
        self.fir_by_icao
            .get(&icao.to_ascii_uppercase())
            .into_iter()
            .flatten()
            .map(|&i| &self.firs[i])
    }

    pub fn firs_by_prefix(&self, prefix: &str) -> impl Iterator<Item = &Fir> {
        self.fir_by_prefix
            .get(&prefix.to_ascii_uppercase())
            .into_iter()
            .flatten()
            .map(|&i| &self.firs[i])
    }

    pub fn uir(&self, id: &str) -> Option<&Uir> {
        self.uir_by_id.get(&id.to_ascii_uppercase()).map(|&i| &self.uirs[i])
    }

    /// Nearest real airport to a position (planar lon/lat metric, good enough for picking).
    pub fn nearest_airport(&self, lat: f64, lon: f64) -> Option<&Airport> {
        self.airport_tree
            .nearest_neighbor([lon, lat])
            .map(|p| &self.airports[p.data])
    }

    /// Real airports inside a lon/lat envelope.
    pub fn airports_in(&self, min: [f64; 2], max: [f64; 2]) -> impl Iterator<Item = &Airport> {
        self.airport_tree
            .locate_in_envelope(rstar::AABB::from_corners(min, max))
            .map(|p| &self.airports[p.data])
    }

    /// Distinct FIR boundary ids of the data set.
    pub fn fir_name(&self, boundary_or_icao: &str) -> Option<&str> {
        self.firs_by_icao(boundary_or_icao)
            .next()
            .or_else(|| self.firs.iter().find(|f| f.boundary_id == boundary_or_icao))
            .map(|f| f.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAT: &str = "[Countries]\nCanada|CY|Centre\n[Airports]\nCYUL|Montreal Trudeau|45.47|-73.74|YUL|CZUL|0\n\
CYYZ|Toronto Pearson|43.68|-79.63|YYZ|CZYZ|0\nCYYZ|Toronto-Pearson Intl|43.68|-79.63|TOR|CZYZ|1\n\
[FIRs]\nCZUL|Montreal|MTL|CZUL\nCZUL|Montreal||CZUL\n[UIRs]\nCANE|Canada East|CZUL\n";
    const GEO: &str = r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"CZUL","oceanic":"0"},"geometry":{"type":"MultiPolygon","coordinates":[[[[-80,44],[-60,44],[-60,55],[-80,55],[-80,44]]]]}}]}"#;

    #[test]
    fn indices() {
        let (s, r) = StaticData::from_texts(DAT, GEO).unwrap();
        assert!(r.validation.errors().count() <= 1, "{:?}", r.validation);
        assert_eq!(s.airport("cyyz").map(|a| a.is_pseudo), Some(false));
        assert_eq!(s.airports_by_icao("CYYZ").count(), 2);
        assert_eq!(s.airports_by_iata("tor").next().map(|a| a.icao.as_str()), Some("CYYZ"));
        assert_eq!(s.firs_by_prefix("MTL").count(), 1);
        assert_eq!(s.firs_by_icao("CZUL").count(), 2);
        assert!(s.uir("CANE").is_some());
        assert_eq!(s.country_by_prefix("cy").map(|c| c.name.as_str()), Some("Canada"));
        assert_eq!(s.nearest_airport(45.0, -73.0).map(|a| a.icao.as_str()), Some("CYUL"));
        assert_eq!(s.airports_in([-80.0, 43.0], [-79.0, 44.0]).count(), 1);
    }
}
