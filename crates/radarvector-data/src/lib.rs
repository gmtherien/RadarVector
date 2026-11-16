//! Parsing, validation and indexing of the VAT-Spy Data Project files
//! (`VATSpy.dat`, `Boundaries.geojson`). The data is CC-BY-SA-4.0; this
//! crate only reads it.

pub mod boundaries;
pub mod dat;
pub mod static_data;
pub mod tracon;
pub mod validate;

pub use boundaries::{Boundaries, BoundaryFeature};
pub use dat::{Airport, Country, DatFile, Fir, IdlPoint, Uir};
pub use static_data::{LoadReport, StaticData};
pub use validate::ValidationReport;
