# Demo runbook

Goal: a PCE map and Explorer view that tell a story, with no surprises on stage.

## 1. Pick a scenario

Built-in presets (`flowgen presets`):

| Preset | Hosts | Story |
|--------|-------|-------|
| `three-tier` | 46 | Clients -> web (80/443) -> app (8080) -> db (5432/3306), DNS/NTP to infra. Classic app map; sine rate (50/s). |
| `datacenter` | 56 | Four /28 subnets in 10.10.0.0/16: https, mssql, kafka, ldap, smb, ssh. Constant 100/s. Good for "what talks to what" and ssh/smb policy. |
| `scan-demo` | 46 | `three-tier` plus a compromised web server (10.1.1.13) scanning the app tier on ports 1-1024, 200 flows every 60 s. |

Samples (in the examples directory, see [quickstart](quickstart.md#1-install)):

| File | Hosts | Story |
|------|-------|-------|
| `web-shop.toml` | 49 | E-commerce: internet -> LB -> web -> app -> data/payment, shared DNS/NTP. Sine rate gives a load wave. |
| `active-directory.toml` | 50 | Domain controllers as the hub (Kerberos, LDAP, RPC, SMB, DNS); one admin jump host is the only RDP/WinRM source. |
| `kubernetes.toml` | 33 | Workers to a 3-node control plane, ingress LB to NodePorts, east-west pod traffic, CoreDNS. |
| `lateral-movement.toml` | 30 | Ransomware-style: workstation 10.40.1.5 sweeps the server subnet (22/135/139/445/3389/5985) every 45 s. |
| `ot-iot.toml` | 38 | HMIs/historian/engineering ws to PLCs, sensors to an MQTT broker; small flows, industrial ports. |
| `burst-demo.toml` | 17 | Small two-tier app, base 20 flows/s with 8x bursts: spikes in the flowgen display and in Explorer. |
| `load-test.toml` | 131,068 | 4000 flows/s between two /16s. Collector throughput test only, not a demo. |

`scenario.toml` is the annotated reference, not a story.

## 2. Create the workloads

The PCE shows a flow only if at least one of its IPs is a managed or unmanaged workload.

```sh
flowgen ips --preset scan-demo > ips.txt           # one IP per line, sorted, deduplicated
flowgen ips -c /usr/share/flowgen/examples/lateral-movement.toml > ips.txt
wc -l ips.txt
```

Create unmanaged workloads (or a workload import) from `ips.txt`, with labels that match your story (role, app, env). Scan sources are included in the list. `flowgen ips` refuses scenarios with more than 4096 hosts unless you pass `--all`.

## 3. Flowlink listener

Match parser to protocol and port (v5/v9 = `netflow`, IPFIX = `ipfix`):

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

- `remote_addrs`: if set in your Flowlink config, it lists allowed exporter IPs (no CIDRs); flows from other sources are discarded. Use the "exporter source IP" line flowgen prints at start (the laptop/VM it runs on). The exact placement is defined by your Flowlink version: check `/usr/local/illumio/flowlink_config_schema.json`.
- Some Flowlink docs use `destinations:` instead of `consumers:`: follow the schema on your host.
- `aggregation_minutes` (default 10, minimum 5) sets the PCE delay. Lower it for demos if your setup allows.

## 4. Timing

Flowlink posts to the PCE once per aggregation interval, so:

- Start flowgen at least one aggregation interval (10 min by default) before you go on stage, ideally 15-20 min.
- Leave it running: `flowgen run ... --no-progress >flowgen.log 2>&1 &` or in `tmux`. No `--duration` means it runs until Ctrl-C/SIGTERM.
- Scans fire for the first time one interval after start (60 s for `scan-demo`, 45 s for `lateral-movement`), so that is covered by the warm-up.

```sh
flowgen run --preset scan-demo --collector flowlink.lab                   # IPFIX
flowgen run -c /usr/share/flowgen/examples/web-shop.toml --collector 10.0.0.5 --protocol v9
```

## 5. Several scenarios at once

The six story samples (everything except `scenario.toml` and `load-test.toml`) and `datacenter` use disjoint subnets, so they can share one Flowlink. `three-tier`, `scan-demo` and `scenario.toml` overlap (10.1.x) - run only one of them.

```sh
E=/usr/share/flowgen/examples      # or /usr/local/share/flowgen/examples
# one terminal each (or tmux panes)
flowgen run -c $E/web-shop.toml        --collector flowlink.lab
flowgen run -c $E/active-directory.toml --collector flowlink.lab
flowgen run --preset datacenter         --collector flowlink.lab
```

Keep the combined rate well under 5000 flows/s (flowgen warns above that per instance). All instances use the same exporter source IP.

## 6. The scan / lateral-movement story

Setup: `scan-demo` (web server 10.1.1.13 scans the app tier) or `lateral-movement` (workstation 10.40.1.5 sweeps 10.40.2.0/28).

1. Show the baseline map in Illumination: normal tiers, normal ports.
2. After the first scan burst (see `scan burst from 10.40.1.5: 84 flows` in the flowgen terminal), and one more aggregation interval, open Explorer and filter the source to the scanner IP.
3. Point out one source reaching every host in the target subnet on many ports (1-1024 for `scan-demo`; 22/135/139/445/3389/5985 for `lateral-movement`). Scan flows are 1-packet, 44-60 byte TCP SYNs.
4. Close with policy: draft a rule/boundary that blocks the scanner's source to the server tier, or segment the subnet.

Tip: `--seed 42` makes the random choices (hosts, ports, sizes) repeat between rehearsal and demo.

## 7. Pre-demo checklist

- [ ] `flowgen --version` works; scenario runs with `--dry-run --duration 5s`.
- [ ] Flowlink listener parser type matches protocol (netflow for v5/v9, ipfix) and port.
- [ ] `remote_addrs` unset, or contains the exporter source IP flowgen printed.
- [ ] UDP port reachable from the flowgen host (firewall / security group).
- [ ] Unmanaged workloads exist for every IP in `flowgen ips` (at least one side of each flow).
- [ ] `illumio-flowlink-ctl ... --debug` showed records.
- [ ] flowgen started >= 1 aggregation interval ago; status line shows `errs 0`.
- [ ] Flows visible in Explorer for the last interval.
- [ ] Scenario subnets do not overlap with other running instances.
