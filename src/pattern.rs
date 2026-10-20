//! Flow generation: host sampling, weighted rules, port scans and rate shapes.

use std::collections::BTreeSet;
use std::f64::consts::TAU;
use std::net::Ipv4Addr;
use std::ops::RangeInclusive;
use std::time::{Duration, SystemTime};

use anyhow::{Result, bail};
use ipnet::Ipv4Net;
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{RngExt, SeedableRng};

use crate::config::{Proto, Rate, Scenario, Shape};
use crate::model::{FlowRecord, PROTO_ICMP, PROTO_TCP};

const EPHEMERAL_PORTS: RangeInclusive<u16> = 32768..=60999;
const TCP_FLAGS_SESSION: u8 = 0x1b; // FIN|SYN|PSH|ACK
const TCP_FLAGS_SYN: u8 = 0x02;
const FLOW_DURATION_MS: RangeInclusive<u64> = 100..=30_000;
const SCAN_BYTES: RangeInclusive<u64> = 44..=60;
pub const IPS_LIMIT: u64 = 4096;

/// Seeded RNG; picks (and returns) a random seed when none is given so runs can be reproduced.
pub fn make_rng(seed: Option<u64>) -> (StdRng, u64) {
    let seed = seed.unwrap_or_else(rand::random::<u64>);
    (StdRng::seed_from_u64(seed), seed)
}

/// Uniform host sampling over a list of CIDRs without enumerating them.
#[derive(Debug, Clone)]
pub struct HostPool {
    /// (first host as u32, host count)
    ranges: Vec<(u32, u64)>,
    total: u64,
}

impl HostPool {
    pub fn new(nets: &[Ipv4Net]) -> Self {
        let ranges: Vec<(u32, u64)> = nets
            .iter()
            .map(|net| {
                let base = u32::from(net.network());
                match net.prefix_len() {
                    32 => (base, 1),
                    31 => (base, 2),
                    len => (base + 1, (1u64 << (32 - len)) - 2),
                }
            })
            .collect();
        let total = ranges.iter().map(|r| r.1).sum();
        Self { ranges, total }
    }

    pub fn len(&self) -> u64 {
        self.total
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn sample(&self, rng: &mut StdRng) -> Ipv4Addr {
        let mut i = rng.random_range(0..self.total);
        for &(first, count) in &self.ranges {
            if i < count {
                return Ipv4Addr::from(first + i as u32);
            }
            i -= count;
        }
        unreachable!("sample index is below total")
    }

    pub fn iter(&self) -> impl Iterator<Item = Ipv4Addr> + '_ {
        self.ranges
            .iter()
            .flat_map(|&(first, count)| (0..count).map(move |i| Ipv4Addr::from(first + i as u32)))
    }
}

/// Target flows/s over time.
pub struct RateShape {
    cfg: Rate,
    next_burst_check: f64,
    burst_until: f64,
}

impl RateShape {
    pub fn new(cfg: &Rate) -> Self {
        Self {
            cfg: cfg.clone(),
            next_burst_check: 1.0,
            burst_until: f64::NEG_INFINITY,
        }
    }

    pub fn rate_at(&mut self, t: f64, rng: &mut StdRng) -> f64 {
        let c = &self.cfg;
        let base = c.flows_per_sec;
        let rate = match c.shape {
            Shape::Constant => base,
            Shape::Sine => base * (1.0 + c.amplitude * (TAU * t / c.period_secs).sin()),
            Shape::Burst => {
                while self.next_burst_check <= t {
                    if rng.random_bool(c.burst_chance) {
                        self.burst_until = self.next_burst_check + c.burst_secs;
                    }
                    self.next_burst_check += 1.0;
                }
                if t < self.burst_until {
                    base * c.burst_multiplier
                } else {
                    base
                }
            }
        };
        rate.max(0.0)
    }
}

/// Converts a rate into whole flows per tick, carrying the fractional remainder.
#[derive(Debug, Default)]
pub struct Pacer {
    carry: f64,
}

impl Pacer {
    pub fn due(&mut self, rate: f64, dt_secs: f64) -> usize {
        self.carry += rate * dt_secs;
        let n = self.carry.floor();
        self.carry -= n;
        n as usize
    }
}

pub struct ScanBurst {
    pub src: Ipv4Addr,
    pub flows: Vec<FlowRecord>,
}

struct CompiledRule {
    src: HostPool,
    dst: HostPool,
    protocol: u8,
    ports: Vec<u16>,
    packets: (u64, u64),
    bytes_per_packet: (u64, u64),
}

struct CompiledScan {
    src: Ipv4Addr,
    dst: HostPool,
    ports: Vec<u16>,
    every: Duration,
    flows_per_burst: u32,
    next_at: Duration,
}

pub struct Generator {
    rng: StdRng,
    rule_names: Vec<String>,
    rules: Vec<CompiledRule>,
    /// Running sum of weights, for weighted rule selection.
    cumulative: Vec<u64>,
    scans: Vec<CompiledScan>,
}

fn default_sizes(proto: Proto) -> ((u64, u64), (u64, u64)) {
    match proto {
        Proto::Tcp => ((5, 200), (60, 1400)),
        Proto::Udp => ((1, 20), (60, 1200)),
        Proto::Icmp => ((1, 4), (84, 84)),
    }
}

/// Resample attempts when the destination equals the source.
const MAX_DST_RETRIES: usize = 8;

impl Generator {
    pub fn new(s: &Scenario, rng: StdRng) -> Result<Self> {
        let rules = s
            .rules
            .iter()
            .map(|r| {
                let (packets, bpp) = default_sizes(r.proto);
                CompiledRule {
                    src: HostPool::new(&r.src),
                    dst: HostPool::new(&r.dst),
                    protocol: r.proto.number(),
                    ports: if r.proto == Proto::Icmp {
                        vec![0]
                    } else {
                        r.dst_ports.clone()
                    },
                    packets: r.packets.map_or(packets, |[lo, hi]| (lo, hi)),
                    bytes_per_packet: r.bytes_per_packet.map_or(bpp, |[lo, hi]| (lo, hi)),
                }
            })
            .collect();
        let cumulative = s
            .rules
            .iter()
            .scan(0u64, |acc, r| {
                *acc += u64::from(r.weight);
                Some(*acc)
            })
            .collect();
        let scans = s
            .scans
            .iter()
            .map(|sc| {
                let every = Duration::from_secs(sc.every_secs);
                Ok(CompiledScan {
                    src: sc.src,
                    dst: HostPool::new(&sc.dst),
                    ports: sc.ports.expand()?,
                    every,
                    flows_per_burst: sc.flows_per_burst,
                    next_at: every,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            rng,
            rule_names: s.rules.iter().map(|r| r.name.clone()).collect(),
            rules,
            cumulative,
            scans,
        })
    }

    pub fn rule_names(&self) -> &[String] {
        &self.rule_names
    }

    pub fn has_rules(&self) -> bool {
        !self.rules.is_empty()
    }

    pub fn rng(&mut self) -> &mut StdRng {
        &mut self.rng
    }

    /// One flow from a weighted-random rule. Panics if the scenario has no rules.
    pub fn next_flow(&mut self, now: SystemTime) -> (usize, FlowRecord) {
        let total = *self
            .cumulative
            .last()
            .expect("next_flow requires at least one rule");
        let x = self.rng.random_range(0..total);
        let idx = self.cumulative.partition_point(|&c| c <= x);
        let rule = &self.rules[idx];
        let rng = &mut self.rng;

        let packets = rng.random_range(rule.packets.0..=rule.packets.1);
        let bytes = packets * rng.random_range(rule.bytes_per_packet.0..=rule.bytes_per_packet.1);
        let duration = Duration::from_millis(rng.random_range(FLOW_DURATION_MS));
        let src_addr = rule.src.sample(rng);
        // Avoid host-to-self flows when pools overlap; bounded so a single-host
        // pool equal to src still terminates (and emits the self-flow).
        let mut dst_addr = rule.dst.sample(rng);
        for _ in 0..MAX_DST_RETRIES {
            if dst_addr != src_addr {
                break;
            }
            dst_addr = rule.dst.sample(rng);
        }
        let record = FlowRecord {
            src_addr,
            dst_addr,
            src_port: if rule.protocol == PROTO_ICMP {
                0
            } else {
                rng.random_range(EPHEMERAL_PORTS)
            },
            dst_port: *rule.ports.choose(rng).expect("validated: ports not empty"),
            protocol: rule.protocol,
            packets,
            bytes,
            tcp_flags: if rule.protocol == PROTO_TCP {
                TCP_FLAGS_SESSION
            } else {
                0
            },
            start: now - duration,
            end: now,
        };
        (idx, record)
    }

    /// All scan bursts due at `elapsed` since start (first burst after one interval).
    pub fn due_scans(&mut self, elapsed: Duration, now: SystemTime) -> Vec<ScanBurst> {
        let mut out = Vec::new();
        for scan in &mut self.scans {
            while elapsed >= scan.next_at {
                scan.next_at += scan.every;
                let flows = (0..scan.flows_per_burst)
                    .map(|_| FlowRecord {
                        src_addr: scan.src,
                        dst_addr: scan.dst.sample(&mut self.rng),
                        src_port: self.rng.random_range(EPHEMERAL_PORTS),
                        dst_port: *scan
                            .ports
                            .choose(&mut self.rng)
                            .expect("validated: ports not empty"),
                        protocol: PROTO_TCP,
                        packets: 1,
                        bytes: self.rng.random_range(SCAN_BYTES),
                        tcp_flags: TCP_FLAGS_SYN,
                        start: now,
                        end: now,
                    })
                    .collect();
                out.push(ScanBurst {
                    src: scan.src,
                    flows,
                });
            }
        }
        out
    }
}

/// Every host IP the scenario can emit (sorted, deduplicated), for creating
/// matching unmanaged workloads in the PCE. `limit` caps an upper bound computed
/// from the aggregated nets, checked before anything is enumerated.
pub fn scenario_hosts(s: &Scenario, limit: Option<u64>) -> Result<Vec<Ipv4Addr>> {
    let mut pools = Vec::new();
    for r in &s.rules {
        pools.push(HostPool::new(&r.src));
        pools.push(HostPool::new(&r.dst));
    }
    for sc in &s.scans {
        pools.push(HostPool::new(&sc.dst));
    }
    // Upper bound on the deduplicated count: aggregate overlapping/identical
    // nets first so repeated subnets are not counted once per rule.
    let mut all_nets: Vec<Ipv4Net> = Vec::new();
    for r in &s.rules {
        all_nets.extend(r.src.iter().chain(&r.dst));
    }
    for sc in &s.scans {
        all_nets.extend(&sc.dst);
    }
    let all_nets = Ipv4Net::aggregate(&all_nets);
    let mut upper = HostPool::new(&all_nets).len();
    let extra_srcs: BTreeSet<Ipv4Addr> = s
        .scans
        .iter()
        .map(|sc| sc.src)
        .filter(|ip| !all_nets.iter().any(|n| n.contains(ip)))
        .collect();
    upper += extra_srcs.len() as u64;
    if let Some(limit) = limit
        && upper > limit
    {
        bail!(
            "scenario covers up to {upper} host IPs (limit {limit}); pass --all to list them anyway"
        );
    }
    let mut hosts = BTreeSet::new();
    for pool in &pools {
        hosts.extend(pool.iter());
    }
    hosts.extend(s.scans.iter().map(|sc| sc.src));
    Ok(hosts.into_iter().collect())
}
