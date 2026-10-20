//! NetFlow v5: fixed 24-byte header + up to 30 fixed 48-byte records.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{Encoder, put_u8, put_u16, put_u32, sat_u32, uptime_ms};
use crate::model::FlowRecord;

const HEADER_LEN: usize = 24;
const RECORD_LEN: usize = 48;
const MAX_RECORDS: usize = 30;

pub struct V5Encoder {
    boot: SystemTime,
    max_datagram: usize,
    flow_sequence: u32,
}

impl V5Encoder {
    pub fn new(boot: SystemTime, max_datagram: usize) -> Self {
        Self {
            boot,
            max_datagram,
            flow_sequence: 0,
        }
    }
}

impl Encoder for V5Encoder {
    fn encode(&mut self, records: &[FlowRecord], now: SystemTime) -> Vec<Vec<u8>> {
        let per_packet =
            (self.max_datagram.saturating_sub(HEADER_LEN) / RECORD_LEN).clamp(1, MAX_RECORDS);
        let epoch = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let mut out = Vec::new();
        for chunk in records.chunks(per_packet) {
            let mut buf = Vec::with_capacity(HEADER_LEN + chunk.len() * RECORD_LEN);
            put_u16(&mut buf, 5);
            put_u16(&mut buf, chunk.len() as u16);
            put_u32(&mut buf, uptime_ms(self.boot, now));
            put_u32(&mut buf, epoch.as_secs() as u32);
            put_u32(&mut buf, epoch.subsec_nanos());
            put_u32(&mut buf, self.flow_sequence);
            put_u8(&mut buf, 0); // engine type
            put_u8(&mut buf, 0); // engine id
            put_u16(&mut buf, 0); // sampling interval
            for r in chunk {
                put_u32(&mut buf, r.src_addr.into());
                put_u32(&mut buf, r.dst_addr.into());
                put_u32(&mut buf, 0); // next hop
                put_u16(&mut buf, 0); // input ifindex
                put_u16(&mut buf, 0); // output ifindex
                put_u32(&mut buf, sat_u32(r.packets));
                put_u32(&mut buf, sat_u32(r.bytes));
                put_u32(&mut buf, uptime_ms(self.boot, r.start));
                put_u32(&mut buf, uptime_ms(self.boot, r.end));
                put_u16(&mut buf, r.src_port);
                put_u16(&mut buf, r.dst_port);
                put_u8(&mut buf, 0); // pad1
                put_u8(&mut buf, r.tcp_flags);
                put_u8(&mut buf, r.protocol);
                put_u8(&mut buf, 0); // tos
                put_u16(&mut buf, 0); // src as
                put_u16(&mut buf, 0); // dst as
                put_u8(&mut buf, 0); // src mask
                put_u8(&mut buf, 0); // dst mask
                put_u16(&mut buf, 0); // pad2
            }
            self.flow_sequence = self.flow_sequence.wrapping_add(chunk.len() as u32);
            out.push(buf);
        }
        out
    }
}
