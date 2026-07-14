use std::collections::VecDeque;
use std::time::{Duration, Instant};

use sysinfo::System;

use crate::port_info::{PortEntry, memory_percent};
use crate::system::{self, KillResult, Signal};

/// How often `tick()` re-scans ports automatically.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
/// How long a status/result message stays visible before auto-clearing.
const STATUS_MESSAGE_TIMEOUT: Duration = Duration::from_secs(3);
/// Cap on each system-metric ring buffer. Bounded so history can never grow
/// without limit — the oldest sample is dropped once full.
const METRICS_HISTORY_LEN: usize = 120;
/// Percent scale for CPU/memory metrics (values are clamped to `0..=100`).
const PERCENT_MAX: f32 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Input,
    Confirm,
}

/// Table column the list can be sorted by. `next()` defines the cycle order
/// used by the `s` key; mouse clicks jump straight to the clicked column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Port,
    Command,
    Pid,
    Cpu,
    Mem,
    Severity,
    Exposure,
}

impl SortColumn {
    const CYCLE_ORDER: [SortColumn; 7] = [
        SortColumn::Port,
        SortColumn::Command,
        SortColumn::Pid,
        SortColumn::Cpu,
        SortColumn::Mem,
        SortColumn::Severity,
        SortColumn::Exposure,
    ];

    pub fn next(self) -> Self {
        let position = Self::CYCLE_ORDER
            .iter()
            .position(|&column| column == self)
            .unwrap_or(0);
        Self::CYCLE_ORDER[(position + 1) % Self::CYCLE_ORDER.len()]
    }

    pub fn label(self) -> &'static str {
        match self {
            SortColumn::Port => "Port",
            SortColumn::Command => "Command",
            SortColumn::Pid => "PID",
            SortColumn::Cpu => "CPU%",
            SortColumn::Mem => "Mem",
            SortColumn::Severity => "Severity",
            SortColumn::Exposure => "Exposure",
        }
    }
}

/// How the list is ordered. `KnownFirst` is the startup default; the first
/// explicit sort (header click or `s`) switches to `Column` and stays there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    KnownFirst,
    Column(SortColumn),
}

/// Pushes `value` onto `buffer`, evicting the oldest entry once the cap
/// `METRICS_HISTORY_LEN` is reached. Pure and unit-tested so the "never grows
/// unbounded" guarantee has a check behind it.
fn push_bounded(buffer: &mut VecDeque<u64>, value: u64) {
    if buffer.len() == METRICS_HISTORY_LEN {
        buffer.pop_front();
    }
    buffer.push_back(value);
}

/// Reorders `indices` in place, reading values from `entries`. Pure: no I/O,
/// safe to unit test with plain data. `ascending` only applies to
/// `Column` mode; `KnownFirst` is always known-then-unknown, port ascending.
pub fn sort_indices(entries: &[PortEntry], indices: &mut [usize], mode: SortMode, ascending: bool) {
    indices.sort_by(|&left, &right| {
        let ordering = match mode {
            SortMode::KnownFirst => entries[right]
                .known
                .cmp(&entries[left].known)
                .then(entries[left].port.cmp(&entries[right].port)),
            SortMode::Column(SortColumn::Port) => entries[left].port.cmp(&entries[right].port),
            SortMode::Column(SortColumn::Command) => entries[left]
                .command
                .to_lowercase()
                .cmp(&entries[right].command.to_lowercase()),
            SortMode::Column(SortColumn::Pid) => entries[left].pid.cmp(&entries[right].pid),
            SortMode::Column(SortColumn::Cpu) => entries[left]
                .cpu_percent
                .total_cmp(&entries[right].cpu_percent),
            SortMode::Column(SortColumn::Mem) => {
                entries[left].memory_bytes.cmp(&entries[right].memory_bytes)
            }
            SortMode::Column(SortColumn::Severity) => {
                entries[left].severity.cmp(&entries[right].severity)
            }
            SortMode::Column(SortColumn::Exposure) => {
                entries[left].exposure.cmp(&entries[right].exposure)
            }
        };
        // KnownFirst has a fixed direction; `ascending` governs Column mode only.
        if ascending || mode == SortMode::KnownFirst {
            ordering
        } else {
            ordering.reverse()
        }
    });
}

pub struct App {
    pub entries: Vec<PortEntry>,
    pub filtered_indices: Vec<usize>,
    pub selected_index: usize,
    pub mode: AppMode,
    pub input_buffer: String,
    pub should_quit: bool,
    pub last_refresh: Instant,
    pub status_message: Option<String>,
    pub status_message_time: Option<Instant>,
    pub sort_mode: SortMode,
    pub sort_ascending: bool,
    /// Whether the pending `Confirm` modal is a force-kill (SIGKILL) rather
    /// than a normal kill (SIGTERM). Set by `request_kill`, read by
    /// `confirm_kill` and by `ui.rs` to style the modal.
    pub confirm_force: bool,
    /// True when running as root. When false, `lsof`/`ss` only see the current
    /// user's sockets, so the title bar shows a "run with sudo" hint.
    pub elevated: bool,
    /// First filtered-list index visible in the table, mirrored from the
    /// table's rendered scroll offset so a row click maps to the right port
    /// even when scrolled. Recorded by `record_table_offset` during render.
    pub table_offset: usize,
    /// Bounded ring buffers (cap `METRICS_HISTORY_LEN`) of global CPU% and
    /// memory% for the bottom sparklines. Oldest sample dropped when full.
    pub cpu_history: VecDeque<u64>,
    pub mem_history: VecDeque<u64>,
    system: System,
}

impl App {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            filtered_indices: Vec::new(),
            selected_index: 0,
            mode: AppMode::Normal,
            input_buffer: String::new(),
            should_quit: false,
            last_refresh: Instant::now(),
            status_message: None,
            status_message_time: None,
            sort_mode: SortMode::KnownFirst,
            sort_ascending: true,
            confirm_force: false,
            elevated: nix::unistd::geteuid().is_root(),
            table_offset: 0,
            cpu_history: VecDeque::with_capacity(METRICS_HISTORY_LEN),
            mem_history: VecDeque::with_capacity(METRICS_HISTORY_LEN),
            system: System::new(),
        }
    }

    pub fn refresh_ports(&mut self) -> color_eyre::Result<()> {
        self.entries = system::scan_ports(&mut self.system)?;
        let cpu_pct = self
            .system
            .global_cpu_usage()
            .round()
            .clamp(0.0, PERCENT_MAX) as u64;
        let mem_pct = memory_percent(self.system.used_memory(), self.system.total_memory());
        self.record_system_metrics(cpu_pct, mem_pct);
        self.apply_filter();
        self.last_refresh = Instant::now();
        Ok(())
    }

    /// Appends one CPU%/memory% sample to the bounded history ring buffers,
    /// dropping the oldest when at capacity so memory stays bounded.
    pub fn record_system_metrics(&mut self, cpu_pct: u64, mem_pct: u64) {
        push_bounded(&mut self.cpu_history, cpu_pct);
        push_bounded(&mut self.mem_history, mem_pct);
    }

    pub fn apply_filter(&mut self) {
        if self.input_buffer.is_empty() {
            self.filtered_indices = (0..self.entries.len()).collect();
        } else {
            let query = self.input_buffer.to_lowercase();
            self.filtered_indices = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.port.to_string().contains(&query)
                        || entry.command.to_lowercase().contains(&query)
                        || entry.description.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }

        sort_indices(
            &self.entries,
            &mut self.filtered_indices,
            self.sort_mode,
            self.sort_ascending,
        );

        if self.filtered_indices.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len() - 1;
        }
    }

    /// Switches to explicit column sort, toggling direction on a repeated
    /// selection of the same column (used by both `s` and header clicks).
    pub fn set_sort_column(&mut self, column: SortColumn) {
        if self.sort_mode == SortMode::Column(column) {
            self.sort_ascending = !self.sort_ascending;
        } else {
            self.sort_mode = SortMode::Column(column);
            self.sort_ascending = true;
        }
        self.apply_filter();
    }

    /// `s`: advance the explicit column sort. From the KnownFirst default it
    /// enters column sort at the first column.
    pub fn cycle_sort_column(&mut self) {
        let next = match self.sort_mode {
            SortMode::Column(column) => column.next(),
            SortMode::KnownFirst => SortColumn::CYCLE_ORDER[0],
        };
        self.sort_mode = SortMode::Column(next);
        self.sort_ascending = true;
        self.apply_filter();
    }

    pub fn move_selection_up(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.filtered_indices.len() - 1;
        } else {
            self.selected_index -= 1;
        }
    }

    pub fn move_selection_down(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        if self.selected_index >= self.filtered_indices.len() - 1 {
            self.selected_index = 0;
        } else {
            self.selected_index += 1;
        }
    }

    pub fn selected_entry(&self) -> Option<&PortEntry> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&idx| self.entries.get(idx))
    }

    /// Selects the row at `visible_offset` rows below the first visible row.
    /// Offset is added to the recorded scroll offset so it stays correct when
    /// the table is scrolled. Clicks past the last row are ignored.
    pub fn select_visible_row(&mut self, visible_offset: usize) {
        let index = self.table_offset + visible_offset;
        if index < self.filtered_indices.len() {
            self.selected_index = index;
        }
    }

    /// Records the table's rendered scroll offset so mouse hit-testing can map
    /// a visible row back to a filtered-list index.
    pub fn record_table_offset(&mut self, offset: usize) {
        self.table_offset = offset;
    }

    pub fn enter_input_mode(&mut self) {
        self.mode = AppMode::Input;
    }

    pub fn exit_input_mode(&mut self) {
        self.mode = AppMode::Normal;
    }

    /// `force` selects which signal `confirm_kill` will send: SIGTERM for a
    /// normal kill, SIGKILL for a force-kill. Only the explicit force-kill
    /// key sets it true — no auto-escalation on a stuck process.
    pub fn request_kill(&mut self, force: bool) {
        if self.selected_entry().is_some() {
            self.mode = AppMode::Confirm;
            self.confirm_force = force;
        }
    }

    pub fn confirm_kill(&mut self) -> color_eyre::Result<()> {
        self.mode = AppMode::Normal;
        let signal = if self.confirm_force {
            Signal::SIGKILL
        } else {
            Signal::SIGTERM
        };
        if let Some(entry) = self.selected_entry().cloned() {
            match system::kill_process(entry.pid, signal)? {
                KillResult::Success => {
                    self.status_message = Some(format!(
                        "Killed \"{}\" (PID {}) on port {}",
                        entry.command, entry.pid, entry.port
                    ));
                }
                KillResult::PermissionDenied => {
                    self.status_message = Some(format!(
                        "Permission denied: cannot kill PID {} (try sudo)",
                        entry.pid
                    ));
                }
                KillResult::ProcessNotFound => {
                    self.status_message = Some(format!(
                        "Process \"{}\" (PID {}) no longer exists",
                        entry.command, entry.pid
                    ));
                }
            }
            self.status_message_time = Some(Instant::now());
            self.refresh_ports()?;
        }
        Ok(())
    }

    pub fn cancel_kill(&mut self) {
        self.mode = AppMode::Normal;
    }

    pub fn tick(&mut self) -> color_eyre::Result<()> {
        if self.last_refresh.elapsed() >= REFRESH_INTERVAL {
            self.refresh_ports()?;
        }

        if let Some(time) = self.status_message_time
            && time.elapsed() >= STATUS_MESSAGE_TIMEOUT
        {
            self.status_message = None;
            self.status_message_time = None;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::port_info::{Exposure, Severity};

    fn entry(
        port: u16,
        pid: u32,
        command: &str,
        cpu: f32,
        memory: u64,
        severity: Severity,
    ) -> PortEntry {
        PortEntry {
            port,
            pid,
            command: command.to_string(),
            severity,
            description: String::new(),
            known: false,
            cpu_percent: cpu,
            memory_bytes: memory,
            bind_addr: String::new(),
            exposure: Exposure::Specific,
            cmdline: String::new(),
            cwd: None,
            age_seconds: 0,
            parent: None,
        }
    }

    #[test]
    fn sort_indices_by_port_ascending_and_descending() {
        let entries = vec![
            entry(8080, 1, "b", 0.0, 0, Severity::Low),
            entry(80, 2, "a", 0.0, 0, Severity::Critical),
            entry(3000, 3, "c", 0.0, 0, Severity::Medium),
        ];
        let mut indices: Vec<usize> = (0..entries.len()).collect();

        sort_indices(
            &entries,
            &mut indices,
            SortMode::Column(SortColumn::Port),
            true,
        );
        assert_eq!(indices, vec![1, 2, 0]);

        sort_indices(
            &entries,
            &mut indices,
            SortMode::Column(SortColumn::Port),
            false,
        );
        assert_eq!(indices, vec![0, 2, 1]);
    }

    #[test]
    fn sort_indices_by_command_is_case_insensitive() {
        let entries = vec![
            entry(1, 1, "Zebra", 0.0, 0, Severity::Low),
            entry(2, 2, "apple", 0.0, 0, Severity::Low),
        ];
        let mut indices: Vec<usize> = (0..entries.len()).collect();

        sort_indices(
            &entries,
            &mut indices,
            SortMode::Column(SortColumn::Command),
            true,
        );
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn sort_indices_by_cpu_and_memory() {
        let entries = vec![
            entry(1, 1, "a", 5.0, 200, Severity::Low),
            entry(2, 2, "b", 50.0, 100, Severity::Low),
        ];
        let mut indices: Vec<usize> = (0..entries.len()).collect();

        sort_indices(
            &entries,
            &mut indices,
            SortMode::Column(SortColumn::Cpu),
            true,
        );
        assert_eq!(indices, vec![0, 1]);

        sort_indices(
            &entries,
            &mut indices,
            SortMode::Column(SortColumn::Mem),
            true,
        );
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn known_first_sorts_known_ahead_of_unknown_then_by_port() {
        let mut unknown_low_port = entry(80, 1, "a", 0.0, 0, Severity::Low);
        unknown_low_port.known = false;
        let mut known_high_port = entry(9999, 2, "b", 0.0, 0, Severity::Low);
        known_high_port.known = true;
        let mut known_mid_port = entry(3000, 3, "c", 0.0, 0, Severity::Low);
        known_mid_port.known = true;

        let entries = vec![unknown_low_port, known_high_port, known_mid_port];
        let mut indices: Vec<usize> = (0..entries.len()).collect();

        sort_indices(&entries, &mut indices, SortMode::KnownFirst, true);
        // known ports (3000, 9999) first ordered by port, then unknown (80).
        assert_eq!(indices, vec![2, 1, 0]);
    }

    #[test]
    fn push_bounded_never_exceeds_cap() {
        let mut buffer = VecDeque::new();
        for value in 0..(METRICS_HISTORY_LEN as u64 + 50) {
            push_bounded(&mut buffer, value);
        }
        assert_eq!(buffer.len(), METRICS_HISTORY_LEN);
        // Oldest samples evicted: front is the (len+50 - cap)th value pushed.
        assert_eq!(*buffer.front().unwrap(), 50);
        assert_eq!(*buffer.back().unwrap(), METRICS_HISTORY_LEN as u64 + 49);
    }

    #[test]
    fn sort_indices_by_exposure() {
        let mut entries = vec![
            entry(1, 1, "a", 0.0, 0, Severity::Low),
            entry(2, 2, "b", 0.0, 0, Severity::Low),
            entry(3, 3, "c", 0.0, 0, Severity::Low),
        ];
        entries[0].exposure = Exposure::AllInterfaces;
        entries[1].exposure = Exposure::Loopback;
        entries[2].exposure = Exposure::Specific;
        let mut indices: Vec<usize> = (0..entries.len()).collect();

        sort_indices(
            &entries,
            &mut indices,
            SortMode::Column(SortColumn::Exposure),
            true,
        );
        // Loopback < Specific < AllInterfaces
        assert_eq!(indices, vec![1, 2, 0]);
    }

    #[test]
    fn sort_column_cycles_through_all_variants() {
        let mut column = SortColumn::Port;
        for _ in 0..SortColumn::CYCLE_ORDER.len() {
            column = column.next();
        }
        assert_eq!(column, SortColumn::Port);
    }
}
