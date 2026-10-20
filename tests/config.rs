use flowgen::config::*;
use flowgen::export::Protocol;

const EXAMPLE: &str = r#"
seed = 42

[target]
collector = "10.0.0.5:2055"
protocol = "v9"
observation_domain = 3

[rate]
flows_per_sec = 200
shape = "sine"
period_secs = 300

[[rule]]
name = "web-to-app"
src = ["10.1.1.0/24"]
dst = ["10.1.2.0/24"]
proto = "tcp"
dst_ports = [8080, 8443]
weight = 5

[[rule]]
name = "app-to-db"
src = ["10.1.2.0/24"]
dst = ["10.1.3.10"]
proto = "tcp"
dst_ports = [5432]

[[scan]]
src = "10.9.9.9"
dst = ["10.1.2.0/24"]
ports = "1-1024"
every_secs = 120
"#;

fn base() -> Scenario {
    Scenario::from_toml(EXAMPLE).unwrap()
}

#[test]
fn parses_full_example_with_defaults() {
    let s = base();
    assert_eq!(s.seed, Some(42));
    assert_eq!(s.stats_interval_secs, 10);
    assert_eq!(s.target.protocol, Protocol::V9);
    assert_eq!(s.target.max_datagram, 1400);
    assert_eq!(s.target.template_refresh_secs, 30);
    assert_eq!(s.rate.shape, Shape::Sine);
    assert_eq!(s.rate.amplitude, 0.5);
    assert_eq!(s.rules.len(), 2);
    assert_eq!(s.rules[1].weight, 1);
    assert_eq!(s.rules[1].dst[0].to_string(), "10.1.3.10/32");
    assert_eq!(s.scans[0].flows_per_burst, 200);
    assert_eq!(s.scans[0].ports.expand().unwrap().len(), 1024);
    assert!(s.validate().unwrap().is_empty());
}

#[test]
fn empty_document_uses_defaults_but_fails_validation() {
    let s = Scenario::from_toml("").unwrap();
    assert_eq!(s.target.protocol, Protocol::Ipfix);
    assert_eq!(s.rate.flows_per_sec, 100.0);
    let err = s.validate().unwrap_err().to_string();
    assert!(err.contains("no [[rule]]"), "{err}");
}

#[test]
fn unknown_keys_and_bad_values_are_rejected() {
    assert!(Scenario::from_toml("bogus = 1").is_err());
    assert!(Scenario::from_toml("[target]\nprotocol = \"v8\"").is_err());
    assert!(Scenario::from_toml("[rate]\nshape = \"zigzag\"").is_err());
    let bad_cidr = EXAMPLE.replace("10.1.1.0/24", "10.1.1.0/33");
    let err = format!("{:#}", Scenario::from_toml(&bad_cidr).unwrap_err());
    assert!(err.contains("10.1.1.0/33"), "{err}");
}

#[test]
fn parse_net_truncates_and_accepts_bare_ip() {
    assert_eq!(
        parse_net("10.1.1.77/24").unwrap().to_string(),
        "10.1.1.0/24"
    );
    assert_eq!(
        parse_net(" 192.168.1.1 ").unwrap().to_string(),
        "192.168.1.1/32"
    );
    assert!(parse_net("not-an-ip").is_err());
}

#[test]
fn validation_errors() {
    type Case = (Box<dyn Fn(&mut Scenario)>, &'static str);
    let cases: Vec<Case> = vec![
        (Box::new(|s| s.rules[0].weight = 0), "weight"),
        (Box::new(|s| s.rules[0].dst_ports.clear()), "dst_ports"),
        (Box::new(|s| s.rules[0].src.clear()), "src and dst"),
        (
            Box::new(|s| s.rules[0].dst = vec![parse_net("224.0.0.0/8").unwrap()]),
            "multicast",
        ),
        (
            Box::new(|s| s.rules[0].src = vec![parse_net("192.0.0.0/2").unwrap()]),
            "multicast",
        ),
        (
            Box::new(|s| s.scans[0].src = "239.1.1.1".parse().unwrap()),
            "multicast",
        ),
        (
            Box::new(|s| s.scans[0].ports = PortSpec::Range("100-1".into())),
            "port range",
        ),
        (Box::new(|s| s.scans[0].every_secs = 0), "every_secs"),
        (Box::new(|s| s.rate.flows_per_sec = -1.0), "flows_per_sec"),
        (Box::new(|s| s.rate.period_secs = 0.0), "period_secs"),
        (Box::new(|s| s.rate.amplitude = 1.5), "amplitude"),
        (Box::new(|s| s.target.max_datagram = 100), "max_datagram"),
        (
            Box::new(|s| s.rate.flows_per_sec = 1_000_001.0),
            "flows_per_sec",
        ),
        (Box::new(|s| s.rate.period_secs = f64::NAN), "period_secs"),
        (
            Box::new(|s| s.rate.burst_multiplier = f64::NAN),
            "burst_multiplier",
        ),
        (Box::new(|s| s.rate.amplitude = f64::NAN), "amplitude"),
        (
            Box::new(|s| s.rate.burst_secs = f64::INFINITY),
            "burst_secs",
        ),
        (Box::new(|s| s.rules[0].packets = Some([10, 5])), "packets"),
    ];
    for (mutate, needle) in cases {
        let mut s = base();
        mutate(&mut s);
        let err = format!("{:#}", s.validate().unwrap_err());
        assert!(err.contains(needle), "expected '{needle}' in: {err}");
    }
}

#[test]
fn validation_warnings() {
    let mut s = base();
    s.rate.flows_per_sec = 6000.0;
    s.rules[0].proto = Proto::Icmp;
    let warnings = s.validate().unwrap();
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    assert!(warnings.iter().any(|w| w.contains("5000")));
    assert!(warnings.iter().any(|w| w.contains("icmp")));
}

#[test]
fn scan_only_scenario_is_valid() {
    let mut s = base();
    s.rules.clear();
    s.rate.flows_per_sec = 0.0;
    assert!(s.validate().is_ok());
}

#[test]
fn overrides_apply() {
    let mut s = base();
    s.apply(&Overrides {
        collector: Some("127.0.0.1".into()),
        protocol: Some(Protocol::Ipfix),
        rate: Some(5.0),
        seed: Some(1),
    });
    assert_eq!(s.target.collector.as_deref(), Some("127.0.0.1"));
    assert_eq!(s.target.protocol, Protocol::Ipfix);
    assert_eq!(s.rate.flows_per_sec, 5.0);
    assert_eq!(s.seed, Some(1));

    let mut untouched = base();
    untouched.apply(&Overrides::default());
    assert_eq!(untouched.target.collector.as_deref(), Some("10.0.0.5:2055"));
}

#[test]
fn port_spec_forms() {
    assert_eq!(PortSpec::Range("22".into()).expand().unwrap(), vec![22]);
    assert_eq!(
        PortSpec::Range(" 80 - 82 ".into()).expand().unwrap(),
        vec![80, 81, 82]
    );
    assert_eq!(PortSpec::List(vec![1, 2]).expand().unwrap(), vec![1, 2]);
    assert!(PortSpec::Range("0-10".into()).expand().is_err());
    assert!(PortSpec::Range("a-b".into()).expand().is_err());
    let list: Scenario =
        Scenario::from_toml(&EXAMPLE.replace("ports = \"1-1024\"", "ports = [22, 3389]")).unwrap();
    assert_eq!(list.scans[0].ports.expand().unwrap(), vec![22, 3389]);
}

#[test]
fn collector_parsing_and_default_ports() {
    assert_eq!(split_host_port("10.0.0.5:9995"), ("10.0.0.5", Some(9995)));
    assert_eq!(split_host_port("flowlink.lab"), ("flowlink.lab", None));
    assert_eq!(split_host_port("host:notaport"), ("host:notaport", None));

    let v9 = resolve_collector("127.0.0.1", Protocol::V9).unwrap();
    assert_eq!(v9.port(), 2055);
    let ipfix = resolve_collector("localhost", Protocol::Ipfix).unwrap();
    assert_eq!(ipfix.port(), 4739);
    assert!(ipfix.is_ipv4());
    let explicit = resolve_collector("127.0.0.1:6500", Protocol::Ipfix).unwrap();
    assert_eq!(explicit.port(), 6500);
    assert!(resolve_collector("no-such-host.invalid", Protocol::V5).is_err());
}

#[test]
fn encoder_options_from_target() {
    let o = base().target.encoder_options();
    assert_eq!(o.observation_domain, 3);
    assert_eq!(o.max_datagram, 1400);
    assert_eq!(o.template_refresh.as_secs(), 30);
}

#[test]
fn large_datagram_warns_about_fragmentation() {
    let mut s = base();
    s.target.max_datagram = 9000;
    let warnings = s.validate().unwrap();
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("max_datagram = 9000") && w.contains("1472")),
        "{warnings:?}"
    );
    s.target.max_datagram = 1472;
    assert!(s.validate().unwrap().is_empty());
}

fn with_hosts(hosts: &str) -> Result<Vec<String>, String> {
    let text = format!("{EXAMPLE}\n{hosts}");
    Scenario::from_toml(&text)
        .unwrap()
        .validate()
        .map_err(|e| format!("{e:#}"))
}

#[test]
fn host_groups_parse() {
    let s = Scenario::from_toml(&format!(
        "{EXAMPLE}\n[[host]]\nnets = [\"10.1.1.0/24\", \"10.1.2.5\"]\nhostname_prefix = \"web\"\n[host.labels]\nrole = \"web\"\n"
    ))
    .unwrap();
    assert_eq!(s.hosts.len(), 1);
    assert_eq!(s.hosts[0].nets.len(), 2);
    assert_eq!(s.hosts[0].labels["role"], "web");
    assert_eq!(s.hosts[0].hostname_prefix.as_deref(), Some("web"));
    assert!(base().hosts.is_empty());
}

#[test]
fn host_empty_nets_rejected() {
    let e = with_hosts("[[host]]\nnets = []\n").unwrap_err();
    assert!(e.contains("[[host]] #1: nets must not be empty"), "{e}");
}

#[test]
fn host_blank_label_value_rejected() {
    let e = with_hosts("[[host]]\nnets = [\"10.1.1.0/24\"]\n[host.labels]\nrole = \" \"\n")
        .unwrap_err();
    assert!(
        e.contains(
            "[[host]] #1: label keys and values must not be empty or have surrounding whitespace"
        ),
        "{e}"
    );
}

#[test]
fn host_label_whitespace_rejected() {
    for labels in ["role = \" web\"", "role = \"web \"", "\" role\" = \"web\""] {
        let e = with_hosts(&format!(
            "[[host]]\nnets = [\"10.1.1.0/24\"]\n[host.labels]\n{labels}\n"
        ))
        .unwrap_err();
        assert!(
            e.contains(
                "[[host]] #1: label keys and values must not be empty or have surrounding whitespace"
            ),
            "{labels}: {e}"
        );
    }
}

#[test]
fn host_reserved_label_keys_rejected() {
    for key in [
        "hostname",
        "Name",
        "INTERFACES",
        "description",
        "href",
        "public_ip",
    ] {
        let e = with_hosts(&format!(
            "[[host]]\nnets = [\"10.1.1.0/24\"]\n[host.labels]\n{key} = \"x\"\n"
        ))
        .unwrap_err();
        assert!(
            e.contains(&format!("[[host]] #1: label key '{key}' is reserved")),
            "{key}: {e}"
        );
    }
}

#[test]
fn host_bad_prefix_rejected() {
    for bad in ["web_1", "-web", "web-", ""] {
        let e = with_hosts(&format!(
            "[[host]]\nnets = [\"10.1.1.0/24\"]\nhostname_prefix = \"{bad}\"\n"
        ))
        .unwrap_err();
        assert!(
            e.contains("[[host]] #1: hostname_prefix must be letters, digits and inner '-'"),
            "{bad}: {e}"
        );
    }
}

#[test]
fn host_good_prefix_accepted() {
    with_hosts("[[host]]\nnets = [\"10.1.1.0/24\"]\nhostname_prefix = \"web-1\"\n").unwrap();
}
