mod app;
mod clipboard;
mod debugger;
mod editor;
mod ide;
mod project;
mod ui;

use std::{
    env,
    ffi::OsString,
    fmt,
    io::{self, IsTerminal, Stdout},
    path::PathBuf,
    time::Duration,
};

use app::{Action, App};
use crossterm::{
    Command,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyModifiers, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

type TerminalUi = Terminal<CrosstermBackend<Stdout>>;

struct EnableXtermModifiedKeys;

impl Command for EnableXtermModifiedKeys {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[>1;2m\x1b[>2;2m\x1b[>4;2m")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }
}

struct ResetXtermModifiedKeys;

impl Command for ResetXtermModifiedKeys {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b[>1;0m\x1b[>2;0m\x1b[>4;0m")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }
}

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
    println!(
        "Keys: F1 Help, F2 Save, F3 Open, F5 Run, F6 Breakpoint, Ctrl+F Find, Ctrl+G Next Match, Ctrl+D Debug, F11/F12 Step, Ctrl+Space Complete, Ctrl+Z Undo, Ctrl+Y Redo, Ctrl+Q Quit"
    );
}

fn setup_terminal() -> io::Result<TerminalUi> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS,
        ),
        EnableXtermModifiedKeys,
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
        PopKeyboardEnhancementFlags,
        ResetXtermModifiedKeys,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()
}

fn run(terminal: &mut TerminalUi, app: &mut App) -> io::Result<()> {
    loop {
        app.tick();
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

    if key.modifiers.contains(KeyModifiers::CONTROL)
        && key.modifiers.contains(KeyModifiers::SHIFT)
        && is_navigation_key(key.code)
    {
        app.handle_active_key(key);
        return Action::None;
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
            KeyCode::Char('f') | KeyCode::Char('F') => app.open_find_dialog(),
            KeyCode::Char('g') | KeyCode::Char('G') => app.find_next(),
            KeyCode::Char('z') | KeyCode::Char('Z') => app.undo_editor(),
            KeyCode::Char('y') | KeyCode::Char('Y') => app.redo_editor(),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                app.save_current();
            }
            KeyCode::Char('o') | KeyCode::Char('O') => app.open_selected_file(),
            KeyCode::Char('r') | KeyCode::Char('R') => app.run_cargo("run"),
            KeyCode::Char('t') | KeyCode::Char('T') => app.run_cargo("test"),
            KeyCode::Char('b') | KeyCode::Char('B') => app.run_cargo("build"),
            KeyCode::Char('d') | KeyCode::Char('D') => app.start_or_continue_debug(),
            KeyCode::Char(' ') => app.request_completion(true),
            _ => {}
        }
        return Action::None;
    }

    match key.code {
        KeyCode::Esc => {
            if app.completion_visible() {
                app.close_completion();
                return Action::None;
            }
            return Action::Quit;
        }
        KeyCode::F(1) => app.help_open = true,
        KeyCode::F(2) => {
            app.save_current();
        }
        KeyCode::F(3) => app.open_selected_file(),
        KeyCode::F(4) => app.toggle_focus(),
        KeyCode::F(5) if key.modifiers.contains(KeyModifiers::SHIFT) => app.stop_debug(),
        KeyCode::F(5) => app.run_cargo("run"),
        KeyCode::F(6) => app.toggle_breakpoint_at_cursor(),
        KeyCode::F(7) => app.run_cargo("check"),
        KeyCode::F(8) => app.run_cargo("test"),
        KeyCode::F(9) => app.run_cargo("build"),
        KeyCode::F(10) => app.toggle_menu(),
        KeyCode::F(11) if key.modifiers.contains(KeyModifiers::SHIFT) => app.debug_step_out(),
        KeyCode::F(11) => app.debug_step_into(),
        KeyCode::F(12) => app.debug_step_over(),
        _ => app.handle_active_key(key),
    }

    Action::None
}

fn is_navigation_key(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::Left
            | KeyCode::Right
            | KeyCode::Up
            | KeyCode::Down
            | KeyCode::Home
            | KeyCode::End
            | KeyCode::PageUp
            | KeyCode::PageDown
    )
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::handle_key;
    use crate::app::{Action, App, Dialog, Focus};

    #[test]
    fn tab_in_editor_indents_instead_of_cycling_focus() {
        let root = temp_project("main-tab");
        let mut app = App::new_for_tests(root.clone());
        app.dialog = None;
        app.focus = Focus::Editor;
        app.editor.insert_text("letvalue");
        app.editor.set_cursor(0, 3);

        assert_eq!(
            handle_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Action::None
        );

        assert_eq!(app.focus, Focus::Editor);
        assert_eq!(app.editor.lines(), &["let    value".to_string()]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn control_shift_navigation_reaches_editor_selection() {
        let root = temp_project("main-shift-arrow");
        for (code, row, col, expected) in [
            (KeyCode::Left, 1, 1, "d"),
            (KeyCode::Right, 1, 1, "e"),
            (KeyCode::Up, 1, 1, "bc\nd"),
            (KeyCode::Down, 1, 1, "ef\ng"),
            (KeyCode::Home, 1, 2, "de"),
            (KeyCode::End, 1, 1, "ef"),
        ] {
            let mut app = App::new_for_tests(root.clone());
            app.dialog = None;
            app.focus = Focus::Editor;
            app.editor.insert_text("abc\ndef\nghi");
            app.editor.set_cursor(row, col);

            assert_eq!(
                handle_key(
                    &mut app,
                    KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                ),
                Action::None
            );

            assert_eq!(app.editor.selected_text().as_deref(), Some(expected));
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn plain_shift_navigation_reaches_editor_selection() {
        let root = temp_project("main-plain-shift-arrow");
        for (code, row, col, expected) in [
            (KeyCode::Left, 1, 1, "d"),
            (KeyCode::Right, 1, 1, "e"),
            (KeyCode::Up, 1, 1, "bc\nd"),
            (KeyCode::Down, 1, 1, "ef\ng"),
            (KeyCode::Home, 1, 2, "de"),
            (KeyCode::End, 1, 1, "ef"),
        ] {
            let mut app = App::new_for_tests(root.clone());
            app.dialog = None;
            app.focus = Focus::Editor;
            app.editor.insert_text("abc\ndef\nghi");
            app.editor.set_cursor(row, col);

            assert_eq!(
                handle_key(&mut app, KeyEvent::new(code, KeyModifiers::SHIFT)),
                Action::None
            );

            assert_eq!(app.editor.selected_text().as_deref(), Some(expected));
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn alt_shift_navigation_reaches_editor_selection() {
        let root = temp_project("main-alt-shift-arrow");
        for (code, row, col, expected) in [
            (KeyCode::Left, 1, 1, "d"),
            (KeyCode::Right, 1, 1, "e"),
            (KeyCode::Up, 1, 1, "bc\nd"),
            (KeyCode::Down, 1, 1, "ef\ng"),
            (KeyCode::Home, 1, 2, "de"),
            (KeyCode::End, 1, 1, "ef"),
        ] {
            let mut app = App::new_for_tests(root.clone());
            app.dialog = None;
            app.focus = Focus::Editor;
            app.editor.insert_text("abc\ndef\nghi");
            app.editor.set_cursor(row, col);

            assert_eq!(
                handle_key(
                    &mut app,
                    KeyEvent::new(code, KeyModifiers::SHIFT | KeyModifiers::ALT),
                ),
                Action::None
            );

            assert_eq!(app.editor.selected_text().as_deref(), Some(expected));
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn control_f_opens_find_dialog() {
        let root = temp_project("main-find");
        let mut app = App::new_for_tests(root.clone());
        app.dialog = None;
        app.focus = Focus::Editor;

        assert_eq!(
            handle_key(
                &mut app,
                KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
            ),
            Action::None
        );

        assert_eq!(app.dialog, Some(Dialog::Find));

        let _ = fs::remove_dir_all(root);
    }

    fn temp_project(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("trust-{name}-{unique}"));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"trust_test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        root
    }
}
