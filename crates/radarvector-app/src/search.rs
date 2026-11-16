//! Jump-box suggestions (JMP-01) and unified search (JMP-02).

use radarvector_core::NetworkSnapshot;
use radarvector_data::StaticData;

use crate::map_view::Selection;

/// Suggestions while typing in the jump box: ICAO airports, FIR codes,
/// two-letter country prefixes. Returns `(code, label)`.
pub fn jump_suggestions(data: &StaticData, text: &str, max: usize) -> Vec<(String, String)> {
    let q = text.trim().to_ascii_uppercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<(String, String)> = Vec::new();
    if q.len() <= 2
        && let Some(c) = data.countries.iter().find(|c| c.prefix == q)
    {
        out.push((c.prefix.clone(), c.name.clone()));
    }
    let mut seen = std::collections::HashSet::new();
    for f in data.firs.iter().filter(|f| f.icao.starts_with(&q)) {
        if seen.insert(f.boundary_id.clone()) {
            out.push((f.icao.clone(), format!("FIR {}", f.name)));
        }
        if out.len() >= max / 2 {
            break;
        }
    }
    for a in data.airports.iter().filter(|a| !a.is_pseudo && a.icao.starts_with(&q)) {
        out.push((a.icao.clone(), a.name.clone()));
        if out.len() >= max {
            break;
        }
    }
    out.truncate(max);
    out
}

fn score(hay: &str, q: &str) -> Option<u32> {
    let h = hay.to_uppercase();
    if h == q {
        Some(0)
    } else if h.starts_with(q) {
        Some(1)
    } else if h.contains(q) {
        Some(2)
    } else {
        None
    }
}

/// Search callsigns, CIDs, names, ICAO and IATA codes.
pub fn search(data: &StaticData, snap: Option<&NetworkSnapshot>, text: &str, max: usize) -> Vec<(Selection, String)> {
    let q = text.trim().to_uppercase();
    if q.len() < 2 {
        return Vec::new();
    }
    let mut hits: Vec<(u32, Selection, String)> = Vec::new();
    if let Some(s) = snap {
        for p in &s.pilots {
            let best = [
                score(&p.callsign, &q),
                score(&p.cid.to_string(), &q),
                score(&p.name, &q).map(|x| x + 1),
            ]
            .into_iter()
            .flatten()
            .min();
            if let Some(sc) = best {
                hits.push((
                    sc,
                    Selection::Pilot(p.callsign.clone()),
                    format!("✈ {}  {}  {}", p.callsign, p.name, p.cid),
                ));
            }
        }
        for st in &s.stations {
            let best = [
                score(&st.data.callsign, &q),
                score(&st.data.cid.to_string(), &q),
                score(&st.data.name, &q).map(|x| x + 1),
            ]
            .into_iter()
            .flatten()
            .min();
            if let Some(sc) = best {
                hits.push((
                    sc,
                    Selection::Station(st.data.callsign.clone()),
                    format!("📡 {}  {}  {}", st.data.callsign, st.data.name, st.data.frequency),
                ));
            }
        }
    }
    for a in data.airports.iter().filter(|a| !a.is_pseudo) {
        let best = [
            score(&a.icao, &q),
            a.iata_lid.as_deref().and_then(|i| score(i, &q)),
            score(&a.name, &q).map(|x| x + 2),
        ]
        .into_iter()
        .flatten()
        .min();
        if let Some(sc) = best {
            hits.push((
                sc + 1,
                Selection::Airport(a.icao.clone()),
                format!("🛬 {}  {}  {}", a.icao, a.iata_lid.as_deref().unwrap_or(""), a.name),
            ));
        }
    }
    let mut seen = std::collections::HashSet::new();
    for f in &data.firs {
        if !seen.insert(&f.boundary_id) {
            continue;
        }
        let best = [
            score(&f.icao, &q),
            score(&f.boundary_id, &q),
            score(&f.name, &q).map(|x| x + 2),
        ]
        .into_iter()
        .flatten()
        .min();
        if let Some(sc) = best {
            hits.push((
                sc + 1,
                Selection::Fir(f.boundary_id.clone()),
                format!("▱ {}  {}", f.boundary_id, f.name),
            ));
        }
    }
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.2.cmp(&b.2)));
    hits.into_iter().take(max).map(|(_, s, l)| (s, l)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> StaticData {
        StaticData::from_texts(
            "[Countries]\nCanada|CY|\n[Airports]\nCYUL|Montreal Trudeau|45.47|-73.74|YUL|CZUL|0\nCYQB|Quebec|46.79|-71.39|YQB|CZUL|0\n[FIRs]\nCZUL|Montreal|MTL|CZUL\n",
            r#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"id":"CZUL"},"geometry":{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,0]]]]}}]}"#,
        )
        .unwrap()
        .0
    }

    #[test]
    fn jump() {
        let d = data();
        let s = jump_suggestions(&d, "cy", 8);
        assert_eq!(s[0].0, "CY");
        assert!(s.iter().any(|x| x.0 == "CYUL"));
        assert_eq!(jump_suggestions(&d, "CZ", 8)[0].0, "CZUL");
    }

    #[test]
    fn unified() {
        let d = data();
        let r = search(&d, None, "yul", 10);
        assert!(matches!(&r[0].0, Selection::Airport(c) if c == "CYUL"));
        let r = search(&d, None, "montreal", 10);
        assert!(r.len() >= 2);
    }
}
