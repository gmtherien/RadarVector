use proptest::prelude::*;
use radarvector_data::{boundaries, dat};

proptest! {
    #[test]
    fn dat_never_panics(s in "\\PC*") {
        let _ = dat::parse(&s);
    }

    #[test]
    fn dat_lines_never_panic(lines in prop::collection::vec("[\\[\\]A-Za-z0-9|;., _-]{0,40}", 0..40)) {
        let text = format!("[Airports]\n{}", lines.join("\n"));
        let p = dat::parse(&text);
        prop_assert!(p.data.airports.len() + p.rejected_lines() <= p.data_lines);
    }

    #[test]
    fn airport_roundtrip(icao in "[A-Z]{4}", name in "[A-Za-z][A-Za-z -]{0,20}[A-Za-z]", lat in -90.0f64..90.0, lon in -180.0f64..180.0, pseudo in any::<bool>()) {
        let line = format!("[Airports]\n{icao}|{name}|{lat}|{lon}||CZUL|{}\n", u8::from(pseudo));
        let p = dat::parse(&line);
        prop_assert_eq!(p.rejected_lines(), 0);
        let a = &p.data.airports[0];
        prop_assert_eq!(&a.icao, &icao);
        prop_assert_eq!(&a.name, &name);
        prop_assert_eq!(a.lat, lat);
        prop_assert_eq!(a.lon, lon);
        prop_assert_eq!(a.is_pseudo, pseudo);
    }

    #[test]
    fn geojson_never_panics(s in "\\PC*") {
        let _ = boundaries::parse(&s);
    }
}
