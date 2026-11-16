//! Clients for VATSIM's public feeds (status, v3 data, transceivers, METAR)
//! and for the VAT-Spy Data Project releases.

pub mod appupdate;
pub mod backoff;
pub mod client;
pub mod datasource;
pub mod models;

pub use client::{FeedClient, FeedError, FetchOutcome};
pub use models::{Controller, DataFeed, FlightPlan, Pilot};
pub use tokio_util::sync::CancellationToken;
