mod common;

use std::time::Duration;

use common::{sample_records, t0};
use flowgen::export::v9::V9Encoder;
use flowgen::export::{Encoder, EncoderOptions};
use netflow_parser::NetflowParser;

fn opts(max: usize) -> EncoderOptions {
    EncoderOptions {
        observation_domain: 7,
        template_refresh: Duration::from_secs(30),
        max_datagram: max,
    }
}

fn encoder(max: usize) -> V9Encoder {
    V9Encoder::new(t0() - Duration::from_secs(3600), &opts(max))
}

fn first_set_id(d: &[u8]) -> u16 {
    u16::from_be_bytes([d[20], d[21]])
}

fn sequence(d: &[u8]) -> u32 {
    u32::from_be_bytes([d[12], d[13], d[14], d[15]])
}

#[test]
fn v9_roundtrip_fields() {
    let recs = sample_records(3);
    let dgs = encoder(1400).encode(&recs, t0());
    assert_eq!(dgs.len(), 1);
    let d = &dgs[0];
    assert_eq!(u16::from_be_bytes([d[0], d[1]]), 9);
    assert_eq!(u16::from_be_bytes([d[2], d[3]]), 4); // 1 template + 3 data records
    assert_eq!(u32::from_be_bytes([d[16], d[17], d[18], d[19]]), 7); // source id

    let (flows, err) = NetflowParser::default().parse_bytes_as_netflow_common_flowsets(d);
    assert!(err.is_none(), "{err:?}");
    assert_eq!(flows.len(), 3);
    for (f, r) in flows.iter().zip(&recs) {
        assert_eq!(f.src_addr, Some(r.src_addr.into()));
        assert_eq!(f.dst_addr, Some(r.dst_addr.into()));
        assert_eq!(f.src_port, Some(r.src_port));
        assert_eq!(f.dst_port, Some(r.dst_port));
        assert_eq!(f.protocol_number, Some(r.protocol));
        assert_eq!(f.last_seen, Some(3_600_000));
        assert_eq!(f.first_seen, Some(3_600_000 - 1500));
    }
}

#[test]
fn v9_template_sent_first_then_on_timer() {
    let mut enc = encoder(1400);
    let one = sample_records(1);
    assert_eq!(first_set_id(&enc.encode(&one, t0())[0]), 0);
    assert_eq!(
        first_set_id(&enc.encode(&one, t0() + Duration::from_secs(1))[0]),
        256
    );
    assert_eq!(
        first_set_id(&enc.encode(&one, t0() + Duration::from_secs(31))[0]),
        0
    );
}

#[test]
fn v9_template_resent_every_100_packets() {
    let mut enc = encoder(1400);
    let one = sample_records(1);
    let ids: Vec<u16> = (0..201)
        .map(|_| first_set_id(&enc.encode(&one, t0())[0]))
        .collect();
    let template_packets: Vec<usize> = ids
        .iter()
        .enumerate()
        .filter(|(_, id)| **id == 0)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(template_packets, vec![0, 100, 200]);
}

#[test]
fn v9_splits_and_stays_under_limit() {
    let recs = sample_records(200);
    let dgs = encoder(512).encode(&recs, t0());
    assert!(dgs.len() > 1);
    assert!(dgs.iter().all(|d| d.len() <= 512), "datagram over limit");
    assert!(
        dgs.iter().all(|d| d.len() % 4 == 0),
        "flowsets padded to 32-bit"
    );
    let seqs: Vec<u32> = dgs.iter().map(|d| sequence(d)).collect();
    assert_eq!(seqs, (0..dgs.len() as u32).collect::<Vec<_>>());

    // A fresh collector decodes the first datagram on its own (template included).
    let (first, err) = NetflowParser::default().parse_bytes_as_netflow_common_flowsets(&dgs[0]);
    assert!(err.is_none(), "{err:?}");
    assert!(!first.is_empty());

    // One parser across all datagrams sees every record.
    let mut parser = NetflowParser::default();
    let total: usize = dgs
        .iter()
        .map(|d| parser.parse_bytes_as_netflow_common_flowsets(d).0.len())
        .sum();
    assert_eq!(total, 200);
}
