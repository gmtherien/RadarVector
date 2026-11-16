//! TRACON boundaries for the map (SimAware TRACON Project, CC-BY-SA-4.0).

use radarvector_core::StationKind;
use radarvector_data::tracon::{self, Tracons};
use radarvector_render::geometry::ring_segments;

pub struct TraconSet {
    pub tracons: Tracons,
}

impl TraconSet {
    /// Installed copy (updated daily) if valid, else the embedded snapshot.
    pub fn load() -> Self {
        radarvector_feed::datasource::tracon::default_dir()
            .ok()
            .and_then(|d| Self::load_dir(&d))
            .unwrap_or_else(Self::embedded)
    }

    pub fn load_dir(dir: &std::path::Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join(radarvector_feed::datasource::tracon::FILE)).ok()?;
        radarvector_feed::datasource::tracon::check(&text).ok()?;
        tracon::parse(&text).ok().map(|(t, _)| Self { tracons: t })
    }

    pub fn embedded() -> Self {
        let text = include_str!("../../../assets/simaware-tracon/TRACONBoundaries.geojson");
        Self {
            tracons: tracon::parse(text).map(|(t, _)| t).unwrap_or_default(),
        }
    }

    /// Outline segments of the TRACON(s) matched by `prefix` for a station kind.
    pub fn segments(&self, prefix: &str, kind: StationKind) -> Vec<([f32; 2], [f32; 2])> {
        let mut out = Vec::new();
        for f in self.tracons.find(prefix, kind == StationKind::Departure) {
            for poly in &f.polygons {
                for ring in poly {
                    ring_segments(ring, &mut out);
                }
            }
        }
        out
    }

    pub fn name(&self, prefix: &str, kind: StationKind) -> Option<String> {
        self.tracons
            .find(prefix, kind == StationKind::Departure)
            .first()
            .map(|f| f.name.clone())
    }
}
