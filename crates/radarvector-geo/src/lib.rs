//! Geodesy helpers for RadarVector.
//!
//! All coordinates are WGS84 degrees. Distances are in nautical miles.
//! Formulas: standard spherical great-circle (haversine) and initial
//! bearing, see docs/DATA-SOURCES.md.

/// Mean Earth radius in nautical miles (6371.0088 km / 1.852).
pub const EARTH_RADIUS_NM: f64 = 3440.065;

/// A geographic point, `lat` and `lon` in degrees.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

impl LatLon {
    pub const fn new(lat: f64, lon: f64) -> Self {
        Self { lat, lon }
    }
}

/// Normalise a longitude into `[-180, 180)`. Non-finite input is returned as is.
pub fn normalize_lon(lon: f64) -> f64 {
    if !lon.is_finite() {
        return lon;
    }
    let mut l = (lon + 180.0).rem_euclid(360.0) - 180.0;
    if l >= 180.0 {
        l -= 360.0;
    }
    l
}

/// Great-circle distance in nautical miles (haversine).
pub fn distance_nm(a: LatLon, b: LatLon) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dp = p2 - p1;
    let dl = (b.lon - a.lon).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_NM * h.sqrt().min(1.0).asin()
}

/// Initial true bearing from `a` to `b`, degrees in `[0, 360)`.
pub fn initial_bearing_true(a: LatLon, b: LatLon) -> f64 {
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dl = (b.lon - a.lon).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// Point reached from `start` after `distance_nm` on initial `bearing_deg`.
pub fn destination(start: LatLon, bearing_deg: f64, distance_nm: f64) -> LatLon {
    let d = distance_nm / EARTH_RADIUS_NM;
    let b = bearing_deg.to_radians();
    let p1 = start.lat.to_radians();
    let l1 = start.lon.to_radians();
    let p2 = (p1.sin() * d.cos() + p1.cos() * d.sin() * b.cos()).asin();
    let l2 = l1 + (b.sin() * d.sin() * p1.cos()).atan2(d.cos() - p1.sin() * p2.sin());
    LatLon::new(p2.to_degrees(), normalize_lon(l2.to_degrees()))
}

/// Intermediate points along the great circle from `a` to `b` (inclusive),
/// `segments + 1` points. Longitudes are unwrapped (continuous), so the
/// result can exceed ±180 when the path crosses the antimeridian.
pub fn great_circle_points(a: LatLon, b: LatLon, segments: usize) -> Vec<LatLon> {
    let segments = segments.max(1);
    let to_vec = |p: LatLon| {
        let (la, lo) = (p.lat.to_radians(), p.lon.to_radians());
        [la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin()]
    };
    let va = to_vec(a);
    let vb = to_vec(b);
    let dot = (va[0] * vb[0] + va[1] * vb[1] + va[2] * vb[2]).clamp(-1.0, 1.0);
    let omega = dot.acos();
    let mut out = Vec::with_capacity(segments + 1);
    let mut prev_lon: Option<f64> = None;
    for i in 0..=segments {
        let t = i as f64 / segments as f64;
        let (wa, wb) = if omega.abs() < 1e-12 {
            (1.0 - t, t)
        } else {
            (((1.0 - t) * omega).sin() / omega.sin(), (t * omega).sin() / omega.sin())
        };
        let v = [
            wa * va[0] + wb * vb[0],
            wa * va[1] + wb * vb[1],
            wa * va[2] + wb * vb[2],
        ];
        let lat = v[2].atan2((v[0] * v[0] + v[1] * v[1]).sqrt()).to_degrees();
        let mut lon = v[1].atan2(v[0]).to_degrees();
        if let Some(p) = prev_lon {
            while lon - p > 180.0 {
                lon -= 360.0;
            }
            while lon - p < -180.0 {
                lon += 360.0;
            }
        }
        prev_lon = Some(lon);
        out.push(LatLon::new(lat, lon));
    }
    out
}

/// A ring is a closed or open list of `[lon, lat]` positions (GeoJSON order).
pub type Ring = Vec<[f64; 2]>;

/// A polygon: first ring is the exterior, others are holes.
pub type Polygon = Vec<Ring>;

/// Even-odd point-in-ring test in planar lon/lat space.
pub fn point_in_ring(lon: f64, lat: f64, ring: &[[f64; 2]]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let [xi, yi] = ring[i];
        let [xj, yj] = ring[j];
        if ((yi > lat) != (yj > lat)) && (lon < (xj - xi) * (lat - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Point inside a polygon (exterior minus holes).
pub fn point_in_polygon(lon: f64, lat: f64, poly: &Polygon) -> bool {
    match poly.split_first() {
        None => false,
        Some((outer, holes)) => point_in_ring(lon, lat, outer) && !holes.iter().any(|h| point_in_ring(lon, lat, h)),
    }
}

/// Point inside any polygon of a multipolygon. Longitude is also tried
/// shifted by ±360 so polygons stored with unwrapped longitudes match.
pub fn point_in_multipolygon(lon: f64, lat: f64, polys: &[Polygon]) -> bool {
    [lon, lon + 360.0, lon - 360.0]
        .iter()
        .any(|&l| polys.iter().any(|p| point_in_polygon(l, lat, p)))
}

/// Axis-aligned bounding box in lon/lat.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub min_lon: f64,
    pub min_lat: f64,
    pub max_lon: f64,
    pub max_lat: f64,
}

impl BBox {
    pub fn empty() -> Self {
        Self {
            min_lon: f64::INFINITY,
            min_lat: f64::INFINITY,
            max_lon: f64::NEG_INFINITY,
            max_lat: f64::NEG_INFINITY,
        }
    }
    pub fn extend(&mut self, lon: f64, lat: f64) {
        self.min_lon = self.min_lon.min(lon);
        self.max_lon = self.max_lon.max(lon);
        self.min_lat = self.min_lat.min(lat);
        self.max_lat = self.max_lat.max(lat);
    }
    pub fn of_polygons(polys: &[Polygon]) -> Self {
        let mut b = Self::empty();
        for p in polys {
            for r in p {
                for &[lon, lat] in r {
                    b.extend(lon, lat);
                }
            }
        }
        b
    }
    pub fn is_valid(&self) -> bool {
        self.min_lon <= self.max_lon && self.min_lat <= self.max_lat
    }
    pub fn contains(&self, lon: f64, lat: f64) -> bool {
        lon >= self.min_lon && lon <= self.max_lon && lat >= self.min_lat && lat <= self.max_lat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize() {
        assert_eq!(normalize_lon(190.0), -170.0);
        assert_eq!(normalize_lon(-192.5), 167.5);
        assert_eq!(normalize_lon(180.0), -180.0);
        assert_eq!(normalize_lon(-180.0), -180.0);
        assert_eq!(normalize_lon(45.0), 45.0);
    }

    #[test]
    fn distance_cyul_kjfk() {
        let cyul = LatLon::new(45.4706, -73.7408);
        let kjfk = LatLon::new(40.6398, -73.7789);
        let d = distance_nm(cyul, kjfk);
        assert!((d - 290.0).abs() < 2.0, "{d}");
        let brg = initial_bearing_true(cyul, kjfk);
        assert!((brg - 180.0).abs() < 1.0, "{brg}");
    }

    #[test]
    fn bearing_east() {
        let b = initial_bearing_true(LatLon::new(0.0, 0.0), LatLon::new(0.0, 10.0));
        assert!((b - 90.0).abs() < 1e-9);
    }

    #[test]
    fn destination_roundtrip() {
        let a = LatLon::new(45.0, -73.0);
        let b = destination(a, 123.0, 250.0);
        assert!((distance_nm(a, b) - 250.0).abs() < 1e-6);
        assert!((initial_bearing_true(a, b) - 123.0).abs() < 1e-6);
    }

    #[test]
    fn great_circle_crosses_antimeridian_continuously() {
        let pts = great_circle_points(LatLon::new(35.0, 140.0), LatLon::new(37.0, -122.0), 32);
        assert_eq!(pts.len(), 33);
        for w in pts.windows(2) {
            assert!((w[1].lon - w[0].lon).abs() < 30.0);
        }
        assert!(pts.last().is_some_and(|p| (p.lon - 238.0).abs() < 1e-6));
    }

    #[test]
    fn pip() {
        let sq: Polygon = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]];
        let holed: Polygon = vec![
            sq[0].clone(),
            vec![[4.0, 4.0], [6.0, 4.0], [6.0, 6.0], [4.0, 6.0], [4.0, 4.0]],
        ];
        assert!(point_in_polygon(5.0, 5.0, &sq));
        assert!(!point_in_polygon(5.0, 5.0, &holed));
        assert!(point_in_polygon(2.0, 2.0, &holed));
        assert!(!point_in_multipolygon(11.0, 5.0, std::slice::from_ref(&sq)));
        let east: Polygon = vec![vec![[170.0, 0.0], [190.0, 0.0], [190.0, 10.0], [170.0, 10.0]]];
        assert!(point_in_multipolygon(-175.0, 5.0, &[east]));
    }
}

pub mod projection;
pub use projection::{Camera, Projection};
