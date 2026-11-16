//! CPU-side map geometry: tessellated fills and line segments.

use bytemuck::{Pod, Zeroable};
use lyon::math::point;
use lyon::path::Path;
use lyon::tessellation::{
    BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex as LyonFillVertex, VertexBuffers,
};
use radarvector_data::StaticData;
use radarvector_geo::Polygon;

/// sRGB colour with straight alpha.
pub type Rgba = [u8; 4];

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct FillVertex {
    pub pos: [f32; 2],
    pub color: Rgba,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct SegmentInstance {
    pub a: [f32; 2],
    pub b: [f32; 2],
    pub color: Rgba,
    pub width: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable, PartialEq)]
pub struct AircraftInstance {
    pub pos: [f32; 2],
    pub heading: f32,
    pub size: f32,
    pub color: Rgba,
}

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<FillVertex>,
    pub indices: Vec<u32>,
}

/// Tessellate polygons (`[lon, lat]` rings) into triangles.
pub fn tessellate(polys: &[Polygon], color: Rgba, out: &mut Mesh) -> usize {
    let mut path = Path::builder();
    for poly in polys {
        for ring in poly {
            let mut it = ring.iter().filter(|p| p[0].is_finite() && p[1].is_finite());
            let Some(first) = it.next() else { continue };
            path.begin(point(first[0] as f32, first[1] as f32));
            for p in it {
                path.line_to(point(p[0] as f32, p[1] as f32));
            }
            path.end(true);
        }
    }
    let path = path.build();
    let mut buffers: VertexBuffers<FillVertex, u32> = VertexBuffers::new();
    let mut tess = FillTessellator::new();
    let base = out.vertices.len() as u32;
    let r = tess.tessellate_path(
        &path,
        &FillOptions::tolerance(0.01).with_fill_rule(FillRule::EvenOdd),
        &mut BuffersBuilder::new(&mut buffers, |v: LyonFillVertex| FillVertex {
            pos: v.position().to_array(),
            color,
        }),
    );
    if r.is_err() {
        return 0;
    }
    let n = buffers.vertices.len();
    out.vertices.extend(buffers.vertices);
    out.indices.extend(buffers.indices.into_iter().map(|i| i + base));
    n
}

/// One FIR boundary feature prepared for drawing.
#[derive(Debug, Clone)]
pub struct FeatureGeom {
    pub boundary_id: String,
    pub oceanic: bool,
    /// Whether this id is a sub-sector of another boundary (e.g. `EGTT-D`).
    pub subsector: bool,
    /// Range of vertices in the FIR fill mesh.
    pub fill_vertices: std::ops::Range<usize>,
    /// Outline segments `(a, b)` in lon/lat; edges on the antimeridian are omitted.
    pub segments: Vec<([f32; 2], [f32; 2])>,
}

#[derive(Debug, Clone, Default)]
pub struct FirGeometry {
    pub features: Vec<FeatureGeom>,
    pub fill: Mesh,
}

fn on_antimeridian(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0].abs() >= 179.9999) && (b[0].abs() >= 179.9999) && (a[0] - b[0]).abs() < 1e-6
}

pub fn ring_segments(ring: &[[f64; 2]], out: &mut Vec<([f32; 2], [f32; 2])>) {
    for w in ring.windows(2) {
        if !on_antimeridian(w[0], w[1]) {
            out.push(([w[0][0] as f32, w[0][1] as f32], [w[1][0] as f32, w[1][1] as f32]));
        }
    }
}

/// `EGTT-D` is a sub-sector of `EGTT` when both ids exist.
pub fn is_subsector(id: &str, exists: impl Fn(&str) -> bool) -> bool {
    id.split_once(['-', '_'])
        .is_some_and(|(base, _)| base != id && exists(base))
}

pub fn build_fir_geometry(data: &StaticData) -> FirGeometry {
    let mut g = FirGeometry::default();
    let mut ids: Vec<&String> = data.boundaries.by_id.keys().collect();
    ids.sort();
    for id in ids {
        for f in data.boundaries.get(id) {
            let start = g.fill.vertices.len();
            tessellate(&f.polygons, [0, 0, 0, 0], &mut g.fill);
            let mut segments = Vec::new();
            for poly in &f.polygons {
                for ring in poly {
                    ring_segments(ring, &mut segments);
                }
            }
            g.features.push(FeatureGeom {
                boundary_id: id.clone(),
                oceanic: f.oceanic,
                subsector: is_subsector(id, |b| data.boundaries.contains(b)),
                fill_vertices: start..g.fill.vertices.len(),
                segments,
            });
        }
    }
    g
}

/// Natural Earth background layers.
#[derive(Debug, Clone, Default)]
pub struct Basemap {
    pub land: Vec<Polygon>,
    pub coastlines: Vec<([f32; 2], [f32; 2])>,
    pub borders: Vec<([f32; 2], [f32; 2])>,
}

fn json_lines(text: &str, out: &mut Vec<([f32; 2], [f32; 2])>) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return;
    };
    let Some(features) = v.get("features").and_then(|f| f.as_array()) else {
        return;
    };
    let line = |l: &serde_json::Value, out: &mut Vec<([f32; 2], [f32; 2])>| {
        let pts: Vec<[f64; 2]> = l
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
                    .collect()
            })
            .unwrap_or_default();
        ring_segments(&pts, out);
    };
    for f in features {
        let Some(g) = f.get("geometry") else { continue };
        let coords = g.get("coordinates");
        match g.get("type").and_then(|t| t.as_str()) {
            Some("LineString") => {
                if let Some(c) = coords {
                    line(c, out);
                }
            }
            Some("MultiLineString") => {
                for l in coords.and_then(|c| c.as_array()).into_iter().flatten() {
                    line(l, out);
                }
            }
            _ => {}
        }
    }
}

fn json_polygons(text: &str) -> Vec<Polygon> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let ring = |r: &serde_json::Value| -> Vec<[f64; 2]> {
        r.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]))
                    .collect()
            })
            .unwrap_or_default()
    };
    let poly =
        |p: &serde_json::Value| -> Polygon { p.as_array().map(|a| a.iter().map(ring).collect()).unwrap_or_default() };
    let mut out = Vec::new();
    for f in v.get("features").and_then(|f| f.as_array()).into_iter().flatten() {
        let Some(g) = f.get("geometry") else { continue };
        let Some(c) = g.get("coordinates") else { continue };
        match g.get("type").and_then(|t| t.as_str()) {
            Some("Polygon") => out.push(poly(c)),
            Some("MultiPolygon") => out.extend(c.as_array().into_iter().flatten().map(poly)),
            _ => {}
        }
    }
    out
}

impl Basemap {
    pub fn from_geojson(land: &str, coast: &str, borders: &str) -> Self {
        let mut b = Basemap {
            land: json_polygons(land),
            ..Default::default()
        };
        json_lines(coast, &mut b.coastlines);
        json_lines(borders, &mut b.borders);
        b
    }

    /// The Natural Earth 1:50m layers embedded in the binary.
    pub fn embedded() -> Self {
        Self::from_geojson(
            include_str!("../../../assets/natural-earth/ne_50m_land.geojson"),
            include_str!("../../../assets/natural-earth/ne_50m_coastline.geojson"),
            include_str!("../../../assets/natural-earth/ne_50m_admin_0_boundary_lines_land.geojson"),
        )
    }
}

/// Segments approximating a circle of `radius_nm` around a point.
pub fn circle_segments(lat: f64, lon: f64, radius_nm: f64, n: usize) -> Vec<([f32; 2], [f32; 2])> {
    let c = radarvector_geo::LatLon::new(lat, lon);
    let pts: Vec<[f32; 2]> = (0..=n)
        .map(|i| {
            let p = radarvector_geo::destination(c, 360.0 * i as f64 / n as f64, radius_nm);
            // Keep longitudes continuous around the centre.
            let mut l = p.lon;
            while l - lon > 180.0 {
                l -= 360.0;
            }
            while l - lon < -180.0 {
                l += 360.0;
            }
            [l as f32, p.lat as f32]
        })
        .collect();
    pts.windows(2).map(|w| (w[0], w[1])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tessellates_square() {
        let mut m = Mesh::default();
        let sq: Polygon = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]];
        assert!(tessellate(&[sq], [1, 2, 3, 4], &mut m) >= 4);
        assert_eq!(m.indices.len() % 3, 0);
        assert!(m.indices.len() >= 6);
    }

    #[test]
    fn antimeridian_edges_skipped() {
        let mut s = Vec::new();
        ring_segments(
            &[
                [-180.0, -40.0],
                [-131.0, -40.0],
                [-131.0, -20.0],
                [-180.0, -20.0],
                [-180.0, -40.0],
            ],
            &mut s,
        );
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn subsectors() {
        let ids = ["EGTT", "EGTT-D", "ADR", "ADR-E", "LFFF"];
        let exists = |x: &str| ids.contains(&x);
        assert!(is_subsector("EGTT-D", exists));
        assert!(is_subsector("ADR-E", exists));
        assert!(!is_subsector("EGTT", exists));
        assert!(!is_subsector("DAAA-NC", exists));
    }

    #[test]
    fn embedded_basemap_loads() {
        let b = Basemap::embedded();
        assert!(b.land.len() > 1000);
        assert!(b.coastlines.len() > 10_000);
        assert!(b.borders.len() > 1000);
    }

    #[test]
    fn circle() {
        let s = circle_segments(45.0, 179.9, 30.0, 32);
        assert_eq!(s.len(), 32);
        for (a, b) in s {
            assert!((a[0] - b[0]).abs() < 5.0);
        }
    }
}
