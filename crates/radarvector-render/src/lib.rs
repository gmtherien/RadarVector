//! GPU map rendering for RadarVector (wgpu through egui's paint callbacks).

pub mod geometry;
pub mod gpu;
pub mod offscreen;

pub use geometry::{AircraftInstance, Basemap, FillVertex, FirGeometry, Mesh, Rgba, SegmentInstance};
pub use gpu::{Layer, MapCallback, MapFrame, MapGpu, install};
