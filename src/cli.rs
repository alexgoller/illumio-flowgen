//! Command-line interface.

use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::config::{Overrides, Scenario, resolve_collector};
use crate::export::{Protocol, new_encoder};
use crate::inventory::{resolve, write_workloader_csv};
use crate::pattern::{Generator, IPS_LIMIT, RateShape, make_rng, scenario_hosts};
use crate::presets::{PRESETS, find};
use crate::sender::{Engine, Sink};
use crate::stats::Stats;
use crate::ui::{self, Header, Ui};

const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("FLOWGEN_GIT_HASH"),
    ")"
);

#[derive(Parser)]
#[command(name = "flowgen", version = VERSION, about = "NetFlow v5/v9 and IPFIX generator for Illumio Flowlink testing and demos")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate flows and send them to a collector (or print them with --dry-run)
    Run(RunArgs),
    /// List built-in presets, or print one as TOML
    Presets {
        /// Print the preset's scenario TOML
        #[arg(long, value_name = "NAME")]
        show: Option<String>,
    },
    /// Print every host IP a scenario can emit (for creating unmanaged workloads)
    Ips {
        #[command(flatten)]
        source: Source,
        /// List even if the scenario covers more than 4096 hosts
        #[arg(long)]
        all: bool,
        /// Output format: bare IPs, or a workloader wkld-import CSV
        #[arg(long, value_enum, default_value_t = IpsFormat::Plain)]
        format: IpsFormat,
    },
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
enum IpsFormat {
    /// One IP per line
    #[default]
    Plain,
    /// CSV for `workloader wkld-import --umwl`
    Workloader,
}

#[derive(Args)]
#[group(required = true, multiple = false)]
struct Source {
    /// Scenario TOML file
    #[arg(short = 'c', long = "config", value_name = "FILE")]
    config: Option<PathBuf>,
    /// Built-in preset name (see `flowgen presets`)
    #[arg(long, value_name = "NAME")]
    preset: Option<String>,
}

#[derive(Args)]
struct RunArgs {
    #[command(flatten)]
    source: Source,
    /// Collector HOST[:PORT]; port defaults to 2055 (v5/v9) or 4739 (ipfix)
    #[arg(long)]
    collector: Option<String>,
    /// Export protocol
    #[arg(long, value_enum)]
    protocol: Option<Protocol>,
    /// Base flows per second
    #[arg(long)]
    rate: Option<f64>,
    /// Stop after this long (e.g. 30s, 10m, 1h); default: run until Ctrl-C
    #[arg(long, value_parser = humantime::parse_duration)]
    duration: Option<Duration>,
    /// RNG seed for reproducible traffic
    #[arg(long)]
    seed: Option<u64>,
    /// Print flows to stdout instead of sending
    #[arg(long)]
    dry_run: bool,
    /// Plain periodic stats lines instead of a progress bar
    #[arg(long)]
    no_progress: bool,
    /// Only print warnings and the final summary
    #[arg(long)]
    quiet: bool,
}

pub fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Run(args) => run(args),
        Command::Presets { show } => presets(show),
        Command::Ips {
            source,
            all,
            format,
        } => ips(&source, all, format),
    }
}

fn load(source: &Source) -> Result<(Scenario, String)> {
    match (&source.config, &source.preset) {
        (Some(path), _) => Ok((Scenario::from_file(path)?, path.display().to_string())),
        (None, Some(name)) => {
            let preset = find(name).ok_or_else(|| {
                let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
                anyhow!("unknown preset '{name}' (available: {})", names.join(", "))
            })?;
            Ok((Scenario::from_toml(preset.toml)?, format!("preset {name}")))
        }
        (None, None) => unreachable!("clap requires --config or --preset"),
    }
}

fn run(args: RunArgs) -> Result<()> {
    let (mut scenario, label) = load(&args.source)?;
    scenario.apply(&Overrides {
        collector: args.collector.clone(),
        protocol: args.protocol,
        rate: args.rate,
        seed: args.seed,
    });
    let warnings = scenario.validate()?;
    let mode = ui::pick_mode(args.no_progress, args.quiet, args.dry_run);
    for w in &warnings {
        eprintln!("flowgen: warning: {w}");
    }

    let protocol = scenario.target.protocol;
    let (sink, destination) = if args.dry_run {
        (
            Sink::dry_run(Box::new(BufWriter::new(io::stdout()))),
            "dry-run (stdout)".to_string(),
        )
    } else {
        let spec = scenario.target.collector.as_deref().context(
            "no collector: set [target].collector or pass --collector HOST[:PORT] (or use --dry-run)",
        )?;
        let addr = resolve_collector(spec, protocol)?;
        (Sink::udp(addr)?, addr.to_string())
    };

    let (rng, seed) = make_rng(scenario.seed);
    let generator = Generator::new(&scenario, rng)?;
    let rule_names = generator.rule_names().to_vec();
    let header = Header {
        protocol,
        destination,
        local_addr: sink.local_addr(),
        scenario: scenario.name.clone().unwrap_or(label),
        seed,
    };
    let mut engine = Engine {
        generator,
        shape: RateShape::new(&scenario.rate),
        encoder: new_encoder(
            protocol,
            &scenario.target.encoder_options(),
            SystemTime::now(),
        ),
        sink,
    };

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = stop.clone();
        ctrlc::set_handler(move || stop.store(true, Ordering::Relaxed))
            .context("cannot install Ctrl-C handler")?;
    }

    let stats_interval = Duration::from_secs(scenario.stats_interval_secs);
    let mut ui = Ui::start(mode, &header, args.duration, stats_interval);
    let mut stats = Stats::new(rule_names.len());
    let result = engine.run(args.duration, &mut stats, &mut ui, stats_interval, &stop);
    ui.finish(&stats, &rule_names);
    result
}

fn presets(show: Option<String>) -> Result<()> {
    match show {
        Some(name) => {
            let preset = find(&name).ok_or_else(|| anyhow!("unknown preset '{name}'"))?;
            print!("{}", preset.toml);
        }
        None => {
            for p in PRESETS {
                println!("{:<12} {}", p.name, p.description);
            }
        }
    }
    Ok(())
}

fn ips(source: &Source, all: bool, format: IpsFormat) -> Result<()> {
    let (scenario, _) = load(source)?;
    scenario.validate()?;
    let limit = if all { None } else { Some(IPS_LIMIT) };
    let ips = scenario_hosts(&scenario, limit)?;
    match format {
        IpsFormat::Plain => {
            for ip in ips {
                println!("{ip}");
            }
        }
        IpsFormat::Workloader => {
            let hosts: Vec<_> = ips
                .into_iter()
                .map(|ip| resolve(&scenario.hosts, ip))
                .collect();
            let description = match &scenario.name {
                Some(n) => format!("flowgen {n}"),
                None => "flowgen".to_string(),
            };
            let mut out = BufWriter::new(io::stdout().lock());
            write_workloader_csv(&mut out, &description, &hosts)?;
            out.flush()?;
        }
    }
    Ok(())
}
