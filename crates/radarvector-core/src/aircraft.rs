//! Aircraft type without ICAO equipment / wake codes.

use radarvector_feed::FlightPlan;

/// `A321/M-SDE3FGHIRWY/LB1` → `A321`, `H/B744/L` → `B744`, `B738` → `B738`.
pub fn type_from_icao_string(s: &str) -> String {
    let s = s.trim();
    let mut parts = s.split('/');
    let first = parts.next().unwrap_or("");
    // A leading single-letter field is a wake category / heavy prefix.
    let t = if first.len() == 1 {
        parts.next().unwrap_or("")
    } else {
        first
    };
    t.split('-').next().unwrap_or("").trim().to_ascii_uppercase()
}

/// Best aircraft type for a flight plan.
pub fn aircraft_type(fp: &FlightPlan) -> String {
    if !fp.aircraft_short.trim().is_empty() {
        return fp.aircraft_short.trim().to_ascii_uppercase();
    }
    if !fp.aircraft.trim().is_empty() {
        return type_from_icao_string(&fp.aircraft);
    }
    type_from_icao_string(&fp.aircraft_faa)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types() {
        assert_eq!(type_from_icao_string("A321/M-SDE3FGHIRWY/LB1"), "A321");
        assert_eq!(type_from_icao_string("H/B744/L"), "B744");
        assert_eq!(type_from_icao_string("B738"), "B738");
        assert_eq!(type_from_icao_string("C172/G"), "C172");
        assert_eq!(type_from_icao_string(""), "");
        let fp = FlightPlan {
            aircraft: "A20N/M-SDFGHIRWY/LB1".into(),
            ..Default::default()
        };
        assert_eq!(aircraft_type(&fp), "A20N");
        let fp = FlightPlan {
            aircraft_short: "b77w".into(),
            aircraft: "x".into(),
            ..Default::default()
        };
        assert_eq!(aircraft_type(&fp), "B77W");
    }
}
