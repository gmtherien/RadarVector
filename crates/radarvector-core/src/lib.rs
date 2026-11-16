//! Network state for RadarVector: callsign resolution, snapshots, filters.

pub mod aircraft;
pub mod callsign;
pub mod filter;
pub mod snapshot;

pub use callsign::{Resolved, StationKind, resolve};
pub use filter::{Filter, wildcard_match};
pub use snapshot::{NetworkSnapshot, SnapshotDiff, Station};
