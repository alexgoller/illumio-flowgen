# flowgen

Generate NetFlow v5, NetFlow v9 and IPFIX traffic for testing and demoing **Illumio Flowlink**.
Single static binary, Linux and macOS.

## Documentation

Docs site: **https://alexgoller.github.io/illumio-flowgen/**

Step-by-step guides live in [`docs/`](docs/README.md): [quickstart](docs/quickstart.md), [demo runbook](docs/demo-guide.md), [writing scenarios](docs/scenarios.md), [protocol details](docs/protocols.md) and [troubleshooting](docs/troubleshooting.md).

## Install

**Script (Linux/macOS):**
```sh
curl -fsSL https://github.com/alexgoller/illumio-flowgen/releases/latest/download/install.sh | sh
# or into your home dir:
curl -fsSL https://github.com/alexgoller/illumio-flowgen/releases/latest/download/install.sh | sh -s -- --prefix ~/.local/bin
```

**Private repository:** download with the GitHub CLI, then install locally:
```sh
gh release download --repo alexgoller/illumio-flowgen --dir flowgen-dist
sh flowgen-dist/install.sh --from flowgen-dist
```

**Packages:** `.deb` (`sudo dpkg -i flowgen_<ver>_amd64.deb`), `.rpm` (`sudo rpm -i flowgen-<ver>-1.x86_64.rpm`), macOS `.pkg` (double-click). The macOS `.pkg` installs only the binary to `/usr/local/bin`; the sample scenarios are in the tarball.

**macOS note:** binaries are not signed. For the tarball: `xattr -d com.apple.quarantine /usr/local/bin/flowgen` (install.sh does this). For the `.pkg`: right-click -> Open.

## Quick start

```sh
flowgen presets                                   # list built-in scenarios
flowgen run --preset three-tier --dry-run --duration 10s
flowgen run --preset three-tier --collector flowlink.lab            # IPFIX to :4739
flowgen run --preset scan-demo --collector 10.0.0.5:2055 --protocol v9 --duration 30m
flowgen run -c my-scenario.toml --rate 500 --seed 42
```

Progress bar and live stats in a terminal; `--no-progress` for plain log lines, `--quiet` for summary only.
Ctrl-C stops gracefully and prints a summary.

## Sample scenarios

The release ships 7 sample scenarios plus the annotated `scenario.toml`. Locations by install method:

- `install.sh`: `/usr/local/share/flowgen/examples/` (`<prefix>/../share/flowgen/examples/` when the prefix ends in `bin`, otherwise `<prefix>/examples/`; e.g. `~/.local/share/flowgen/examples/` for `--prefix ~/.local/bin`)
- `.deb` / `.rpm`: `/usr/share/flowgen/examples/`
- tarball: `examples/` inside the archive
- macOS `.pkg`: binary only; use the tarball or the repository for samples
- repository: `examples/`

| File | Demonstrates | Hosts |
|------|--------------|-------|
| `web-shop.toml` | Three-tier e-commerce shop (internet -> LB -> web -> app -> data/payment) with shared DNS/NTP; sine-wave rate | 49 |
| `active-directory.toml` | Windows estate: domain controllers as hub (Kerberos, LDAP, RPC, SMB, DNS), file servers, one admin jump host (RDP/WinRM) | 50 |
| `kubernetes.toml` | Workers to a 3-node control plane, ingress LB to NodePorts, east-west pod traffic, CoreDNS | 33 |
| `lateral-movement.toml` | Ransomware-style spread for segmentation demos: compromised workstation 10.40.1.5 sweeps the server subnet over SMB/RDP | 30 |
| `ot-iot.toml` | OT/IoT segment: HMIs, historian and engineering workstation to PLCs, sensors to an MQTT broker; industrial protocols, small flows | 38 |
| `burst-demo.toml` | Small two-tier app with bursty rate (base 20 flows/s, 8x bursts) to show spikes on the progress display and in Explorer | 17 |
| `load-test.toml` | Functional load test, 4000 flows/s between two /16 networks | 131,068 distinct IPs (refused by `flowgen ips` without `--all`) |
| `scenario.toml` | Annotated reference with every option | 31 |

`load-test.toml` is intentionally huge: `flowgen ips` refuses it without `--all`, and it is not meant for PCE visibility. Use it to check collector throughput and datagram packing, not to build a demo map (Flowlink/PCE handle about 10K unique flows/s in total).

The samples use disjoint subnets, so several can run at once against one Flowlink.

```sh
flowgen run -c /usr/local/share/flowgen/examples/web-shop.toml --collector flowlink.lab   # install.sh
flowgen run -c /usr/share/flowgen/examples/web-shop.toml --collector flowlink.lab     # .deb / .rpm
flowgen ips -c /usr/share/flowgen/examples/web-shop.toml     # plain list of IPs
flowgen ips -c /usr/share/flowgen/examples/web-shop.toml --format workloader > web-shop.csv   # labelled CSV for workloader
```

## Making flows visible in the PCE

1. **Matching Flowlink listener.** v5/v9 need a `netflow` parser, IPFIX an `ipfix` parser on the port you send to:
   ```yaml
   consumers:
     - name: netflow
       parser:
         type: netflow
       connectors:
         - type: udp
           properties:
             ports: '2055'
     - name: ipfix
       parser:
         type: ipfix
       connectors:
         - type: udp
           properties:
             ports: '4739'
   ```
   Some Flowlink doc examples use `destinations:` instead of `consumers:` - check `/usr/local/illumio/flowlink_config_schema.json` on your Flowlink host.
2. **Exporter allow-list.** If Flowlink sets `remote_addrs`, add the "exporter source IP" flowgen prints at start.
3. **Workloads for the IPs.** The PCE shows Flowlink flows only when at least one IP of a flow is a managed or unmanaged workload. `flowgen ips --preset three-tier` lists every IP a scenario can emit. To create labelled unmanaged workloads with [workloader](https://github.com/brian1917/workloader):
   ```sh
   flowgen ips --preset three-tier --format workloader > three-tier.csv
   workloader wkld-import three-tier.csv --umwl            # dry run: logs what would change, writes nothing without --update-pce
   workloader wkld-import three-tier.csv --umwl --update-pce --no-prompt
   ```
   Label types (role/app/env/loc, plus custom keys) must exist in the PCE; values are created. Labels come from optional `[[host]]` tables, see [scenarios.md](docs/scenarios.md#host-labels).
   Scenario files that use `[[host]]` need flowgen 0.2.0 or later.
4. **Wait one aggregation interval.** Flowlink posts to the PCE every `aggregation_minutes` (default 10, minimum 5).

## Scenario files

See [`examples/scenario.toml`](examples/scenario.toml) for every option with comments. `flowgen presets --show <NAME>` prints a built-in preset as TOML. Reference:

| Key | Meaning | Default |
|-----|---------|---------|
| `seed` | RNG seed | random (printed at start) |
| `stats_interval_secs` | plain stats line interval | 10 |
| `[target] collector` | `host[:port]` | - (required unless `--collector` / `--dry-run`) |
| `[target] protocol` | `v5` \| `v9` \| `ipfix` | `ipfix` |
| `[target] observation_domain` | v9 source id / IPFIX domain | 1 |
| `[target] template_refresh_secs` | template resend interval (also every 100 packets) | 30 |
| `[target] max_datagram` | max UDP payload bytes (256-65000; above 1472 IP-fragments, macOS rejects above 9216) | 1400 |
| `[rate] flows_per_sec` | base rate | 100 |
| `[rate] shape` | `constant` \| `sine` \| `burst` | `constant` |
| `[rate] period_secs`, `amplitude` | sine wave | 300, 0.5 |
| `[rate] burst_chance`, `burst_multiplier`, `burst_secs` | random bursts | 0.05, 5, 10 |
| `[[rule]] name, src, dst, proto, dst_ports, weight` | weighted traffic rule (CIDRs or bare IPs) | weight 1 |
| `[[rule]] packets`, `bytes_per_packet` | `[min, max]` ranges | by protocol |
| `[[scan]] src, dst, ports, every_secs, flows_per_burst` | port scan bursts | every 120 s, 200 flows |
| `[[host]] nets, labels, hostname_prefix` | labels/hostnames for `flowgen ips --format workloader` | prefix `fg` |

Multicast ranges (224.0.0.0/4) are rejected because Flowlink ignores them. Rates above 5000 flows/s produce a warning. When the destination pool has other hosts, the generator never emits a flow from a host to itself.

## Troubleshooting

- Nothing arrives: run Flowlink with `illumio-flowlink-ctl start --config <file> --debug` to print incoming records; check UDP reachability/firewalls; confirm protocol <-> parser type on that port.
- `send failed: Connection refused`: nothing listens on the collector port (ICMP port unreachable).
- See exactly what would be sent: `--dry-run` (stdout, pipeable: `flowgen run --preset datacenter --dry-run | head`).

## Development

```sh
cargo test
cargo run -- run --preset three-tier --dry-run --duration 5s
```

Releases: bump `version` in `Cargo.toml`, tag `vX.Y.Z`, push the tag. GitHub Actions builds and verifies all packages and publishes the release (`.github/workflows/release.yml`). Packages are never built locally.
