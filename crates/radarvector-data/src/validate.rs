//! Cross-file validation, following the rules of the data project's
//! `scripts/bin/validate-dat.ts` (read 2026-11-15, see PROVENANCE.md).

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::boundaries::Boundaries;
use crate::dat::DatFile;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Severity {
    /// The upstream validator would reject the file.
    Error,
    /// Suspicious but tolerated.
    Warning,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ValidationReport {
    pub findings: Vec<Finding>,
}

impl ValidationReport {
    fn err(&mut self, m: String) {
        self.findings.push(Finding {
            severity: Severity::Error,
            message: m,
        });
    }
    fn warn(&mut self, m: String) {
        self.findings.push(Finding {
            severity: Severity::Warning,
            message: m,
        });
    }
    pub fn errors(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Error)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Finding> {
        self.findings.iter().filter(|f| f.severity == Severity::Warning)
    }
    pub fn is_ok(&self) -> bool {
        self.errors().next().is_none()
    }
}

/// Validate a parsed DAT file against itself and the boundaries.
pub fn validate(dat: &DatFile, boundaries: &Boundaries) -> ValidationReport {
    let mut r = ValidationReport::default();

    let mut prefixes = HashSet::new();
    for c in &dat.countries {
        if c.prefix.chars().count() != 2 {
            r.err(format!(
                "country {:?}: prefix {:?} must be 2 characters",
                c.name, c.prefix
            ));
        }
        if !prefixes.insert(c.prefix.as_str()) {
            r.err(format!("country prefix {} is duplicated", c.prefix));
        }
    }

    let mut icao_count: HashMap<&str, (usize, usize)> = HashMap::new();
    let mut iatas = HashSet::new();
    for a in &dat.airports {
        if a.icao.chars().count() != 4 {
            r.err(format!("airport {}: ICAO must be 4 characters", a.icao));
        }
        if a.fir.chars().count() != 4 {
            r.err(format!("airport {}: FIR {} must be 4 characters", a.icao, a.fir));
        }
        let e = icao_count.entry(a.icao.as_str()).or_default();
        if a.is_pseudo {
            e.1 += 1
        } else {
            e.0 += 1
        }
        if let Some(i) = &a.iata_lid
            && !iatas.insert(i.as_str())
        {
            r.warn(format!("airport {}: IATA/LID {} is duplicated", a.icao, i));
        }
    }
    for (icao, (real, _pseudo)) in &icao_count {
        // Upstream validate-dat.ts does not actually reject these (its pseudo
        // check tests a non-empty string), and the published file has some.
        if *real > 1 {
            r.warn(format!("airport {icao} is duplicated ({real} non-pseudo entries)"));
        }
    }

    let fir_icaos: HashSet<&str> = dat.firs.iter().map(|f| f.icao.as_str()).collect();
    let mut callsigns: HashSet<&str> = HashSet::new();
    let mut pairs = HashSet::new();
    for f in &dat.firs {
        if !boundaries.contains(&f.boundary_id) {
            r.err(format!(
                "FIR {}: boundary {} not found in Boundaries.geojson",
                f.icao, f.boundary_id
            ));
        }
        if !pairs.insert((f.callsign_prefix.as_deref(), f.icao.as_str())) {
            r.err(format!(
                "FIR {} with prefix {:?} is duplicated",
                f.icao,
                f.callsign_prefix.as_deref().unwrap_or("")
            ));
        }
        if let Some(p) = &f.callsign_prefix {
            callsigns.insert(p);
        }
        callsigns.insert(&f.icao);
    }

    let mut uir_ids = HashSet::new();
    for u in &dat.uirs {
        let missing: Vec<&str> = u
            .firs
            .iter()
            .map(String::as_str)
            .filter(|f| !fir_icaos.contains(f))
            .collect();
        if !missing.is_empty() {
            r.err(format!("UIR {}: FIRs {} missing in [FIRs]", u.id, missing.join(",")));
        }
        if callsigns.contains(u.id.as_str()) {
            r.err(format!("UIR {}: id collides with a FIR callsign", u.id));
        }
        if !uir_ids.insert(u.id.as_str()) {
            r.err(format!("UIR {} is duplicated", u.id));
        }
    }

    for a in &dat.airports {
        if !fir_icaos.contains(a.fir.as_str()) {
            r.warn(format!("airport {}: FIR {} not in [FIRs]", a.icao, a.fir));
        }
    }

    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_errors() {
        let dat = crate::dat::parse(
            "[Countries]\nFrance|LF|\nFrance bis|LF|\n\
[Airports]\nLFPG|CDG|49|2.5|CDG|LFFF|0\nLFPG|Alias|49|2.5|ALI|LFFF|1\nLFPO|Orly|48|2.3||LFFF|0\nLFPO|Orly2|48|2.3||LFFF|0\n\
[FIRs]\nLFFF|Paris|PAR|LFFF\nLFMM|Marseille||NOPE\n\
[UIRs]\nPAR|Collides|LFFF\nEURW|West|LFFF,XXXX\n",
        );
        let b = crate::boundaries::parse(
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"LFFF"},"geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}}]}"#,
        )
        .unwrap()
        .boundaries;
        let r = validate(&dat.data, &b);
        let msgs: Vec<_> = r.errors().map(|f| f.message.as_str()).collect();
        assert!(msgs.iter().any(|m| m.contains("prefix LF is duplicated")));
        assert!(r.warnings().any(|m| m.message.contains("LFPO is duplicated")));
        assert!(!msgs.iter().any(|m| m.contains("LFPG is duplicated")));
        assert!(msgs.iter().any(|m| m.contains("boundary NOPE")));
        assert!(msgs.iter().any(|m| m.contains("UIR PAR: id collides")));
        assert!(msgs.iter().any(|m| m.contains("XXXX missing")));
    }
}
