use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::sync::OnceLock;

/// Path consulted for the OS-provided port → service-name table.
const ETC_SERVICES_PATH: &str = "/etc/services";

/// TCP protocol marker used in `/etc/services` entries (`name  port/proto`).
const TCP_PROTOCOL: &str = "tcp";

/// Byte-unit thresholds for `format_memory`.
const BYTES_PER_KB: u64 = 1024;
const BYTES_PER_MB: u64 = BYTES_PER_KB * 1024;
const BYTES_PER_GB: u64 = BYTES_PER_MB * 1024;

/// Time-unit thresholds for `format_age`.
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * 60;
const SECONDS_PER_DAY: u64 = SECONDS_PER_HOUR * 24;

/// Bind addresses that mean "only reachable from this machine".
const LOOPBACK_ADDRS: &[&str] = &["127.0.0.1", "::1"];
/// Bind addresses that mean "reachable from any interface" (incl. LAN).
const ALL_INTERFACES_ADDRS: &[&str] = &["0.0.0.0", "::", "*"];

/// Short exposure labels, shared by the table column and the detail panel.
pub const EXPOSURE_LABEL_LOOPBACK: &str = "local";
pub const EXPOSURE_LABEL_ALL_INTERFACES: &str = "LAN";
pub const EXPOSURE_LABEL_SPECIFIC: &str = "specific";

/// Detail-panel placeholders for data that isn't always available.
pub const NO_CWD_PLACEHOLDER: &str = "-";
pub const NO_PARENT_PLACEHOLDER: &str = "-";

/// Dev-tool ports that are commonly used but not present in `/etc/services`.
/// This is the one place to add a new well-known dev-server port.
const DEV_TOOL_PORTS: &[(u16, &str)] = &[
    (3000, "Dev Server"),
    (3001, "Dev Server"),
    (3002, "Dev Server"),
    (4200, "Angular Dev"),
    (4321, "Astro Dev"),
    (5173, "Vite Dev"),
    (5174, "Vite Dev"),
    (5500, "Live Server"),
    (5555, "ADB / Dev"),
    (8888, "Jupyter"),
    (9229, "Node.js Debug"),
    (24678, "Vite HMR"),
];

/// Command-name substrings used as a last-resort description heuristic.
const COMMAND_HEURISTICS: &[(&str, &str)] = &[
    ("node", "Node.js Process"),
    ("python", "Python Process"),
    ("ruby", "Ruby Process"),
    ("java", "Java Process"),
    ("nginx", "Nginx"),
    ("apache", "Apache HTTP"),
    ("httpd", "Apache HTTP"),
    ("docker", "Docker"),
    ("code", "VS Code"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Low => write!(f, "LOW"),
            Severity::Medium => write!(f, "MEDIUM"),
            Severity::High => write!(f, "HIGH"),
            Severity::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Who can reach this port, derived from the bind address. Ordered least to
/// most exposed so it sorts sensibly like `Severity` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Exposure {
    Loopback,
    Specific,
    AllInterfaces,
}

impl fmt::Display for Exposure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exposure::Loopback => write!(f, "{EXPOSURE_LABEL_LOOPBACK}"),
            Exposure::Specific => write!(f, "{EXPOSURE_LABEL_SPECIFIC}"),
            Exposure::AllInterfaces => write!(f, "{EXPOSURE_LABEL_ALL_INTERFACES}"),
        }
    }
}

/// Classifies a bind address (e.g. `127.0.0.1`, `0.0.0.0`, `*`, `[::1]`) into
/// an `Exposure`. Strips IPv6 brackets before matching. Pure, unit-tested.
pub fn classify_exposure(addr: &str) -> Exposure {
    let addr = addr
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(addr);

    if LOOPBACK_ADDRS.contains(&addr) {
        Exposure::Loopback
    } else if ALL_INTERFACES_ADDRS.contains(&addr) {
        Exposure::AllInterfaces
    } else {
        Exposure::Specific
    }
}

#[derive(Debug, Clone)]
pub struct PortEntry {
    pub port: u16,
    pub pid: u32,
    pub command: String,
    pub severity: Severity,
    pub description: String,
    pub known: bool,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    /// Raw bind address as reported by `lsof`/`ss` (e.g. `127.0.0.1`, `*`).
    pub bind_addr: String,
    pub exposure: Exposure,
    /// Full command line, joined with spaces. Falls back to `command` when
    /// the OS didn't hand back argv (permissions, zombie process, etc).
    pub cmdline: String,
    /// Working directory, when readable.
    pub cwd: Option<String>,
    /// Seconds the process has been running.
    pub age_seconds: u64,
    /// `(pid, name)` of the parent process, when known.
    pub parent: Option<(u32, String)>,
}

impl PortEntry {
    /// Builds an entry from scan data alone. CPU/memory/cmdline/cwd/age/parent
    /// default to empty; `system.rs` (the I/O edge) enriches them from a live
    /// `sysinfo::System`.
    pub fn new(port: u16, pid: u32, command: String, bind_addr: String) -> Self {
        let severity = classify_severity(port);
        let description = describe_port(port, &command, services());
        let known = is_known_port(port, services());
        let exposure = classify_exposure(&bind_addr);
        Self {
            port,
            pid,
            command,
            severity,
            description,
            known,
            cpu_percent: 0.0,
            memory_bytes: 0,
            bind_addr,
            exposure,
            cmdline: String::new(),
            cwd: None,
            age_seconds: 0,
            parent: None,
        }
    }
}

/// A port is "known" when it resolves to a real recognized service — either a
/// curated dev-tool port or an `/etc/services` entry. Command-heuristic and
/// raw-command fallbacks do NOT count as known.
pub fn is_known_port(port: u16, services: &HashMap<u16, String>) -> bool {
    DEV_TOOL_PORTS
        .iter()
        .any(|(candidate, _)| *candidate == port)
        || services.contains_key(&port)
}

pub fn classify_severity(port: u16) -> Severity {
    const HIGH_PORTS: &[u16] = &[
        3306,  // MySQL
        5432,  // PostgreSQL
        6379,  // Redis
        27017, // MongoDB
        5672,  // RabbitMQ
        9200,  // Elasticsearch
        2181,  // ZooKeeper
        9092,  // Kafka
        1433,  // MSSQL (above 1023 but still a DB)
        26257, // CockroachDB
        8529,  // ArangoDB
        7474,  // Neo4j
        11211, // Memcached
    ];

    const MEDIUM_PORTS: &[u16] = &[
        3000, 3001, 3002, 4000, 4200, 4321, 5000, 5173, 5174, 5500, 5555, 8000, 8001, 8080, 8081,
        8443, 8888, 9000, 9090, 9229, 9999, 24678,
    ];

    if port <= 1023 {
        Severity::Critical
    } else if HIGH_PORTS.contains(&port) {
        Severity::High
    } else if MEDIUM_PORTS.contains(&port) {
        Severity::Medium
    } else {
        Severity::Low
    }
}

/// Pure description resolver: curated dev-tool ports, then `/etc/services`,
/// then a command-name heuristic, then the raw command as a last resort.
pub fn describe_port(port: u16, command: &str, services: &HashMap<u16, String>) -> String {
    if let Some((_, label)) = DEV_TOOL_PORTS
        .iter()
        .find(|(candidate, _)| *candidate == port)
    {
        return (*label).to_string();
    }

    if let Some(name) = services.get(&port) {
        return name.clone();
    }

    let command_lower = command.to_lowercase();
    for (needle, label) in COMMAND_HEURISTICS {
        if command_lower.contains(needle) {
            return (*label).to_string();
        }
    }

    command.to_string()
}

/// Parses the tcp entries of an `/etc/services`-formatted file.
/// Line format: `name  port/proto  aliases...  # comment`.
pub fn parse_services(contents: &str) -> HashMap<u16, String> {
    let mut services = HashMap::new();

    for line in contents.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else {
            continue;
        };
        let Some(port_and_proto) = fields.next() else {
            continue;
        };
        let Some((port_str, proto)) = port_and_proto.split_once('/') else {
            continue;
        };
        if proto != TCP_PROTOCOL {
            continue;
        }
        let Ok(port) = port_str.parse::<u16>() else {
            continue;
        };

        services.entry(port).or_insert_with(|| name.to_string());
    }

    services
}

/// Thin I/O wrapper around `parse_services`: reads `/etc/services` once and
/// caches the result for the process lifetime.
fn services() -> &'static HashMap<u16, String> {
    static SERVICES: OnceLock<HashMap<u16, String>> = OnceLock::new();
    SERVICES.get_or_init(|| {
        fs::read_to_string(ETC_SERVICES_PATH)
            .map(|contents| parse_services(&contents))
            .unwrap_or_default()
    })
}

/// Formats a byte count as a short human-readable string, e.g. `"125 MB"`.
pub fn format_memory(bytes: u64) -> String {
    if bytes >= BYTES_PER_GB {
        format!("{:.1} GB", bytes as f64 / BYTES_PER_GB as f64)
    } else if bytes >= BYTES_PER_MB {
        format!("{} MB", bytes / BYTES_PER_MB)
    } else if bytes >= BYTES_PER_KB {
        format!("{} KB", bytes / BYTES_PER_KB)
    } else {
        format!("{bytes} B")
    }
}

/// Rounds a `used/total` byte ratio to an integer percent clamped to `0..=100`.
/// `total == 0` yields `0` (guards against an uninitialized `System`).
pub fn memory_percent(used: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    ((used as f64 / total as f64) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u64
}

/// Formats a process-uptime duration as a short human-readable string,
/// e.g. `"2d 3h"`, `"45m"`, `"30s"`.
pub fn format_age(seconds: u64) -> String {
    if seconds >= SECONDS_PER_DAY {
        let days = seconds / SECONDS_PER_DAY;
        let hours = (seconds % SECONDS_PER_DAY) / SECONDS_PER_HOUR;
        format!("{days}d {hours}h")
    } else if seconds >= SECONDS_PER_HOUR {
        let hours = seconds / SECONDS_PER_HOUR;
        let minutes = (seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
        format!("{hours}h {minutes}m")
    } else if seconds >= SECONDS_PER_MINUTE {
        format!("{}m", seconds / SECONDS_PER_MINUTE)
    } else {
        format!("{seconds}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_services_reads_tcp_entries_only() {
        let contents = "\
ssh   22/tcp
ssh   22/udp
http  80/tcp   www www-http  # World Wide Web
# a full comment line
malformed-line
custom 5999/tcp
";
        let services = parse_services(contents);
        assert_eq!(services.get(&22), Some(&"ssh".to_string()));
        assert_eq!(services.get(&80), Some(&"http".to_string()));
        assert_eq!(services.get(&5999), Some(&"custom".to_string()));
        assert_eq!(services.len(), 3);
    }

    #[test]
    fn parse_services_ignores_udp_only_ports() {
        let contents = "onlyudp 12345/udp\n";
        let services = parse_services(contents);
        assert!(!services.contains_key(&12345));
    }

    #[test]
    fn describe_port_prefers_dev_tool_map_over_services() {
        let mut services = HashMap::new();
        services.insert(3000, "some-registered-name".to_string());
        assert_eq!(describe_port(3000, "node", &services), "Dev Server");
    }

    #[test]
    fn describe_port_falls_back_to_services_lookup() {
        let mut services = HashMap::new();
        services.insert(80, "http".to_string());
        assert_eq!(describe_port(80, "nginx", &services), "http");
    }

    #[test]
    fn describe_port_falls_back_to_command_heuristic() {
        let services = HashMap::new();
        assert_eq!(
            describe_port(54321, "node-server", &services),
            "Node.js Process"
        );
    }

    #[test]
    fn describe_port_falls_back_to_raw_command() {
        let services = HashMap::new();
        assert_eq!(
            describe_port(54321, "mystery-binary", &services),
            "mystery-binary"
        );
    }

    #[test]
    fn format_memory_picks_the_right_unit() {
        assert_eq!(format_memory(512), "512 B");
        assert_eq!(format_memory(2 * 1024), "2 KB");
        assert_eq!(format_memory(125 * 1024 * 1024), "125 MB");
        assert_eq!(format_memory(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn is_known_port_recognizes_dev_and_service_ports_only() {
        let mut services = HashMap::new();
        services.insert(80, "http".to_string());
        assert!(is_known_port(3000, &services)); // dev-tool map
        assert!(is_known_port(80, &services)); // /etc/services
        assert!(!is_known_port(54321, &services)); // heuristic/raw fallback
    }

    #[test]
    fn classify_severity_unchanged_thresholds() {
        assert_eq!(classify_severity(22), Severity::Critical);
        assert_eq!(classify_severity(5432), Severity::High);
        assert_eq!(classify_severity(3000), Severity::Medium);
        assert_eq!(classify_severity(54321), Severity::Low);
    }

    #[test]
    fn classify_exposure_recognizes_loopback() {
        assert_eq!(classify_exposure("127.0.0.1"), Exposure::Loopback);
        assert_eq!(classify_exposure("::1"), Exposure::Loopback);
        assert_eq!(classify_exposure("[::1]"), Exposure::Loopback);
    }

    #[test]
    fn classify_exposure_recognizes_all_interfaces() {
        assert_eq!(classify_exposure("0.0.0.0"), Exposure::AllInterfaces);
        assert_eq!(classify_exposure("*"), Exposure::AllInterfaces);
        assert_eq!(classify_exposure("::"), Exposure::AllInterfaces);
        assert_eq!(classify_exposure("[::]"), Exposure::AllInterfaces);
    }

    #[test]
    fn classify_exposure_falls_back_to_specific() {
        assert_eq!(classify_exposure("192.168.1.5"), Exposure::Specific);
        assert_eq!(classify_exposure("[2001:db8::1]"), Exposure::Specific);
    }

    #[test]
    fn memory_percent_rounds_and_guards_zero_total() {
        assert_eq!(memory_percent(0, 0), 0);
        assert_eq!(memory_percent(50, 100), 50);
        assert_eq!(memory_percent(2, 3), 67);
        assert_eq!(memory_percent(100, 100), 100);
    }

    #[test]
    fn format_age_picks_the_right_unit() {
        assert_eq!(format_age(30), "30s");
        assert_eq!(format_age(45 * 60), "45m");
        assert_eq!(format_age(2 * 3600 + 30 * 60), "2h 30m");
        assert_eq!(format_age(2 * 86400 + 3 * 3600), "2d 3h");
    }
}
