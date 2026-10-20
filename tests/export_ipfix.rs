mod common;

use std::time::{Duration, SystemTime};

use common::{sample_records, t0};
use flowgen::export::ipfix::IpfixEncoder;
use flowgen::export::{Encoder, EncoderOptions, Protocol, new_encoder};
use flowgen::model::unix_millis;
use netflow_parser::{NetflowPacket, NetflowParser};

fn opts(max: usize) -> EncoderOptions {
    EncoderOptions {
        observation_domain: 9,
        template_refresh: Duration::from_secs(30),
        max_datagram: max,
    }
}

fn be16(d: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([d[at], d[at + 1]])
}

fn be32(d: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(d[at..at + 4].try_into().unwrap())
}

fn be64(d: &[u8], at: usize) -> u64 {
    u64::from_be_bytes(d[at..at + 8].try_into().unwrap())
}

#[test]
fn ipfix_roundtrip_fields() {
    let recs = sample_records(3);
    let dgs = IpfixEncoder::new(&opts(1400)).encode(&recs, t0());
    assert_eq!(dgs.len(), 1);
    let d = &dgs[0];
    assert_eq!(be16(d, 0), 10);
    assert_eq!(be16(d, 2) as usize, d.len()); // message length
    assert_eq!(be32(d, 4), 1_760_000_000); // export time
    assert_eq!(be32(d, 12), 9); // observation domain

    let (flows, err) = NetflowParser::default().parse_bytes_as_netflow_common_flowsets(d);
    assert!(err.is_none(), "{err:?}");
    assert_eq!(flows.len(), 3);
    for (f, r) in flows.iter().zip(&recs) {
        assert_eq!(f.src_addr, Some(r.src_addr.into()));
        assert_eq!(f.dst_addr, Some(r.dst_addr.into()));
        assert_eq!(f.src_port, Some(r.src_port));
        assert_eq!(f.dst_port, Some(r.dst_port));
        assert_eq!(f.protocol_number, Some(r.protocol));
    }
}

#[test]
fn ipfix_record_layout_bytes_packets_timestamps() {
    let recs = sample_records(1);
    let d = &IpfixEncoder::new(&opts(1400)).encode(&recs, t0())[0];
    // header 16 + template set 48 + data set header 4
    let rec = 16 + 48 + 4;
    assert_eq!(be16(d, 16), 2); // template set id
    assert_eq!(be16(d, 16 + 48), 256); // data set id
    assert_eq!(be64(d, rec), recs[0].bytes);
    assert_eq!(be64(d, rec + 8), recs[0].packets);
    assert_eq!(d[rec + 16], recs[0].protocol);
    assert_eq!(be16(d, rec + 17), u16::from(recs[0].tcp_flags));
    assert_eq!(be64(d, rec + 31), unix_millis(recs[0].start));
    assert_eq!(be64(d, rec + 39), unix_millis(recs[0].end));
    assert_eq!(d.len(), rec + 47);
}

#[test]
fn ipfix_sequence_counts_data_records() {
    let mut enc = IpfixEncoder::new(&opts(1400));
    let a = enc.encode(&sample_records(5), t0());
    let b = enc.encode(&sample_records(2), t0());
    assert_eq!(be32(&a[0], 8), 0);
    assert_eq!(be32(&b[0], 8), 5);
}

#[test]
fn ipfix_template_sent_first_then_on_timer() {
    let mut enc = IpfixEncoder::new(&opts(1400));
    let one = sample_records(1);
    assert_eq!(be16(&enc.encode(&one, t0())[0], 16), 2);
    assert_eq!(
        be16(&enc.encode(&one, t0() + Duration::from_secs(1))[0], 16),
        256
    );
    assert_eq!(
        be16(&enc.encode(&one, t0() + Duration::from_secs(31))[0], 16),
        2
    );
}

#[test]
fn ipfix_splits_and_stays_under_limit() {
    let dgs = IpfixEncoder::new(&opts(512)).encode(&sample_records(200), t0());
    assert!(dgs.len() > 1);
    assert!(dgs.iter().all(|d| d.len() <= 512));
    assert!(dgs.iter().all(|d| be16(d, 2) as usize == d.len()));

    let (first, err) = NetflowParser::default().parse_bytes_as_netflow_common_flowsets(&dgs[0]);
    assert!(err.is_none(), "{err:?}");
    assert!(!first.is_empty());

    let mut parser = NetflowParser::default();
    let total: usize = dgs
        .iter()
        .map(|d| parser.parse_bytes_as_netflow_common_flowsets(d).0.len())
        .sum();
    assert_eq!(total, 200);
}

#[test]
fn factory_builds_each_protocol() {
    let start = SystemTime::now();
    for (p, version) in [
        (Protocol::V5, 5u16),
        (Protocol::V9, 9),
        (Protocol::Ipfix, 10),
    ] {
        let mut enc = new_encoder(p, &opts(1400), start);
        let dgs = enc.encode(&sample_records(2), start);
        assert_eq!(be16(&dgs[0], 0), version);
        let res = NetflowParser::default().parse_bytes(&dgs[0]);
        assert!(res.error.is_none(), "{p:?}: {:?}", res.error);
        assert!(matches!(
            (&res.packets[0], p),
            (NetflowPacket::V5(_), Protocol::V5)
                | (NetflowPacket::V9(_), Protocol::V9)
                | (NetflowPacket::IPFix(_), Protocol::Ipfix)
        ));
    }
}
