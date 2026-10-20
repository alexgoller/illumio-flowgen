use std::net::SocketAddr;
use std::time::{Duration, Instant};

use flowgen::export::Protocol;
use flowgen::stats::Stats;
use flowgen::ui::*;

#[test]
fn thousands_separators() {
    assert_eq!(thousands(0), "0");
    assert_eq!(thousands(999), "999");
    assert_eq!(thousands(1000), "1,000");
    assert_eq!(thousands(1_234_567), "1,234,567");
}

#[test]
fn elapsed_format() {
    assert_eq!(fmt_elapsed(Duration::from_secs(3725)), "01:02:05");
}

#[test]
fn measured_rate_uses_five_second_window() {
    let mut s = Stats::new(0);
    let t0 = Instant::now();
    s.sample(t0);
    s.flows = 600;
    s.sample(t0 + Duration::from_secs(6));
    s.flows = 700;
    s.sample(t0 + Duration::from_secs(7));
    assert!(
        (s.measured_rate() - 100.0).abs() < 1e-6,
        "{}",
        s.measured_rate()
    );
    assert_eq!(Stats::new(0).measured_rate(), 0.0);
}

#[test]
fn status_line_shows_counters() {
    let mut s = Stats::new(1);
    s.flows = 12_345;
    s.datagrams = 1_020;
    s.target_rate = 200.0;
    s.scan_bursts = 1;
    let line = status_line(&s);
    assert!(line.contains("(target 200)"), "{line}");
    assert!(line.contains("flows 12,345"), "{line}");
    assert!(line.contains("pkts 1,020"), "{line}");
    assert!(line.contains("errs 0"), "{line}");
    assert!(line.contains("scans 1"), "{line}");
}

#[test]
fn summary_has_rule_table() {
    let mut s = Stats::new(2);
    s.flows = 1000;
    s.per_rule = vec![600, 200];
    s.scan_flows = 200;
    s.scan_bursts = 1;
    let out = summary(
        &s,
        &["web".to_string(), "db".to_string()],
        Duration::from_secs(10),
    );
    assert!(out.contains("avg 100.0 flows/s"), "{out}");
    assert!(out.contains("web"), "{out}");
    assert!(out.contains("60.0%"), "{out}");
    assert!(out.contains("(scans)"), "{out}");
    assert!(out.contains("20.0%"), "{out}");
}

#[test]
fn mode_selection() {
    assert_eq!(pick_mode(false, true), Mode::Quiet);
    assert_eq!(pick_mode(true, true), Mode::Quiet);
    assert_eq!(pick_mode(true, false), Mode::Plain);
}

#[test]
fn header_mentions_source_ip_and_aggregation() {
    let h = Header {
        protocol: Protocol::Ipfix,
        destination: "10.0.0.5:4739".into(),
        local_addr: Some("192.168.1.20:50000".parse::<SocketAddr>().unwrap()),
        scenario: "preset three-tier".into(),
        seed: 7,
    };
    let text = h.lines().join("\n");
    assert!(text.contains("IPFIX -> 10.0.0.5:4739"), "{text}");
    assert!(text.contains("192.168.1.20"), "{text}");
    assert!(text.contains("remote_addrs"), "{text}");
    assert!(text.contains("aggregation"), "{text}");
    assert!(text.contains("seed: 7"), "{text}");
}

#[test]
fn quiet_ui_lifecycle_does_not_panic() {
    let h = Header {
        protocol: Protocol::V5,
        destination: "dry-run".into(),
        local_addr: None,
        scenario: "x".into(),
        seed: 1,
    };
    let mut ui = Ui::start(
        Mode::Quiet,
        &h,
        Some(Duration::from_secs(1)),
        Duration::from_secs(10),
    );
    let s = Stats::new(0);
    ui.update(&s);
    ui.notice("hidden");
    ui.finish(&s, &[]);
}
