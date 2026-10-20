use std::cell::RefCell;
use std::io::{self, Write};
use std::net::UdpSocket;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use flowgen::config::{Overrides, Scenario};
use flowgen::export::{Protocol, new_encoder};
use flowgen::pattern::{Generator, RateShape, make_rng, scenario_hosts};
use flowgen::presets::find;
use flowgen::sender::{Engine, Sink};
use flowgen::stats::Stats;
use flowgen::ui::{Header, Mode, Ui};
use netflow_parser::NetflowParser;

fn scenario(protocol: Protocol) -> Scenario {
    let mut s = Scenario::from_toml(find("three-tier").unwrap().toml).unwrap();
    s.apply(&Overrides {
        protocol: Some(protocol),
        rate: Some(200.0),
        seed: Some(7),
        ..Default::default()
    });
    s.validate().unwrap();
    s
}

fn engine(s: &Scenario, sink: Sink) -> Engine {
    let (rng, _) = make_rng(s.seed);
    Engine {
        generator: Generator::new(s, rng).unwrap(),
        shape: RateShape::new(&s.rate),
        encoder: new_encoder(
            s.target.protocol,
            &s.target.encoder_options(),
            SystemTime::now(),
        ),
        sink,
    }
}

fn quiet_ui() -> Ui {
    let h = Header {
        protocol: Protocol::Ipfix,
        destination: "test".into(),
        local_addr: None,
        scenario: "test".into(),
        seed: 7,
    };
    Ui::start(Mode::Quiet, &h, None, Duration::from_secs(10))
}

fn run(engine: &mut Engine, for_: Duration, stop: bool) -> Stats {
    let mut stats = Stats::new(engine.generator.rule_names().len());
    engine
        .run(
            Some(for_),
            &mut stats,
            &mut quiet_ui(),
            Duration::from_secs(10),
            &AtomicBool::new(stop),
        )
        .unwrap();
    stats
}

#[test]
fn every_protocol_arrives_and_decodes() {
    for protocol in [Protocol::V5, Protocol::V9, Protocol::Ipfix] {
        let rx = UdpSocket::bind("127.0.0.1:0").unwrap();
        rx.set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let s = scenario(protocol);
        let mut e = engine(&s, Sink::udp(rx.local_addr().unwrap()).unwrap());
        let stats = run(&mut e, Duration::from_millis(1500), false);

        assert!(
            stats.flows > 150,
            "{protocol:?}: only {} flows",
            stats.flows
        );
        assert_eq!(stats.send_errors, 0);
        assert_eq!(stats.per_rule.iter().sum::<u64>(), stats.flows);

        let hosts = scenario_hosts(&s, None).unwrap();
        let mut parser = NetflowParser::default();
        let mut buf = [0u8; 65535];
        let (mut datagrams, mut decoded) = (0u64, 0u64);
        while let Ok(n) = rx.recv(&mut buf) {
            datagrams += 1;
            let (flows, err) = parser.parse_bytes_as_netflow_common_flowsets(&buf[..n]);
            assert!(err.is_none(), "{protocol:?}: {err:?}");
            for f in &flows {
                let std::net::IpAddr::V4(src) = f.src_addr.unwrap() else {
                    panic!("ipv6")
                };
                assert!(hosts.contains(&src), "{src} not in scenario");
            }
            decoded += flows.len() as u64;
        }
        assert_eq!(datagrams, stats.datagrams, "{protocol:?}");
        assert_eq!(decoded, stats.flows, "{protocol:?}");
    }
}

#[test]
fn closed_port_does_not_abort() {
    let port = UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap(); // dropped => closed
    let s = scenario(Protocol::Ipfix);
    let mut e = engine(&s, Sink::udp(port).unwrap());
    let stats = run(&mut e, Duration::from_millis(500), false);
    assert!(stats.flows > 0);
}

#[test]
fn stop_flag_ends_run_immediately() {
    let s = scenario(Protocol::V5);
    let mut e = engine(&s, Sink::dry_run(Box::new(io::sink())));
    let stats = run(&mut e, Duration::from_secs(30), true);
    assert_eq!(stats.flows, 0);
}

#[derive(Clone, Default)]
struct SharedBuf(Rc<RefCell<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn dry_run_prints_one_line_per_flow() {
    let buf = SharedBuf::default();
    let s = scenario(Protocol::V9);
    let mut e = engine(&s, Sink::dry_run(Box::new(buf.clone())));
    let stats = run(&mut e, Duration::from_millis(500), false);
    let text = String::from_utf8(buf.0.borrow().clone()).unwrap();
    assert_eq!(text.lines().count() as u64, stats.flows);
    assert!(text.lines().all(|l| l.contains(" -> ")));
    assert!(stats.datagrams > 0, "encoders still run in dry-run");
}

struct BrokenPipe;

impl Write for BrokenPipe {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::from(io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn dry_run_broken_pipe_stops_cleanly() {
    let s = scenario(Protocol::Ipfix);
    let mut e = engine(&s, Sink::dry_run(Box::new(BrokenPipe)));
    let started = std::time::Instant::now();
    run(&mut e, Duration::from_secs(30), false);
    assert!(started.elapsed() < Duration::from_secs(5));
}
