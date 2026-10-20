//! Flow export encoders. Each encoder turns `FlowRecord`s into UDP payloads.

pub mod v5;

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

// Unused until the v9/IPFIX encoders land.
#[allow(dead_code)]
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
