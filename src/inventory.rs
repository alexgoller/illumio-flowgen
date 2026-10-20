//! Resolves an IP to its Illumio labels and hostname from `[[host]]` groups.

use std::collections::BTreeMap;
use std::net::Ipv4Addr;

use crate::config::HostGroup;

pub const DEFAULT_HOSTNAME_PREFIX: &str = "fg";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub ip: Ipv4Addr,
    /// `{prefix}-{a}-{b}-{c}-{d}`
    pub hostname: String,
    pub labels: BTreeMap<String, String>,
}

/// Merge every group containing `ip`; the most specific net wins per label key
/// and for the hostname prefix, and at equal specificity the later group wins.
pub fn resolve(groups: &[HostGroup], ip: Ipv4Addr) -> HostInfo {
    let mut matches: Vec<(u8, usize, &HostGroup)> = groups
        .iter()
        .enumerate()
        .filter_map(|(i, g)| {
            g.nets
                .iter()
                .filter(|n| n.contains(&ip))
                .map(|n| n.prefix_len())
                .max()
                .map(|spec| (spec, i, g))
        })
        .collect();
    matches.sort_by_key(|&(spec, i, _)| (spec, i));

    let mut labels = BTreeMap::new();
    let mut prefix = DEFAULT_HOSTNAME_PREFIX;
    for (_, _, g) in matches {
        labels.extend(g.labels.iter().map(|(k, v)| (k.clone(), v.clone())));
        if let Some(p) = &g.hostname_prefix {
            prefix = p;
        }
    }
    let o = ip.octets();
    HostInfo {
        ip,
        hostname: format!("{prefix}-{}-{}-{}-{}", o[0], o[1], o[2], o[3]),
        labels,
    }
}
