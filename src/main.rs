mod app;
mod port_info;
mod system;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, AppMode};

fn main() -> color_eyre::Result<()> {
    // Handle --version before TUI setup
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--version" || args[1] == "-V") {
        println!("port-killer {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    color_eyre::install()?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run app
    let mut app = App::new();
    app.refresh_ports()?;
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> color_eyre::Result<()> {
    let tick_rate = Duration::from_millis(250);

    loop {
        terminal.draw(|frame| ui::render(frame, app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                // Skip key release events (Windows/some terminals send these)
                if key.kind != event::KeyEventKind::Press {
                    continue;
                }
                handle_key_event(app, key)?;
            }
        }

        app.tick()?;

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_key_event(app: &mut App, key: event::KeyEvent) -> color_eyre::Result<()> {
    // Ctrl+C always quits
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.should_quit = true;
        return Ok(());
    }

    match app.mode {
        AppMode::Normal => match key.code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => app.move_selection_up(),
            KeyCode::Down | KeyCode::Char('j') => app.move_selection_down(),
            KeyCode::Enter => app.request_kill(),
            KeyCode::Char('/') => app.enter_input_mode(),
            KeyCode::Char('r') => app.refresh_ports()?,
            _ => {}
        },
        AppMode::Input => match key.code {
            KeyCode::Esc => {
                app.input_buffer.clear();
                app.apply_filter();
                app.exit_input_mode();
            }
            KeyCode::Enter => app.exit_input_mode(),
            KeyCode::Backspace => {
                app.input_buffer.pop();
                app.apply_filter();
            }
            KeyCode::Char(c) => {
                app.input_buffer.push(c);
                app.apply_filter();
            }
            _ => {}
        },
        AppMode::Confirm => match key.code {
            KeyCode::Char('y') | KeyCode::Enter => app.confirm_kill()?,
            KeyCode::Char('n') | KeyCode::Esc => app.cancel_kill(),
            _ => {}
        },
    }

    Ok(())
}
