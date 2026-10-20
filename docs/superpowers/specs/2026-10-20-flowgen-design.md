# flowgen — NetFlow/IPFIX generator for Illumio Flowlink

Date: 2026-10-20
Status: Draft — updated with Flowlink research and CI packaging, pending review

## Purpose

Internal tool for testing and demoing Illumio Flowlink. It generates
realistic-looking flow records and exports them over UDP as NetFlow v5,
NetFlow v9, or IPFIX, so that Flowlink ingests them and the PCE shows
meaningful traffic (Illumination / Explorer).

Primary goal: **demos and functional testing** — plausible traffic at
modest rates (tens to a few thousand flows/s). Throughput tuning for load
testing is explicitly not a v1 goal.

## Flowlink facts that shape the design

Sources: Illumio Flowlink 2.0.0 Configuration and Usage guide
(https://product-docs-repo.illumio.com/Tech-Docs/Core/PDFs/Illumio_Flowlink_Configure_and_Usage_2_0.pdf)
and the 25.4 "Configure Flowlink" page.

| Fact | Consequence for flowgen |
|------|-------------------------|
| Flowlink accepts NetFlow v5/v7/v9, IPFIX, sFlow v5, AWS, text. NetFlow/IPFIX examples are all UDP. | v5, v9, IPFIX over UDP covers it. |
| Listeners are YAML `consumers` with `parser.type` (`netflow` / `ipfix`) and `connectors: [{type: udp, properties: {ports: ...}}]`. Documented ports: NetFlow 2055, IPFIX 4739 (also 6500/6514 in examples). | Default collector port 2055 for v5/v9 and 4739 for IPFIX when `--collector` gives only a host. Note: v5/v9 need a `netflow` parser, IPFIX an `ipfix` parser. A wrong protocol on a port gets silently dropped, so README shows a matching Flowlink snippet. |
| Optional `remote_addrs` lists the exporter IPs that are allowed (no CIDRs). Flows from other sources are discarded. | Exporter = the host flowgen runs on. README tells users to add that IP or leave `remote_addrs` unset. The startup header prints the local source IP used. |
| Flow key is effectively (src_ip, dst_ip, dst_port, proto). Illumio's own F5 IPFIX template uses flowStart/EndMilliseconds, src/dst IPv4, ports, protocolIdentifier, octet/packet delta counts. | Our templates use exactly these fields (plus tcpControlBits). |
| Multicast (224.0.0.0/4) flows are ignored. | Validation rejects multicast CIDRs in rules/scans. |
| Flowlink aggregates and posts to the PCE every `aggregation_minutes` (default 10, min 5). | README: flows appear in the PCE after up to one aggregation interval. The live UI shows a hint line saying so. |
| PCE shows Flowlink flows only if at least one IP of each flow matches a managed or unmanaged workload. | Presets use small subnets (a few dozen IPs total). New `flowgen ips` subcommand prints every host IP a scenario can emit, so users can create unmanaged workloads or IP lists for them. |
| PCE limit: about 10K unique flows/s total. Flowlink may consume more slowly than the source sends. | Rates stay modest. A warning is shown if `flows_per_sec` > 5000. |
| Timestamp handling for NetFlow/IPFIX is not documented. | Records use current wall-clock times (end = now). |
| `illumio-flowlink-ctl ... --debug` prints incoming records. | README troubleshooting points to it. |

## Scope

In scope (v1):

- Single statically-buildable Rust binary `flowgen`.
- Export protocols: NetFlow v5, NetFlow v9 (RFC 3954), IPFIX (RFC 7011), UDP only.
- IPv4 only.
- Scenario configuration via TOML file, with CLI overrides for common knobs.
- Traffic patterns: weighted flow rules, rate shapes (constant/sine/burst),
  port-scan anomaly injection, built-in presets.
- Dry-run mode (print flows, send nothing); live progress bar + stats in a TTY, plain stats lines otherwise.
- Linux (.tar.gz, .deb, .rpm; x86_64 + arm64, static musl) and macOS (universal .tar.gz + .pkg) packages built in GitHub Actions, plus a one-command `install.sh`.

Out of scope (v1): sFlow, IPv6, TCP/SCTP transport, exporter source-IP
spoofing (raw sockets), multiple simultaneous collectors, Homebrew tap, code signing/notarization, high-throughput
optimization, pcap output, interactive UI.

## Architecture

One Cargo binary crate (`src/main.rs` + library modules in `src/lib.rs` so
tests can reach them).

| Module      | Responsibility |
|-------------|----------------|
| `config`    | TOML scenario structs (`serde`), CLI (`clap` derive), merge CLI overrides into scenario, validation. |
| `model`     | Protocol-neutral `FlowRecord`. |
| `pattern`   | `Generator` producing `FlowRecord`s from rules + scans; `RateShape` computing target flows/s at time t. |
| `presets`   | Built-in scenarios embedded as TOML strings (`include_str!`). |
| `export`    | `Encoder` trait; `v5`, `v9`, `ipfix` implementations packing records into MTU-bounded datagrams. |
| `sender`    | UDP socket, pacing loop, dry-run sink; updates `Stats`. |
| `ui`        | Renders `Stats`: progress bar/spinner, live status line, plain log lines, final summary. |

### FlowRecord

```rust
pub struct FlowRecord {
    pub src_addr: Ipv4Addr,
    pub dst_addr: Ipv4Addr,
    pub src_port: u16,
    pub dst_port: u16,
    pub protocol: u8,          // 6 tcp, 17 udp, 1 icmp
    pub packets: u64,
    pub bytes: u64,
    pub tcp_flags: u8,
    pub start: SystemTime,
    pub end: SystemTime,
}
```

### Encoder trait

```rust
pub trait Encoder {
    /// Encode records into one or more datagrams, each <= max_datagram bytes.
    /// `now` is the export time. Encoders own sequence numbers and template state.
    fn encode(&mut self, records: &[FlowRecord], now: SystemTime) -> Vec<Vec<u8>>;
}
```

- **v5**: header (24 B) + up to 30 records (48 B each). `sys_uptime`
  relative to encoder creation; `first`/`last` as uptime ms. Sequence =
  cumulative flow count.
- **v9**: one template (ID 256) with IN_BYTES, IN_PKTS, PROTOCOL, TCP_FLAGS,
  L4_SRC_PORT, IPV4_SRC_ADDR, L4_DST_PORT, IPV4_DST_ADDR, FIRST_SWITCHED,
  LAST_SWITCHED. Template FlowSet sent in the first packet and then every
  `template_refresh_secs` (default 30) or every 100 packets, whichever comes
  first. Sequence = packet count. Source ID = configurable observation domain.
- **IPFIX**: template set (ID 256) with octetDeltaCount, packetDeltaCount,
  protocolIdentifier, tcpControlBits, sourceTransportPort,
  sourceIPv4Address, destinationTransportPort, destinationIPv4Address,
  flowStartMilliseconds, flowEndMilliseconds. Same refresh policy as v9.
  Sequence = cumulative data record count. Observation domain configurable.

Default max datagram size: 1400 bytes (configurable).

### Pattern generation

- **Rule** (`[[rule]]`): `name`, `src` (list of CIDRs), `dst` (list of
  CIDRs), `proto` (`tcp|udp|icmp`), `dst_ports` (list of ports), `weight`
  (u32 > 0). Each generated flow: pick rule by weight, pick src/dst host
  uniformly from the CIDR lists (network/broadcast excluded for prefixes
  < /31), pick dst port uniformly, src port ephemeral 32768–60999 (0 for
  icmp). Optional `bytes` range and `packets` range per rule; defaults
  derived from protocol (tcp: 5–200 pkts, 60–1400 B/pkt avg; udp: 1–20
  pkts; icmp: 1–4 pkts, 84 B/pkt). TCP flags default SYN|ACK|PSH|FIN (0x1b).
- **Flow timing**: end = now; duration random 0.1–30 s; start = end − duration.
- **Scan** (`[[scan]]`): `src` (single IPv4), `dst` (CIDR list), `ports`
  (range string like `"1-1024"` or list), `every_secs` (interval between
  scan bursts), `flows_per_burst` (default 200). A burst emits that many
  1-packet TCP SYN (flags 0x02, 44–60 B) flows from `src` to random
  dst host/port combos, in addition to the regular rate. First burst after
  one interval.
- **Rate** (`[rate]`): `flows_per_sec` (base), `shape`:
  - `constant`: rate = base
  - `sine`: rate = base × (1 + amplitude × sin(2πt / period_secs)),
    `amplitude` default 0.5
  - `burst`: rate = base, plus with probability `burst_chance` per second
    (default 0.05) a burst of `burst_multiplier` × base (default 5) lasting
    `burst_secs` (default 10)
  Rate is clamped to ≥ 0.
- RNG seedable via `seed` (top-level) for reproducible runs; random otherwise.

### Sending loop

Tick every 100 ms: compute flows due this tick from the rate shape
(fractional remainder carried over), plus any scan burst due, generate,
encode, send each datagram. Run until `duration` elapses or Ctrl-C
(graceful: print final summary). Send errors are counted and logged at most
once per stats interval; they do not abort the run.

### CLI output (stats and progress)

Stats are collected in a `Stats` struct (flows, datagrams, bytes on wire,
send errors, scan bursts, per-rule flow counts, current target rate,
measured flows/s over the last ~5 s). Rendering lives in a separate
`ui` module using `indicatif`:

- **Interactive (stderr is a TTY)**:
  - Header line at start: protocol, collector, scenario/preset name, seed.
  - With `--duration`: a progress bar for elapsed/total time with ETA.
    Without: a spinner with elapsed time.
  - Live status line under it, refreshed ~4×/s:
    `rate 212/s (target 200)  flows 12,345  pkts 1,020  errs 0  scans 1`.
  - Scan bursts print a one-line notice above the bars.
- **Non-interactive (not a TTY, or `--no-progress`)**: no bars; one plain
  stats line every `stats_interval_secs` (default 10), suitable for logs.
- **`--quiet`**: only errors and the final summary.
- **Final summary** (always, on finish or Ctrl-C): duration, totals,
  average flows/s, send errors, and a small per-rule table (rule name,
  flows, share %).
- Dry-run writes flows to stdout; bars/stats go to stderr so stdout stays
  pipeable.

Dry-run: instead of sending, print each flow as one line
(`proto src:port -> dst:port pkts bytes`) to stdout; encoders still run so
encoding errors surface.

## Configuration

Full example:

```toml
seed = 42                       # optional

[target]
collector = "10.0.0.5:2055"     # required unless --collector given
protocol  = "ipfix"             # v5 | v9 | ipfix (default ipfix)
observation_domain = 1          # v9 source id / IPFIX domain id
template_refresh_secs = 30
max_datagram = 1400

[rate]
flows_per_sec = 200
shape = "sine"                  # constant | sine | burst
period_secs = 300
amplitude = 0.5

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
dst = ["10.1.3.10/32"]
proto = "tcp"
dst_ports = [5432]
weight = 3

[[scan]]
src = "10.9.9.9"
dst = ["10.1.2.0/24"]
ports = "1-1024"
every_secs = 120
```

### CLI

```
flowgen run [-c FILE | --preset NAME] [--collector HOST:PORT]
            [--protocol v5|v9|ipfix] [--rate N] [--duration DUR]
            [--seed N] [--dry-run] [--no-progress] [--quiet]
flowgen presets [--show NAME]
flowgen ips [-c FILE | --preset NAME]    # unique host IPs, one per line
flowgen --version
```

- Exactly one of `-c` / `--preset` is required.
- CLI flags override the corresponding scenario fields.
- `--duration` accepts humantime (`30s`, `10m`, `1h`); default: run forever.
- `--collector` accepts `host:port` or `host`. If the port is omitted, it
  defaults to 2055 (v5/v9) or 4739 (IPFIX). Hostname resolved once at
  startup.
- `flowgen ips` expands every rule/scan CIDR into host IPs (deduplicated,
  sorted). It refuses scenarios with more than 4096 hosts unless `--all`
  is given.

### Presets

Presets keep the host count small (about 60 IPs or fewer) so a demo PCE
can get matching unmanaged workloads quickly.

- `three-tier`: web 10.1.1.8/29, app 10.1.2.8/29, db 10.1.3.8/30, clients
  192.168.10.0/27 → web 80/443, web → app 8080, app → db 5432/3306,
  plus DNS (udp/53) and NTP (udp/123) to infra 10.1.0.10/31.
- `datacenter`: four /28 app subnets in 10.10.0.0/16 with mixed services
  (http, https, ssh, ldap, smb, mssql, kafka), constant rate.
- `scan-demo`: `three-tier` traffic plus a scanner hitting the app tier
  every 60 s.

Presets have no collector; one must come from `--collector`.

## Validation and errors

At startup, fail with a clear message (exit code 2) on: unparseable TOML,
invalid CIDR/IP, empty rule list with no scans, weight 0, empty port list
on a tcp/udp rule, unknown preset, missing collector (unless dry-run),
unresolvable collector, `flows_per_sec` < 0, unknown protocol/shape,
multicast (224.0.0.0/4) CIDR or IP in a rule or scan.
Errors via `anyhow` with context.

Warnings only (not errors):
- an icmp rule with `dst_ports`: the ports are ignored and the dst port is 0.
- `flows_per_sec` > 5000: above Flowlink/PCE comfort levels.

## Testing

- Encoder unit tests: encode known records, decode with `netflow_parser`
  (dev-dependency), assert field equality; assert datagram size ≤ limit and
  correct record counts per packet; v9/IPFIX template presence on first
  packet and after refresh interval; sequence-number progression.
- Pattern tests: CIDR host sampling stays in range and skips
  network/broadcast; weighted selection roughly matches weights over many
  samples (seeded); scan bursts fire on schedule; rate-shape math.
- Config tests: example and all presets parse and validate; CLI overrides
  apply; invalid inputs produce errors.
- Collector parsing: host-only gets 2055 or 4739 depending on protocol;
  multicast CIDRs are rejected; `ips` expansion dedups, sorts, and respects
  the 4096-host cap.
- Integration test: run the sender for a short duration against a local
  `UdpSocket`, decode received datagrams, assert flows arrive and decode.

## Packaging and installation

Goal: a colleague can install `flowgen` on Linux or macOS in one command,
without Rust or Homebrew (Homebrew tap is out of scope for now).

**All packages are built in GitHub Actions, never locally.**

### CI workflows

- `.github/workflows/ci.yml` — on push / pull request: `cargo fmt --check`,
  `cargo clippy -- -D warnings`, `cargo test` on `ubuntu-latest` and
  `macos-latest`.
- `.github/workflows/release.yml` — on tag `v*` (and `workflow_dispatch`
  for a dry run that uploads workflow artifacts without creating a release):

| Job | Runner | Target | Artifacts |
|-----|--------|--------|-----------|
| linux-x86_64 | `ubuntu-latest` | `x86_64-unknown-linux-musl` (static) | `flowgen-<ver>-linux-x86_64.tar.gz`, `.deb`, `.rpm` |
| linux-aarch64 | `ubuntu-24.04-arm` | `aarch64-unknown-linux-musl` (static) | `flowgen-<ver>-linux-aarch64.tar.gz`, `.deb`, `.rpm` |
| macos | `macos-latest` | universal (`lipo` of x86_64 + aarch64 apple-darwin) | `flowgen-<ver>-macos-universal.tar.gz`, `.pkg` |
| verify | `ubuntu-latest` (+ `ubuntu-24.04-arm`) | — | installs the `.deb` in `debian:stable-slim`, `.rpm` in `rockylinux:9`, tarball via `install.sh` in `alpine`; runs `flowgen --version` and a 2 s `--dry-run` |
| publish | `ubuntu-latest` | — | `SHA256SUMS`, GitHub Release with all artifacts + `install.sh` |

Native runners per architecture (no cross-compilation): musl via
`musl-tools` on Ubuntu; `.deb` via `cargo-deb`, `.rpm` via
`cargo-generate-rpm` (installed in CI with `taiki-e/install-action`).
The macOS job also runs `install.sh --prefix` into a temp dir and checks
the pkg with `pkgutil --payload-files`.

### Artifact contents

- Tarballs: `flowgen` binary, `README.md`, `LICENSE` (if present),
  `examples/*.toml`.
- `.deb` / `.rpm`: `/usr/bin/flowgen`, examples in
  `/usr/share/doc/flowgen/examples/`. No systemd unit (CLI tool).
- macOS `.pkg` (`pkgbuild`): `/usr/local/bin/flowgen`. Unsigned / not
  notarized in v1; README documents `xattr -d com.apple.quarantine` for
  tarballs and right-click → Open for the pkg.
- Release profile: `lto = "thin"`, `strip = true`, `codegen-units = 1`.
- Version from `Cargo.toml` (tag must match, checked in CI);
  `flowgen --version` prints it plus git short hash (via `build.rs`,
  falls back to `unknown`).

### install.sh

POSIX sh. Detects OS/arch, downloads (or takes from `--from DIR`) the
matching tarball from the GitHub Release (`--version vX.Y.Z`, default
latest), verifies against `SHA256SUMS`, installs to `/usr/local/bin`
(using `sudo` if needed) or `--prefix DIR`, clears macOS quarantine,
prints `flowgen --version`. Usage:
`curl -fsSL https://github.com/<org>/illumio-flowgen/releases/latest/download/install.sh | sh`.
The repo slug is a variable at the top of the script (repo has no remote
yet). Private repo note: if the repo is private, use `gh release download`
(documented in README) instead of curl.

## Documentation

`README.md` covers: install (`install.sh`, `.deb`/`.rpm`, `.pkg`, private
repo via `gh release download`), quick start with a preset, scenario file
reference, a matching Flowlink `consumers` YAML snippet for NetFlow
(2055) and IPFIX (4739), PCE visibility checklist (unmanaged workloads via
`flowgen ips`, `remote_addrs`, wait one aggregation interval), and
troubleshooting (`illumio-flowlink-ctl --debug`, firewall/UDP reachability,
`--dry-run`).

## Dependencies

`clap` (derive), `serde`, `toml`, `anyhow`, `rand` (with seedable `StdRng`),
`ipnet`, `humantime`, `ctrlc`, `indicatif`. Dev: `netflow_parser`.
Packaging tools (CI only): `cargo-deb`, `cargo-generate-rpm`, `musl-tools`.
