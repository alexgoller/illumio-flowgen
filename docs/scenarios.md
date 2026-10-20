# Writing scenarios

A scenario is a TOML file: `flowgen run -c my.toml`. Unknown keys are rejected. The annotated reference is [`examples/scenario.toml`](../examples/scenario.toml); built-ins print with `flowgen presets --show three-tier`.

## Structure

```toml
name = "..."                 # top level: name, seed, stats_interval_secs
[target]                     # where/how to send (optional, CLI can override)
[rate]                       # how fast (optional)
[[rule]]                     # one or more weighted traffic rules
[[scan]]                     # zero or more port-scan bursts
[[host]]                     # zero or more label groups for `flowgen ips --format workloader`
```

At least one `[[rule]]` or `[[scan]]` is required.

## Keys and defaults

| Key | Meaning | Default |
|-----|---------|---------|
| `name` | label in the header | file path / `preset <n>` |
| `seed` | RNG seed (`--seed` overrides) | random, printed at start |
| `stats_interval_secs` | plain-mode stats line interval, error-log throttle (must be > 0) | 10 |
| `[target] collector` | `host[:port]`; port defaults to 2055 (v5/v9) or 4739 (ipfix) | none; required unless `--collector`/`--dry-run` |
| `[target] protocol` | `v5` `v9` `ipfix` | `ipfix` |
| `[target] observation_domain` | v9 source id / IPFIX domain id | 1 |
| `[target] template_refresh_secs` | v9/IPFIX template resend (also every 100 packets); > 0 | 30 |
| `[target] max_datagram` | max UDP payload, 256-65000 | 1400 |
| `[rate] flows_per_sec` | base rate, 0-1,000,000 | 100 |
| `[rate] shape` | `constant` `sine` `burst` | `constant` |
| `[rate] period_secs` | sine wave length (> 0) | 300 |
| `[rate] amplitude` | sine swing, 0-1 | 0.5 |
| `[rate] burst_chance` | probability per second to start a burst, 0-1 | 0.05 |
| `[rate] burst_multiplier` | rate multiplier in a burst (>= 1) | 5 |
| `[rate] burst_secs` | burst length (> 0) | 10 |
| `[[rule]] name` | label in the summary | required |
| `[[rule]] src`, `dst` | lists of CIDRs or bare IPs (/32) | required, non-empty |
| `[[rule]] proto` | `tcp` `udp` `icmp` | required |
| `[[rule]] dst_ports` | destination ports; required for tcp/udp, ignored for icmp | `[]` |
| `[[rule]] weight` | relative share, integer > 0 | 1 |
| `[[rule]] packets` | `[min, max]` packets per flow, 1 <= min <= max | by proto (below) |
| `[[rule]] bytes_per_packet` | `[min, max]`, 1 <= min <= max | by proto (below) |
| `[[scan]] src` | single scanner IP | required |
| `[[scan]] dst` | list of CIDRs/IPs to scan | required |
| `[[scan]] ports` | `"1-1024"`, `"22"` or `[22, 3389]` | required |
| `[[scan]] every_secs` | interval between bursts (> 0) | 120 |
| `[[scan]] flows_per_burst` | flows per burst (> 0) | 200 |
| `[[host]] nets` | CIDRs or bare IPs the group covers | required, non-empty |
| `[[host]] labels` | inline table of Illumio labels, e.g. `{ role = "web", env = "prod" }`; keys and values non-empty | none |
| `[[host]] hostname_prefix` | hostname prefix: letters, digits, inner `-` | `fg` |

CLI overrides: `--collector`, `--protocol`, `--rate` (sets `flows_per_sec`), `--seed`. Everything else comes from the file.

## Semantics

**Rules and weights.** Each flow picks a rule with probability `weight / sum(weights)`, then a random src host from `src`, a random dst host from `dst`, a random port from `dst_ports`. Source port is random in 32768-60999 (0 for icmp, which also has dst port 0). Flow duration is random 0.1-30 s ending now. TCP flows carry flags 0x1b (FIN|SYN|PSH|ACK).

**Hosts.** CIDR hosts are sampled uniformly. For prefixes shorter than /31 the network and broadcast addresses are excluded (`10.1.1.8/29` = .9-.14); `/31` gives both addresses, `/32` or a bare IP exactly one. Several CIDRs in a list are weighted by their host counts. A flow never goes from a host to itself when the dst pool has other hosts (after 8 resamples a single-host pool equal to the source still emits it).

**Packets and bytes.** Flow bytes = packets x bytes-per-packet, each drawn uniformly from the range.

| proto | packets | bytes_per_packet |
|-------|---------|------------------|
| tcp | 5-200 | 60-1400 |
| udp | 1-20 | 60-1200 |
| icmp | 1-4 | 84-84 |

**Rate shapes** (t = seconds since start; rate never goes below 0):

- `constant`: `rate = flows_per_sec`
- `sine`: `rate = flows_per_sec * (1 + amplitude * sin(2*pi*t / period_secs))`
- `burst`: `rate = flows_per_sec`; each second there is a `burst_chance` of starting a burst, during which `rate = flows_per_sec * burst_multiplier` for `burst_secs`.

**Scans** run on top of the regular rate. The first burst fires `every_secs` after start, then repeats. A burst is `flows_per_burst` one-packet TCP SYN flows (flags 0x02, 44-60 bytes) from `src` to random dst host / port combinations. flowgen prints `scan burst from <ip>: <n> flows`. A scenario with only `[[scan]]` entries is valid.

## Host labels

`[[host]]` tables are optional and do not affect traffic. They give the generated IPs labels and hostnames for `flowgen ips --format workloader` (see [demo-guide](demo-guide.md#2-create-the-workloads)).

```toml
[[host]]                                   # broad group: shared labels
nets = ["10.1.0.0/16"]
labels = { app = "shop", env = "prod", loc = "dc1" }

[[host]]                                   # narrower group: adds role and hostname prefix
nets = ["10.1.1.0/28"]
labels = { role = "web" }
hostname_prefix = "web"

[[host]]
nets = ["10.1.3.10"]
labels = { role = "db" }
hostname_prefix = "db"
```

**Merge rules.** Every group whose `nets` contain an IP applies. Groups are ordered by the most specific matching prefix length, file order breaking ties; later (more specific) groups overwrite earlier ones per label key and for `hostname_prefix`. Put app/env/loc on a broad net and role plus prefix on narrow ones. Scan source IPs need labels too.

**Hostname** is `{hostname_prefix or "fg"}-a-b-c-d`, so 10.1.1.9 in the group above becomes `web-10-1-1-9`. It is derived from the IP, so re-imports match the same workload.

**CSV columns** (`flowgen ips --format workloader`): `hostname,name,interfaces,description,role,app,env,loc`, then any other label keys sorted. `name` equals the hostname, `interfaces` is the bare IP (workloader names the interface `umwl`), `description` is `flowgen <scenario name>`. Empty cells leave existing PCE values unchanged.

```
hostname,name,interfaces,description,role,app,env,loc
dns-10-1-0-10,dns-10-1-0-10,10.1.0.10,flowgen three-tier,dns,infra,prod,dc1
dns-10-1-0-11,dns-10-1-0-11,10.1.0.11,flowgen three-tier,dns,infra,prod,dc1
web-10-1-1-9,web-10-1-1-9,10.1.1.9,flowgen three-tier,web,portal,prod,dc1
```

The built-in presets and sample scenarios already carry `[[host]]` groups.

## Validation

Errors stop flowgen with exit code 2; warnings go to stderr and the run continues.

| Level | Condition | Message (excerpt) |
|-------|-----------|-------------------|
| error | no rules and no scans | `scenario has no [[rule]] and no [[scan]] entries` |
| error | tcp/udp rule without ports | `rule 'a': dst_ports must not be empty for tcp/udp` |
| error | weight 0 | `rule 'a': weight must be > 0` |
| error | multicast CIDR/IP (224.0.0.0/4) | `... overlaps multicast range 224.0.0.0/4, which Flowlink ignores` |
| error | bad packets/bytes range | `rule 'a': packets must be [min, max] with 1 <= min <= max` |
| error | bad port range | `scan #1: invalid port range '900-10' (expected 1 <= start <= end)` |
| error | `max_datagram` out of range | `target.max_datagram must be between 256 and 65000, got 100` |
| error | `amplitude`/`burst_chance` outside 0-1, `flows_per_sec` < 0 or > 1,000,000, sine `period_secs` <= 0, `burst_multiplier` < 1, `burst_secs` <= 0, zero `template_refresh_secs`/`stats_interval_secs`/`every_secs`/`flows_per_burst` | `rate.amplitude must be between 0 and 1, got 3` |
| error | unknown key, bad enum, bad CIDR/IP | TOML parse error with line and column |
| warning | `max_datagram` > 1472 | `... exceeds 1472 bytes; datagrams will be IP-fragmented, and macOS rejects UDP payloads above 9216 bytes` |
| warning | `flows_per_sec` > 5000 | `... Flowlink/PCE handle about 10K unique flows/s in total` |
| warning | `dst_ports` on an icmp rule | `rule 'a': dst_ports are ignored for icmp` |

Check a file without sending: `flowgen run -c my.toml --dry-run --duration 2s`.

## Worked example

Build a small shop, checking after each step with `flowgen run -c shop.toml --dry-run --duration 3s` and `flowgen ips -c shop.toml`.

**Step 1: one rule.** Users reach the web tier.

```toml
name = "my-shop"
[[rule]]
name = "users-to-web"
src = ["192.168.50.0/28"]      # 14 hosts
dst = ["10.50.1.8/29"]         # 6 hosts
proto = "tcp"
dst_ports = [443]
weight = 6
```

**Step 2: more rules.** Web to database with bigger flows, and DNS. Weights make users-to-web 60 % of the traffic (6 of 10).

```toml
[[rule]]
name = "web-to-db"
src = ["10.50.1.8/29"]
dst = ["10.50.2.10"]
proto = "tcp"
dst_ports = [5432]
weight = 3
packets = [10, 50]
bytes_per_packet = [200, 900]

[[rule]]
name = "dns"
src = ["10.50.1.8/29", "10.50.2.10"]
dst = ["10.50.0.53"]
proto = "udp"
dst_ports = [53]
weight = 1
```

**Step 3: rate and target.** Bursty traffic, 60 flows/s base, 4x for 15 s with 10 % chance per second; IPFIX to a collector (or leave `collector` out and use `--collector`).

```toml
seed = 7
[target]
protocol = "ipfix"
# collector = "flowlink.lab"
[rate]
flows_per_sec = 60
shape = "burst"
burst_chance = 0.1
burst_multiplier = 4
burst_secs = 15
```

**Step 4: a scan.** The web server 10.50.1.13 probes the database every 90 s.

```toml
[[scan]]
src = "10.50.1.13"
dst = ["10.50.2.10"]
ports = [22, 3306, 5432]
every_secs = 90
flows_per_burst = 100
```

`flowgen ips -c shop.toml` now lists 22 IPs: create them as unmanaged workloads (or add `--format workloader` for a labelled CSV) and run it.

## Tips

- Keep scenarios at or below roughly 60 hosts for PCE demos: every IP should become a workload, and small maps stay readable. Use `/28`-`/29` subnets or single IPs instead of `/24`.
- `flowgen ips` (plain and `--format workloader`) refuses more than 4096 hosts (`scenario covers up to N host IPs (limit 4096); pass --all to list them anyway`). `--all` lists everything, for example `flowgen ips -c examples/load-test.toml --all` prints 131,068 IPs.
- Use disjoint subnets per scenario if you want to run several at once.
- Do not use multicast ranges; Flowlink ignores them.
- Keep `flows_per_sec` modest (tens to hundreds). Warning above 5000.
- Use `seed` for repeatable rehearsals.
