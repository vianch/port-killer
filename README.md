# port-killer

A fast TUI tool to view all listening TCP ports and kill processes. Built in Rust with [Ratatui](https://ratatui.rs).

![Rust](https://img.shields.io/badge/rust-1.94%2B-orange)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)

<img width="929" height="298" alt="Screenshot 2026-04-07 at 14 17 45" src="https://github.com/user-attachments/assets/3f86e45a-becc-4dfc-a0d8-b92b828466c2" />
<img width="930" height="317" alt="Screenshot 2026-04-07 at 14 17 57" src="https://github.com/user-attachments/assets/82c96522-da59-45bf-9192-d48f949b9ad5" />
<img width="924" height="300" alt="Screenshot 2026-04-07 at 14 18 11" src="https://github.com/user-attachments/assets/2b9fb9ac-a78b-420a-975e-a6269238dca9" />


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
