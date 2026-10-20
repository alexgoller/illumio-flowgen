# What is on the wire

All exports are UDP, IPv4 only, one flow per record. Port defaults: 2055 for v5/v9, 4739 for IPFIX.

## Common behaviour

- flowgen ticks every 100 ms: it generates the flows due in that tick (plus any scan burst), encodes them into as few datagrams as fit `max_datagram`, and sends each datagram.
- Timestamps are wall clock: a flow ends "now" (export time) and starts 0.1-30 s earlier. Scan flows have start = end = now.
- Counters are 64-bit in v9/IPFIX; v5 counters are 32-bit and saturate.
- No sampling, no interface indexes, no IPv6, no ICMP type/code (icmp flows have ports 0).

## NetFlow v5

Header (24 bytes): version 5, count, sysUptime, unix secs, unix nsecs, `flow_sequence`, engine type 0, engine id 0, sampling 0. `flow_sequence` is the cumulative number of flows sent before this packet.

Record (48 bytes): srcaddr, dstaddr, nexthop (0), input/output ifindex (0), dPkts, dOctets, first, last, srcport, dstport, tcp_flags, prot, tos (0), src/dst AS (0), src/dst mask (0).

- Up to 30 records per packet, limited by `max_datagram`: `(max_datagram - 24) / 48` records. Default 1400 gives 28 records (1368 bytes).
- `first`/`last` are sysUptime-relative milliseconds. flowgen pretends the exporter booted one hour before it started, so values never underflow.
- No template, no `observation_domain`.

## NetFlow v9

Header (20 bytes): version 9, count, sysUptime, unix secs, `sequence`, source ID (= `observation_domain`, default 1). `count` includes the template record when present. `sequence` is the packet count (+1 per datagram).

Template FlowSet (id 0) defines **template ID 256** with 10 fields:

| Field | Type | Length |
|-------|------|--------|
| IN_BYTES | 1 | 8 |
| IN_PKTS | 2 | 8 |
| PROTOCOL | 4 | 1 |
| TCP_FLAGS | 6 | 1 |
| L4_SRC_PORT | 7 | 2 |
| IPV4_SRC_ADDR | 8 | 4 |
| L4_DST_PORT | 11 | 2 |
| IPV4_DST_ADDR | 12 | 4 |
| FIRST_SWITCHED | 22 | 4 |
| LAST_SWITCHED | 21 | 4 |

Data FlowSet id 256, 38-byte records padded to a 32-bit boundary. FIRST/LAST_SWITCHED are uptime-relative like v5.

## IPFIX

Header (16 bytes): version 10, message length, export time (secs), `sequence`, observation domain ID (= `observation_domain`). `sequence` is the cumulative number of data records sent before this message.

Template Set (set id 2) defines **template ID 256** with 10 fields:

| Information element | ID | Length |
|---------------------|----|--------|
| octetDeltaCount | 1 | 8 |
| packetDeltaCount | 2 | 8 |
| protocolIdentifier | 4 | 1 |
| tcpControlBits | 6 | 2 |
| sourceTransportPort | 7 | 2 |
| sourceIPv4Address | 8 | 4 |
| destinationTransportPort | 11 | 2 |
| destinationIPv4Address | 12 | 4 |
| flowStartMilliseconds | 152 | 8 |
| flowEndMilliseconds | 153 | 8 |

Data Set id 256, 47-byte records (absolute Unix milliseconds, no padding).

## Template refresh

v9 and IPFIX packets carry the template in the first packet, then again whenever `template_refresh_secs` (default 30) have passed or 100 packets have been sent since the last one, whichever comes first (also if the clock jumps backwards). Verified on the wire: the first v9 datagram has `count` = records + 1 and a flowset id 0 followed by template 256 with 10 fields; the first IPFIX datagram starts with set id 2, template 256, 10 fields.

## Datagram size

`max_datagram` (default 1400, allowed 256-65000) caps the UDP payload.

- 1472 = 1500 MTU - 20 IP - 8 UDP. Above that, datagrams are IP-fragmented and flowgen warns.
- macOS rejects UDP payloads above 9216 bytes (sends fail with `send failed: Message too long (os error 40)`; `sysctl net.inet.udp.maxdgram` shows 9216).
- Larger datagrams mean fewer packets; they rarely matter for demos. Keep the default.
- v5 never exceeds 30 records (1464 bytes) no matter how large `max_datagram` is.

## Exporter identity and Flowlink `remote_addrs`

flowgen binds an ephemeral UDP port on `0.0.0.0` and connects to the collector, so the exporter source IP is whichever local address the OS routes to the collector. It is printed at start:

```
exporter source IP: 10.0.0.23 (add it to Flowlink remote_addrs if you restrict senders)
```

If Flowlink sets `remote_addrs` (exact IPs, no CIDRs), that address must be listed or flows are dropped. NAT between flowgen and Flowlink changes the address Flowlink sees. Dry-run has no exporter IP. Source-IP spoofing is not supported. Run several instances from the same host and they share one exporter IP; use different `observation_domain` values if you want to tell them apart.

## Inspecting traffic

Print flows without sending (stdout; headers go to stderr):

```sh
flowgen run --preset three-tier --dry-run --duration 5s
flowgen run --preset datacenter --dry-run 2>/dev/null | head
```

Line format: `proto src:port -> dst:port pkts=N bytes=N`.

Capture the real datagrams (needs root; use the port you send to):

```sh
sudo tcpdump -i any -n udp port 4739              # IPFIX
sudo tcpdump -i any -n udp port 2055 -X           # v5/v9 with hex dump
sudo tcpdump -i lo0 -n udp port 4739              # macOS, flowgen -> 127.0.0.1
```

Wireshark decodes NetFlow/IPFIX (protocol `CFLOW`); for a non-standard port use Analyze -> Decode As. It needs the template packet to decode v9/IPFIX data, so start the capture before flowgen or wait up to 30 s for the next template.

Sizes seen on loopback with `--preset three-tier`: v5 264 bytes (24 + 5 x 48); v9 264 bytes with template, 216 without (5 records); IPFIX 303 bytes with template (16 + 48 template set + 4 + 5 x 47), 255 without.
