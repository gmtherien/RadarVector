//! Named filters: CID lists and callsign patterns with `*` wildcards.

use serde::{Deserialize, Serialize};

/// Case-insensitive match of `text` against `pattern`, where `*` matches any
/// run of characters (prefix, suffix or middle). Without `*` the match is exact.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.trim().to_uppercase().chars().collect();
    let t: Vec<char> = text.trim().to_uppercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] != '*' && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Filter {
    pub name: String,
    pub cids: Vec<i64>,
    pub flight_patterns: Vec<String>,
    pub atc_patterns: Vec<String>,
    /// Highlight colour on the map, `#RRGGBB` (FLT-03).
    pub color: Option<String>,
}

impl Filter {
    pub fn matches_pilot(&self, cid: i64, callsign: &str) -> bool {
        self.cids.contains(&cid) || self.flight_patterns.iter().any(|p| wildcard_match(p, callsign))
    }
    pub fn matches_controller(&self, cid: i64, callsign: &str) -> bool {
        self.cids.contains(&cid) || self.atc_patterns.iter().any(|p| wildcard_match(p, callsign))
    }
    pub fn is_empty(&self) -> bool {
        self.cids.is_empty() && self.flight_patterns.is_empty() && self.atc_patterns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("ACA*", "ACA123"));
        assert!(wildcard_match("*_CTR", "lon_dk_ctr"));
        assert!(wildcard_match("CY*_TWR", "CYUL_TWR"));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("A*B*C", "AxxBxxC"));
        assert!(!wildcard_match("A*B*C", "AxxBxx"));
        assert!(wildcard_match("ACA123", "aca123"));
        assert!(!wildcard_match("ACA12", "ACA123"));
        assert!(!wildcard_match("", "X"));
        assert!(wildcard_match("", ""));
    }

    #[test]
    fn filter() {
        let f = Filter {
            name: "Amis".into(),
            cids: vec![42],
            flight_patterns: vec!["ACA*".into()],
            atc_patterns: vec!["CZUL*".into()],
            color: None,
        };
        assert!(f.matches_pilot(42, "XYZ"));
        assert!(f.matches_pilot(1, "ACA870"));
        assert!(!f.matches_pilot(1, "WJA1"));
        assert!(f.matches_controller(1, "CZUL_CTR"));
        assert!(!f.matches_controller(1, "MTL_CTR"));
    }
}
