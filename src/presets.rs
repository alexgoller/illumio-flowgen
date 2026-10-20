//! Built-in scenarios compiled into the binary.

pub struct Preset {
    pub name: &'static str,
    pub description: &'static str,
    pub toml: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "three-tier",
        description: "clients -> web -> app -> db plus DNS/NTP, sine-wave rate (46 hosts)",
        toml: include_str!("../presets/three-tier.toml"),
    },
    Preset {
        name: "datacenter",
        description: "four /28 app subnets with https, mssql, kafka, ldap, smb, ssh (56 hosts)",
        toml: include_str!("../presets/datacenter.toml"),
    },
    Preset {
        name: "scan-demo",
        description: "three-tier plus a web server port-scanning the app tier every 60 s",
        toml: include_str!("../presets/scan-demo.toml"),
    },
];

pub fn find(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.name == name)
}
