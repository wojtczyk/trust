mod app;
mod clipboard;
mod editor;
mod project;
mod ui;

use std::{
    env,
    ffi::OsString,
    io::{self, IsTerminal, Stdout},
    path::PathBuf,
    time::Duration,
};

use app::{Action, App};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

type TerminalUi = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = match parse_args()? {
        Startup::Help => {
            print_usage();
            return Ok(());
        }
        Startup::Open(root) => root,
    };

    if !io::stdout().is_terminal() {
        return Err("TRUST must be run in an interactive terminal".into());
    }

    let root = root.canonicalize().unwrap_or(root);

    let mut terminal = setup_terminal()?;
    let mut app = App::new(root);
    app.refresh_project();

    let result = run(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;

    if let Err(error) = result {
        eprintln!("trust: {error}");
        std::process::exit(1);
    }

    Ok(())
}

enum Startup {
    Help,
    Open(PathBuf),
}

fn parse_args() -> Result<Startup, Box<dyn std::error::Error>> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();

    match args.as_slice() {
        [] => Ok(Startup::Open(env::current_dir()?)),
        [flag] if is_help_flag(flag) => Ok(Startup::Help),
        [path] => Ok(Startup::Open(PathBuf::from(path))),
        _ => Err("usage: trust [PROJECT_PATH]".into()),
    }
}

fn is_help_flag(value: &OsString) -> bool {
    value == "-h" || value == "--help"
}

fn print_usage() {
    println!("TRUST - retro DOS-style TUI IDE for Rust projects");
    println!();
    println!("Usage:");
    println!("  trust [PROJECT_PATH]");
    println!();
    println!("Keys: F1 Help, F2 Save, F3 Open, F5 Run, F7 Check, F9 Build, Ctrl+Z Undo, Ctrl+Y Redo, Ctrl+Q Quit");
}

fn setup_terminal() -> io::Result<TerminalUi> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore_terminal(terminal: &mut TerminalUi) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()
}

fn run(terminal: &mut TerminalUi, app: &mut App) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(Duration::from_millis(120))? {
            match event::read()? {
                Event::Key(key) => {
                    if handle_key(app, key) == Action::Quit {
                        break;
                    }
                }
                Event::Mouse(mouse) => {
                    if app.handle_mouse(mouse) == Action::Quit {
                        break;
                    }
                }
                Event::Paste(text) => app.paste_text(&text),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Action {
    if key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
    {
        return Action::Quit;
    }

    if app.help_open {
        app.help_open = false;
        return Action::None;
    }

    if app.dialog.is_some() {
        return app.handle_dialog_key(key);
    }

    if app.menu_open {
        return app.handle_menu_key(key);
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        if key.modifiers.contains(KeyModifiers::SHIFT)
            && matches!(key.code, KeyCode::Char('z') | KeyCode::Char('Z'))
        {
            app.redo_editor();
            return Action::None;
        }

        match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => app.copy_selection(),
            KeyCode::Char('x') | KeyCode::Char('X') => app.cut_selection(),
            KeyCode::Char('v') | KeyCode::Char('V') => app.paste_from_clipboard(),
            KeyCode::Char('z') | KeyCode::Char('Z') => app.undo_editor(),
            KeyCode::Char('y') | KeyCode::Char('Y') => app.redo_editor(),
            KeyCode::Char('s') | KeyCode::Char('S') => app.save_current(),
            KeyCode::Char('f') | KeyCode::Char('F') => app.toggle_focus(),
            KeyCode::Char('o') | KeyCode::Char('O') => app.open_selected_file(),
            KeyCode::Char('r') | KeyCode::Char('R') => app.run_cargo("run"),
            KeyCode::Char('t') | KeyCode::Char('T') => app.run_cargo("test"),
            KeyCode::Char('b') | KeyCode::Char('B') => app.run_cargo("build"),
            _ => {}
        }
        return Action::None;
    }

    match key.code {
        KeyCode::Esc => {
            return Action::Quit;
        }
        KeyCode::F(1) => app.help_open = true,
        KeyCode::F(2) => app.save_current(),
        KeyCode::F(3) => app.open_selected_file(),
        KeyCode::F(4) => app.toggle_focus(),
        KeyCode::F(5) => app.run_cargo("run"),
        KeyCode::F(7) => app.run_cargo("check"),
        KeyCode::F(8) => app.run_cargo("test"),
        KeyCode::F(9) => app.run_cargo("build"),
        KeyCode::F(10) => app.toggle_menu(),
        KeyCode::Tab => app.toggle_focus(),
        KeyCode::BackTab => app.toggle_focus(),
        _ => app.handle_active_key(key),
    }

    Action::None
}
