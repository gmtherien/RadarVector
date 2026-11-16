//! `radarvector`: diagnostic command line for RadarVector's data core.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use radarvector_core::snapshot::{NetworkSnapshot, unresolved_histogram};
use radarvector_data::StaticData;
use radarvector_feed::datasource::{self, UpdateOutcome};
use radarvector_feed::models::{DataFeed, decode_data_feed};
use radarvector_feed::{CancellationToken, FeedClient, FetchOutcome};

#[derive(Parser)]
#[command(name = "radarvector", version, about = "RadarVector data diagnostics")]
struct Cli {
    /// Directory holding VATSpy.dat and Boundaries.geojson (default: RadarVector's data dir).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// VAT-Spy Data Project files.
    Data {
        #[command(subcommand)]
        cmd: DataCmd,
    },
    /// Live VATSIM data feed.
    Feed {
        #[command(subcommand)]
        cmd: FeedCmd,
    },
    /// Resolve every ATC callsign of a snapshot and report unresolved ones.
    Resolve {
        /// Saved v3 JSON files (default: fetch the live feed).
        snapshots: Vec<PathBuf>,
        /// Print the full list of unresolved callsigns.
        #[arg(long)]
        verbose: bool,
    },
    /// Record snapshots of the live feed.
    Snapshot {
        #[command(subcommand)]
        cmd: SnapshotCmd,
    },
}

#[derive(Subcommand)]
enum DataCmd {
    /// Parse and validate the files.
    Validate {
        #[arg(long)]
        json: bool,
    },
    /// Download the latest release and install it if valid.
    Update {
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum FeedCmd {
    /// Fetch one snapshot and print a summary.
    Dump {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum SnapshotCmd {
    /// Save `count` snapshots, `interval` seconds apart (minimum 15).
    Record {
        #[arg(long, default_value_t = 50)]
        count: usize,
        #[arg(long, default_value_t = 60)]
        interval: u64,
        #[arg(long)]
        out: PathBuf,
        /// Replace names with empty strings before saving.
        #[arg(long)]
        anonymize: bool,
    },
}

fn data_dir(cli: &Cli) -> Result<PathBuf> {
    match &cli.data_dir {
        Some(d) => Ok(d.clone()),
        None => Ok(datasource::default_data_dir()?),
    }
}

fn load_static(dir: &Path) -> Result<StaticData> {
    let (data, report) = StaticData::load_dir(dir).with_context(|| {
        format!(
            "loading data from {} (run `radarvector data update` first?)",
            dir.display()
        )
    })?;
    if !report.is_clean() {
        eprintln!(
            "warning: {} rejected lines, {} boundary issues, {} validation errors",
            report.dat.rejected_lines(),
            report.boundary_issues.len(),
            report.validation.errors().count()
        );
    }
    Ok(data)
}

async fn fetch_live(client: &FeedClient) -> Result<DataFeed> {
    client.reset_unchanged_check();
    match client.fetch_data(&CancellationToken::new()).await? {
        FetchOutcome::Updated(f) => Ok(*f),
        FetchOutcome::Unchanged => bail!("feed unchanged"),
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Data {
            cmd: DataCmd::Validate { json },
        } => {
            let dir = data_dir(&cli)?;
            let t = std::time::Instant::now();
            let (data, report) = StaticData::load_dir(&dir)?;
            let elapsed = t.elapsed();
            if *json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                for i in &report.dat.issues {
                    println!("VATSpy.dat:{}: {:?} {:?}", i.line, i.section, i.kind);
                }
                for i in &report.boundary_issues {
                    println!("Boundaries.geojson feature {}: {}", i.index, i.message);
                }
                for f in &report.validation.findings {
                    println!("{:?}: {}", f.severity, f.message);
                }
                let m = datasource::read_manifest(&dir);
                println!("---");
                println!(
                    "dir: {}  release: {}",
                    dir.display(),
                    m.map(|m| m.tag).unwrap_or_else(|| "?".into())
                );
                println!(
                    "{} data lines, {} rejected ({:.2}% accepted)",
                    report.dat.data_lines,
                    report.dat.rejected_lines(),
                    100.0 * (1.0 - report.dat.rejected_lines() as f64 / report.dat.data_lines.max(1) as f64)
                );
                println!(
                    "{} countries, {} airports ({} pseudo), {} FIR lines, {} UIRs, {} IDL points, {} boundary features",
                    data.countries.len(),
                    data.airports.len(),
                    data.airports.iter().filter(|a| a.is_pseudo).count(),
                    data.firs.len(),
                    data.uirs.len(),
                    data.idl.len(),
                    data.boundaries.feature_count
                );
                println!(
                    "{} errors, {} warnings, loaded in {:.0} ms",
                    report.validation.errors().count(),
                    report.validation.warnings().count(),
                    elapsed.as_secs_f64() * 1000.0
                );
            }
            if !report.is_clean() {
                std::process::exit(1);
            }
        }
        Cmd::Data {
            cmd: DataCmd::Update { force },
        } => {
            let dir = data_dir(&cli)?;
            let client = FeedClient::new()?;
            match datasource::update(client.http(), &dir, *force, &CancellationToken::new()).await? {
                UpdateOutcome::Installed(m) => println!(
                    "installed {} into {} (sha256 {})",
                    m.tag,
                    dir.display(),
                    &m.dat_sha256[..12]
                ),
                UpdateOutcome::UpToDate(m) => println!("up to date: {} in {}", m.tag, dir.display()),
            }
        }
        Cmd::Feed {
            cmd: FeedCmd::Dump { json },
        } => {
            let client = FeedClient::new()?;
            let feed = fetch_live(&client).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&feed)?);
            } else {
                println!("update: {}", feed.general.update_timestamp);
                println!(
                    "clients: {}  unique users: {}",
                    feed.general.connected_clients, feed.general.unique_users
                );
                println!(
                    "pilots: {} ({} with flight plan)  controllers: {}  atis: {}  servers: {}  prefiles: {}",
                    feed.pilots.len(),
                    feed.pilots.iter().filter(|p| p.flight_plan.is_some()).count(),
                    feed.controllers.len(),
                    feed.atis.len(),
                    feed.servers.len(),
                    feed.prefiles.len()
                );
                if !feed.skipped.is_empty() {
                    println!("skipped invalid records: {:?}", feed.skipped);
                }
            }
        }
        Cmd::Resolve { snapshots, verbose } => {
            let data = load_static(&data_dir(&cli)?)?;
            let feeds: Vec<DataFeed> = if snapshots.is_empty() {
                vec![fetch_live(&FeedClient::new()?).await?]
            } else {
                snapshots
                    .iter()
                    .map(|p| {
                        let bytes = std::fs::read(p).with_context(|| p.display().to_string())?;
                        Ok(decode_data_feed(&bytes)?)
                    })
                    .collect::<Result<_>>()?
            };
            let tracons = datasource::tracon::default_dir()
                .ok()
                .and_then(|d| std::fs::read_to_string(d.join(datasource::tracon::FILE)).ok())
                .unwrap_or_else(|| {
                    include_str!("../../../assets/simaware-tracon/TRACONBoundaries.geojson").to_string()
                });
            let tracons = radarvector_data::tracon::parse(&tracons)
                .map(|(t, _)| t)
                .unwrap_or_default();
            let snaps: Vec<NetworkSnapshot> = feeds
                .into_iter()
                .map(|f| NetworkSnapshot::build_with(&data, &tracons, f))
                .collect();
            let (mut approaches, mut with_tracon) = (0, 0);
            for s in &snaps {
                for st in &s.stations {
                    if let radarvector_core::Resolved::Approach { tracon, .. } = &st.resolved {
                        approaches += 1;
                        with_tracon += usize::from(tracon.is_some());
                    }
                }
            }
            let (mut placeable, mut unresolved) = (0, 0);
            for s in &snaps {
                placeable += s.placeable().count();
                unresolved += s.unresolved().count();
            }
            let rate = if placeable == 0 {
                1.0
            } else {
                1.0 - unresolved as f64 / placeable as f64
            };
            println!(
                "{} snapshot(s): {placeable} ATC stations, {unresolved} unresolved, resolution rate {:.2}%",
                snaps.len(),
                rate * 100.0
            );
            println!(
                "APP/DEP stations: {approaches}, drawn with a TRACON polygon: {with_tracon} ({:.1}%), circle otherwise",
                100.0 * with_tracon as f64 / approaches.max(1) as f64
            );
            let hist = unresolved_histogram(&snaps);
            let mut list: Vec<_> = hist.into_iter().collect();
            list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            let shown = if *verbose { list.len() } else { list.len().min(30) };
            for (cs, n) in &list[..shown] {
                println!("  {cs:<16} x{n}");
            }
            if shown < list.len() {
                println!("  ... {} more (use --verbose)", list.len() - shown);
            }
        }
        Cmd::Snapshot {
            cmd:
                SnapshotCmd::Record {
                    count,
                    interval,
                    out,
                    anonymize,
                },
        } => {
            std::fs::create_dir_all(out)?;
            let client = FeedClient::new()?;
            let interval = radarvector_feed::backoff::clamp_interval(Duration::from_secs(*interval));
            let mut saved = 0;
            while saved < *count {
                match client.fetch_data(&CancellationToken::new()).await {
                    Ok(FetchOutcome::Updated(mut feed)) => {
                        if *anonymize {
                            feed.pilots.iter_mut().for_each(|p| p.name.clear());
                            feed.controllers
                                .iter_mut()
                                .chain(feed.atis.iter_mut())
                                .for_each(|c| c.name.clear());
                            feed.prefiles.iter_mut().for_each(|p| p.name.clear());
                        }
                        saved += 1;
                        let path = out.join(format!("snapshot-{saved:03}.json"));
                        std::fs::write(&path, serde_json::to_vec(&feed)?)?;
                        eprintln!(
                            "saved {} ({} pilots, {} controllers)",
                            path.display(),
                            feed.pilots.len(),
                            feed.controllers.len()
                        );
                    }
                    Ok(FetchOutcome::Unchanged) => eprintln!("unchanged, waiting"),
                    Err(e) => eprintln!("fetch failed: {e}"),
                }
                if saved < *count {
                    tokio::time::sleep(interval).await;
                }
            }
        }
    }
    Ok(())
}
