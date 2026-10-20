mod common;

use std::time::Duration;

use common::{sample_records, t0};
use flowgen::export::Encoder;
use flowgen::export::v5::V5Encoder;
use netflow_parser::{NetflowPacket, NetflowParser};

fn encoder(max: usize) -> V5Encoder {
    V5Encoder::new(t0() - Duration::from_secs(3600), max)
}

#[test]
fn v5_roundtrip_fields() {
    let recs = sample_records(3);
    let dgs = encoder(1400).encode(&recs, t0());
    assert_eq!(dgs.len(), 1);
    assert_eq!(dgs[0].len(), 24 + 3 * 48);

    let res = NetflowParser::default().parse_bytes(&dgs[0]);
    assert!(res.error.is_none(), "{:?}", res.error);
    let NetflowPacket::V5(v5) = &res.packets[0] else {
        panic!("expected v5 packet");
    };
    assert_eq!(v5.header.version, 5);
    assert_eq!(v5.header.count, 3);
    assert_eq!(v5.header.unix_secs, 1_760_000_000);
    assert_eq!(v5.header.sys_up_time, 3_600_000);
    for (f, r) in v5.flowsets.iter().zip(&recs) {
        assert_eq!(f.src_addr, r.src_addr);
        assert_eq!(f.dst_addr, r.dst_addr);
        assert_eq!(f.src_port, r.src_port);
        assert_eq!(f.dst_port, r.dst_port);
        assert_eq!(f.protocol_number, r.protocol);
        assert_eq!(f.tcp_flags, r.tcp_flags);
        assert_eq!(u64::from(f.d_pkts), r.packets);
        assert_eq!(u64::from(f.d_octets), r.bytes);
        assert_eq!(f.last, 3_600_000);
        assert_eq!(f.last - f.first, 1500);
    }
}

#[test]
fn v5_splits_at_30_records_and_sequence_counts_flows() {
    // 1500-byte budget: 24 + 30*48 = 1464 fits; 1400 would only fit 28 per datagram.
    let dgs = encoder(1500).encode(&sample_records(65), t0());
    let counts: Vec<u16> = dgs
        .iter()
        .map(|d| u16::from_be_bytes([d[2], d[3]]))
        .collect();
    assert_eq!(counts, vec![30, 30, 5]);
    let seqs: Vec<u32> = dgs
        .iter()
        .map(|d| u32::from_be_bytes([d[16], d[17], d[18], d[19]]))
        .collect();
    assert_eq!(seqs, vec![0, 30, 60]);
    assert!(dgs.iter().all(|d| d.len() <= 1500));
}

#[test]
fn v5_respects_small_max_datagram() {
    let dgs = encoder(256).encode(&sample_records(9), t0());
    assert_eq!(dgs.len(), 3); // (256-24)/48 = 4 per packet
    assert!(dgs.iter().all(|d| d.len() <= 256));
}

#[test]
fn v5_empty_input_produces_nothing() {
    assert!(encoder(1400).encode(&[], t0()).is_empty());
}
