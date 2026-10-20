//! Flow export encoders. Each encoder turns `FlowRecord`s into UDP payloads.

pub mod v5;
pub mod v9;

use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::model::FlowRecord;

/// Pretend the exporter booted this long before flowgen started, so
/// sysUptime-relative flow start times never underflow.
pub const BOOT_OFFSET: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    V5,
    V9,
    #[default]
    Ipfix,
}

impl Protocol {
    /// Port used when the collector is given without one (Flowlink docs: 2055 NetFlow, 4739 IPFIX).
    pub fn default_port(self) -> u16 {
        match self {
            Protocol::V5 | Protocol::V9 => 2055,
            Protocol::Ipfix => 4739,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Protocol::V5 => "NetFlow v5",
            Protocol::V9 => "NetFlow v9",
            Protocol::Ipfix => "IPFIX",
        }
    }
}

pub trait Encoder {
    /// Encode records into one or more datagrams, each <= the configured max size.
    /// `now` is the export time. Encoders own sequence numbers and template state.
    fn encode(&mut self, records: &[FlowRecord], now: SystemTime) -> Vec<Vec<u8>>;
}

pub(crate) fn put_u8(buf: &mut Vec<u8>, v: u8) {
    buf.push(v);
}

pub(crate) fn put_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_be_bytes());
}

pub(crate) fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_be_bytes());
}

/// Saturating u64 -> u32 for 32-bit counters.
pub(crate) fn sat_u32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// Milliseconds between `boot` and `t` (0 if `t` is before boot), wrapping like a router uptime.
pub(crate) fn uptime_ms(boot: SystemTime, t: SystemTime) -> u32 {
    t.duration_since(boot).unwrap_or_default().as_millis() as u32
}

pub const TEMPLATE_ID: u16 = 256;
pub const TEMPLATE_EVERY_PACKETS: u32 = 100;

#[derive(Debug, Clone, Copy)]
pub struct EncoderOptions {
    /// v9 source id / IPFIX observation domain id.
    pub observation_domain: u32,
    pub template_refresh: Duration,
    pub max_datagram: usize,
}

/// Decides when a v9/IPFIX packet must carry the template:
/// first packet, then every `interval` or every 100 packets, whichever comes first.
pub(crate) struct TemplateTimer {
    interval: Duration,
    last: Option<SystemTime>,
    packets_since: u32,
}

impl TemplateTimer {
    pub(crate) fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
            packets_since: 0,
        }
    }

    /// Call once per outgoing packet; returns whether it must include the template.
    pub(crate) fn due(&mut self, now: SystemTime) -> bool {
        let due = match self.last {
            None => true,
            Some(last) => {
                self.packets_since >= TEMPLATE_EVERY_PACKETS
                    || match now.duration_since(last) {
                        Ok(age) => age >= self.interval,
                        Err(_) => true, // clock jumped backwards: resend
                    }
            }
        };
        if due {
            self.last = Some(now);
            self.packets_since = 0;
        }
        self.packets_since += 1;
        due
    }
}
