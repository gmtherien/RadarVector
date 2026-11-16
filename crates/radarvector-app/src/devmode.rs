//! Developer modes driven by environment variables:
//! - `RADARVECTOR_SCREENSHOT=path.png`: save a screenshot once data is shown, then quit.
//! - `RADARVECTOR_BENCH=N`: replace the feed by N synthetic aircraft, animate the
//!   camera for 10 s, print frame statistics and quit (NF-03).
//! - `RADARVECTOR_VIEW=lat,lon,span`: initial camera.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use radarvector_feed::models::{DataFeed, FlightPlan};
use radarvector_feed::{Controller, Pilot};

pub struct DevMode {
    pub screenshot: Option<PathBuf>,
    pub bench: Option<usize>,
    pub view: Option<(f64, f64, f64)>,
    pub start: Instant,
    pub requested: bool,
    pub dts: Vec<f32>,
}

impl DevMode {
    pub fn from_env() -> Self {
        Self {
            screenshot: std::env::var_os("RADARVECTOR_SCREENSHOT").map(PathBuf::from),
            bench: std::env::var("RADARVECTOR_BENCH").ok().and_then(|v| v.parse().ok()),
            view: std::env::var("RADARVECTOR_VIEW").ok().and_then(|v| {
                let p: Vec<f64> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                (p.len() == 3).then(|| (p[0], p[1], p[2]))
            }),
            start: Instant::now(),
            requested: false,
            dts: Vec::new(),
        }
    }

    pub fn active(&self) -> bool {
        self.screenshot.is_some() || self.bench.is_some()
    }

    pub fn bench_elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Deterministic pseudo-random feed with `n` aircraft and some ATC.
pub fn synthetic_feed(n: usize) -> DataFeed {
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    let pilots = (0..n)
        .map(|i| Pilot {
            cid: 1_000_000 + i as i64,
            callsign: format!("SIM{i:04}"),
            latitude: rnd() * 120.0 - 50.0,
            longitude: rnd() * 360.0 - 180.0,
            altitude: (rnd() * 40_000.0) as i64,
            groundspeed: 120 + (rnd() * 380.0) as i64,
            heading: (rnd() * 360.0) as i64,
            flight_plan: Some(FlightPlan {
                aircraft_short: "A320".into(),
                departure: "CYUL".into(),
                arrival: "KJFK".into(),
                ..Default::default()
            }),
            ..Default::default()
        })
        .collect();
    let controllers = [
        "MTL_CTR",
        "NY_CTR",
        "LON_CTR",
        "CYUL_TWR",
        "KJFK_APP",
        "EGLL_GND",
        "LAX_APP",
        "AFRE_E_CTR",
    ]
    .iter()
    .enumerate()
    .map(|(i, cs)| Controller {
        cid: i as i64,
        callsign: cs.to_string(),
        frequency: "199.998".into(),
        ..Default::default()
    })
    .collect();
    DataFeed {
        pilots,
        controllers,
        ..Default::default()
    }
}

/// Save an egui screenshot as PNG.
pub fn save_png(path: &std::path::Path, img: &egui::ColorImage) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(std::io::Error::other)?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
    w.write_image_data(&bytes).map_err(std::io::Error::other)?;
    Ok(())
}
