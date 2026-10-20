//! Scenario configuration: TOML schema, CLI overrides, validation, collector parsing.

use std::net::{Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use ipnet::Ipv4Net;
use serde::{Deserialize, Deserializer};

use crate::export::{EncoderOptions, Protocol};
use crate::model::{PROTO_ICMP, PROTO_TCP, PROTO_UDP};

pub const MIN_DATAGRAM: usize = 256;
pub const MAX_DATAGRAM: usize = 65000;
pub const HIGH_RATE_WARNING: f64 = 5000.0;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
    #[serde(default = "default_stats_interval")]
    pub stats_interval_secs: u64,
    #[serde(default)]
    pub target: Target,
    #[serde(default)]
    pub rate: Rate,
    #[serde(default, rename = "rule")]
    pub rules: Vec<Rule>,
    #[serde(default, rename = "scan")]
    pub scans: Vec<Scan>,
}

fn default_stats_interval() -> u64 {
    10
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Target {
    pub collector: Option<String>,
    pub protocol: Protocol,
    pub observation_domain: u32,
    pub template_refresh_secs: u64,
    pub max_datagram: usize,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            collector: None,
            protocol: Protocol::Ipfix,
            observation_domain: 1,
            template_refresh_secs: 30,
            max_datagram: 1400,
        }
    }
}

impl Target {
    pub fn encoder_options(&self) -> EncoderOptions {
        EncoderOptions {
            observation_domain: self.observation_domain,
            template_refresh: Duration::from_secs(self.template_refresh_secs),
            max_datagram: self.max_datagram,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    #[default]
    Constant,
    Sine,
    Burst,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Rate {
    pub flows_per_sec: f64,
    pub shape: Shape,
    pub period_secs: f64,
    pub amplitude: f64,
    pub burst_chance: f64,
    pub burst_multiplier: f64,
    pub burst_secs: f64,
}

impl Default for Rate {
    fn default() -> Self {
        Self {
            flows_per_sec: 100.0,
            shape: Shape::Constant,
            period_secs: 300.0,
            amplitude: 0.5,
            burst_chance: 0.05,
            burst_multiplier: 5.0,
            burst_secs: 10.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Proto {
    Tcp,
    Udp,
    Icmp,
}

impl Proto {
    pub fn number(self) -> u8 {
        match self {
            Proto::Tcp => PROTO_TCP,
            Proto::Udp => PROTO_UDP,
            Proto::Icmp => PROTO_ICMP,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub name: String,
    #[serde(deserialize_with = "de_nets")]
    pub src: Vec<Ipv4Net>,
    #[serde(deserialize_with = "de_nets")]
    pub dst: Vec<Ipv4Net>,
    pub proto: Proto,
    #[serde(default)]
    pub dst_ports: Vec<u16>,
    #[serde(default = "default_weight")]
    pub weight: u32,
    /// Packets per flow, inclusive [min, max]. Default depends on proto.
    #[serde(default)]
    pub packets: Option<[u64; 2]>,
    /// Bytes per packet, inclusive [min, max]. Default depends on proto.
    #[serde(default)]
    pub bytes_per_packet: Option<[u64; 2]>,
}

fn default_weight() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scan {
    pub src: Ipv4Addr,
    #[serde(deserialize_with = "de_nets")]
    pub dst: Vec<Ipv4Net>,
    pub ports: PortSpec,
    #[serde(default = "default_scan_every")]
    pub every_secs: u64,
    #[serde(default = "default_flows_per_burst")]
    pub flows_per_burst: u32,
}

fn default_scan_every() -> u64 {
    120
}

fn default_flows_per_burst() -> u32 {
    200
}

/// `ports = "1-1024"`, `ports = "22"` or `ports = [22, 3389]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum PortSpec {
    List(Vec<u16>),
    Range(String),
}

impl PortSpec {
    pub fn expand(&self) -> Result<Vec<u16>> {
        match self {
            PortSpec::List(ports) => Ok(ports.clone()),
            PortSpec::Range(spec) => {
                let spec = spec.trim();
                let (lo, hi) = spec.split_once('-').unwrap_or((spec, spec));
                let parse = |s: &str| -> Result<u16> {
                    s.trim()
                        .parse()
                        .with_context(|| format!("invalid port range '{spec}'"))
                };
                let (lo, hi) = (parse(lo)?, parse(hi)?);
                if lo == 0 || lo > hi {
                    bail!("invalid port range '{spec}' (expected 1 <= start <= end)");
                }
                Ok((lo..=hi).collect())
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub collector: Option<String>,
    pub protocol: Option<Protocol>,
    pub rate: Option<f64>,
    pub seed: Option<u64>,
}

impl Scenario {
    pub fn from_toml(text: &str) -> Result<Self> {
        toml::from_str(text).context("invalid scenario")
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read scenario file {}", path.display()))?;
        Self::from_toml(&text).with_context(|| format!("in {}", path.display()))
    }

    pub fn apply(&mut self, o: &Overrides) {
        if let Some(c) = &o.collector {
            self.target.collector = Some(c.clone());
        }
        if let Some(p) = o.protocol {
            self.target.protocol = p;
        }
        if let Some(r) = o.rate {
            self.rate.flows_per_sec = r;
        }
        if let Some(s) = o.seed {
            self.seed = Some(s);
        }
    }

    /// Returns warnings on success; the first hard problem as an error.
    pub fn validate(&self) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        if self.rules.is_empty() && self.scans.is_empty() {
            bail!("scenario has no [[rule]] and no [[scan]] entries");
        }
        if self.stats_interval_secs == 0 {
            bail!("stats_interval_secs must be > 0");
        }

        let t = &self.target;
        if !(MIN_DATAGRAM..=MAX_DATAGRAM).contains(&t.max_datagram) {
            bail!(
                "target.max_datagram must be between {MIN_DATAGRAM} and {MAX_DATAGRAM}, got {}",
                t.max_datagram
            );
        }
        if t.template_refresh_secs == 0 {
            bail!("target.template_refresh_secs must be > 0");
        }

        let r = &self.rate;
        if !r.flows_per_sec.is_finite() || r.flows_per_sec < 0.0 {
            bail!("rate.flows_per_sec must be >= 0, got {}", r.flows_per_sec);
        }
        if r.flows_per_sec > HIGH_RATE_WARNING {
            warnings.push(format!(
                "rate.flows_per_sec = {} is above {HIGH_RATE_WARNING}; Flowlink/PCE handle about 10K unique flows/s in total",
                r.flows_per_sec
            ));
        }
        if r.shape == Shape::Sine && r.period_secs <= 0.0 {
            bail!("rate.period_secs must be > 0 for shape = \"sine\"");
        }
        if !(0.0..=1.0).contains(&r.amplitude) {
            bail!(
                "rate.amplitude must be between 0 and 1, got {}",
                r.amplitude
            );
        }
        if !(0.0..=1.0).contains(&r.burst_chance) {
            bail!(
                "rate.burst_chance must be between 0 and 1, got {}",
                r.burst_chance
            );
        }
        if r.burst_multiplier < 1.0 || r.burst_secs <= 0.0 {
            bail!("rate.burst_multiplier must be >= 1 and rate.burst_secs > 0");
        }

        for rule in &self.rules {
            let ctx = format!("rule '{}'", rule.name);
            if rule.src.is_empty() || rule.dst.is_empty() {
                bail!("{ctx}: src and dst must not be empty");
            }
            if rule.weight == 0 {
                bail!("{ctx}: weight must be > 0");
            }
            check_multicast(&ctx, rule.src.iter().chain(&rule.dst))?;
            match rule.proto {
                Proto::Icmp if !rule.dst_ports.is_empty() => {
                    warnings.push(format!("{ctx}: dst_ports are ignored for icmp"));
                }
                Proto::Tcp | Proto::Udp if rule.dst_ports.is_empty() => {
                    bail!("{ctx}: dst_ports must not be empty for tcp/udp");
                }
                _ => {}
            }
            for (field, range) in [
                ("packets", rule.packets),
                ("bytes_per_packet", rule.bytes_per_packet),
            ] {
                if let Some([lo, hi]) = range
                    && (lo == 0 || lo > hi)
                {
                    bail!("{ctx}: {field} must be [min, max] with 1 <= min <= max");
                }
            }
        }

        for (i, scan) in self.scans.iter().enumerate() {
            let ctx = format!("scan #{}", i + 1);
            if scan.dst.is_empty() {
                bail!("{ctx}: dst must not be empty");
            }
            if scan.src.is_multicast() {
                bail!(
                    "{ctx}: src {} is multicast (224.0.0.0/4), which Flowlink ignores",
                    scan.src
                );
            }
            check_multicast(&ctx, scan.dst.iter())?;
            if scan.ports.expand().with_context(|| ctx.clone())?.is_empty() {
                bail!("{ctx}: ports must not be empty");
            }
            if scan.every_secs == 0 || scan.flows_per_burst == 0 {
                bail!("{ctx}: every_secs and flows_per_burst must be > 0");
            }
        }
        Ok(warnings)
    }
}

fn check_multicast<'a>(ctx: &str, nets: impl Iterator<Item = &'a Ipv4Net>) -> Result<()> {
    for net in nets {
        if overlaps_multicast(net) {
            bail!("{ctx}: {net} overlaps multicast range 224.0.0.0/4, which Flowlink ignores");
        }
    }
    Ok(())
}

pub fn overlaps_multicast(net: &Ipv4Net) -> bool {
    let lo = u32::from(net.network());
    let hi = u32::from(net.broadcast());
    lo <= u32::from(Ipv4Addr::new(239, 255, 255, 255))
        && hi >= u32::from(Ipv4Addr::new(224, 0, 0, 0))
}

/// Parse "a.b.c.d/len" (host bits cleared) or a bare "a.b.c.d" (as /32).
pub fn parse_net(s: &str) -> Result<Ipv4Net> {
    let s = s.trim();
    if s.contains('/') {
        let net: Ipv4Net = s
            .parse()
            .with_context(|| format!("invalid IPv4 CIDR '{s}'"))?;
        Ok(net.trunc())
    } else {
        let addr: Ipv4Addr = s
            .parse()
            .with_context(|| format!("invalid IPv4 address '{s}'"))?;
        Ok(Ipv4Net::from(addr))
    }
}

fn de_nets<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Ipv4Net>, D::Error> {
    let raw = Vec::<String>::deserialize(d)?;
    raw.iter()
        .map(|s| parse_net(s).map_err(|e| serde::de::Error::custom(format!("{e:#}"))))
        .collect()
}

/// "host:port" -> (host, Some(port)); anything else -> (spec, None).
pub fn split_host_port(spec: &str) -> (&str, Option<u16>) {
    match spec.rsplit_once(':') {
        Some((host, port)) => match port.parse() {
            Ok(port) => (host, Some(port)),
            Err(_) => (spec, None),
        },
        None => (spec, None),
    }
}

/// Resolve the collector once; missing port defaults by protocol (2055 / 4739).
pub fn resolve_collector(spec: &str, protocol: Protocol) -> Result<SocketAddr> {
    let (host, port) = split_host_port(spec.trim());
    let port = port.unwrap_or_else(|| protocol.default_port());
    (host, port)
        .to_socket_addrs()
        .with_context(|| format!("cannot resolve collector '{host}'"))?
        .find(SocketAddr::is_ipv4)
        .with_context(|| format!("collector '{host}' has no IPv4 address"))
}
