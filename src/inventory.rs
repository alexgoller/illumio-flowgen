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

const STANDARD_LABELS: [&str; 4] = ["role", "app", "env", "loc"];

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Write the CSV consumed by `workloader wkld-import <file> --umwl`.
pub fn write_workloader_csv<W: std::io::Write>(
    w: &mut W,
    description: &str,
    hosts: &[HostInfo],
) -> std::io::Result<()> {
    let mut columns: Vec<&str> = STANDARD_LABELS.to_vec();
    let extra: std::collections::BTreeSet<&str> = hosts
        .iter()
        .flat_map(|h| h.labels.keys().map(String::as_str))
        .filter(|k| !STANDARD_LABELS.contains(k))
        .collect();
    columns.extend(extra);

    let mut header = vec!["hostname", "name", "interfaces", "description"];
    header.extend(&columns);
    let header: Vec<String> = header.into_iter().map(csv_field).collect();
    writeln!(w, "{}", header.join(","))?;

    for h in hosts {
        let mut row = vec![
            csv_field(&h.hostname),
            csv_field(&h.hostname),
            h.ip.to_string(),
            csv_field(description),
        ];
        row.extend(
            columns
                .iter()
                .map(|c| csv_field(h.labels.get(*c).map_or("", String::as_str))),
        );
        writeln!(w, "{}", row.join(","))?;
    }
    Ok(())
}
