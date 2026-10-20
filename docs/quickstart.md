# Quickstart (5 minutes)

## 1. Install

Pick one. `flowgen --version` should print `flowgen 0.1.0 (<hash>)` afterwards.

```sh
# Script (Linux/macOS), installs to /usr/local/bin
curl -fsSL https://github.com/alexgoller/illumio-flowgen/releases/latest/download/install.sh | sh
# ... or into your home directory
curl -fsSL https://github.com/alexgoller/illumio-flowgen/releases/latest/download/install.sh | sh -s -- --prefix ~/.local/bin

sudo dpkg -i flowgen_<ver>_amd64.deb        # Debian/Ubuntu
sudo rpm -i flowgen-<ver>-1.x86_64.rpm      # RHEL/Rocky
# macOS: double-click flowgen-<ver>.pkg (unsigned: right-click -> Open)
```

Sample scenarios land in `/usr/local/share/flowgen/examples/` (script), `/usr/share/flowgen/examples/` (deb/rpm). The macOS `.pkg` installs the binary only; use the tarball or the repo for samples. Tarball users on macOS: `xattr -d com.apple.quarantine <path>/flowgen`.

Private repo? `gh release download --repo alexgoller/illumio-flowgen --dir flowgen-dist && sh flowgen-dist/install.sh --from flowgen-dist`.

## 2. First dry-run (sends nothing)

```sh
flowgen presets
flowgen run --preset three-tier --dry-run --duration 5s
```

Output: header lines on stderr, one line per flow on stdout:

```
flowgen IPFIX -> dry-run (stdout) | scenario: three-tier | seed: 1
dry run: flows are printed to stdout, nothing is sent
tcp 10.1.2.9:55485 -> 10.1.3.10:5432 pkts=106 bytes=61056
udp 10.1.1.12:55129 -> 10.1.0.11:123 pkts=14 bytes=7672
```

Add `--seed 42` to get the same traffic every time.

## 3. Prepare the Flowlink listener

IPFIX needs an `ipfix` parser, NetFlow v5/v9 a `netflow` parser, on the port you send to:

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

If your Flowlink restricts senders with `remote_addrs`, add the "exporter source IP" that flowgen prints at start. More in [demo-guide.md](demo-guide.md#3-flowlink-listener).

## 4. Send

```sh
flowgen run --preset three-tier --collector flowlink.lab                    # IPFIX -> :4739
flowgen run --preset three-tier --collector 10.0.0.5 --protocol v9          # NetFlow v9 -> :2055
flowgen run --preset three-tier --collector 10.0.0.5:6500 --duration 10m    # explicit port
```

Without `--duration` it runs until Ctrl-C. The terminal shows a progress bar and a live line such as `rate 212/s (target 200)  flows 12,345  pkts 1,020  errs 0  scans 1`. `errs` must stay 0.

## 5. Check it arrived at Flowlink

On the Flowlink host, run it with `--debug` to print incoming records:

```sh
illumio-flowlink-ctl start --config <file> --debug
```

You should see records with the preset's IPs (10.1.x.x, 192.168.10.x). Nothing printed? See [troubleshooting.md](troubleshooting.md).

## 6. See it in the PCE

1. Create unmanaged workloads for the IPs: `flowgen ips --preset three-tier` (46 IPs).
   The PCE shows a Flowlink flow only if at least one IP of the flow is a managed or unmanaged workload.
2. Wait. Flowlink posts to the PCE every `aggregation_minutes` (default 10, minimum 5). Expect the first flows in Illumination / Explorer after up to one interval.
3. Open Explorer (or Illumination) and filter on the workloads you created.

Next: [demo-guide.md](demo-guide.md).
