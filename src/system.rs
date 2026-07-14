use std::collections::HashSet;
use std::process::Command;

use sysinfo::{Pid, ProcessesToUpdate, System};

use crate::port_info::PortEntry;

/// Re-exported so callers (e.g. `app.rs`) can pick a signal without depending
/// on `nix` directly.
pub use nix::sys::signal::Signal;

#[derive(Debug)]
pub enum KillResult {
    Success,
    PermissionDenied,
    ProcessNotFound,
}

/// Scans listening TCP ports and enriches each entry with live CPU/memory
/// stats from `system`. `system` must be a persistent instance kept across
/// scans (in `App`) so `Process::cpu_usage` has a time delta to measure.
pub fn scan_ports(system: &mut System) -> color_eyre::Result<Vec<PortEntry>> {
    system.refresh_processes(ProcessesToUpdate::All, true);
    // Global CPU%/memory for the bottom sparklines. CPU usage is a delta since
    // the previous refresh, so the first reading after startup reads ~0.
    system.refresh_cpu_usage();
    system.refresh_memory();

    let mut entries = if cfg!(target_os = "macos") {
        scan_macos()?
    } else {
        scan_linux()?
    };

    for entry in &mut entries {
        if let Some(process) = system.process(Pid::from_u32(entry.pid)) {
            entry.cpu_percent = process.cpu_usage();
            entry.memory_bytes = process.memory();

            let cmdline = process
                .cmd()
                .iter()
                .map(|arg| arg.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" ");
            entry.cmdline = if cmdline.is_empty() {
                entry.command.clone()
            } else {
                cmdline
            };

            entry.cwd = process.cwd().map(|path| path.to_string_lossy().to_string());

            entry.age_seconds = process.run_time();

            entry.parent = process.parent().and_then(|parent_pid| {
                system.process(parent_pid).map(|parent| {
                    (
                        parent_pid.as_u32(),
                        parent.name().to_string_lossy().to_string(),
                    )
                })
            });
        }
    }

    Ok(entries)
}

/// Splits a `host:port` (or bracketed IPv6 `[host]:port`) field into its
/// address and port substrings. The port is always the substring after the
/// last `:`; the address is everything before that (brackets included, so
/// `classify_exposure` can strip them). Pure, unit-tested.
fn split_addr_port(field: &str) -> Option<(&str, &str)> {
    let port = field.rsplit(':').next()?;
    if port.is_empty() || port.len() == field.len() {
        return None;
    }
    let address = &field[..field.len() - port.len() - 1];
    Some((address, port))
}

fn scan_macos() -> color_eyre::Result<Vec<PortEntry>> {
    let output = Command::new("lsof")
        // `+c 0` (not `-c 0`): `-c 0` truncates the command name to 9 chars,
        // `+c 0` disables the truncation entirely.
        .args(["-iTCP", "-sTCP:LISTEN", "-P", "-n", "+c", "0"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashSet<(u16, u32)> = HashSet::new();

    for line in stdout.lines().skip(1) {
        if let Some(entry) = parse_lsof_line(line)
            && seen.insert((entry.port, entry.pid))
        {
            entries.push(entry);
        }
    }

    entries.sort_by_key(|entry| entry.port);
    Ok(entries)
}

fn parse_lsof_line(line: &str) -> Option<PortEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 10 {
        return None;
    }

    let command = parts[0].replace("\\x20", " ");
    let pid: u32 = parts[1].parse().ok()?;
    let name = parts[parts.len() - 2];
    let (address, port_str) = split_addr_port(name)?;
    let port: u16 = port_str.parse().ok()?;

    Some(PortEntry::new(port, pid, command, address.to_string()))
}

fn scan_linux() -> color_eyre::Result<Vec<PortEntry>> {
    let output = Command::new("ss").args(["-tlnp"]).output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashSet<(u16, u32)> = HashSet::new();

    for line in stdout.lines().skip(1) {
        if let Some(entry) = parse_ss_line(line)
            && seen.insert((entry.port, entry.pid))
        {
            entries.push(entry);
        }
    }

    entries.sort_by_key(|entry| entry.port);
    Ok(entries)
}

fn parse_ss_line(line: &str) -> Option<PortEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let local_addr = parts[3];
    let (address, port_str) = split_addr_port(local_addr)?;
    let port: u16 = port_str.parse().ok()?;

    let process_field = parts.get(5).unwrap_or(&"");
    let (command, pid) = parse_ss_process_field(process_field)?;

    Some(PortEntry::new(port, pid, command, address.to_string()))
}

fn parse_ss_process_field(field: &str) -> Option<(String, u32)> {
    let cmd_start = field.find("((\"")? + 3;
    let cmd_end = field[cmd_start..].find('"')? + cmd_start;
    let command = field[cmd_start..cmd_end].to_string();

    let pid_start = field.find("pid=")? + 4;
    let pid_end = field[pid_start..].find(|c: char| !c.is_ascii_digit())? + pid_start;
    let pid: u32 = field[pid_start..pid_end].parse().ok()?;

    Some((command, pid))
}

/// Sends `signal` to `pid`. Callers pick `Signal::SIGTERM` for a normal kill
/// or `Signal::SIGKILL` to force-kill.
pub fn kill_process(pid: u32, signal: Signal) -> color_eyre::Result<KillResult> {
    use nix::sys::signal;
    use nix::unistd::Pid;

    let nix_pid = Pid::from_raw(pid as i32);

    match signal::kill(nix_pid, signal) {
        Ok(()) => Ok(KillResult::Success),
        Err(nix::errno::Errno::EPERM) => Ok(KillResult::PermissionDenied),
        Err(nix::errno::Errno::ESRCH) => Ok(KillResult::ProcessNotFound),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_addr_port_handles_plain_ipv4() {
        assert_eq!(
            split_addr_port("127.0.0.1:3000"),
            Some(("127.0.0.1", "3000"))
        );
    }

    #[test]
    fn split_addr_port_handles_wildcard() {
        assert_eq!(split_addr_port("*:7000"), Some(("*", "7000")));
    }

    #[test]
    fn split_addr_port_handles_bracketed_ipv6() {
        assert_eq!(split_addr_port("[::1]:5432"), Some(("[::1]", "5432")));
        assert_eq!(split_addr_port("[::]:22"), Some(("[::]", "22")));
    }

    #[test]
    fn split_addr_port_rejects_missing_port() {
        assert_eq!(split_addr_port("no-colon-here"), None);
    }

    #[test]
    fn parse_lsof_line_captures_bind_address() {
        let line = "node    1234 user   20u  IPv4 0x0 0t0  TCP 127.0.0.1:3000 (LISTEN)";
        let entry = parse_lsof_line(line).expect("should parse");
        assert_eq!(entry.port, 3000);
        assert_eq!(entry.bind_addr, "127.0.0.1");
    }

    #[test]
    fn parse_lsof_line_captures_ipv6_bind_address() {
        let line = "postgres 1234 user  20u  IPv6 0x0 0t0  TCP [::1]:5432 (LISTEN)";
        let entry = parse_lsof_line(line).expect("should parse");
        assert_eq!(entry.port, 5432);
        assert_eq!(entry.bind_addr, "[::1]");
    }
}
