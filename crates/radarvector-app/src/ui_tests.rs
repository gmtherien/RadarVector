//! Headless end-to-end UI tests (needs a GPU and network; `--ignored`).
//! Screenshots are written to `target/visual/ui_*.png`.

use std::time::{Duration, Instant};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::RadarVectorApp;
use crate::settings::{Language, Settings};

/// UI language is process-global: run UI tests one at a time.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn save(h: &mut Harness<RadarVectorApp>, name: &str) {
    let img = h.render().expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/visual");
    std::fs::create_dir_all(&dir).unwrap();
    img.save(dir.join(format!("ui_{name}.png"))).unwrap();
}

fn wait_for(h: &mut Harness<RadarVectorApp>, what: &str, cond: impl Fn(&mut RadarVectorApp) -> bool) {
    let t = Instant::now();
    while !cond(h.state_mut()) {
        assert!(t.elapsed() < Duration::from_secs(60), "timeout waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
        h.step();
    }
    for _ in 0..3 {
        h.step();
    }
}

#[test]
#[ignore = "needs a GPU and network"]
fn ui_smoke() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let settings = Settings {
        language: Language::English,
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size([1500.0, 900.0])
        .wgpu()
        .build_eframe(|cc| RadarVectorApp::with_settings(cc, 1, settings, false));
    wait_for(&mut h, "live snapshot and geometry", |a| {
        a.state().snapshot.is_some() && a.state().map.prepared.is_some()
    });
    save(&mut h, "main");

    // Flights list (LST-02).
    h.state_mut().state().focus_tab = Some(crate::app::Tab::Flights);
    h.run_steps(3);
    assert!(h.query_by_label("Callsign ▲").is_some() || h.query_by_label_contains("Callsign").is_some());
    save(&mut h, "flights");

    // Jump box (JMP-01) then airport sheet (DET-02).
    let n = h
        .state_mut()
        .state()
        .snapshot
        .as_ref()
        .map(|s| s.airports.len())
        .unwrap_or(0);
    assert!(n > 0);
    let busiest = {
        let s = h.state_mut().state().snapshot.clone().unwrap();
        let mut v: Vec<_> = s.airports.iter().map(|(k, a)| (a.stations.len(), k.clone())).collect();
        v.sort();
        v.last().unwrap().1.clone()
    };
    {
        let st = h.state_mut().state();
        assert!(st.jump_to(&busiest));
        st.select(crate::map_view::Selection::Airport(busiest.clone()), true);
    }
    wait_for(&mut h, "METAR", |a| {
        a.state().airport_metars.get(&busiest).is_some_and(|m| m.1.is_some())
    });
    save(&mut h, "airport");

    // MAP-07: single clicks reuse one preview tab; the pinned KMIA tab stays.
    let pilots: Vec<String> = h
        .state_mut()
        .state()
        .snapshot
        .as_ref()
        .unwrap()
        .pilots
        .iter()
        .take(2)
        .map(|p| p.callsign.clone())
        .collect();
    let before = h.state_mut().details_tabs();
    for cs in &pilots {
        h.state_mut()
            .state()
            .preview(crate::map_view::Selection::Pilot(cs.clone()));
        h.run_steps(2);
    }
    assert_eq!(h.state_mut().details_tabs(), before + 1);
    save(&mut h, "pilot");

    // Settings window (CFG-01).
    h.state_mut().state().settings_open = true;
    h.run_steps(3);
    save(&mut h, "settings");
    h.state_mut().state().settings_open = false;

    // Unified search (JMP-02).
    h.state_mut().state().search_open = true;
    h.state_mut().state().search_text = busiest[1..].to_string();
    h.run_steps(3);
    save(&mut h, "search");
}

#[test]
#[ignore = "needs a GPU and network"]
fn ui_french_light_search() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let settings = Settings {
        language: Language::French,
        theme: crate::settings::Theme::Light,
        ..Default::default()
    };
    let mut h = Harness::builder()
        .with_size([1500.0, 900.0])
        .wgpu()
        .build_eframe(|cc| RadarVectorApp::with_settings(cc, 1, settings, false));
    wait_for(&mut h, "live snapshot and geometry", |a| {
        a.state().snapshot.is_some() && a.state().map.prepared.is_some()
    });
    assert!(h.query_by_label_contains("Actualiser").is_some(), "French UI");
    // Unified search (JMP-02) on a live callsign prefix.
    let cs = h.state_mut().state().snapshot.as_ref().unwrap().pilots[0]
        .callsign
        .clone();
    h.state_mut().state().search_open = true;
    h.state_mut().state().search_text = cs.clone();
    h.run_steps(3);
    assert!(h.query_by_label_contains(&cs).is_some(), "search lists {cs}");
    save(&mut h, "fr_light_search");
}
