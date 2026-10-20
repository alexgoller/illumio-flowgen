# Troubleshooting

## Nothing shows up in the PCE

Work from the source to the PCE. Stop at the first step that fails.

1. flowgen: status line shows `errs 0` and `pkts` growing. Try `--dry-run` to see the flows.
2. Network: UDP reaches the Flowlink host (`sudo tcpdump -i any -n udp port 4739` on the Flowlink host).
3. Flowlink: `illumio-flowlink-ctl start --config <file> --debug` prints incoming records.
4. PCE: workloads exist for the IPs (`flowgen ips --format workloader` + `workloader wkld-import --umwl`), and one aggregation interval has passed.

| Symptom | Cause | Fix |
|---------|-------|-----|
| `--debug` prints nothing | Firewall / security group drops UDP; wrong port or host | Open the UDP port; check `--collector` host:port. The default port is 2055 (v5/v9) or 4739 (ipfix). |
| `--debug` prints nothing, tcpdump on Flowlink host shows packets | Parser type mismatch: v5/v9 need `type: netflow`, IPFIX needs `type: ipfix`, on the port you send to. A wrong protocol is silently dropped. | Match `--protocol` to the consumer on that port (see [demo-guide](demo-guide.md#3-flowlink-listener)). |
| `--debug` prints nothing, packets arrive | `remote_addrs` is set and does not contain the exporter IP | Add the "exporter source IP" printed at flowgen start (no CIDRs), or remove `remote_addrs`. NAT changes the IP Flowlink sees. |
| Flowlink prints records, PCE shows nothing | No workload matches either IP of a flow | Create unmanaged workloads: `flowgen ips --preset <name> --format workloader > x.csv`, then `workloader wkld-import x.csv --umwl --update-pce --no-prompt` (or create them by hand from `flowgen ips --preset <name>`). Only one side of a flow must match. |
| Flowlink prints records, workloads exist, PCE still empty | Aggregation delay | Flowlink posts every `aggregation_minutes` (default 10, minimum 5). Wait one interval; keep flowgen running. |
| Flows were visible, then stop | flowgen ended (`--duration`) or send errors | Check the terminal / log for `send failed`. |
| Only some IPs appear | Overlapping scenarios, or sampling has not hit every host yet | Use disjoint subnets; run longer. |

## Workloader import

| Symptom | Cause | Fix |
|---------|-------|-----|
| `wkld-import` reports unknown label or label type | Label types (role/app/env/loc, or your custom keys) must already exist in the PCE; only label values are created | Create the label type in the PCE, or remove the key from the `[[host]]` labels. |
| Nothing changes in the PCE | Without `--update-pce` workloader only logs what it would do | Re-run with `--update-pce --no-prompt`. |
| Rows are skipped as unmatched | `--umwl` missing, so unmatched hostnames do not create workloads | Add `--umwl`. |
| Re-import updates existing workloads instead of creating new ones | Rows match on hostname (`{prefix}-a-b-c-d`, derived from the IP); empty CSV cells leave PCE values unchanged | Expected. Changing `hostname_prefix` changes the hostname, so the next import creates a new workload for that IP. |
| `flowgen ips --format workloader` shows empty label cells | No `[[host]]` group covers that IP (scan sources need one too) | Add a `[[host]]` group, see [scenarios.md](scenarios.md#host-labels). |

## Messages

`send failed: Connection refused (os error 61)` (error number differs per OS): the host answered with ICMP port unreachable, so nothing listens on that UDP port. Check the Flowlink consumer port and that Flowlink is running. flowgen keeps running; send errors are counted (`errs`, `send errors` in the summary) and logged at most once per `stats_interval_secs`. If a firewall silently drops UDP you see no error at all - that is the "nothing arrives" case above.

Exit code 2 means bad input (flowgen or CLI parser); the run never started. Real messages:

| Message | Fix |
|---------|-----|
| `flowgen: error: unknown preset 'nope' (available: three-tier, datacenter, scan-demo)` | `flowgen presets` |
| `flowgen: error: no collector: set [target].collector or pass --collector HOST[:PORT] (or use --dry-run)` | Add `--collector` or `--dry-run`. |
| `flowgen: error: cannot resolve collector 'nosuchhost.invalid': failed to lookup address information: ...` | Fix the hostname or use an IPv4 address. IPv6-only names are rejected (`has no IPv4 address`). |
| `flowgen: error: cannot read scenario file /nonexistent.toml: No such file or directory (os error 2)` | Check the `-c` path. |
| `error: the following required arguments were not provided: <--config <FILE>\|--preset <NAME>>` | Pass exactly one of `-c FILE` or `--preset NAME`. Both together: `the argument '--preset <NAME>' cannot be used with '--config <FILE>'`. |
| `error: invalid value 'abc' for '--duration <DURATION>': expected number at 0` | Use `30s`, `10m`, `1h`. |
| `unknown variant 'v7', expected one of 'v5', 'v9', 'ipfix'` (TOML parse error with line/column) | Fix the value. Same format for bad shapes and protos. |
| `invalid IPv4 CIDR '10.0.0.0/33': invalid IP address syntax` | Fix the CIDR. |
| `unknown field 'bogus', expected one of 'name', 'seed', ...` | Remove or fix the key; all structs reject unknown keys. |
| `rule 'a': dst_ports must not be empty for tcp/udp` | Add `dst_ports`. |
| `rule 'a': 239.1.1.0/24 overlaps multicast range 224.0.0.0/4, which Flowlink ignores` | Use unicast addresses. |
| `scenario has no [[rule]] and no [[scan]] entries` | Add a rule (an empty file also gives this). |
| `target.max_datagram must be between 256 and 65000, got 100` | Fix `max_datagram`. |
| `scenario covers up to 131070 host IPs (limit 4096); pass --all to list them anyway` (`flowgen ips`) | Add `--all`, or shrink the scenario. |

More messages and the full list of rules: [scenarios.md](scenarios.md#validation). Warnings (`flowgen: warning: ...`) do not stop the run.

## Output problems

| Symptom | Cause | Fix |
|---------|-------|-----|
| Progress bar garbles a log file or terminal | Live bar redraws stderr | `--no-progress` for one stats line per `stats_interval_secs`, or `--quiet` for the summary only. flowgen already falls back to plain mode when stderr is not a terminal, or on `--dry-run` printing to a terminal. |
| Flow lines mixed with headers | Headers/stats go to stderr, flows to stdout | `flowgen run ... --dry-run 2>/dev/null` or `>flows.txt`. |
| `flowgen ... --dry-run \| head` ends quietly | Closed pipe stops the run | Expected. |
| No scan notice | First burst comes `every_secs` after start | Wait (60 s for `scan-demo`). |

## Large datagrams on macOS

`target.max_datagram` above 1472 triggers a warning (IP fragmentation). macOS refuses UDP payloads above 9216 bytes: `send failed: Message too long (os error 40)`. Lower `max_datagram` (default 1400).

## Stopping flowgen

Ctrl-C (SIGINT) and `kill` (SIGTERM) both stop gracefully: the run loop exits, the final summary (flows, packets, bytes, send errors, per-rule table) is printed and the exit code is 0. Send errors do not change the exit code. `kill -9` skips the summary. With `--duration` flowgen stops by itself.
