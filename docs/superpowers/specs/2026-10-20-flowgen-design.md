# flowgen — NetFlow/IPFIX generator for Illumio Flowlink

Date: 2026-10-20
Status: Approved design, pending implementation plan

## Purpose

Internal tool for testing and demoing Illumio Flowlink. It generates
realistic-looking flow records and exports them over UDP as NetFlow v5,
NetFlow v9, or IPFIX, so that Flowlink ingests them and the PCE shows
meaningful traffic (Illumination / Explorer).

Primary goal: **demos and functional testing** — plausible traffic at
modest rates (tens to a few thousand flows/s). Throughput tuning for load
testing is explicitly not a v1 goal.

## Scope

In scope (v1):

- Single statically-buildable Rust binary `flowgen`.
- Export protocols: NetFlow v5, NetFlow v9 (RFC 3954), IPFIX (RFC 7011), UDP only.
- IPv4 only.
- Scenario configuration via TOML file, with CLI overrides for common knobs.
- Traffic patterns: weighted flow rules, rate shapes (constant/sine/burst),
  port-scan anomaly injection, built-in presets.
- Dry-run mode (print flows, send nothing); live progress bar + stats in a TTY, plain stats lines otherwise.
- Linux (.tar.gz, .deb, .rpm; x86_64 + arm64, static musl) and macOS (universal .tar.gz + .pkg) packages, plus a one-command `install.sh`.

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
flowgen --version
```

- Exactly one of `-c` / `--preset` is required.
- CLI flags override the corresponding scenario fields.
- `--duration` accepts humantime (`30s`, `10m`, `1h`); default: run forever.
- `--collector` accepts `host:port`; hostname resolved once at startup.

### Presets

- `three-tier`: web/app/db tiers in 10.1.{1,2,3}.0/24, clients from
  192.168.0.0/16 → web 80/443, web → app 8080, app → db 5432/3306,
  plus DNS (udp/53) and NTP (udp/123) to an infra subnet.
- `datacenter`: several app subnets in 10.10.0.0/16 with mixed services
  (http, https, ssh, ldap, smb, mssql, kafka), constant rate.
- `scan-demo`: `three-tier` traffic plus a scanner hitting the app tier
  every 60 s.

Presets have no collector; one must come from `--collector`.

## Validation and errors

At startup, fail with a clear message (exit code 2) on: unparseable TOML,
invalid CIDR/IP, empty rule list with no scans, weight 0, empty port list
on a tcp/udp rule, unknown preset, missing collector (unless dry-run),
unresolvable collector, `flows_per_sec` < 0, unknown protocol/shape.
Errors via `anyhow` with context.

Warning only (not an error): an icmp rule with `dst_ports` — ports are
ignored and dst port is 0.

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
- Integration test: run the sender for a short duration against a local
  `UdpSocket`, decode received datagrams, assert flows arrive and decode.

## Packaging and installation

Goal: a colleague can install `flowgen` on Linux or macOS in one command,
without Rust or Homebrew (Homebrew tap is out of scope for now).

Targets and artifacts (all built by `scripts/package.sh` into `dist/`):

| Platform | Target | Artifacts |
|----------|--------|-----------|
| Linux x86_64 | `x86_64-unknown-linux-musl` (static) | `flowgen-<ver>-linux-x86_64.tar.gz`, `.deb`, `.rpm` |
| Linux arm64 | `aarch64-unknown-linux-musl` (static) | `flowgen-<ver>-linux-aarch64.tar.gz`, `.deb`, `.rpm` |
| macOS | universal (`lipo` of x86_64 + aarch64 apple-darwin) | `flowgen-<ver>-macos-universal.tar.gz`, `flowgen-<ver>-macos-universal.pkg` |

Plus `SHA256SUMS` over all artifacts.

- Each tarball contains: `flowgen` binary, `README.md`, `LICENSE`
  (if present), `examples/*.toml`.
- Linux musl builds via `cargo-zigbuild` (requires `zig`), so they can be
  produced from a Mac. `.deb` via `cargo-deb`, `.rpm` via
  `cargo-generate-rpm`; both install `/usr/bin/flowgen` and examples to
  `/usr/share/doc/flowgen/examples/`. No systemd unit (it's a CLI tool).
- macOS `.pkg` via `pkgbuild`, installs `/usr/local/bin/flowgen`.
  Unsigned/not notarized in v1 — README documents
  `xattr -d com.apple.quarantine` for downloaded tarballs and
  right-click → Open for the pkg.
- `install.sh` (POSIX sh): detects OS/arch, picks the matching tarball
  (from a local `dist/` dir next to it, or a `--from URL_OR_DIR` base),
  verifies SHA256 if `SHA256SUMS` is present, installs to `/usr/local/bin`
  (uses `sudo` if needed) or `--prefix DIR`, removes macOS quarantine
  attribute, prints `flowgen --version`. Usage:
  `sh install.sh` or `curl -fsSL <base>/install.sh | sh -s -- --from <base>`.
- Release binaries built with `--release`, `lto = "thin"`, `strip = true`.
- Version comes from `Cargo.toml`; `flowgen --version` prints it plus git
  short hash.

Packaging verification: run the Linux x86_64/aarch64 binaries in Docker
(`debian:stable-slim`, `install .deb`; `rockylinux:9`, `install .rpm`;
`alpine` for the bare tarball via `install.sh`) and check
`flowgen --version` and a short `--dry-run`. macOS: run `install.sh
--prefix` into a temp dir and `pkgutil --payload-files` on the pkg.

## Dependencies

`clap` (derive), `serde`, `toml`, `anyhow`, `rand` (with seedable `StdRng`),
`ipnet`, `humantime`, `ctrlc`, `indicatif`. Dev: `netflow_parser`.
Packaging tools (dev machine only): `zig`, `cargo-zigbuild`, `cargo-deb`,
`cargo-generate-rpm`.
