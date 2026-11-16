# Provenance

RadarVector is a cleanroom project. It is written only from public sources:
the project specification, the VAT-Spy Data Project repository, VATSIM's
public documentation and live public feeds. VAT-Spy itself (its executable,
libraries, configuration, UI text, icons, colours) was **never** decompiled,
disassembled or inspected.

Each entry below records where a format or algorithm comes from.

## Data formats

| Item | Source | Consulted |
| --- | --- | --- |
| `VATSpy.dat` sections and fields | `VATSpy.dat` and `README.md` of https://github.com/vatsimnetwork/vatspy-data-project (release v2611.2) | 2026-11-15 |
| `VATSpy.dat` validation rules | `scripts/bin/validate-dat.ts` in the same repository | 2026-11-15 |
| Trailing `; comment` after the last field (277 airport lines) | Observed in `VATSpy.dat` v2611.2 | 2026-11-15 |
| Duplicate non-pseudo ICAO tolerated (YBAF, AYKG) | `validate-dat.ts` only rejects when `isPseudo` is falsy, which the string `"0"` never is; published file contains such duplicates | 2026-11-15 |
| `Boundaries.geojson` structure and properties | `Boundaries.geojson` and `schema.json` in the same repository | 2026-11-15 |
| Release layout (tag = AIRAC, assets `VATSpy.dat`, `Boundaries.geojson`, `FIRBoundaries.dat`) | GitHub API `repos/vatsimnetwork/vatspy-data-project/releases/latest` | 2026-11-15 |
| TRACON boundaries (`id`, `prefix[]`, `suffix` DEP, `name`, `label_lat/lon`), licence CC-BY-SA-4.0, release asset `TRACONBoundaries.geojson` (v1.2.14) | https://github.com/vatsimnetwork/simaware-tracon-project (`schema-single.json`, GitHub API licence field) | 2026-11-15 |
| Coastlines, land, country borders 1:50m (public domain) | https://github.com/nvkelso/natural-earth-vector (`geojson/ne_50m_*`) | 2026-11-15 |

## VATSIM feeds

| Item | Source | Consulted |
| --- | --- | --- |
| Feed URLs (`data.v3`, `transceivers`, `metar`) | https://status.vatsim.net/status.json (live response) | 2026-11-15 |
| v3 data feed schema (`general`, `pilots`, `controllers`, `atis`, `servers`, `prefiles`, reference tables) | Live response of https://data.vatsim.net/v3/vatsim-data.json | 2026-11-15 |
| Facility ids 0 OBS, 1 FSS, 2 DEL, 3 GND, 4 TWR, 5 APP, 6 CTR | `facilities` table of the live v3 feed | 2026-11-15 |
| Transceivers schema (`callsign`, `transceivers[].latDeg/lonDeg/heightMslM/frequency`) | Live response of https://data.vatsim.net/v3/transceivers-data.json | 2026-11-15 |
| METAR endpoint `metar.php?id=A,B` returning one line per station | Live response of https://metar.vatsim.net/metar.php | 2026-11-15 |
| Feed cache period (~15 s) | `cache-control: max-age=15` header of the v3 feed | 2026-11-15 |
| "Use status.json, don't hard-code feed URLs" | https://forums.vatsim.net/topic/29914-datafeed-urls | per spec |
| v3 schema, "re-generates every 15 seconds", `heading` is **magnetic** | https://vatsim.dev/api/data-api/get-network-data | 2026-11-15 |
| Public APIs do not expose names except the data feed itself | https://vatsim.dev/services/apis | 2026-11-15 |

## Algorithms

| Item | Source |
| --- | --- |
| Callsign resolution (`radarvector-core/src/callsign.rs`) | Spec §5.3, derived from the structure of `[FIRs]`, `[UIRs]`, `[Airports]`. Tested against live snapshots only. |
| US ARTCC rule: 3-letter key starting with `Z` → FIR ICAO `K` + key (e.g. `ZJX_T_CTR` → `KZJX`) | FAA ARTCC identifiers (public), added after `ZJX_T_CTR` was the only unresolved callsign on a live snapshot, 2026-11-15 |
| Oceanic / domestic choice (`_FSS` → oceanic, `_CTR` → domestic when both exist) | Spec §5.3 step 3, to be confirmed by observation of the network |
| Great-circle distance, initial bearing, destination point, slerp | Standard spherical trigonometry (public domain formulas) |
| Civil date from Unix days | Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms" (public domain) |
| Wildcard matcher (`*` only) | Classic greedy backtracking glob match, written from scratch |

## Rendering and UI

| Item | Source |
| --- | --- |
| Colours (Okabe–Ito based, colour-blind safe accents), airport symbol design, icon | Designed for RadarVector; no VAT-Spy asset, palette or text was used |
| Aircraft icon orientation uses the feed `heading` (magnetic); the few-degree difference from true track is accepted | vatsim.dev data feed schema |
| TRACON lookup: full callsign key, then drop trailing segments; `DEP` boundaries preferred for `_DEP` | SimAware `schema-single.json` description of `prefix`/`suffix` |
| Thick lines as instanced screen-space quads; equirectangular / Mercator in the vertex shader | Standard GPU techniques, written from scratch |
| Update manifest format (`latest.json`) and minisign signatures | cargo-packager updater format; https://jedisct1.github.io/minisign/ |
