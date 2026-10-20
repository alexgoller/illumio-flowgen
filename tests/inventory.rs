use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use flowgen::config::HostGroup;
use flowgen::inventory::resolve;
use ipnet::Ipv4Net;

fn group(nets: &[&str], labels: &[(&str, &str)], prefix: Option<&str>) -> HostGroup {
    HostGroup {
        nets: nets.iter().map(|n| n.parse::<Ipv4Net>().unwrap()).collect(),
        labels: labels
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<BTreeMap<_, _>>(),
        hostname_prefix: prefix.map(str::to_string),
    }
}

const IP: Ipv4Addr = Ipv4Addr::new(10, 1, 1, 9);

#[test]
fn no_groups_gives_defaults() {
    let h = resolve(&[], IP);
    assert_eq!(h.ip, IP);
    assert_eq!(h.hostname, "fg-10-1-1-9");
    assert!(h.labels.is_empty());
}

#[test]
fn merges_labels_and_prefix_from_nested_groups() {
    let groups = [
        group(&["10.1.0.0/16"], &[("app", "shop"), ("env", "prod")], None),
        group(&["10.1.1.8/29"], &[("role", "web")], Some("web")),
    ];
    let h = resolve(&groups, IP);
    assert_eq!(h.hostname, "web-10-1-1-9");
    assert_eq!(h.labels["app"], "shop");
    assert_eq!(h.labels["env"], "prod");
    assert_eq!(h.labels["role"], "web");
}

#[test]
fn more_specific_wins_in_either_order() {
    let broad = group(&["10.1.0.0/16"], &[("role", "any")], None);
    let narrow = group(&["10.1.1.8/29"], &[("role", "web")], None);
    assert_eq!(
        resolve(&[broad.clone(), narrow.clone()], IP).labels["role"],
        "web"
    );
    assert_eq!(resolve(&[narrow, broad], IP).labels["role"], "web");
}

#[test]
fn equal_specificity_later_group_wins() {
    let groups = [
        group(&["10.1.1.8/29"], &[("role", "a")], None),
        group(&["10.1.1.8/29"], &[("role", "b")], None),
    ];
    assert_eq!(resolve(&groups, IP).labels["role"], "b");
}

#[test]
fn specificity_is_largest_matching_prefix_of_group() {
    // First group has a /8 and a /29 net: specificity 29, beating the later /16 group.
    let groups = [
        group(&["10.0.0.0/8", "10.1.1.8/29"], &[("role", "web")], None),
        group(&["10.1.0.0/16"], &[("role", "any")], None),
    ];
    assert_eq!(resolve(&groups, IP).labels["role"], "web");
}

#[test]
fn ip_outside_all_groups_gets_defaults() {
    let groups = [group(&["192.168.0.0/16"], &[("role", "x")], Some("x"))];
    let h = resolve(&groups, IP);
    assert_eq!(h.hostname, "fg-10-1-1-9");
    assert!(h.labels.is_empty());
}
