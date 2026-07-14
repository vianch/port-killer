# CLAUDE.md

Guidance for Claude Code working in this repository.

## What this is

`port-killer` — a terminal UI (Rust + [Ratatui](https://ratatui.rs)) that lists listening TCP ports with their owning process and kills the selected one via `SIGTERM`. Single binary, no subcommands (only `--version` / `-V`). Runs on macOS (`lsof`) and Linux (`ss`).

## Commands

```bash
cargo run                # Launch the TUI
cargo build --release    # Optimized binary (strip + lto, opt-level "z" → ~737KB)
cargo test               # Run tests (CI runs this; there are currently none)
cargo run -- --version   # Print version, skip TUI
```

CI (`.github/workflows/rust.yml`): `cargo build --verbose` + `cargo test --verbose` on push/PR to `main`.

## Architecture

Event loop in `main.rs` (250ms poll): draw → read key → `app.tick()` → repeat. Modules:

- **`main.rs`** — terminal setup/teardown (raw mode + alt screen), the run loop, and `handle_key_event`. Key handling branches on `AppMode`. `--version` is handled before any TUI setup.
- **`app.rs`** — `App` state + `AppMode` (`Normal` / `Input` / `Confirm`). Owns entries, the filtered-index list, selection, and the transient status message (auto-clears after 3s). `tick()` auto-refreshes every 2s. Filtering keeps `entries` intact and rebuilds `filtered_indices`.
- **`system.rs`** — I/O edge. `scan_ports()` shells out to `lsof`/`ss` and parses each line into a `PortEntry`, deduping by `(port, pid)`, sorted by port. `kill_process()` sends `SIGTERM` via `nix` and maps `EPERM`/`ESRCH` to typed `KillResult` variants (never bare errors).
- **`port_info.rs`** — pure logic (no I/O). The `PortEntry` model plus `classify_severity` (port → `Severity`) and `describe_port` (port/command → human label). This is where the port tables live.
- **`ui.rs`** — pure rendering from `&App`: title bar, table (severity-colored), conditional filter input, help bar, and the centered confirm modal.

## Conventions

- **Keep parsing/derivation pure and I/O at the edges.** `classify_severity`, `describe_port`, `parse_lsof_line`, `parse_ss_line` take plain inputs and return values — no `Command`, no terminal. Any parser returns `Option` and skips malformed lines rather than panicking. New derivation logic follows this split so it stays testable (CI already runs `cargo test`; add tests for pure fns).
- **Kill outcomes are typed, not stringly.** `KillResult` (`Success` / `PermissionDenied` / `ProcessNotFound`) carries the distinction; the UI turns it into a message. Don't collapse permission/not-found into a generic error.
- **Adding a known port** = edit `classify_severity` (the `HIGH_PORTS`/`MEDIUM_PORTS` arrays) and/or `describe_port` in `port_info.rs`. Nowhere else.
- **Rendering reads, never mutates.** `ui.rs` takes `&App`. State changes happen only through `App` methods driven by `handle_key_event`.
- Edition 2024. `color_eyre::Result` throughout; `?` propagation.
