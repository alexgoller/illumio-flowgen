use std::path::{Path, PathBuf};

use flowgen::config::Scenario;
use flowgen::pattern::{IPS_LIMIT, scenario_hosts};

const SAMPLES: [&str; 7] = [
    "web-shop",
    "active-directory",
    "lateral-movement",
    "ot-iot",
    "kubernetes",
    "burst-demo",
    "load-test",
];

fn path(stem: &str) -> PathBuf {
    Path::new("examples").join(format!("{stem}.toml"))
}

#[test]
fn all_samples_exist_parse_and_validate_without_warnings() {
    for stem in SAMPLES {
        let s = Scenario::from_file(&path(stem)).unwrap_or_else(|e| panic!("{stem}: {e:#}"));
        let warnings = s.validate().unwrap_or_else(|e| panic!("{stem}: {e:#}"));
        assert!(warnings.is_empty(), "{stem}: {warnings:?}");
        assert_eq!(
            s.name.as_deref(),
            Some(stem),
            "{stem}: name must equal file stem"
        );
        assert!(
            s.target.collector.is_none(),
            "{stem}: samples must not set a collector"
        );
    }
}

#[test]
fn demo_samples_stay_small_enough_for_workloads() {
    for stem in SAMPLES.iter().filter(|s| **s != "load-test") {
        let s = Scenario::from_file(&path(stem)).unwrap();
        let hosts = scenario_hosts(&s, Some(IPS_LIMIT)).unwrap();
        assert!(hosts.len() <= 60, "{stem} has {} hosts", hosts.len());
    }
}

#[test]
fn every_example_file_is_covered() {
    let mut on_disk: Vec<String> = std::fs::read_dir("examples")
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .filter(|s| s != "scenario")
        .collect();
    on_disk.sort();
    let mut expected: Vec<String> = SAMPLES.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(on_disk, expected);
}

#[test]
fn feature_samples_use_their_feature() {
    use flowgen::config::Shape;
    let burst = Scenario::from_file(&path("burst-demo")).unwrap();
    assert_eq!(burst.rate.shape, Shape::Burst);
    let lateral = Scenario::from_file(&path("lateral-movement")).unwrap();
    assert!(!lateral.scans.is_empty());
    let load = Scenario::from_file(&path("load-test")).unwrap();
    assert!(load.rate.flows_per_sec >= 1000.0 && load.rate.flows_per_sec <= 5000.0);
    assert!(
        scenario_hosts(&load, Some(IPS_LIMIT)).is_err(),
        "load-test is intentionally large"
    );
}

#[test]
fn demo_samples_have_pairwise_disjoint_hosts() {
    use std::collections::HashSet;
    let demos: Vec<&str> = SAMPLES
        .iter()
        .copied()
        .filter(|s| *s != "load-test")
        .collect();
    let sets: Vec<HashSet<_>> = demos
        .iter()
        .map(|stem| {
            let s = Scenario::from_file(&path(stem)).unwrap();
            scenario_hosts(&s, Some(IPS_LIMIT))
                .unwrap()
                .into_iter()
                .collect()
        })
        .collect();
    for i in 0..sets.len() {
        for j in i + 1..sets.len() {
            let overlap: Vec<_> = sets[i].intersection(&sets[j]).collect();
            assert!(
                overlap.is_empty(),
                "{} and {} share {overlap:?}",
                demos[i],
                demos[j]
            );
        }
    }
}

fn assert_all_labelled(name: &str, s: &Scenario) {
    let hosts = scenario_hosts(s, None).unwrap();
    assert!(!hosts.is_empty(), "{name}: no hosts");
    for ip in hosts {
        let info = flowgen::inventory::resolve(&s.hosts, ip);
        for key in ["role", "app", "env"] {
            assert!(
                info.labels.get(key).is_some_and(|v| !v.is_empty()),
                "{name}: {ip} has no {key} label ({:?})",
                info.labels
            );
        }
    }
}

#[test]
fn samples_and_reference_label_every_host() {
    for stem in SAMPLES.iter().copied().chain(["scenario"]) {
        let s = Scenario::from_file(&path(stem)).unwrap();
        assert_all_labelled(stem, &s);
    }
}
