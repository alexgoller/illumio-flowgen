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
