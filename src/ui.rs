//! Terminal output: header, live progress (indicatif), plain log lines and summary.
//! Everything goes to stderr so dry-run output on stdout stays pipeable.

use std::io::IsTerminal;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::export::Protocol;
use crate::stats::Stats;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Progress bar / spinner with a live status line.
    Interactive,
    /// One stats line per interval, for logs and pipes.
    Plain,
    /// Only warnings and the final summary.
    Quiet,
}

/// Terminal-aware wrapper around [`select_mode`].
pub fn pick_mode(no_progress: bool, quiet: bool, dry_run: bool) -> Mode {
    select_mode(
        std::io::stderr().is_terminal(),
        std::io::stdout().is_terminal(),
        dry_run,
        no_progress,
        quiet,
    )
}

/// Pure mode selection. A live progress bar redraws on stderr and would garble
/// dry-run flow lines printed to a terminal on stdout, so that case is plain.
pub fn select_mode(
    stderr_tty: bool,
    stdout_tty: bool,
    dry_run: bool,
    no_progress: bool,
    quiet: bool,
) -> Mode {
    if quiet {
        Mode::Quiet
    } else if no_progress || !stderr_tty || (dry_run && stdout_tty) {
        Mode::Plain
    } else {
        Mode::Interactive
    }
}

pub struct Header {
    pub protocol: Protocol,
    pub destination: String,
    pub local_addr: Option<SocketAddr>,
    pub scenario: String,
    pub seed: u64,
}

impl Header {
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!(
                "flowgen {} -> {} | scenario: {} | seed: {}",
                self.protocol.label(),
                self.destination,
                self.scenario,
                self.seed
            ),
            match self.local_addr {
                Some(addr) => format!(
                    "exporter source IP: {} (add it to Flowlink remote_addrs if you restrict senders)",
                    addr.ip()
                ),
                None => "dry run: flows are printed to stdout, nothing is sent".to_string(),
            },
            "note: Flowlink posts to the PCE every aggregation_minutes (default 10, min 5); expect that delay in Explorer".to_string(),
        ]
    }
}

pub struct Ui {
    mode: Mode,
    bar: Option<ProgressBar>,
    start: Instant,
    duration: Option<Duration>,
    stats_interval: Duration,
    last_plain: Instant,
}

impl Ui {
    pub fn start(
        mode: Mode,
        header: &Header,
        duration: Option<Duration>,
        stats_interval: Duration,
    ) -> Self {
        if mode != Mode::Quiet {
            for line in header.lines() {
                eprintln!("{line}");
            }
        }
        let bar = (mode == Mode::Interactive).then(|| make_bar(duration));
        let now = Instant::now();
        Self {
            mode,
            bar,
            start: now,
            duration,
            stats_interval,
            last_plain: now,
        }
    }

    pub fn update(&mut self, stats: &Stats) {
        let elapsed = self.start.elapsed();
        match self.mode {
            Mode::Interactive => {
                if let Some(bar) = &self.bar {
                    if self.duration.is_some() {
                        bar.set_position(elapsed.as_millis() as u64);
                    }
                    bar.set_message(status_line(stats));
                }
            }
            Mode::Plain => {
                if self.last_plain.elapsed() >= self.stats_interval {
                    self.last_plain = Instant::now();
                    eprintln!("[{}] {}", fmt_elapsed(elapsed), status_line(stats));
                }
            }
            Mode::Quiet => {}
        }
    }

    /// Informational event (e.g. scan burst); hidden in quiet mode.
    pub fn notice(&self, msg: &str) {
        match (&self.bar, self.mode) {
            (Some(bar), _) => bar.println(msg),
            (None, Mode::Plain) => eprintln!("{msg}"),
            _ => {}
        }
    }

    /// Always shown, also in quiet mode.
    pub fn warn(&self, msg: &str) {
        let line = format!("flowgen: warning: {msg}");
        match &self.bar {
            Some(bar) => bar.println(line),
            None => eprintln!("{line}"),
        }
    }

    pub fn finish(self, stats: &Stats, rule_names: &[String]) {
        if let Some(bar) = &self.bar {
            bar.finish_and_clear();
        }
        eprintln!("{}", summary(stats, rule_names, self.start.elapsed()));
    }
}

fn make_bar(duration: Option<Duration>) -> ProgressBar {
    let bar = match duration {
        Some(d) => {
            let bar = ProgressBar::new(d.as_millis() as u64);
            bar.set_style(
                ProgressStyle::with_template(
                    "{spinner:.green} [{elapsed_precise}] {wide_bar:.cyan/blue} {percent:>3}%  eta {eta}\n  {msg}",
                )
                .expect("valid progress template")
                .progress_chars("=> "),
            );
            bar
        }
        None => {
            let bar = ProgressBar::new_spinner();
            bar.set_style(
                ProgressStyle::with_template("{spinner:.green} [{elapsed_precise}] {msg}")
                    .expect("valid spinner template"),
            );
            bar
        }
    };
    bar.set_draw_target(ProgressDrawTarget::stderr());
    bar.enable_steady_tick(Duration::from_millis(250));
    bar
}

pub fn status_line(s: &Stats) -> String {
    format!(
        "rate {:.0}/s (target {:.0})  flows {}  pkts {}  errs {}  scans {}",
        s.measured_rate(),
        s.target_rate,
        thousands(s.flows),
        thousands(s.datagrams),
        thousands(s.send_errors),
        s.scan_bursts
    )
}

pub fn summary(s: &Stats, rule_names: &[String], elapsed: Duration) -> String {
    let secs = elapsed.as_secs_f64();
    let avg = if secs > 0.0 {
        s.flows as f64 / secs
    } else {
        0.0
    };
    let share = |n: u64| {
        if s.flows > 0 {
            n as f64 * 100.0 / s.flows as f64
        } else {
            0.0
        }
    };
    let width = rule_names
        .iter()
        .map(String::len)
        .chain([7])
        .max()
        .unwrap_or(7);

    let mut out = format!(
        "\nsummary: ran {}  flows {}  pkts {}  bytes {}  avg {:.1} flows/s  send errors {}  scan bursts {}\n",
        fmt_elapsed(elapsed),
        thousands(s.flows),
        thousands(s.datagrams),
        thousands(s.wire_bytes),
        avg,
        thousands(s.send_errors),
        s.scan_bursts
    );
    out.push_str(&format!(
        "  {:<width$}  {:>12}  {:>6}\n",
        "rule", "flows", "share"
    ));
    for (name, &n) in rule_names.iter().zip(&s.per_rule) {
        out.push_str(&format!(
            "  {:<width$}  {:>12}  {:>5.1}%\n",
            name,
            thousands(n),
            share(n)
        ));
    }
    if s.scan_flows > 0 {
        out.push_str(&format!(
            "  {:<width$}  {:>12}  {:>5.1}%\n",
            "(scans)",
            thousands(s.scan_flows),
            share(s.scan_flows)
        ));
    }
    out
}

pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn fmt_elapsed(d: Duration) -> String {
    let t = d.as_secs();
    format!("{:02}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
}
