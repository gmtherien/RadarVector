# Development

RadarVector is a Cargo workspace (Rust 2024 edition).

## Workspace

| Crate | Role |
| --- | --- |
| `radarvector-data` | Parse and validate the upstream boundary and airport files, indices |
| `radarvector-feed` | VATSIM status / v3 / transceivers / METAR client; upstream data updater; app self-update |
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

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
RADARVECTOR_DATA_DIR=<dir> cargo test -p radarvector-data -- --ignored   # real data files
cargo deny check
# GPU + network: offscreen visual references, frame-time benchmark, headless UI tests
cargo test -p radarvector-app --release -- --ignored --nocapture
```

## Further reading

- [RELEASING.md](../RELEASING.md): packaging, signing and releases.
- [DATA-SOURCES.md](DATA-SOURCES.md): origin of every data format, feed schema and algorithm.
- [ACCEPTANCE.md](ACCEPTANCE.md): acceptance checklist.
