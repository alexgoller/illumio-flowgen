# flowgen documentation

flowgen generates NetFlow v5/v9 and IPFIX traffic for testing and demoing Illumio Flowlink.

| Guide | Read it when |
|-------|--------------|
| [quickstart.md](quickstart.md) | You want flows in a PCE in 5 minutes: install, dry-run, send, verify. |
| [demo-guide.md](demo-guide.md) | You are preparing a customer demo: pick a scenario, create workloads, time it, tell the story. |
| [scenarios.md](scenarios.md) | You want to write your own scenario TOML: every key, default, rule and validation message. |
| [protocols.md](protocols.md) | You need to know what is on the wire: field lists, templates, sequence numbers, datagram sizes. |
| [troubleshooting.md](troubleshooting.md) | Something does not work: symptom, cause, fix. |

Other references:

- [`../README.md`](../README.md) - install options, PCE visibility checklist, short reference.
- [`../examples/scenario.toml`](../examples/scenario.toml) - annotated scenario with every option.
- `flowgen --help`, `flowgen run --help` - all flags.

Command cheat sheet:

```sh
flowgen presets                                   # list built-in scenarios
flowgen presets --show three-tier                 # print a preset as TOML
flowgen ips --preset three-tier                   # IPs to create as unmanaged workloads
flowgen run --preset three-tier --dry-run --duration 10s            # print flows, send nothing
flowgen run --preset three-tier --collector flowlink.lab            # IPFIX to port 4739
flowgen run -c examples/web-shop.toml --collector 10.0.0.5 --protocol v9   # NetFlow v9 to port 2055
```
