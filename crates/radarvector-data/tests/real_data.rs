//! Validates the real data project files. Ignored by default; run with
//! `RADARVECTOR_DATA_DIR=/path/to/files cargo test -p radarvector-data -- --ignored`.

use std::path::PathBuf;

use radarvector_data::StaticData;

#[test]
#[ignore = "needs RADARVECTOR_DATA_DIR with VATSpy.dat and Boundaries.geojson"]
fn real_files_parse_cleanly() {
    let dir = PathBuf::from(std::env::var("RADARVECTOR_DATA_DIR").unwrap());
    let (data, report) = StaticData::load_dir(&dir).unwrap();
    for i in &report.dat.issues {
        eprintln!("line {}: {:?}", i.line, i.kind);
    }
    for i in &report.boundary_issues {
        eprintln!("feature {}: {}", i.index, i.message);
    }
    for f in &report.validation.findings {
        eprintln!("{:?}: {}", f.severity, f.message);
    }
    eprintln!(
        "{} lines, {} countries, {} airports, {} FIRs, {} UIRs, {} IDL points, {} features",
        report.dat.data_lines,
        data.countries.len(),
        data.airports.len(),
        data.firs.len(),
        data.uirs.len(),
        data.idl.len(),
        data.boundaries.feature_count
    );
    assert_eq!(report.dat.rejected_lines(), 0);
    assert!(report.boundary_issues.is_empty());
    assert!(report.validation.is_ok());
    assert!(data.airports.len() > 10_000);
    assert!(data.boundaries.feature_count > 1_000);
}
