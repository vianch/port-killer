# port-killer

A fast TUI tool to view all listening TCP ports and kill processes. Built in Rust with [Ratatui](https://ratatui.rs).

![Rust](https://img.shields.io/badge/rust-1.94%2B-orange)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

## Install

### Homebrew

```sh
brew tap vianch/tap
brew install port-killer
```

### From source

```sh
cargo install --git https://github.com/vianch/port-killer
```

## Usage

```sh
port-killer
```

### Keybindings

| Key | Action |
|-----|--------|
| `↑` / `k` | Move up |
| `↓` / `j` | Move down |
| `Enter` | Kill selected process |
| `/` | Filter by port or command |
| `r` | Refresh port list |
| `q` / `Ctrl+C` | Quit |

### Severity Levels

| Level | Color | Ports |
|-------|-------|-------|
| Critical | Magenta | 1-1023 (privileged) |
| High | Red | Database ports (MySQL, PostgreSQL, Redis, etc.) |
| Medium | Yellow | Common dev ports (3000, 8080, 5173, etc.) |
| Low | Green | Everything else |

## Features

- Shows all listening TCP ports with process info
- Color-coded severity classification
- Real-time filter by port number or command name
- Kill confirmation modal before terminating processes
- Auto-refresh every 2 seconds
- Works on macOS (`lsof`) and Linux (`ss`)
- 737KB stripped release binary

## License

Apache-2.0
