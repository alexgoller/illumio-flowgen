use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant, SystemTime};

use flowgen::config::{Rate, Scenario, Shape, parse_net};
use flowgen::model::{PROTO_ICMP, PROTO_TCP, PROTO_UDP};
use flowgen::pattern::*;
use rand::SeedableRng;
use rand::rngs::StdRng;

fn rng() -> StdRng {
    StdRng::seed_from_u64(42)
}

fn nets(list: &[&str]) -> Vec<ipnet::Ipv4Net> {
    list.iter().map(|s| parse_net(s).unwrap()).collect()
}

const SCENARIO: &str = r#"
[[rule]]
name = "heavy"
src = ["10.0.0.0/30"]
dst = ["10.0.1.0/30"]
proto = "tcp"
dst_ports = [443]
weight = 3

[[rule]]
name = "light"
src = ["10.0.2.1"]
dst = ["10.0.3.0/31"]
proto = "udp"
dst_ports = [53, 123]
weight = 1

[[rule]]
name = "ping"
src = ["10.0.4.1"]
dst = ["10.0.4.2"]
proto = "icmp"
weight = 1
packets = [2, 2]
bytes_per_packet = [84, 84]
"#;

#[test]
fn host_pool_counts_and_excludes_network_broadcast() {
    assert_eq!(HostPool::new(&nets(&["10.0.0.0/24"])).len(), 254);
    assert_eq!(HostPool::new(&nets(&["10.0.0.0/31"])).len(), 2);
    assert_eq!(HostPool::new(&nets(&["10.0.0.7/32"])).len(), 1);
    let pool = HostPool::new(&nets(&["10.0.0.0/30", "10.0.9.9"]));
    let all: Vec<Ipv4Addr> = pool.iter().collect();
    assert_eq!(
        all,
        vec![
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(10, 0, 0, 2),
            Ipv4Addr::new(10, 0, 9, 9)
        ]
    );
    let mut r = rng();
    for _ in 0..500 {
        assert!(all.contains(&pool.sample(&mut r)));
    }
}

#[test]
fn host_pool_samples_large_net_without_enumerating() {
    let pool = HostPool::new(&nets(&["10.0.0.0/8"]));
    assert_eq!(pool.len(), (1u64 << 24) - 2);
    let started = Instant::now();
    let mut r = rng();
    for _ in 0..10_000 {
        let ip = pool.sample(&mut r);
        assert_eq!(ip.octets()[0], 10);
        assert_ne!(ip, Ipv4Addr::new(10, 0, 0, 0));
        assert_ne!(ip, Ipv4Addr::new(10, 255, 255, 255));
    }
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn rate_shapes() {
    let mut r = rng();
    let constant = Rate {
        flows_per_sec: 100.0,
        ..Rate::default()
    };
    assert_eq!(RateShape::new(&constant).rate_at(123.0, &mut r), 100.0);

    let sine = Rate {
        flows_per_sec: 100.0,
        shape: Shape::Sine,
        period_secs: 40.0,
        amplitude: 0.5,
        ..Rate::default()
    };
    let mut s = RateShape::new(&sine);
    assert!((s.rate_at(0.0, &mut r) - 100.0).abs() < 1e-9);
    assert!((s.rate_at(10.0, &mut r) - 150.0).abs() < 1e-9);
    assert!((s.rate_at(30.0, &mut r) - 50.0).abs() < 1e-9);

    let always = Rate {
        flows_per_sec: 10.0,
        shape: Shape::Burst,
        burst_chance: 1.0,
        burst_multiplier: 5.0,
        burst_secs: 3.0,
        ..Rate::default()
    };
    assert_eq!(RateShape::new(&always).rate_at(1.5, &mut r), 50.0);
    let never = Rate {
        burst_chance: 0.0,
        ..always.clone()
    };
    assert_eq!(RateShape::new(&never).rate_at(100.0, &mut r), 10.0);
}

#[test]
fn pacer_carries_fractions() {
    // dt values are exact binary fractions so the sums are exact.
    let mut p = Pacer::default();
    let total: usize = (0..32).map(|_| p.due(0.5, 0.125)).sum(); // 4 s at 0.5/s
    assert_eq!(total, 2);
    let mut p = Pacer::default();
    assert_eq!(p.due(0.0, 0.125), 0);
    let total: usize = (0..8).map(|_| p.due(1000.0, 0.125)).sum();
    assert_eq!(total, 1000);
}

#[test]
fn generator_respects_rules_and_weights() {
    let s = Scenario::from_toml(SCENARIO).unwrap();
    let mut g = Generator::new(&s, rng()).unwrap();
    assert_eq!(g.rule_names(), ["heavy", "light", "ping"]);
    let now = SystemTime::now();
    let mut counts: HashMap<usize, usize> = HashMap::new();
    for _ in 0..5000 {
        let (i, f) = g.next_flow(now);
        *counts.entry(i).or_default() += 1;
        assert_eq!(f.end, now);
        let dur = now.duration_since(f.start).unwrap();
        assert!(dur >= Duration::from_millis(100) && dur <= Duration::from_secs(30));
        match i {
            0 => {
                assert_eq!(f.protocol, PROTO_TCP);
                assert_eq!(f.dst_port, 443);
                assert_eq!(f.tcp_flags, 0x1b);
                assert!((32768..=60999).contains(&f.src_port));
                assert!((5..=200).contains(&f.packets));
                assert!(f.bytes >= f.packets * 60 && f.bytes <= f.packets * 1400);
                assert!([1, 2].contains(&f.src_addr.octets()[3]));
            }
            1 => {
                assert_eq!(f.protocol, PROTO_UDP);
                assert!([53, 123].contains(&f.dst_port));
                assert_eq!(f.tcp_flags, 0);
                assert_eq!(f.src_addr, Ipv4Addr::new(10, 0, 2, 1));
            }
            2 => {
                assert_eq!(f.protocol, PROTO_ICMP);
                assert_eq!((f.src_port, f.dst_port), (0, 0));
                assert_eq!((f.packets, f.bytes), (2, 168));
            }
            _ => unreachable!(),
        }
    }
    let heavy = counts[&0] as f64 / 5000.0;
    assert!((heavy - 0.6).abs() < 0.05, "heavy share {heavy}");
}

#[test]
fn seeded_generators_are_reproducible() {
    let s = Scenario::from_toml(SCENARIO).unwrap();
    let now = SystemTime::now();
    let (r1, seed) = make_rng(Some(7));
    assert_eq!(seed, 7);
    let (r2, _) = make_rng(Some(7));
    let mut a = Generator::new(&s, r1).unwrap();
    let mut b = Generator::new(&s, r2).unwrap();
    for _ in 0..100 {
        assert_eq!(a.next_flow(now), b.next_flow(now));
    }
    let (_, random_seed) = make_rng(None);
    let (_, other_seed) = make_rng(None);
    assert_ne!(random_seed, other_seed);
}

#[test]
fn scan_only_scenario_emits_bursts() {
    let s = Scenario::from_toml(
        r#"
[rate]
flows_per_sec = 0
[[scan]]
src = "10.9.9.9"
dst = ["10.1.2.0/29"]
ports = "20-25"
every_secs = 60
flows_per_burst = 50
"#,
    )
    .unwrap();
    s.validate().unwrap();
    let mut g = Generator::new(&s, rng()).unwrap();
    assert!(!g.has_rules());
    let now = SystemTime::now();
    assert!(g.due_scans(Duration::from_secs(59), now).is_empty());
    let bursts = g.due_scans(Duration::from_secs(60), now);
    assert_eq!(bursts.len(), 1);
    assert_eq!(bursts[0].src, Ipv4Addr::new(10, 9, 9, 9));
    assert_eq!(bursts[0].flows.len(), 50);
    for f in &bursts[0].flows {
        assert_eq!((f.protocol, f.tcp_flags, f.packets), (PROTO_TCP, 0x02, 1));
        assert!((20..=25).contains(&f.dst_port));
        assert!((44..=60).contains(&f.bytes));
    }
    assert!(g.due_scans(Duration::from_secs(61), now).is_empty());
    assert_eq!(g.due_scans(Duration::from_secs(185), now).len(), 2); // 120 s and 180 s
}

#[test]
fn scenario_hosts_sorted_dedup_and_includes_scan_src() {
    let mut s = Scenario::from_toml(SCENARIO).unwrap();
    s.scans = Scenario::from_toml(
        "[[scan]]\nsrc = \"10.0.0.1\"\ndst = [\"10.0.0.0/30\"]\nports = \"22\"",
    )
    .unwrap()
    .scans;
    let hosts = scenario_hosts(&s, Some(IPS_LIMIT)).unwrap();
    let expected: Vec<Ipv4Addr> = [
        "10.0.0.1", "10.0.0.2", "10.0.1.1", "10.0.1.2", "10.0.2.1", "10.0.3.0", "10.0.3.1",
        "10.0.4.1", "10.0.4.2",
    ]
    .iter()
    .map(|s| s.parse().unwrap())
    .collect();
    assert_eq!(hosts, expected);
}

#[test]
fn scenario_hosts_respects_limit() {
    let mut s = Scenario::from_toml(SCENARIO).unwrap();
    s.rules[0].dst = nets(&["10.200.0.0/16"]);
    let err = scenario_hosts(&s, Some(IPS_LIMIT)).unwrap_err().to_string();
    assert!(err.contains("--all"), "{err}");
    assert_eq!(
        scenario_hosts(&s, None).unwrap().len(),
        65534 + 2 + 1 + 2 + 2
    );
}
