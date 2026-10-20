#![allow(dead_code)]

use std::net::Ipv4Addr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flowgen::model::{FlowRecord, PROTO_TCP};

/// Fixed "now" used by encoder tests.
pub fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_760_000_000)
}

/// Deterministic records: i-th record is 10.0.0.(i%250+1) -> 10.0.1.(i%250+1),
/// tcp 40000+i -> 443, 10+i packets, 1000*(i+1) bytes, 1.5 s long, ending at t0().
pub fn sample_records(n: usize) -> Vec<FlowRecord> {
    (0..n)
        .map(|i| FlowRecord {
            src_addr: Ipv4Addr::new(10, 0, 0, (i % 250 + 1) as u8),
            dst_addr: Ipv4Addr::new(10, 0, 1, (i % 250 + 1) as u8),
            src_port: 40000 + i as u16,
            dst_port: 443,
            protocol: PROTO_TCP,
            packets: 10 + i as u64,
            bytes: 1000 * (i as u64 + 1),
            tcp_flags: 0x1b,
            start: t0() - Duration::from_millis(1500),
            end: t0(),
        })
        .collect()
}
