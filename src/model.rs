//! Protocol-neutral flow record shared by generators and encoders.

use std::net::Ipv4Addr;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROTO_ICMP: u8 = 1;
pub const PROTO_TCP: u8 = 6;
pub const PROTO_UDP: u8 = 17;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowRecord {
    pub src_addr: Ipv4Addr,
    pub dst_addr: Ipv4Addr,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,
    pub packets: u64,
    pub bytes: u64,
    pub tcp_flags: u8,
    pub start: SystemTime,
    pub end: SystemTime,
}

impl FlowRecord {
    /// One-line human form used by `--dry-run`.
    pub fn dry_run_line(&self) -> String {
        format!(
            "{} {}:{} -> {}:{} pkts={} bytes={}",
            proto_name(self.protocol),
            self.src_addr,
            self.src_port,
            self.dst_addr,
            self.dst_port,
            self.packets,
            self.bytes
        )
    }
}

pub fn proto_name(protocol: u8) -> String {
    match protocol {
        PROTO_ICMP => "icmp".to_string(),
        PROTO_TCP => "tcp".to_string(),
        PROTO_UDP => "udp".to_string(),
        other => format!("proto-{other}"),
    }
}

/// Milliseconds since the Unix epoch; times before the epoch clamp to 0.
pub fn unix_millis(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn unix_millis_converts_and_clamps() {
        assert_eq!(unix_millis(UNIX_EPOCH + Duration::from_millis(1234)), 1234);
        assert_eq!(unix_millis(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }

    #[test]
    fn dry_run_line_format() {
        let end = UNIX_EPOCH + Duration::from_secs(1000);
        let r = FlowRecord {
            src_addr: Ipv4Addr::new(10, 0, 0, 1),
            dst_addr: Ipv4Addr::new(10, 0, 1, 1),
            src_port: 40000,
            dst_port: 443,
            protocol: PROTO_TCP,
            packets: 10,
            bytes: 1000,
            tcp_flags: 0x1b,
            start: end - Duration::from_millis(1500),
            end,
        };
        assert_eq!(
            r.dry_run_line(),
            "tcp 10.0.0.1:40000 -> 10.0.1.1:443 pkts=10 bytes=1000"
        );
        assert_eq!(proto_name(PROTO_UDP), "udp");
        assert_eq!(proto_name(PROTO_ICMP), "icmp");
        assert_eq!(proto_name(99), "proto-99");
    }
}
