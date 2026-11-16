# RadarVector acceptance checklist (milestones J1 to J5)

Status as of 15 November 2026. "Auto" = checked by an automated test; "Headless" = checked by the
`egui_kittest` UI tests with wgpu rendering (snapshots in `target/visual/`); "To do" = needs a
manual on-screen check (the macOS session was locked during development).

## Milestone criteria

| Milestone | Criterion | Result |
| --- | --- | --- |
| J1 | 100 % of the upstream `.dat` lines accepted | ✅ 19,888 / 19,888 (v2611.2), `radarvector data validate` |
| J1 | Callsign resolution rate over 50 real snapshots | ✅ see "Corpus" section |
| J2 | NF-03: 60 fps, 3,000 aircraft, all FIRs | ✅ headless 1600×900: p95 ≈ 1.9 ms (3,000 aircraft), ≈ 2.1 ms (5,000) |
| J2 | Reference snapshots, including the antimeridian | ✅ 4 views (`crates/radarvector-app/tests/visual/*.png`), tolerance 2/255 |
| J3 | Every P0 requirement checkable | ✅ table below (mouse interactions to confirm on screen) |
| J4 | Install on the 3 OSes, signed N→N+1 update, invalid signature refused | ◐ macOS `.app`/`.dmg` built locally; cargo-packager signature verified and tampered package refused; Windows/Linux to validate in CI |
| J5 | P1 requirements checked | ✅ table below |

## P0 requirements

| ID | Verification |
| --- | --- |
| MAP-01 | Zoom towards the cursor: test `roundtrip_and_zoom_anchor` (Auto). Drag/wheel: To do |
| MAP-02 | Active/inactive styles, UIR, "hide unstaffed" option: snapshots + `builds_activity` (Auto) |
| MAP-03 | Sub-sectors (e.g. `LON_DK_CTR` → `EGTT-D`, hidden when inactive): tests `enroute`, `subsectors` (Auto) |
| MAP-04 | FIR label at `label_lat/lon`: snapshot `ui_main` (Headless) |
| MAP-05 | DEL/GND/TWR/APP/ATIS symbols + approach circle or TRACON: snapshot `ui_airport` (Headless) |
| MAP-06 | Oriented icon, labels, decluttering: snapshots `ui_airport`, `ui_pilot` (Headless) |
| MAP-07 | Hover tooltip: To do; click → detail sheet (preview tab reused): `ui_smoke` (Headless) |
| MAP-08 | Antimeridian, IDL, wrap-around: snapshot `antimeridian_nzzo`, tests `wraps`, `antimeridian_edges_skipped` (Auto) |
| MAP-09 | NM distance + true bearing: geo tests (Auto); mouse drawing: To do |
| MAP-10 | Named bookmarks + Ctrl+Alt+0..9 / Ctrl+0..9 (Cmd on macOS): persistence tested (`toml_roundtrip`); shortcuts: To do |
| MAP-11 | Colours, font, tooltips, swapped mouse buttons: snapshot `ui_settings` (Headless) |
| LST-01 | Controllers list (ATC only): snapshot `ui_main` (Headless) |
| LST-02 | Flights list, aircraft type without equipment codes: `ui_flights`, test `types` (Auto) |
| DET-01 | FIR sheet (controllers, airports, traffic in the area): code + `point_in_multipolygon` (Auto); on screen: To do |
| DET-02 | Airport sheet (controllers, ATIS, departures/arrivals, METAR): `ui_airport` (Headless) |
| DET-03 | Flight sheet (full flight plan, position, squawk): `ui_pilot` (Headless) |
| DET-04 | Controller sheet: on screen: To do |
| FLT-01 | Named filters, wildcards: tests `wildcards`, `filter` (Auto) |
| FLT-02 | Filter results tab: To do |
| FAV-01 | Favourites with confirmation, centring: persistence tested; on screen: To do |
| JMP-01 | Jump box ICAO / FIR / country prefix + suggestions: test `jump`, `ui_smoke` (Auto/Headless) |
| REF-01..04 | Button disabled while refreshing, Stop (cancellation), auto 15 s min / 60 s, refresh on restore from minimised, error banner + status link: code + `sequence` (backoff); on screen: To do |
| WX-01 | Multi-station METAR: client tested live (KMIA sheet); METAR tab: To do |
| CFG-01 | Single window, restore defaults, auto-save: `restore_keeps_user_content`, `ui_settings` |
| UPD-01 | Upstream data update from GitHub: `radarvector data update` (live, v2611.2 installed) |
| UPD-02 | OS user folders: `~/Library/Application Support/RadarVector` checked |

## P1 (J5) and delivered P2 requirements

| ID | Verification |
| --- | --- |
| MAP-12 | Light and dark themes: `ui_fr_light_search` (Headless) |
| MAP-13 | SimAware TRACONs: 89 % of corpus APP/DEP stations drawn as a polygon; test `approach_uses_tracon_prefixes` |
| MAP-14 | Great-circle route + track: `ui_pilot` (Headless) |
| LST-03, LST-04 | Servers; sorting, text filter, columns: code; on screen: To do |
| LST-05 (P2) | Prefiles tab |
| FLT-03 | Highlight colour per filter |
| JMP-02 | Unified search Ctrl+K: `unified`, `ui_fr_light_search` |
| ALR-01 | Notifications: `followed_and_favorites` (Auto); system display: To do |
| UPD-03 | Signed app update: `signature_roundtrip_and_tamper`, `packager_signature_verifies` |
| I18N-01 | French / English: `switch`, `ui_fr_light_search` |

## Measured non-functional requirements

| ID | Measurement |
| --- | --- |
| NF-02 | Geometry prepared in 31 ms; static data loaded in 148 ms (debug) |
| NF-03 | See J2 |
| NF-05 | Binary 24.7 MB; RSS 174 MB; idle CPU 0 % (window not displayed: to re-measure on screen) |
| NF-06 | proptest on the parsers; `cargo fuzz` targets in `fuzz/` |
| NF-10 | CI on 3 OSes: fmt, clippy `-D warnings`, tests, `cargo deny` |

## Outstanding non-code items

- Manual on-screen check of the "To do" rows (mouse, shortcuts, notifications).
- Apple / Windows signing secrets and the minisign update key (see `RELEASING.md`).
- VATSIM policy on displaying member names: the v3 feed provides them; decision still open.

## Corpus

v3 feed snapshots recorded every 60 s on 15 November 2026, names stripped
(`radarvector snapshot record --anonymize`), folder `corpus/` (not versioned).

    50 snapshot(s): 11297 ATC stations, 0 unresolved, resolution rate 100.00%
    APP/DEP stations: 1393, drawn with a TRACON polygon: 1244 (89.3%), circle otherwise
