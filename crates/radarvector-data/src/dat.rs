//! Parser for `VATSpy.dat` (VAT-Spy Data Project, CC-BY-SA-4.0).
//!
//! Format notes come from the public repository `vatsimnetwork/vatspy-data-project`
//! (file itself and `scripts/bin/validate-dat.ts`), see docs/DATA-SOURCES.md.
//!
//! The parser never fails as a whole: each rejected line is reported as a
//! [`ParseIssue`] and parsing continues.

use serde::Serialize;

/// `[Countries]` line: `name|prefix|center designation`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Country {
    pub name: String,
    /// Two-letter ICAO prefix (e.g. `LF`).
    pub prefix: String,
    /// What the country calls its area control (`Control`, `Radar`, `Centre`...). Often empty.
    pub center_name: Option<String>,
}

/// `[Airports]` line: `icao|name|lat|lon|iata/lid|fir|is_pseudo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Airport {
    pub icao: String,
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub iata_lid: Option<String>,
    pub fir: String,
    /// Pseudo entries are callsign aliases, not drawn as airports.
    pub is_pseudo: bool,
}

/// `[FIRs]` line: `icao|name|callsign prefix|boundary id`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fir {
    pub icao: String,
    pub name: String,
    pub callsign_prefix: Option<String>,
    pub boundary_id: String,
}

/// `[UIRs]` line: `id|name|fir,fir,...`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Uir {
    pub id: String,
    pub name: String,
    pub firs: Vec<String>,
}

/// `[IDL]` line: `lat|lon`. Longitudes may lie outside `[-180, 180]`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct IdlPoint {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Section {
    None,
    Countries,
    Airports,
    Firs,
    Uirs,
    Idl,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum IssueKind {
    /// Data line before any section header.
    OutsideSection,
    /// Wrong number of `|` fields.
    FieldCount { expected: usize, found: usize },
    /// A required field is empty.
    EmptyField(&'static str),
    /// A numeric field could not be parsed.
    BadNumber(&'static str),
    /// A field has an unexpected value.
    BadValue(&'static str),
    /// Unknown `[Section]`; its lines are skipped.
    UnknownSection(String),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParseIssue {
    /// 1-based line number.
    pub line: usize,
    pub section: Section,
    pub kind: IssueKind,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct DatFile {
    pub countries: Vec<Country>,
    pub airports: Vec<Airport>,
    pub firs: Vec<Fir>,
    pub uirs: Vec<Uir>,
    pub idl: Vec<IdlPoint>,
}

/// Result of parsing: data plus every rejected line.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DatParse {
    pub data: DatFile,
    pub issues: Vec<ParseIssue>,
    /// Number of data lines seen (excluding comments, blanks, headers).
    pub data_lines: usize,
}

impl DatParse {
    /// Issues that reject a data line (unknown sections are informational).
    pub fn rejected_lines(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| !matches!(i.kind, IssueKind::UnknownSection(_)))
            .count()
    }
}

fn opt(s: &str) -> Option<String> {
    if s.is_empty() { None } else { Some(s.to_string()) }
}

fn num(s: &str, name: &'static str) -> Result<f64, IssueKind> {
    if s.is_empty() {
        return Err(IssueKind::EmptyField(name));
    }
    s.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or(IssueKind::BadNumber(name))
}

fn req<'a>(s: &'a str, name: &'static str) -> Result<&'a str, IssueKind> {
    if s.is_empty() {
        Err(IssueKind::EmptyField(name))
    } else {
        Ok(s)
    }
}

fn expect(fields: &[&str], n: usize) -> Result<(), IssueKind> {
    if fields.len() == n {
        Ok(())
    } else {
        Err(IssueKind::FieldCount {
            expected: n,
            found: fields.len(),
        })
    }
}

fn parse_line(section: Section, f: &[&str], out: &mut DatFile) -> Result<(), IssueKind> {
    match section {
        Section::Countries => {
            expect(f, 3)?;
            out.countries.push(Country {
                name: req(f[0], "name")?.to_string(),
                prefix: req(f[1], "prefix")?.to_string(),
                center_name: opt(f[2]),
            });
        }
        Section::Airports => {
            expect(f, 7)?;
            let is_pseudo = match f[6] {
                "0" => false,
                "1" => true,
                _ => return Err(IssueKind::BadValue("is_pseudo")),
            };
            let lat = num(f[2], "lat")?;
            let lon = num(f[3], "lon")?;
            if !(-90.0..=90.0).contains(&lat) {
                return Err(IssueKind::BadValue("lat"));
            }
            if !(-180.0..=180.0).contains(&lon) {
                return Err(IssueKind::BadValue("lon"));
            }
            out.airports.push(Airport {
                icao: req(f[0], "icao")?.to_string(),
                name: req(f[1], "name")?.to_string(),
                lat,
                lon,
                iata_lid: opt(f[4]),
                fir: req(f[5], "fir")?.to_string(),
                is_pseudo,
            });
        }
        Section::Firs => {
            expect(f, 4)?;
            out.firs.push(Fir {
                icao: req(f[0], "icao")?.to_string(),
                name: req(f[1], "name")?.to_string(),
                callsign_prefix: opt(f[2]),
                boundary_id: req(f[3], "boundary")?.to_string(),
            });
        }
        Section::Uirs => {
            expect(f, 3)?;
            out.uirs.push(Uir {
                id: req(f[0], "id")?.to_string(),
                name: req(f[1], "name")?.to_string(),
                firs: f[2]
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect(),
            });
        }
        Section::Idl => {
            expect(f, 2)?;
            out.idl.push(IdlPoint {
                lat: num(f[0], "lat")?,
                lon: num(f[1], "lon")?,
            });
        }
        Section::None => return Err(IssueKind::OutsideSection),
        Section::Unknown => {}
    }
    Ok(())
}

/// Parse the full text of a `VATSpy.dat` file.
pub fn parse(text: &str) -> DatParse {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut res = DatParse::default();
    let mut section = Section::None;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = &line[1..line.len() - 1];
            section = match name.trim() {
                "Countries" => Section::Countries,
                "Airports" => Section::Airports,
                "FIRs" => Section::Firs,
                "UIRs" => Section::Uirs,
                "IDL" => Section::Idl,
                other => {
                    res.issues.push(ParseIssue {
                        line: line_no,
                        section: Section::Unknown,
                        kind: IssueKind::UnknownSection(other.to_string()),
                    });
                    Section::Unknown
                }
            };
            continue;
        }
        if section == Section::Unknown {
            continue;
        }
        res.data_lines += 1;
        // Trailing comments exist in the real file (e.g. `...|0 ; io`).
        let line = line.split_once(';').map_or(line, |(data, _)| data.trim_end());
        let fields: Vec<&str> = line.split('|').map(str::trim).collect();
        if let Err(kind) = parse_line(section, &fields, &mut res.data) {
            res.issues.push(ParseIssue {
                line: line_no,
                section,
                kind,
            });
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff};comment\r\n[Countries]\r\nFrance|LF|Control\r\nAfghanistan|OA|\r\n\r\n\
[Airports]\nCSD4|Mont-Laurier|46.564444|-75.577777||CZUL|0\n\
EGLC|Thames|51.505278|0.055278|THAMES|EGTT|1\n\
BAD1|Broken|abc|0||XXXX|0\n\
BAD2|Pseudo?|1|2||XXXX|2\n\
EGKK|Gatwick|51.1|-0.19|LGW|EGTT|0 ; trailing comment\n\
[FIRs]\nCZUL|Montreal|MTL|CZUL\nCZUL|Montreal||CZUL\n\
[Future]\nanything|goes\n\
[UIRs]\nAFRE_E|East Africa Control|HAAA,HCSM\nEMPTY|No FIRs|\n\
[IDL]\n52.0|-192.5\n";

    #[test]
    fn parses_sample() {
        let p = parse(SAMPLE);
        let d = &p.data;
        assert_eq!(d.countries.len(), 2);
        assert_eq!(d.countries[0].center_name.as_deref(), Some("Control"));
        assert_eq!(d.countries[1].center_name, None);
        assert_eq!(d.airports.len(), 3);
        assert!(d.airports[1].is_pseudo);
        assert_eq!(d.airports[1].iata_lid.as_deref(), Some("THAMES"));
        assert_eq!(d.firs.len(), 2);
        assert_eq!(d.firs[0].callsign_prefix.as_deref(), Some("MTL"));
        assert_eq!(d.uirs[0].firs, vec!["HAAA", "HCSM"]);
        assert!(d.uirs[1].firs.is_empty());
        assert_eq!(d.idl[0].lon, -192.5);
        assert_eq!(p.rejected_lines(), 2);
        assert!(
            p.issues
                .iter()
                .any(|i| matches!(&i.kind, IssueKind::UnknownSection(s) if s == "Future"))
        );
        assert!(
            p.issues
                .iter()
                .any(|i| i.line == 9 && i.kind == IssueKind::BadNumber("lat"))
        );
    }

    #[test]
    fn trims_and_field_count() {
        let p = parse("[FIRs]\n  CZUL | Montreal | MTL | CZUL \nCZUL|Montreal|MTL\n");
        assert_eq!(p.data.firs[0].name, "Montreal");
        assert_eq!(p.issues[0].kind, IssueKind::FieldCount { expected: 4, found: 3 });
    }

    #[test]
    fn outside_section() {
        let p = parse("hello|world\n");
        assert_eq!(p.issues[0].kind, IssueKind::OutsideSection);
    }
}
