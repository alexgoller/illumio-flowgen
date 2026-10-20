//! IPFIX (RFC 7011): template set (ID 2) with template 256 + data sets.

use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    Encoder, EncoderOptions, TEMPLATE_ID, TemplateTimer, put_u8, put_u16, put_u32, put_u64,
};
use crate::model::{FlowRecord, unix_millis};

const HEADER_LEN: usize = 16;
const TEMPLATE_SET_ID: u16 = 2;
/// (IE id, length): octetDeltaCount, packetDeltaCount, protocolIdentifier,
/// tcpControlBits, sourceTransportPort, sourceIPv4Address,
/// destinationTransportPort, destinationIPv4Address,
/// flowStartMilliseconds, flowEndMilliseconds.
const FIELDS: [(u16, u16); 10] = [
    (1, 8),
    (2, 8),
    (4, 1),
    (6, 2),
    (7, 2),
    (8, 4),
    (11, 2),
    (12, 4),
    (152, 8),
    (153, 8),
];
const RECORD_LEN: usize = 47;
const TEMPLATE_SET_LEN: usize = 4 + 4 + FIELDS.len() * 4;
const SET_HEADER_LEN: usize = 4;

pub struct IpfixEncoder {
    domain: u32,
    max_datagram: usize,
    sequence: u32,
    templates: TemplateTimer,
}

impl IpfixEncoder {
    pub fn new(opts: &EncoderOptions) -> Self {
        Self {
            domain: opts.observation_domain,
            max_datagram: opts.max_datagram,
            sequence: 0,
            templates: TemplateTimer::new(opts.template_refresh),
        }
    }
}

impl Encoder for IpfixEncoder {
    fn encode(&mut self, records: &[FlowRecord], now: SystemTime) -> Vec<Vec<u8>> {
        let export_secs = now.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as u32;
        let mut out = Vec::new();
        let mut rest = records;
        while !rest.is_empty() {
            let with_template = self.templates.due(now);
            let overhead =
                HEADER_LEN + if with_template { TEMPLATE_SET_LEN } else { 0 } + SET_HEADER_LEN;
            let n = (self.max_datagram.saturating_sub(overhead) / RECORD_LEN)
                .max(1)
                .min(rest.len());
            let (chunk, tail) = rest.split_at(n);
            rest = tail;

            let mut buf = Vec::with_capacity(self.max_datagram);
            put_u16(&mut buf, 10);
            put_u16(&mut buf, 0); // message length, patched below
            put_u32(&mut buf, export_secs);
            put_u32(&mut buf, self.sequence);
            put_u32(&mut buf, self.domain);

            if with_template {
                put_u16(&mut buf, TEMPLATE_SET_ID);
                put_u16(&mut buf, TEMPLATE_SET_LEN as u16);
                put_u16(&mut buf, TEMPLATE_ID);
                put_u16(&mut buf, FIELDS.len() as u16);
                for (ie, len) in FIELDS {
                    put_u16(&mut buf, ie);
                    put_u16(&mut buf, len);
                }
            }

            put_u16(&mut buf, TEMPLATE_ID);
            put_u16(&mut buf, (SET_HEADER_LEN + n * RECORD_LEN) as u16);
            for r in chunk {
                put_u64(&mut buf, r.bytes);
                put_u64(&mut buf, r.packets);
                put_u8(&mut buf, r.protocol);
                put_u16(&mut buf, u16::from(r.tcp_flags));
                put_u16(&mut buf, r.src_port);
                put_u32(&mut buf, r.src_addr.into());
                put_u16(&mut buf, r.dst_port);
                put_u32(&mut buf, r.dst_addr.into());
                put_u64(&mut buf, unix_millis(r.start));
                put_u64(&mut buf, unix_millis(r.end));
            }

            let len = buf.len() as u16;
            buf[2..4].copy_from_slice(&len.to_be_bytes());
            self.sequence = self.sequence.wrapping_add(n as u32);
            out.push(buf);
        }
        out
    }
}
