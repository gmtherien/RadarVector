//! Map projection and camera (screen space in egui points, y down).

use crate::{LatLon, normalize_lon};

/// Latitude limit for Mercator.
pub const MERCATOR_MAX_LAT: f64 = 85.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Projection {
    #[default]
    Equirectangular,
    Mercator,
}

impl Projection {
    /// Projected y (in "degrees") of a latitude.
    pub fn y(self, lat: f64) -> f64 {
        match self {
            Self::Equirectangular => lat,
            Self::Mercator => {
                let l = lat.clamp(-MERCATOR_MAX_LAT, MERCATOR_MAX_LAT).to_radians();
                (std::f64::consts::FRAC_PI_4 + l / 2.0).tan().ln().to_degrees()
            }
        }
    }
    pub fn lat(self, y: f64) -> f64 {
        match self {
            Self::Equirectangular => y,
            Self::Mercator => (2.0 * y.to_radians().exp().atan() - std::f64::consts::FRAC_PI_2).to_degrees(),
        }
    }
    pub fn max_y(self) -> f64 {
        match self {
            Self::Equirectangular => 90.0,
            Self::Mercator => self.y(MERCATOR_MAX_LAT),
        }
    }
}

/// Camera over a world that wraps in longitude.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Camera {
    /// Centre in projected coordinates (x = longitude, y = projected latitude).
    pub center: [f64; 2],
    /// Points per projected degree.
    pub scale: f64,
    /// Viewport size in points.
    #[serde(skip)]
    pub viewport: [f64; 2],
    pub projection: Projection,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            center: [0.0, 20.0],
            scale: 4.0,
            viewport: [1280.0, 720.0],
            projection: Projection::Equirectangular,
        }
    }
}

pub const MAX_SCALE: f64 = 20_000.0;

impl Camera {
    pub fn min_scale(&self) -> f64 {
        (self.viewport[1] / (2.0 * self.projection.max_y()) * 0.6).max(0.05)
    }

    pub fn set_viewport(&mut self, w: f64, h: f64) {
        self.viewport = [w.max(1.0), h.max(1.0)];
        self.clamp();
    }

    fn clamp(&mut self) {
        self.scale = self.scale.clamp(self.min_scale(), MAX_SCALE);
        self.center[0] = normalize_lon(self.center[0]);
        let max_y = self.projection.max_y();
        let half = self.viewport[1] / 2.0 / self.scale;
        // Keep the poles at the edges when the world is taller than the view.
        self.center[1] = if half >= max_y {
            0.0
        } else {
            self.center[1].clamp(-max_y + half, max_y - half)
        };
    }

    /// Screen position of a point, using the world copy nearest the centre.
    pub fn to_screen(&self, p: LatLon) -> [f64; 2] {
        let mut dx = p.lon - self.center[0];
        dx -= 360.0 * (dx / 360.0).round();
        let dy = self.projection.y(p.lat) - self.center[1];
        [
            self.viewport[0] / 2.0 + dx * self.scale,
            self.viewport[1] / 2.0 - dy * self.scale,
        ]
    }

    /// Screen position keeping the given world copy (unwrapped longitude).
    pub fn to_screen_unwrapped(&self, lon: f64, lat: f64) -> [f64; 2] {
        let dy = self.projection.y(lat) - self.center[1];
        [
            self.viewport[0] / 2.0 + (lon - self.center[0]) * self.scale,
            self.viewport[1] / 2.0 - dy * self.scale,
        ]
    }

    pub fn from_screen(&self, s: [f64; 2]) -> LatLon {
        let x = self.center[0] + (s[0] - self.viewport[0] / 2.0) / self.scale;
        let y = self.center[1] - (s[1] - self.viewport[1] / 2.0) / self.scale;
        LatLon::new(
            self.projection
                .lat(y.clamp(-self.projection.max_y(), self.projection.max_y())),
            normalize_lon(x),
        )
    }

    pub fn pan(&mut self, delta_points: [f64; 2]) {
        self.center[0] -= delta_points[0] / self.scale;
        self.center[1] += delta_points[1] / self.scale;
        self.clamp();
    }

    /// Zoom by `factor`, keeping the geographic point under `anchor` fixed.
    pub fn zoom_at(&mut self, anchor: [f64; 2], factor: f64) {
        let before = self.unwrapped_at(anchor);
        self.scale = (self.scale * factor).clamp(self.min_scale(), MAX_SCALE);
        let after = self.unwrapped_at(anchor);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
        self.clamp();
    }

    fn unwrapped_at(&self, s: [f64; 2]) -> [f64; 2] {
        [
            self.center[0] + (s[0] - self.viewport[0] / 2.0) / self.scale,
            self.center[1] - (s[1] - self.viewport[1] / 2.0) / self.scale,
        ]
    }

    pub fn center_on(&mut self, p: LatLon) {
        self.center = [p.lon, self.projection.y(p.lat)];
        self.clamp();
    }

    /// Centre on `p` with `span_deg` degrees of longitude across the width.
    /// The scale is applied first so latitude clamping uses the new zoom.
    pub fn look_at(&mut self, p: LatLon, span_deg: f64) {
        self.scale = (self.viewport[0] / span_deg.max(1e-3)).clamp(self.min_scale(), MAX_SCALE);
        self.center = [p.lon, self.projection.y(p.lat)];
        self.clamp();
    }

    /// Zoom so that roughly `span_deg` degrees of longitude fill the width.
    pub fn set_span(&mut self, span_deg: f64) {
        self.scale = self.viewport[0] / span_deg.max(1e-3);
        self.clamp();
    }

    pub fn set_projection(&mut self, p: Projection) {
        let lat = self.projection.lat(self.center[1]);
        self.projection = p;
        self.center[1] = p.y(lat);
        self.clamp();
    }

    /// Longitude offsets (multiples of 360) of the world copies visible on screen.
    pub fn world_offsets(&self) -> Vec<f64> {
        let half = self.viewport[0] / 2.0 / self.scale;
        let (lo, hi) = (self.center[0] - half, self.center[0] + half);
        let kmin = ((lo + 180.0) / 360.0).floor() as i64;
        let kmax = ((hi - 180.0) / 360.0).ceil() as i64;
        (kmin..=kmax).map(|k| k as f64 * 360.0).collect()
    }

    /// Visible lon/lat envelope of the central copy (lon may exceed ±180).
    pub fn visible_bounds(&self) -> ([f64; 2], [f64; 2]) {
        let a = self.unwrapped_at([0.0, self.viewport[1]]);
        let b = self.unwrapped_at([self.viewport[0], 0.0]);
        ([a[0], self.projection.lat(a[1])], [b[0], self.projection.lat(b[1])])
    }

    /// Approximate nautical miles per screen point at the centre.
    pub fn nm_per_point(&self) -> f64 {
        let lat = self.projection.lat(self.center[1]).to_radians();
        let deg_nm = 60.0;
        match self.projection {
            Projection::Equirectangular => deg_nm / self.scale,
            Projection::Mercator => deg_nm * lat.cos() / self.scale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_zoom_anchor() {
        for proj in [Projection::Equirectangular, Projection::Mercator] {
            let mut c = Camera {
                projection: proj,
                ..Default::default()
            };
            c.set_viewport(1000.0, 800.0);
            let p = LatLon::new(45.5, -73.6);
            let s = c.to_screen(p);
            let q = c.from_screen(s);
            assert!((q.lat - p.lat).abs() < 1e-9 && (q.lon - p.lon).abs() < 1e-9);
            c.zoom_at(s, 8.0);
            let s2 = c.to_screen(p);
            assert!((s2[0] - s[0]).abs() < 1e-6 && (s2[1] - s[1]).abs() < 1e-6, "{proj:?}");
        }
    }

    #[test]
    fn wraps() {
        let mut c = Camera::default();
        c.set_viewport(1000.0, 800.0);
        c.center = [179.0, 0.0];
        c.scale = 10.0;
        let s = c.to_screen(LatLon::new(0.0, -179.0));
        assert!((s[0] - 520.0).abs() < 1e-9);
        assert_eq!(c.world_offsets(), vec![0.0, 360.0]);
        c.pan([-30.0, 0.0]);
        assert!((c.center[0] - -178.0).abs() < 1e-9);
        c.center = [0.0, 0.0];
        assert_eq!(c.world_offsets(), vec![0.0]);
    }

    #[test]
    fn look_at_from_world_view_keeps_latitude() {
        let mut c = Camera::default();
        c.set_viewport(1070.0, 800.0);
        c.set_span(360.0);
        c.look_at(LatLon::new(25.79, -80.29), 3.0);
        let s = c.to_screen(LatLon::new(25.79, -80.29));
        assert!((s[0] - 535.0).abs() < 1e-6 && (s[1] - 400.0).abs() < 1e-6, "{s:?}");
    }

    #[test]
    fn mercator_inverse() {
        let p = Projection::Mercator;
        for lat in [-80.0, -30.0, 0.0, 45.0, 84.0] {
            assert!((p.lat(p.y(lat)) - lat).abs() < 1e-9);
        }
    }
}
