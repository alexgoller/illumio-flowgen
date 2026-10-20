//! The run loop: every tick, generate due flows, encode, and deliver them.

use std::io::{self, Write};
use std::net::{SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};

use crate::export::Encoder;
use crate::model::FlowRecord;
use crate::pattern::{Generator, Pacer, RateShape};
use crate::stats::Stats;
use crate::ui::Ui;

pub const TICK: Duration = Duration::from_millis(100);

pub enum Sink {
    Udp(UdpSocket),
    DryRun(Box<dyn Write>),
}

impl Sink {
    pub fn udp(dest: SocketAddr) -> Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", 0)).context("cannot bind a local UDP socket")?;
        socket
            .connect(dest)
            .with_context(|| format!("cannot set up UDP to collector {dest}"))?;
        Ok(Sink::Udp(socket))
    }

    pub fn dry_run(out: Box<dyn Write>) -> Self {
        Sink::DryRun(out)
    }

    /// Local address used as exporter source (None for dry-run).
    pub fn local_addr(&self) -> Option<SocketAddr> {
        match self {
            Sink::Udp(socket) => socket.local_addr().ok(),
            Sink::DryRun(_) => None,
        }
    }

    /// UDP: send every datagram; failures are counted and the last one returned.
    /// Dry-run: print records; a write failure is returned as `Err`.
    pub fn deliver(
        &mut self,
        records: &[FlowRecord],
        datagrams: &[Vec<u8>],
        stats: &mut Stats,
    ) -> io::Result<Option<io::Error>> {
        match self {
            Sink::Udp(socket) => {
                let mut last_error = None;
                for dg in datagrams {
                    match socket.send(dg) {
                        Ok(n) => {
                            stats.datagrams += 1;
                            stats.wire_bytes += n as u64;
                        }
                        Err(e) => {
                            stats.send_errors += 1;
                            last_error = Some(e);
                        }
                    }
                }
                Ok(last_error)
            }
            Sink::DryRun(out) => {
                for r in records {
                    writeln!(out, "{}", r.dry_run_line())?;
                }
                out.flush()?;
                stats.datagrams += datagrams.len() as u64;
                stats.wire_bytes += datagrams.iter().map(|d| d.len() as u64).sum::<u64>();
                Ok(None)
            }
        }
    }
}

pub struct Engine {
    pub generator: Generator,
    pub shape: RateShape,
    pub encoder: Box<dyn Encoder>,
    pub sink: Sink,
}

impl Engine {
    /// Runs until `duration` elapses, `stop` is set, or dry-run stdout closes.
    pub fn run(
        &mut self,
        duration: Option<Duration>,
        stats: &mut Stats,
        ui: &mut Ui,
        stats_interval: Duration,
        stop: &AtomicBool,
    ) -> Result<()> {
        let start = Instant::now();
        let mut pacer = Pacer::default();
        let mut last_t = 0.0;
        let mut next_tick = start;
        let mut last_error_log: Option<Instant> = None;

        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let elapsed = start.elapsed();
            if duration.is_some_and(|d| elapsed >= d) {
                break;
            }
            let t = elapsed.as_secs_f64();
            let dt = t - last_t;
            last_t = t;

            let rate = self.shape.rate_at(t, self.generator.rng());
            stats.target_rate = rate;
            let now = SystemTime::now();

            let mut records = Vec::new();
            if self.generator.has_rules() {
                for _ in 0..pacer.due(rate, dt) {
                    let (rule, record) = self.generator.next_flow(now);
                    stats.per_rule[rule] += 1;
                    records.push(record);
                }
            }
            for burst in self.generator.due_scans(elapsed, now) {
                stats.scan_bursts += 1;
                stats.scan_flows += burst.flows.len() as u64;
                ui.notice(&format!(
                    "scan burst from {}: {} flows",
                    burst.src,
                    burst.flows.len()
                ));
                records.extend(burst.flows);
            }

            if !records.is_empty() {
                stats.flows += records.len() as u64;
                let datagrams = self.encoder.encode(&records, now);
                match self.sink.deliver(&records, &datagrams, stats) {
                    Ok(None) => {}
                    Ok(Some(e)) => {
                        if last_error_log.is_none_or(|t| t.elapsed() >= stats_interval) {
                            last_error_log = Some(Instant::now());
                            ui.warn(&format!(
                                "send failed: {e} ({} errors so far)",
                                stats.send_errors
                            ));
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::BrokenPipe => break,
                    Err(e) => return Err(e).context("cannot write dry-run output"),
                }
            }

            stats.sample(Instant::now());
            ui.update(stats);

            next_tick += TICK;
            let now = Instant::now();
            if next_tick > now {
                std::thread::sleep(next_tick - now);
            } else {
                next_tick = now; // fell behind; don't try to catch up in a burst
            }
        }
        Ok(())
    }
}
