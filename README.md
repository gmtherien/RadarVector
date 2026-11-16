# RadarVector

A native, cross-platform VATSIM traffic viewer written in Rust. It reads the
[VAT-Spy Data Project](https://github.com/vatsimnetwork/vatspy-data-project)
files as-is and the public VATSIM data feeds.

*Compatible with the data of the VAT-Spy Data Project.* RadarVector is not
affiliated with VAT-Spy or VATSIM.

## Status

Milestones J1–J5 of the specification are implemented: data core and CLI,
GPU map, VAT-Spy feature parity (lists, detail sheets, filters, favourites,
jump box, bookmarks, METAR, settings, data updates), packaging and signed
in-app updates, and the v1.x additions (TRACON polygons, light/dark themes,
unified search, desktop alerts, flight track, French/English UI).

## Workspace

| Crate | Role |
| --- | --- |
| `radarvector-data` | Parse and validate `VATSpy.dat` and `Boundaries.geojson`, indices |
| `radarvector-feed` | VATSIM status / v3 / transceivers / METAR client; data project updater |
| `radarvector-core` | Callsign resolution, network snapshots, diffs, filters |
| `radarvector-geo` | Great-circle maths, point-in-polygon, projection and camera |
| `radarvector-render` | wgpu map renderer (egui paint callback + headless offscreen) |
| `radarvector-app` | `RadarVector` desktop application (egui/eframe, egui_dock) |
| `radarvector-cli` | `radarvector` diagnostic binary |

## Run the app

```bash
cargo run -p radarvector-app --release
```


## CLI

```bash
cargo run -p radarvector-cli -- data update      # download latest data release
cargo run -p radarvector-cli -- data validate    # parse + validate installed files
cargo run -p radarvector-cli -- feed dump        # live feed summary
cargo run -p radarvector-cli -- resolve          # callsign resolution rate (live)
cargo run -p radarvector-cli -- snapshot record --count 50 --out corpus --anonymize
```

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RADARVECTOR_DATA_DIR=<dir> cargo test -p radarvector-data -- --ignored   # real data files
cargo deny check
# GPU + network: offscreen visual references, NF-03 benchmark, headless UI tests
cargo test -p radarvector-app --release -- --ignored --nocapture
```

Packaging and releases: see [RELEASING.md](RELEASING.md).

Cleanroom rules and the source of every format are in [PROVENANCE.md](PROVENANCE.md).

## Licences

Code: MIT OR Apache-2.0, at your option.
TRACON boundaries: [SimAware TRACON Project](https://github.com/vatsimnetwork/simaware-tracon-project), CC-BY-SA-4.0.
Basemap: Natural Earth (public domain).
Data: the VAT-Spy Data Project files are © their contributors, licensed
[CC-BY-SA-4.0](https://creativecommons.org/licenses/by-sa/4.0/). RadarVector
downloads them unmodified; it does not redistribute modified copies.
