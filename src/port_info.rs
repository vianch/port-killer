use std::fmt;

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

#[derive(Debug, Clone)]
pub struct PortEntry {
    pub port: u16,
    pub pid: u32,
    pub command: String,
    pub severity: Severity,
    pub description: String,
}

impl PortEntry {
    pub fn new(port: u16, pid: u32, command: String) -> Self {
        let severity = classify_severity(port);
        let description = describe_port(port, &command);
        Self {
            port,
            pid,
            command,
            severity,
            description,
        }
    }
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
        3000, 3001, 3002, 4000, 4200, 4321, 5000, 5173, 5174, 5500, 5555, 8000, 8001, 8080,
        8081, 8443, 8888, 9000, 9090, 9229, 9999, 24678,
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

pub fn describe_port(port: u16, command: &str) -> String {
    match port {
        20 => "FTP Data".into(),
        21 => "FTP Control".into(),
        22 => "SSH".into(),
        23 => "Telnet".into(),
        25 => "SMTP".into(),
        53 => "DNS".into(),
        80 => "HTTP".into(),
        88 => "Kerberos".into(),
        110 => "POP3".into(),
        143 => "IMAP".into(),
        443 => "HTTPS".into(),
        445 => "SMB".into(),
        465 => "SMTPS".into(),
        587 => "SMTP Submission".into(),
        631 => "CUPS Printing".into(),
        993 => "IMAPS".into(),
        995 => "POP3S".into(),
        1433 => "MSSQL".into(),
        2181 => "ZooKeeper".into(),
        3000 => "Dev Server (3000)".into(),
        3001 => "Dev Server (3001)".into(),
        3002 => "Dev Server (3002)".into(),
        3306 => "MySQL".into(),
        3389 => "RDP".into(),
        4000 => "Dev Server (4000)".into(),
        4200 => "Angular Dev".into(),
        4321 => "Astro Dev".into(),
        5000 => "Dev Server (5000)".into(),
        5173 => "Vite Dev".into(),
        5174 => "Vite Dev (alt)".into(),
        5432 => "PostgreSQL".into(),
        5500 => "Live Server".into(),
        5555 => "ADB / Dev".into(),
        5672 => "RabbitMQ".into(),
        6379 => "Redis".into(),
        7000 => "AirPlay Receiver".into(),
        7474 => "Neo4j".into(),
        8000 => "HTTP Alt / Dev".into(),
        8001 => "HTTP Alt / Dev".into(),
        8080 => "HTTP Proxy / Dev".into(),
        8081 => "HTTP Proxy Alt".into(),
        8443 => "HTTPS Alt".into(),
        8529 => "ArangoDB".into(),
        8888 => "Jupyter / Dev".into(),
        9000 => "PHP-FPM / Dev".into(),
        9090 => "Prometheus".into(),
        9092 => "Kafka".into(),
        9200 => "Elasticsearch".into(),
        9229 => "Node.js Debug".into(),
        9999 => "Dev Server (9999)".into(),
        11211 => "Memcached".into(),
        24678 => "Vite HMR".into(),
        26257 => "CockroachDB".into(),
        27017 => "MongoDB".into(),
        _ => {
            let cmd_lower = command.to_lowercase();
            if cmd_lower.contains("node") {
                "Node.js Process".into()
            } else if cmd_lower.contains("python") {
                "Python Process".into()
            } else if cmd_lower.contains("ruby") {
                "Ruby Process".into()
            } else if cmd_lower.contains("java") {
                "Java Process".into()
            } else if cmd_lower.contains("nginx") {
                "Nginx".into()
            } else if cmd_lower.contains("apache") || cmd_lower.contains("httpd") {
                "Apache HTTP".into()
            } else if cmd_lower.contains("docker") {
                "Docker".into()
            } else if cmd_lower.contains("code") {
                "VS Code".into()
            } else {
                command.to_string()
            }
        }
    }
}
