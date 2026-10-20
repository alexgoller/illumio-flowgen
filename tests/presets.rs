use flowgen::config::Scenario;
use flowgen::pattern::{IPS_LIMIT, scenario_hosts};
use flowgen::presets::{PRESETS, find};

#[test]
fn presets_are_listed() {
    let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
    assert_eq!(names, ["three-tier", "datacenter", "scan-demo"]);
    assert!(find("datacenter").is_some());
    assert!(find("nope").is_none());
}

#[test]
fn presets_parse_validate_and_stay_small() {
    for p in PRESETS {
        let s = Scenario::from_toml(p.toml).unwrap_or_else(|e| panic!("{}: {e:#}", p.name));
        let warnings = s.validate().unwrap_or_else(|e| panic!("{}: {e:#}", p.name));
        assert!(warnings.is_empty(), "{}: {warnings:?}", p.name);
        assert!(
            s.target.collector.is_none(),
            "{} must not set a collector",
            p.name
        );
        let hosts = scenario_hosts(&s, Some(IPS_LIMIT)).unwrap();
        assert!(hosts.len() <= 60, "{} has {} hosts", p.name, hosts.len());
        assert!(!p.description.is_empty());
    }
}

#[test]
fn three_tier_host_set() {
    let s = Scenario::from_toml(find("three-tier").unwrap().toml).unwrap();
    let hosts = scenario_hosts(&s, None).unwrap();
    assert_eq!(hosts.len(), 46);
    assert_eq!(hosts[0].to_string(), "10.1.0.10");
}

#[test]
fn scan_demo_has_scan() {
    let s = Scenario::from_toml(find("scan-demo").unwrap().toml).unwrap();
    assert_eq!(s.scans.len(), 1);
    assert_eq!(s.scans[0].every_secs, 60);
}

#[test]
fn example_scenario_is_valid() {
    let s = Scenario::from_file(std::path::Path::new("examples/scenario.toml")).unwrap();
    s.validate().unwrap();
    assert!(s.target.collector.is_some());
}

#[test]
fn presets_label_every_host() {
    for p in PRESETS {
        let s = Scenario::from_toml(p.toml).unwrap();
        let hosts = scenario_hosts(&s, None).unwrap();
        for ip in hosts {
            let info = flowgen::inventory::resolve(&s.hosts, ip);
            for key in ["role", "app", "env"] {
                assert!(
                    info.labels.get(key).is_some_and(|v| !v.is_empty()),
                    "{}: {ip} has no {key} label",
                    p.name
                );
            }
        }
    }
}

#[test]
fn same_ip_has_same_hostname_across_presets_and_samples() {
    use std::collections::BTreeMap;
    let samples = [
        "web-shop",
        "active-directory",
        "lateral-movement",
        "ot-iot",
        "kubernetes",
        "burst-demo",
    ];
    let mut scenarios: Vec<(String, Scenario)> = PRESETS
        .iter()
        .map(|p| (p.name.to_string(), Scenario::from_toml(p.toml).unwrap()))
        .collect();
    for stem in samples {
        let path = std::path::Path::new("examples").join(format!("{stem}.toml"));
        scenarios.push((stem.to_string(), Scenario::from_file(&path).unwrap()));
    }
    let mut seen: BTreeMap<std::net::Ipv4Addr, (String, String)> = BTreeMap::new();
    for (name, s) in &scenarios {
        for ip in scenario_hosts(s, Some(IPS_LIMIT)).unwrap() {
            let host = flowgen::inventory::resolve(&s.hosts, ip).hostname;
            if let Some((other, h)) = seen.get(&ip) {
                assert_eq!(h, &host, "{ip}: {other} says {h}, {name} says {host}");
            } else {
                seen.insert(ip, (name.clone(), host));
            }
        }
    }
}
