use std::time::{Duration, Instant};

use crate::port_info::PortEntry;
use crate::system::{self, KillResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Input,
    Confirm,
}

pub struct App {
    pub entries: Vec<PortEntry>,
    pub filtered_indices: Vec<usize>,
    pub selected_index: usize,
    pub mode: AppMode,
    pub input_buffer: String,
    pub should_quit: bool,
    pub last_refresh: Instant,
    pub refresh_interval: Duration,
    pub status_message: Option<String>,
    pub status_message_time: Option<Instant>,
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
            refresh_interval: Duration::from_secs(2),
            status_message: None,
            status_message_time: None,
        }
    }

    pub fn refresh_ports(&mut self) -> color_eyre::Result<()> {
        self.entries = system::scan_ports()?;
        self.apply_filter();
        self.last_refresh = Instant::now();
        Ok(())
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

        if self.filtered_indices.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len() - 1;
        }
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

    pub fn enter_input_mode(&mut self) {
        self.mode = AppMode::Input;
    }

    pub fn exit_input_mode(&mut self) {
        self.mode = AppMode::Normal;
    }

    pub fn request_kill(&mut self) {
        if self.selected_entry().is_some() {
            self.mode = AppMode::Confirm;
        }
    }

    pub fn confirm_kill(&mut self) -> color_eyre::Result<()> {
        self.mode = AppMode::Normal;
        if let Some(entry) = self.selected_entry().cloned() {
            match system::kill_process(entry.pid)? {
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
        if self.last_refresh.elapsed() >= self.refresh_interval {
            self.refresh_ports()?;
        }

        if let Some(time) = self.status_message_time {
            if time.elapsed() >= Duration::from_secs(3) {
                self.status_message = None;
                self.status_message_time = None;
            }
        }

        Ok(())
    }
}
