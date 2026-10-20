//! NetFlow v9 (RFC 3954): one template (ID 256) + data FlowSets padded to 32 bits.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    Encoder, EncoderOptions, TEMPLATE_ID, TemplateTimer, put_u8, put_u16, put_u32, put_u64,
    uptime_ms,
};
use crate::model::FlowRecord;

const HEADER_LEN: usize = 20;
/// (field type, length): IN_BYTES, IN_PKTS, PROTOCOL, TCP_FLAGS, L4_SRC_PORT,
/// IPV4_SRC_ADDR, L4_DST_PORT, IPV4_DST_ADDR, FIRST_SWITCHED, LAST_SWITCHED.
const FIELDS: [(u16, u16); 10] = [
    (1, 8),
    (2, 8),
    (4, 1),
    (6, 1),
    (7, 2),
    (8, 4),
    (11, 2),
    (12, 4),
    (22, 4),
    (21, 4),
];
const RECORD_LEN: usize = 38;
const TEMPLATE_SET_LEN: usize = 4 + 4 + FIELDS.len() * 4;
const SET_HEADER_LEN: usize = 4;
const MAX_PADDING: usize = 3;

pub struct V9Encoder {
    boot: SystemTime,
    source_id: u32,
    max_datagram: usize,
    sequence: u32,
    templates: TemplateTimer,
}

impl V9Encoder {
    pub fn new(boot: SystemTime, opts: &EncoderOptions) -> Self {
        Self {
            boot,
            source_id: opts.observation_domain,
            max_datagram: opts.max_datagram,
            sequence: 0,
            templates: TemplateTimer::new(opts.template_refresh),
        }
    }
}

impl Encoder for V9Encoder {
    fn encode(&mut self, records: &[FlowRecord], now: SystemTime) -> Vec<Vec<u8>> {
        let epoch_secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
        let mut out = Vec::new();
        let mut rest = records;
        while !rest.is_empty() {
            let with_template = self.templates.due(now);
            let overhead = HEADER_LEN
                + if with_template { TEMPLATE_SET_LEN } else { 0 }
                + SET_HEADER_LEN
                + MAX_PADDING;
            let n = (self.max_datagram.saturating_sub(overhead) / RECORD_LEN)
                .max(1)
                .min(rest.len());
            let (chunk, tail) = rest.split_at(n);
            rest = tail;

            let mut buf = Vec::with_capacity(self.max_datagram);
            put_u16(&mut buf, 9);
            put_u16(&mut buf, (n + usize::from(with_template)) as u16);
            put_u32(&mut buf, uptime_ms(self.boot, now));
            put_u32(&mut buf, epoch_secs);
            put_u32(&mut buf, self.sequence);
            put_u32(&mut buf, self.source_id);

            if with_template {
                put_u16(&mut buf, 0); // template FlowSet id
                put_u16(&mut buf, TEMPLATE_SET_LEN as u16);
                put_u16(&mut buf, TEMPLATE_ID);
                put_u16(&mut buf, FIELDS.len() as u16);
                for (ty, len) in FIELDS {
                    put_u16(&mut buf, ty);
                    put_u16(&mut buf, len);
                }
            }

            let set_len = SET_HEADER_LEN + n * RECORD_LEN;
            let padded = set_len.div_ceil(4) * 4;
            put_u16(&mut buf, TEMPLATE_ID);
            put_u16(&mut buf, padded as u16);
            for r in chunk {
                put_u64(&mut buf, r.bytes);
                put_u64(&mut buf, r.packets);
                put_u8(&mut buf, r.protocol);
                put_u8(&mut buf, r.tcp_flags);
                put_u16(&mut buf, r.src_port);
                put_u32(&mut buf, r.src_addr.into());
                put_u16(&mut buf, r.dst_port);
                put_u32(&mut buf, r.dst_addr.into());
                put_u32(&mut buf, uptime_ms(self.boot, r.start));
                put_u32(&mut buf, uptime_ms(self.boot, r.end));
            }
            buf.resize(buf.len() + (padded - set_len), 0);

            self.sequence = self.sequence.wrapping_add(1);
            out.push(buf);
        }
        out
    }
}
