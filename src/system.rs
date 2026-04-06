use std::collections::HashMap;
use std::process::Command;

use crate::port_info::PortEntry;

#[derive(Debug)]
pub enum KillResult {
    Success,
    PermissionDenied,
    ProcessNotFound,
}

pub fn scan_ports() -> color_eyre::Result<Vec<PortEntry>> {
    if cfg!(target_os = "macos") {
        scan_macos()
    } else {
        scan_linux()
    }
}

fn scan_macos() -> color_eyre::Result<Vec<PortEntry>> {
    let output = Command::new("lsof")
        .args(["-iTCP", "-sTCP:LISTEN", "-P", "-n", "-c", "0"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashMap<(u16, u32), bool> = HashMap::new();

    for line in stdout.lines().skip(1) {
        if let Some(entry) = parse_lsof_line(line) {
            let key = (entry.port, entry.pid);
            if !seen.contains_key(&key) {
                seen.insert(key, true);
                entries.push(entry);
            }
        }
    }

    entries.sort_by_key(|e| e.port);
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
    let port_str = name.rsplit(':').next()?;
    let port: u16 = port_str.parse().ok()?;

    Some(PortEntry::new(port, pid, command))
}

fn scan_linux() -> color_eyre::Result<Vec<PortEntry>> {
    let output = Command::new("ss")
        .args(["-tlnp"])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries: Vec<PortEntry> = Vec::new();
    let mut seen: HashMap<(u16, u32), bool> = HashMap::new();

    for line in stdout.lines().skip(1) {
        if let Some(entry) = parse_ss_line(line) {
            let key = (entry.port, entry.pid);
            if !seen.contains_key(&key) {
                seen.insert(key, true);
                entries.push(entry);
            }
        }
    }

    entries.sort_by_key(|e| e.port);
    Ok(entries)
}

fn parse_ss_line(line: &str) -> Option<PortEntry> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let local_addr = parts[3];
    let port_str = local_addr.rsplit(':').next()?;
    let port: u16 = port_str.parse().ok()?;

    let process_field = parts.get(5).unwrap_or(&"");
    let (command, pid) = parse_ss_process_field(process_field)?;

    Some(PortEntry::new(port, pid, command))
}

fn parse_ss_process_field(field: &str) -> Option<(String, u32)> {
    let cmd_start = field.find("((\"")? + 3;
    let cmd_end = field[cmd_start..].find('"')? + cmd_start;
    let command = field[cmd_start..cmd_end].to_string();

    let pid_start = field.find("pid=")? + 4;
    let pid_end = field[pid_start..]
        .find(|c: char| !c.is_ascii_digit())?
        + pid_start;
    let pid: u32 = field[pid_start..pid_end].parse().ok()?;

    Some((command, pid))
}

pub fn kill_process(pid: u32) -> color_eyre::Result<KillResult> {
    use nix::sys::signal::{self, Signal};
    use nix::unistd::Pid;

    let nix_pid = Pid::from_raw(pid as i32);

    match signal::kill(nix_pid, Signal::SIGTERM) {
        Ok(()) => Ok(KillResult::Success),
        Err(nix::errno::Errno::EPERM) => Ok(KillResult::PermissionDenied),
        Err(nix::errno::Errno::ESRCH) => Ok(KillResult::ProcessNotFound),
        Err(e) => Err(e.into()),
    }
}
