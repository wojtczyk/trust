use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    debugger::{DebuggerEvent, DebuggerSession, SourceLocation},
    editor::{Editor, SearchMatch},
    ide::{CompletionCandidate, CompletionEngine, CompletionResponse},
    project::{ProjectEntry, list_project_dir},
};

pub const MENUS: [Menu; 9] = [
    Menu {
        title: "File",
        items: &[
            MenuItem::action("New", "", MenuAction::NewFile),
            MenuItem::action("Open", "F3", MenuAction::Open),
            MenuItem::action("Save", "F2", MenuAction::Save),
            MenuItem::separator(),
            MenuItem::action("Quit", "Ctrl+Q", MenuAction::Quit),
        ],
    },
    Menu {
        title: "Edit",
        items: &[
            MenuItem::action("Undo", "Ctrl+Z", MenuAction::Undo),
            MenuItem::action("Redo", "Ctrl+Y", MenuAction::Redo),
            MenuItem::separator(),
            MenuItem::action("Copy", "Ctrl+C", MenuAction::Copy),
            MenuItem::action("Cut", "Ctrl+X", MenuAction::Cut),
            MenuItem::action("Paste", "Ctrl+V", MenuAction::Paste),
            MenuItem::separator(),
            MenuItem::action("Delete line", "Alt+X", MenuAction::DeleteLine),
            MenuItem::action("Duplicate line", "Alt+U", MenuAction::DuplicateLine),
        ],
    },
    Menu {
        title: "Search",
        items: &[
            MenuItem::action("Find", "Ctrl+F", MenuAction::Find),
            MenuItem::action("Find next", "Ctrl+G", MenuAction::FindNext),
        ],
    },
    Menu {
        title: "Run",
        items: &[
            MenuItem::action("Run", "F5", MenuAction::CargoRun),
            MenuItem::action("Test", "F8", MenuAction::CargoTest),
        ],
    },
    Menu {
        title: "Compile",
        items: &[
            MenuItem::action("Check", "F7", MenuAction::CargoCheck),
            MenuItem::action("Build", "F9", MenuAction::CargoBuild),
        ],
    },
    Menu {
        title: "Debug",
        items: &[
            MenuItem::action("Start/Continue", "Ctrl+D", MenuAction::DebugStartOrContinue),
            MenuItem::action("Toggle breakpoint", "F6", MenuAction::ToggleBreakpoint),
            MenuItem::action("Step into", "F11", MenuAction::DebugStepInto),
            MenuItem::action("Step over", "F12", MenuAction::DebugStepOver),
            MenuItem::action("Step out", "Shift+F11", MenuAction::DebugStepOut),
            MenuItem::action("Stop", "Shift+F5", MenuAction::DebugStop),
        ],
    },
    Menu {
        title: "Project",
        items: &[
            MenuItem::action("New project", "", MenuAction::NewProject),
            MenuItem::action("Open manifest", "", MenuAction::OpenManifest),
            MenuItem::action("Refresh tree", "R", MenuAction::RefreshProject),
        ],
    },
    Menu {
        title: "Window",
        items: &[
            MenuItem::action("Project pane", "", MenuAction::FocusProject),
            MenuItem::action("Editor pane", "", MenuAction::FocusEditor),
            MenuItem::action("Messages pane", "", MenuAction::FocusMessages),
            MenuItem::separator(),
            MenuItem::action("Next pane", "F4", MenuAction::ToggleFocus),
        ],
    },
    Menu {
        title: "Help",
        items: &[
            MenuItem::action("Help", "F1", MenuAction::Help),
            MenuItem::action("About", "", MenuAction::About),
        ],
    },
];

#[derive(Debug, Clone, Copy)]
pub struct Menu {
    pub title: &'static str,
    pub items: &'static [MenuItem],
}

#[derive(Debug, Clone, Copy)]
pub struct MenuItem {
    pub label: &'static str,
    pub shortcut: &'static str,
    pub action: MenuAction,
    pub separator: bool,
}

impl MenuItem {
    pub const fn action(label: &'static str, shortcut: &'static str, action: MenuAction) -> Self {
        Self {
            label,
            shortcut,
            action,
            separator: false,
        }
    }

    pub const fn separator() -> Self {
        Self {
            label: "",
            shortcut: "",
            action: MenuAction::None,
            separator: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    None,
    NewFile,
    NewProject,
    Open,
    OpenManifest,
    Save,
    Quit,
    Undo,
    Redo,
    Copy,
    Cut,
    Paste,
    DeleteLine,
    DuplicateLine,
    Find,
    FindNext,
    CargoRun,
    CargoTest,
    CargoCheck,
    CargoBuild,
    DebugStartOrContinue,
    DebugStepInto,
    DebugStepOver,
    DebugStepOut,
    DebugStop,
    ToggleBreakpoint,
    ToggleFocus,
    FocusProject,
    FocusEditor,
    FocusMessages,
    RefreshProject,
    Help,
    About,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MenuGeometry {
    pub bar_items: [Rect; MENUS.len()],
    pub dropdown: Option<Rect>,
    pub run_button: Rect,
    pub debug_button: Rect,
    pub breakpoint_button: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Project,
    Editor,
    Messages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    About,
    CompileResult,
    Find,
    NewFile,
    NewProject,
}

#[derive(Debug, Clone)]
pub struct NewFileForm {
    pub name: String,
}

impl NewFileForm {
    fn new() -> Self {
        Self {
            name: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FindForm {
    pub query: String,
}

impl FindForm {
    fn new() -> Self {
        Self {
            query: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewProjectKind {
    Bin,
    Lib,
}

impl NewProjectKind {
    pub fn toggle(self) -> Self {
        match self {
            Self::Bin => Self::Lib,
            Self::Lib => Self::Bin,
        }
    }

    pub fn cargo_flag(self) -> &'static str {
        match self {
            Self::Bin => "--bin",
            Self::Lib => "--lib",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Lib => "lib",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewProjectField {
    Directory,
    Name,
    Kind,
    Create,
}

#[derive(Debug, Clone)]
pub struct NewProjectForm {
    pub directory: String,
    pub name: String,
    pub kind: NewProjectKind,
    pub field: NewProjectField,
}

impl NewProjectForm {
    fn new(directory: &Path) -> Self {
        Self {
            directory: directory.display().to_string(),
            name: "new_project".to_string(),
            kind: NewProjectKind::Bin,
            field: NewProjectField::Name,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragTarget {
    ProjectDivider,
    MessageDivider,
    EditorSelection,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Geometry {
    pub root: Rect,
    pub menu_area: Rect,
    pub menu: MenuGeometry,
    pub desktop_inner: Rect,
    pub project_area: Rect,
    pub project_inner: Rect,
    pub editor_area: Rect,
    pub editor_inner: Rect,
    pub messages_area: Rect,
    pub messages_inner: Rect,
    pub status_area: Rect,
}

#[derive(Debug, Clone)]
pub struct CompletionPopup {
    pub items: Vec<CompletionCandidate>,
    pub selected: usize,
    pub scroll: usize,
    pub replace_start: usize,
    pub replace_end: usize,
}

#[derive(Debug, Clone)]
pub struct SearchState {
    pub query: String,
    pub matches: Vec<SearchMatch>,
    pub active: Option<usize>,
}

impl SearchState {
    fn new() -> Self {
        Self {
            query: String::new(),
            matches: Vec::new(),
            active: None,
        }
    }
}

impl CompletionPopup {
    fn from_response(response: CompletionResponse) -> Self {
        Self {
            items: response.items,
            selected: 0,
            scroll: 0,
            replace_start: response.replace_start,
            replace_end: response.replace_end,
        }
    }

    fn selected_item(&self) -> Option<&CompletionCandidate> {
        self.items.get(self.selected)
    }

    fn keep_selected_visible(&mut self, visible_rows: usize) {
        let visible_rows = visible_rows.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected + 1 - visible_rows;
        }

        self.scroll = self
            .scroll
            .min(self.items.len().saturating_sub(visible_rows));
    }
}

pub const MIN_PROJECT_PANE_WIDTH: u16 = 18;
pub const MIN_EDITOR_PANE_WIDTH: u16 = 24;
pub const MIN_MESSAGES_PANE_HEIGHT: u16 = 4;
pub const MIN_DESKTOP_HEIGHT: u16 = 8;

#[derive(Debug)]
pub struct App {
    pub root: PathBuf,
    pub browser_dir: PathBuf,
    pub project_files: Vec<ProjectEntry>,
    pub selected_file: usize,
    pub editor: Editor,
    pub focus: Focus,
    pub messages: Vec<String>,
    pub message_scroll: usize,
    pub active_message: usize,
    pub menu_open: bool,
    pub active_menu: usize,
    pub active_menu_item: usize,
    pub help_open: bool,
    pub dialog: Option<Dialog>,
    pub new_file: NewFileForm,
    pub find: FindForm,
    pub new_project: NewProjectForm,
    pub status: String,
    pub project_pane_width: u16,
    pub messages_pane_height: u16,
    pub geometry: Geometry,
    pub completion_popup: Option<CompletionPopup>,
    pub breakpoints: BTreeMap<PathBuf, BTreeSet<usize>>,
    pub debug_location: Option<SourceLocation>,
    search: SearchState,
    completion_engine: CompletionEngine,
    debugger: Option<DebuggerSession>,
    drag_target: Option<DragTarget>,
}

impl App {
    pub fn new(root: PathBuf) -> Self {
        Self::with_completion_engine(root.clone(), CompletionEngine::new(&root))
    }

    fn with_completion_engine(root: PathBuf, completion_engine: CompletionEngine) -> Self {
        let mut app = Self {
            browser_dir: root.clone(),
            new_project: NewProjectForm::new(&root),
            root: root.clone(),
            project_files: Vec::new(),
            selected_file: 0,
            editor: Editor::scratch(),
            focus: Focus::Project,
            messages: Vec::new(),
            message_scroll: 0,
            active_message: 0,
            menu_open: false,
            active_menu: 0,
            active_menu_item: first_selectable_item(0),
            help_open: false,
            dialog: Some(Dialog::About),
            new_file: NewFileForm::new(),
            find: FindForm::new(),
            status: "Welcome to TRUST".to_string(),
            project_pane_width: 30,
            messages_pane_height: 6,
            geometry: Geometry::default(),
            completion_popup: None,
            breakpoints: BTreeMap::new(),
            debug_location: None,
            search: SearchState::new(),
            completion_engine,
            debugger: None,
            drag_target: None,
        };
        app.messages.push("Ready.".to_string());
        app
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(root: PathBuf) -> Self {
        Self::with_completion_engine(root, CompletionEngine::disabled_for_tests())
    }

    pub fn tick(&mut self) {
        while let Some(event) = self.debugger.as_mut().and_then(DebuggerSession::try_recv) {
            match event {
                DebuggerEvent::Output(line) => {
                    if !line.trim().is_empty() {
                        self.push_message(format!("[dbg] {line}"));
                    }
                }
                DebuggerEvent::Stopped(location) => {
                    self.debug_location = Some(location.clone());
                    self.focus_debug_location(&location);
                    self.status = format!(
                        "Paused at {}:{}",
                        relative_label(&self.root, &location.path),
                        location.line + 1
                    );
                }
                DebuggerEvent::Exited(code) => {
                    self.debug_location = None;
                    self.debugger = None;
                    self.status = match code {
                        Some(code) => format!("Debug session exited with {code}"),
                        None => "Debug session exited".to_string(),
                    };
                }
            }
        }
    }

    pub fn refresh_project(&mut self) {
        if !self.browser_dir.is_dir() {
            self.browser_dir = self.root.clone();
        }
        self.project_files = list_project_dir(&self.root, &self.browser_dir);
        if self.selected_file >= self.project_files.len() {
            self.selected_file = self.project_files.len().saturating_sub(1);
        }
        if self.editor.path().is_none() {
            self.open_first_file_in_browser();
        }
    }

    pub fn toggle_focus(&mut self) {
        self.close_menu();
        self.close_completion();
        self.focus = match self.focus {
            Focus::Project => Focus::Editor,
            Focus::Editor => Focus::Messages,
            Focus::Messages => Focus::Project,
        };
        self.status = format!("Focus: {}", self.focus_name());
    }

    fn set_focus(&mut self, focus: Focus) {
        self.close_menu();
        self.close_completion();
        self.focus = focus;
        self.status = format!("Focus: {}", self.focus_name());
    }

    pub fn focus_name(&self) -> &'static str {
        match self.focus {
            Focus::Project => "Project",
            Focus::Editor => "Edit",
            Focus::Messages => "Messages",
        }
    }

    pub fn current_file_label(&self) -> String {
        self.editor
            .path()
            .and_then(|path| path.strip_prefix(&self.root).ok())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    pub fn search_match_ranges_for_line(&self, row: usize) -> Vec<(usize, usize, bool)> {
        self.search
            .matches
            .iter()
            .enumerate()
            .filter(|(_, search_match)| search_match.row == row)
            .map(|(index, search_match)| {
                (
                    search_match.start_col,
                    search_match.end_col,
                    self.search.active == Some(index),
                )
            })
            .collect()
    }

    pub fn search_summary(&self) -> Option<String> {
        if self.search.query.is_empty() {
            return None;
        }

        if self.search.matches.is_empty() {
            return Some("Find 0".to_string());
        }

        let current = self.search.active.unwrap_or(0) + 1;
        Some(format!("Find {current}/{}", self.search.matches.len()))
    }

    pub fn browser_label(&self) -> String {
        self.browser_dir
            .strip_prefix(&self.root)
            .ok()
            .filter(|path| !path.as_os_str().is_empty())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| ".".to_string())
    }

    pub fn open_selected_file(&mut self) {
        self.close_menu();
        let Some(entry) = self.project_files.get(self.selected_file) else {
            self.status = "No editable project files found".to_string();
            return;
        };

        if entry.is_directory() {
            self.navigate_to_project_dir(entry.path.clone());
            return;
        }

        let path = entry.path.clone();
        let label = entry.label.clone();
        self.open_file_path(path, label);
    }

    fn open_manifest(&mut self) {
        self.close_menu();
        let path = self.root.join("Cargo.toml");
        if !path.exists() {
            self.status = "No Cargo.toml found in the project root".to_string();
            return;
        }

        self.open_file_path(path, "Cargo.toml");
    }

    fn open_file_path(&mut self, path: PathBuf, label: impl Into<String>) -> bool {
        if !self.save_dirty_before("Open file") {
            return false;
        }

        let label = label.into();
        match Editor::open(&path) {
            Ok(editor) => {
                self.editor = editor;
                self.close_completion();
                self.refresh_search_matches(true);
                self.focus = Focus::Editor;
                self.status = format!("Opened {label}");
                self.push_message(format!("Opened {}", path.display()));
                true
            }
            Err(error) => {
                self.status = format!("Open failed: {error}");
                false
            }
        }
    }

    fn open_first_file_in_browser(&mut self) {
        let index = self
            .project_files
            .iter()
            .position(|entry| {
                entry.is_file()
                    && entry.path.file_name().and_then(|name| name.to_str()) == Some("Cargo.toml")
            })
            .or_else(|| self.project_files.iter().position(ProjectEntry::is_file));

        if let Some(index) = index {
            self.selected_file = index;
            self.open_selected_file();
        }
    }

    fn navigate_to_project_dir(&mut self, path: PathBuf) {
        let path = path.canonicalize().unwrap_or(path);
        self.browser_dir = if path.starts_with(&self.root) {
            path
        } else {
            self.root.clone()
        };
        self.close_completion();
        self.selected_file = 0;
        self.refresh_project();
        self.focus = Focus::Project;
        self.status = format!("Browsing {}", self.browser_label());
    }

    pub fn save_current(&mut self) -> bool {
        self.close_menu();
        self.close_completion();
        match self.save_editor() {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("Save failed: {error}");
                false
            }
        }
    }

    pub fn run_cargo(&mut self, command: &str) {
        self.close_menu();
        self.close_completion();
        if !self.save_dirty_before("Cargo command") {
            return;
        }

        self.push_message(format!("$ cargo {command}"));
        self.status = format!("Running cargo {command}...");
        self.dialog = None;

        let output = Command::new("cargo")
            .arg(command)
            .current_dir(&self.root)
            .output();

        match output {
            Ok(output) => {
                let mut lines = Vec::new();
                lines.extend(output_lines(&output.stdout));
                lines.extend(output_lines(&output.stderr));
                if lines.is_empty() {
                    lines.push("(cargo produced no output)".to_string());
                }
                for line in lines {
                    self.push_message(line);
                }

                let code = output
                    .status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_string());
                self.status = if output.status.success() {
                    format!("cargo {command} finished successfully")
                } else {
                    format!("cargo {command} exited with {code}")
                };
                self.dialog = Some(Dialog::CompileResult);
                self.focus = Focus::Messages;
            }
            Err(error) => {
                self.status = format!("Could not run cargo {command}: {error}");
                self.push_message(self.status.clone());
            }
        }
    }

    fn save_dirty_before(&mut self, action: &str) -> bool {
        if !self.editor.is_dirty() {
            return true;
        }

        match self.save_editor() {
            Ok(()) => true,
            Err(error) => {
                self.status = format!("{action} canceled: save failed: {error}");
                self.push_message(self.status.clone());
                false
            }
        }
    }

    fn save_editor(&mut self) -> io::Result<()> {
        self.editor.save()?;
        let label = self.current_file_label();
        self.status = format!("Saved {label}");
        self.push_message(format!("Saved {label}"));
        Ok(())
    }

    pub fn open_find_dialog(&mut self) {
        self.close_menu();
        self.close_completion();
        if let Some(selection) = self.editor.selected_text() {
            if !selection.is_empty() && !selection.contains('\n') {
                self.find.query = selection;
            }
        } else if self.find.query.is_empty() {
            self.find.query = self.search.query.clone();
        }
        self.dialog = Some(Dialog::Find);
        self.refresh_search_matches(true);
        self.status = if self.find.query.is_empty() {
            "Find: type a query".to_string()
        } else if self.search.matches.is_empty() {
            format!("Find: no matches for {:?}", self.find.query)
        } else {
            format!("Find: {} match(es)", self.search.matches.len())
        };
    }

    pub fn find_next(&mut self) {
        self.close_menu();
        self.close_completion();
        self.dialog = None;
        self.help_open = false;

        if self.search.query.is_empty() {
            self.open_find_dialog();
            return;
        }

        if self.search.matches.is_empty() {
            self.status = format!("Find: no matches for {:?}", self.search.query);
            return;
        }

        if let Some(index) = self.search.active {
            if !self.active_search_is_at_cursor(index) {
                self.activate_search_match(index);
                return;
            }
        }

        let next = match self.search.active {
            Some(index) => (index + 1) % self.search.matches.len(),
            None => self.search_index_from_cursor(false).unwrap_or(0),
        };
        self.activate_search_match(next);
    }

    pub fn find_previous(&mut self) {
        self.close_menu();
        self.close_completion();
        self.dialog = None;
        self.help_open = false;

        if self.search.query.is_empty() {
            self.open_find_dialog();
            return;
        }

        if self.search.matches.is_empty() {
            self.status = format!("Find: no matches for {:?}", self.search.query);
            return;
        }

        if let Some(index) = self.search.active {
            if !self.active_search_is_at_cursor(index) {
                self.activate_search_match(index);
                return;
            }
        }

        let previous = match self.search.active {
            Some(0) => self.search.matches.len() - 1,
            Some(index) => index - 1,
            None => self
                .search_index_from_cursor(true)
                .unwrap_or(self.search.matches.len() - 1),
        };
        self.activate_search_match(previous);
    }

    fn refresh_search_matches(&mut self, preserve_active: bool) {
        let query = self.find.query.clone();
        if query.is_empty() {
            self.search = SearchState::new();
            return;
        }

        let previous_active = if preserve_active {
            self.search
                .active
                .and_then(|index| self.search.matches.get(index))
                .copied()
        } else {
            None
        };

        self.search.query = query;
        self.search.matches = self.editor.find_matches(&self.search.query);
        self.search.active = previous_active
            .and_then(|search_match| {
                self.search
                    .matches
                    .iter()
                    .position(|candidate| *candidate == search_match)
            })
            .or_else(|| self.search_index_from_cursor(false));
    }

    fn search_index_from_cursor(&self, reverse: bool) -> Option<usize> {
        let row = self.editor.cursor_row();
        let col = self.editor.cursor_col();
        if reverse {
            self.search
                .matches
                .iter()
                .rposition(|search_match| {
                    search_match.row < row
                        || (search_match.row == row && search_match.start_col < col)
                })
                .or_else(|| self.search.matches.len().checked_sub(1))
        } else {
            self.search
                .matches
                .iter()
                .position(|search_match| {
                    search_match.row > row
                        || (search_match.row == row && search_match.start_col >= col)
                })
                .or_else(|| {
                    if self.search.matches.is_empty() {
                        None
                    } else {
                        Some(0)
                    }
                })
        }
    }

    fn activate_search_match(&mut self, index: usize) {
        let Some(search_match) = self.search.matches.get(index).copied() else {
            return;
        };
        self.search.active = Some(index);
        self.focus = Focus::Editor;
        self.editor
            .set_cursor(search_match.row, search_match.start_col);
        self.status = format!(
            "Match {}/{} at line {}, column {}",
            index + 1,
            self.search.matches.len(),
            search_match.row + 1,
            search_match.start_col + 1
        );
    }

    fn active_search_is_at_cursor(&self, index: usize) -> bool {
        self.search.matches.get(index).is_some_and(|search_match| {
            self.editor.cursor_row() == search_match.row
                && self.editor.cursor_col() == search_match.start_col
        })
    }

    pub fn request_completion(&mut self, force: bool) {
        let Some(response) =
            self.completion_engine
                .complete(&self.root, &self.editor, &self.project_files, force)
        else {
            if force {
                self.status = if self.completion_engine.is_language_server_available() {
                    "No completions available here".to_string()
                } else {
                    "No completions available (rust-analyzer unavailable, using fallback mode)"
                        .to_string()
                };
            }
            self.close_completion();
            return;
        };

        let item_count = response.items.len();
        self.completion_popup = Some(CompletionPopup::from_response(response));
        self.status = if force && self.completion_engine.is_language_server_available() {
            format!("Autocomplete: {item_count} suggestion(s)")
        } else {
            format!("Autocomplete fallback: {item_count} suggestion(s)")
        };
    }

    pub fn close_completion(&mut self) {
        self.completion_popup = None;
    }

    pub fn completion_visible(&self) -> bool {
        self.completion_popup.is_some()
    }

    fn accept_completion(&mut self) -> bool {
        let Some(popup) = self.completion_popup.clone() else {
            return false;
        };
        let Some(item) = popup.selected_item() else {
            return false;
        };
        self.editor.replace_range_in_current_line(
            popup.replace_start,
            popup.replace_end,
            &item.insert_text,
        );
        self.status = format!("Inserted completion {}", item.label);
        self.close_completion();
        true
    }

    pub fn toggle_breakpoint_at_cursor(&mut self) {
        let Some(path) = self.editor.path().map(Path::to_path_buf) else {
            self.status = "Breakpoints require a file-backed buffer".to_string();
            return;
        };
        let line = self.editor.cursor_row();
        self.toggle_breakpoint(path, line);
    }

    fn toggle_breakpoint(&mut self, path: PathBuf, line: usize) {
        let Some(lines) = self.breakpoints.get_mut(&path) else {
            let mut set = BTreeSet::new();
            set.insert(line);
            self.breakpoints.insert(path.clone(), set);
            self.status = format!(
                "Breakpoint added at {}:{}",
                relative_label(&self.root, &path),
                line + 1
            );
            return;
        };

        if lines.remove(&line) {
            if lines.is_empty() {
                self.breakpoints.remove(&path);
            }
            self.status = format!(
                "Breakpoint removed at {}:{}",
                relative_label(&self.root, &path),
                line + 1
            );
        } else {
            lines.insert(line);
            self.status = format!(
                "Breakpoint added at {}:{}",
                relative_label(&self.root, &path),
                line + 1
            );
        }
    }

    pub fn has_breakpoint(&self, path: &Path, line: usize) -> bool {
        self.breakpoints
            .get(path)
            .is_some_and(|lines| lines.contains(&line))
    }

    pub fn start_or_continue_debug(&mut self) {
        self.close_menu();
        self.close_completion();
        if self.debugger.is_some() {
            self.debug_command("continue", "Continuing debugger");
            return;
        }

        if !self.save_dirty_before("Debug start") {
            return;
        }

        let breakpoints = self
            .breakpoints
            .iter()
            .flat_map(|(path, lines)| {
                lines.iter().map(|line| SourceLocation {
                    path: path.clone(),
                    line: *line,
                })
            })
            .collect::<Vec<_>>();

        self.push_message("$ cargo build");
        match DebuggerSession::start(&self.root, &breakpoints) {
            Ok(session) => {
                self.debugger = Some(session);
                self.debug_location = None;
                self.focus = Focus::Messages;
                self.status = if breakpoints.is_empty() {
                    "Debugging without breakpoints".to_string()
                } else {
                    format!("Debugging with {} breakpoint(s)", breakpoints.len())
                };
            }
            Err(error) => {
                self.status = format!("Could not start debugger: {error}");
                self.push_message(self.status.clone());
            }
        }
    }

    pub fn stop_debug(&mut self) {
        if let Some(mut debugger) = self.debugger.take() {
            let _ = debugger.stop();
            self.status = "Debug session stopped".to_string();
        } else {
            self.status = "No active debug session".to_string();
        }
        self.debug_location = None;
    }

    pub fn debug_step_into(&mut self) {
        self.debug_command("step", "Step into");
    }

    pub fn debug_step_over(&mut self) {
        self.debug_command("next", "Step over");
    }

    pub fn debug_step_out(&mut self) {
        self.debug_command("finish", "Step out");
    }

    fn debug_command(&mut self, command: &str, status: &str) {
        let Some(debugger) = self.debugger.as_mut() else {
            self.status = "No active debug session".to_string();
            return;
        };
        match debugger.send(command) {
            Ok(()) => {
                self.status = status.to_string();
                self.focus = Focus::Messages;
            }
            Err(error) => self.status = format!("Debugger command failed: {error}"),
        }
    }

    pub fn debug_active(&self) -> bool {
        self.debugger.is_some()
    }

    fn focus_debug_location(&mut self, location: &SourceLocation) {
        let current = self.editor.path().map(Path::to_path_buf);
        if current.as_ref() != Some(&location.path) {
            let label = relative_label(&self.root, &location.path);
            if !self.open_file_path(location.path.clone(), label) {
                return;
            }
        }
        self.editor.set_cursor(location.line, 0);
        self.focus = Focus::Editor;
    }

    pub fn handle_active_key(&mut self, key: KeyEvent) {
        if self.dialog.take().is_some() {
            return;
        }

        if self.menu_open {
            return;
        }

        match self.focus {
            Focus::Project => self.handle_project_key(key),
            Focus::Editor => self.handle_editor_key(key),
            Focus::Messages => self.handle_message_key(key),
        }
    }

    pub fn handle_dialog_key(&mut self, key: KeyEvent) -> Action {
        match self.dialog {
            Some(Dialog::Find) => self.handle_find_key(key),
            Some(Dialog::NewFile) => self.handle_new_file_key(key),
            Some(Dialog::NewProject) => self.handle_new_project_key(key),
            Some(Dialog::About | Dialog::CompileResult) => {
                self.dialog = None;
                Action::None
            }
            None => Action::None,
        }
    }

    fn handle_new_file_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => self.dialog = None,
            KeyCode::Enter => self.create_new_file(),
            KeyCode::Backspace => {
                self.new_file.name.pop();
            }
            KeyCode::Char(character) => self.new_file.name.push(character),
            _ => {}
        }

        Action::None
    }

    fn handle_find_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => self.dialog = None,
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.find_previous();
                } else {
                    self.find_next();
                }
            }
            KeyCode::Backspace => {
                self.find.query.pop();
                self.refresh_search_matches(false);
                self.status = if self.find.query.is_empty() {
                    "Find: type a query".to_string()
                } else if self.search.matches.is_empty() {
                    format!("Find: no matches for {:?}", self.find.query)
                } else {
                    format!("Find: {} match(es)", self.search.matches.len())
                };
            }
            KeyCode::Char(character)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.find.query.push(character);
                self.refresh_search_matches(false);
                self.status = if self.search.matches.is_empty() {
                    format!("Find: no matches for {:?}", self.find.query)
                } else {
                    format!("Find: {} match(es)", self.search.matches.len())
                };
            }
            _ => {}
        }

        Action::None
    }

    fn handle_new_project_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc => self.dialog = None,
            KeyCode::Tab | KeyCode::Down => self.next_new_project_field(),
            KeyCode::BackTab | KeyCode::Up => self.previous_new_project_field(),
            KeyCode::Left | KeyCode::Right if self.new_project.field == NewProjectField::Kind => {
                self.new_project.kind = self.new_project.kind.toggle();
            }
            KeyCode::Enter => {
                if self.new_project.field == NewProjectField::Create {
                    self.create_new_project();
                } else if self.new_project.field == NewProjectField::Kind {
                    self.new_project.kind = self.new_project.kind.toggle();
                } else {
                    self.next_new_project_field();
                }
            }
            KeyCode::Backspace => self.backspace_new_project_field(),
            KeyCode::Char(character) => self.type_new_project_char(character),
            _ => {}
        }

        Action::None
    }

    fn open_new_file_dialog(&mut self) {
        self.close_menu();
        self.close_completion();
        self.new_file = NewFileForm::new();
        self.dialog = Some(Dialog::NewFile);
        self.status = format!("New file in {}", self.browser_label());
    }

    fn open_new_project_dialog(&mut self) {
        self.close_menu();
        self.close_completion();
        self.new_project = NewProjectForm::new(&self.browser_dir);
        self.dialog = Some(Dialog::NewProject);
        self.status = "New Cargo project".to_string();
    }

    fn next_new_project_field(&mut self) {
        self.new_project.field = match self.new_project.field {
            NewProjectField::Directory => NewProjectField::Name,
            NewProjectField::Name => NewProjectField::Kind,
            NewProjectField::Kind => NewProjectField::Create,
            NewProjectField::Create => NewProjectField::Directory,
        };
    }

    fn previous_new_project_field(&mut self) {
        self.new_project.field = match self.new_project.field {
            NewProjectField::Directory => NewProjectField::Create,
            NewProjectField::Name => NewProjectField::Directory,
            NewProjectField::Kind => NewProjectField::Name,
            NewProjectField::Create => NewProjectField::Kind,
        };
    }

    fn backspace_new_project_field(&mut self) {
        match self.new_project.field {
            NewProjectField::Directory => {
                self.new_project.directory.pop();
            }
            NewProjectField::Name => {
                self.new_project.name.pop();
            }
            NewProjectField::Kind | NewProjectField::Create => {}
        }
    }

    fn type_new_project_char(&mut self, character: char) {
        match self.new_project.field {
            NewProjectField::Directory => self.new_project.directory.push(character),
            NewProjectField::Name => self.new_project.name.push(character),
            NewProjectField::Kind => match character.to_ascii_lowercase() {
                'b' => self.new_project.kind = NewProjectKind::Bin,
                'l' => self.new_project.kind = NewProjectKind::Lib,
                ' ' => self.new_project.kind = self.new_project.kind.toggle(),
                _ => {}
            },
            NewProjectField::Create => {
                if character == ' ' {
                    self.create_new_project();
                }
            }
        }
    }

    fn create_new_file(&mut self) {
        let name = self.new_file.name.trim();
        if name.is_empty() {
            self.status = "Filename is required".to_string();
            return;
        }

        let requested = Path::new(name);
        if !is_project_relative_path(requested) {
            self.status = "Use a relative filename inside this project".to_string();
            return;
        }

        let target = self.browser_dir.join(requested);
        let parent = target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.browser_dir.clone());

        if let Err(error) = fs::create_dir_all(&parent) {
            self.status = format!("Could not create parent directory: {error}");
            return;
        }

        if let Err(error) = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            self.status = format!("Could not create file: {error}");
            return;
        }

        let target = target.canonicalize().unwrap_or(target);
        let label = target
            .strip_prefix(&self.root)
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| target.display().to_string());
        self.dialog = None;
        self.push_message(format!("Created {}", target.display()));
        self.refresh_project();
        if let Some(index) = self
            .project_files
            .iter()
            .position(|entry| entry.path == target)
        {
            self.selected_file = index;
        }
        self.open_file_path(target, label);
    }

    fn create_new_project(&mut self) {
        let parent = self.new_project_parent_dir();
        let name = self.new_project.name.trim().to_string();
        let kind = self.new_project.kind;
        if name.is_empty() {
            self.status = "Project name is required".to_string();
            return;
        }

        if !self.save_dirty_before("New project") {
            return;
        }

        if let Err(error) = fs::create_dir_all(&parent) {
            self.status = format!("Could not create parent directory: {error}");
            return;
        }

        let target = parent.join(&name);
        self.push_message(format!(
            "$ cargo new {} {}",
            kind.cargo_flag(),
            target.display()
        ));

        let output = Command::new("cargo")
            .arg("new")
            .arg(kind.cargo_flag())
            .arg(&target)
            .output();

        match output {
            Ok(output) => {
                for line in output_lines(&output.stdout)
                    .into_iter()
                    .chain(output_lines(&output.stderr))
                {
                    self.push_message(line);
                }

                if output.status.success() {
                    self.stop_debug();
                    self.root = target.canonicalize().unwrap_or(target);
                    self.browser_dir = self.root.clone();
                    self.selected_file = 0;
                    self.editor = Editor::scratch();
                    self.breakpoints.clear();
                    self.completion_engine.refresh_root(&self.root);
                    self.close_completion();
                    self.dialog = None;
                    self.refresh_project();
                    self.status = format!("Created {} project {}", kind.label(), name);
                } else {
                    let code = output
                        .status
                        .code()
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string());
                    self.status = format!("cargo new exited with {code}");
                }
            }
            Err(error) => self.status = format!("Could not run cargo new: {error}"),
        }
    }

    fn new_project_parent_dir(&self) -> PathBuf {
        let directory = self.new_project.directory.trim();
        let path = if directory.is_empty() {
            self.browser_dir.clone()
        } else {
            PathBuf::from(directory)
        };

        if path.is_absolute() {
            path
        } else {
            self.root.join(path)
        }
    }

    pub fn open_menu(&mut self) {
        self.close_completion();
        self.menu_open = true;
        self.active_menu = self.active_menu.min(MENUS.len() - 1);
        self.active_menu_item = first_selectable_item(self.active_menu);
        self.status = format!("Menu: {}", MENUS[self.active_menu].title);
    }

    pub fn close_menu(&mut self) {
        self.menu_open = false;
    }

    pub fn toggle_menu(&mut self) {
        if self.menu_open {
            self.close_menu();
        } else {
            self.open_menu();
        }
    }

    pub fn handle_menu_key(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Esc | KeyCode::F(10) => self.close_menu(),
            KeyCode::Left => self.select_previous_menu(),
            KeyCode::Right => self.select_next_menu(),
            KeyCode::Up => self.select_previous_menu_item(),
            KeyCode::Down => self.select_next_menu_item(),
            KeyCode::Home => self.select_menu(0),
            KeyCode::End => self.select_menu(MENUS.len().saturating_sub(1)),
            KeyCode::Enter => return self.activate_selected_menu_item(),
            KeyCode::Char(character) => {
                if let Some(menu_index) = menu_index_for_hotkey(character) {
                    if menu_index == self.active_menu {
                        return self.activate_selected_menu_item();
                    }
                    self.select_menu(menu_index);
                } else if let Some(item_index) = item_index_for_hotkey(self.active_menu, character)
                {
                    self.active_menu_item = item_index;
                    return self.activate_selected_menu_item();
                }
            }
            _ => {}
        }

        Action::None
    }

    fn select_menu(&mut self, index: usize) {
        self.active_menu = index.min(MENUS.len().saturating_sub(1));
        self.active_menu_item = first_selectable_item(self.active_menu);
        self.status = format!("Menu: {}", MENUS[self.active_menu].title);
    }

    fn select_previous_menu(&mut self) {
        let index = if self.active_menu == 0 {
            MENUS.len() - 1
        } else {
            self.active_menu - 1
        };
        self.select_menu(index);
    }

    fn select_next_menu(&mut self) {
        self.select_menu((self.active_menu + 1) % MENUS.len());
    }

    fn select_previous_menu_item(&mut self) {
        let items = MENUS[self.active_menu].items;
        let mut index = self.active_menu_item;
        for _ in 0..items.len() {
            index = if index == 0 {
                items.len() - 1
            } else {
                index - 1
            };
            if !items[index].separator {
                self.active_menu_item = index;
                break;
            }
        }
    }

    fn select_next_menu_item(&mut self) {
        let items = MENUS[self.active_menu].items;
        let mut index = self.active_menu_item;
        for _ in 0..items.len() {
            index = (index + 1) % items.len();
            if !items[index].separator {
                self.active_menu_item = index;
                break;
            }
        }
    }

    fn activate_selected_menu_item(&mut self) -> Action {
        let item = MENUS[self.active_menu].items[self.active_menu_item];
        self.perform_menu_action(item.action)
    }

    fn perform_menu_action(&mut self, action: MenuAction) -> Action {
        self.close_menu();
        match action {
            MenuAction::None => {}
            MenuAction::NewFile => self.open_new_file_dialog(),
            MenuAction::NewProject => self.open_new_project_dialog(),
            MenuAction::Open => self.open_selected_file(),
            MenuAction::OpenManifest => self.open_manifest(),
            MenuAction::Save => {
                self.save_current();
            }
            MenuAction::Quit => return Action::Quit,
            MenuAction::Undo => self.undo_editor(),
            MenuAction::Redo => self.redo_editor(),
            MenuAction::Copy => self.copy_selection(),
            MenuAction::Cut => self.cut_selection(),
            MenuAction::Paste => self.paste_from_clipboard(),
            MenuAction::DeleteLine => {
                self.editor.delete_line();
                self.refresh_search_matches(true);
            }
            MenuAction::DuplicateLine => {
                self.editor.duplicate_line();
                self.refresh_search_matches(true);
            }
            MenuAction::Find => self.open_find_dialog(),
            MenuAction::FindNext => self.find_next(),
            MenuAction::CargoRun => self.run_cargo("run"),
            MenuAction::CargoTest => self.run_cargo("test"),
            MenuAction::CargoCheck => self.run_cargo("check"),
            MenuAction::CargoBuild => self.run_cargo("build"),
            MenuAction::DebugStartOrContinue => self.start_or_continue_debug(),
            MenuAction::DebugStepInto => self.debug_step_into(),
            MenuAction::DebugStepOver => self.debug_step_over(),
            MenuAction::DebugStepOut => self.debug_step_out(),
            MenuAction::DebugStop => self.stop_debug(),
            MenuAction::ToggleBreakpoint => self.toggle_breakpoint_at_cursor(),
            MenuAction::ToggleFocus => self.toggle_focus(),
            MenuAction::FocusProject => self.set_focus(Focus::Project),
            MenuAction::FocusEditor => self.set_focus(Focus::Editor),
            MenuAction::FocusMessages => self.set_focus(Focus::Messages),
            MenuAction::RefreshProject => {
                self.refresh_project();
                self.status = "Project tree refreshed".to_string();
            }
            MenuAction::Help => self.help_open = true,
            MenuAction::About => self.dialog = Some(Dialog::About),
        }

        Action::None
    }

    pub fn copy_selection(&mut self) {
        let Some(text) = self.editor.selected_text() else {
            self.status = "No editor selection to copy".to_string();
            return;
        };

        match crate::clipboard::set_text(&text) {
            Ok(()) => self.status = format!("Copied {} characters", text.chars().count()),
            Err(error) => self.status = format!("Copy failed: {error}"),
        }
    }

    pub fn cut_selection(&mut self) {
        let Some(text) = self.editor.selected_text() else {
            self.status = "No editor selection to cut".to_string();
            return;
        };

        match crate::clipboard::set_text(&text) {
            Ok(()) => {
                self.editor.cut_selection();
                self.refresh_search_matches(true);
                self.status = format!("Cut {} characters", text.chars().count());
            }
            Err(error) => self.status = format!("Cut failed: {error}"),
        }
    }

    pub fn undo_editor(&mut self) {
        self.close_menu();
        self.dialog = None;
        self.help_open = false;
        self.focus = Focus::Editor;
        if self.editor.undo() {
            self.refresh_search_matches(true);
            self.status = if self.editor.can_undo() {
                "Undo".to_string()
            } else {
                "Undo reached the oldest change".to_string()
            };
        } else {
            self.status = "Nothing to undo".to_string();
        }
    }

    pub fn redo_editor(&mut self) {
        self.close_menu();
        self.dialog = None;
        self.help_open = false;
        self.focus = Focus::Editor;
        if self.editor.redo() {
            self.refresh_search_matches(true);
            self.status = if self.editor.can_redo() {
                "Redo".to_string()
            } else {
                "Redo reached the newest change".to_string()
            };
        } else {
            self.status = "Nothing to redo".to_string();
        }
    }

    pub fn paste_from_clipboard(&mut self) {
        match crate::clipboard::get_text() {
            Ok(text) => self.paste_text(&text),
            Err(error) => self.status = format!("Paste failed: {error}"),
        }
    }

    pub fn paste_text(&mut self, text: &str) {
        if text.is_empty() {
            self.status = "Clipboard is empty".to_string();
            return;
        }

        self.dialog = None;
        self.help_open = false;
        self.focus = Focus::Editor;
        self.editor.insert_text(text);
        self.refresh_search_matches(true);
        self.close_completion();
        self.status = format!("Pasted {} characters", text.chars().count());
    }

    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Action {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_mouse_down(mouse.column, mouse.row)
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.handle_mouse_drag(mouse.column, mouse.row);
                Action::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.drag_target = None;
                Action::None
            }
            MouseEventKind::ScrollUp => {
                self.handle_mouse_scroll(mouse.column, mouse.row, -3);
                Action::None
            }
            MouseEventKind::ScrollDown => {
                self.handle_mouse_scroll(mouse.column, mouse.row, 3);
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_mouse_down(&mut self, column: u16, row: u16) -> Action {
        self.drag_target = None;

        if self.help_open {
            self.help_open = false;
            return Action::None;
        }

        if let Some(dialog) = self.dialog {
            if matches!(dialog, Dialog::NewFile | Dialog::NewProject) {
                return Action::None;
            }
            self.dialog = None;
            return Action::None;
        }

        if contains(self.geometry.menu.run_button, column, row) {
            self.run_cargo("run");
            return Action::None;
        }

        if contains(self.geometry.menu.debug_button, column, row) {
            self.start_or_continue_debug();
            return Action::None;
        }

        if contains(self.geometry.menu.breakpoint_button, column, row) {
            self.toggle_breakpoint_at_cursor();
            return Action::None;
        }

        if contains(self.geometry.menu_area, column, row) {
            self.open_menu();
            if let Some(menu_index) = menu_bar_index_at(&self.geometry.menu, column, row) {
                self.select_menu(menu_index);
            }
            return Action::None;
        }

        if self.menu_open {
            if let Some((menu_index, item_index)) =
                menu_dropdown_item_at(&self.geometry.menu, self.active_menu, column, row)
            {
                self.select_menu(menu_index);
                self.active_menu_item = item_index;
                return self.activate_selected_menu_item();
            }
            self.close_menu();
        }

        if self.is_project_divider(column, row) {
            self.drag_target = Some(DragTarget::ProjectDivider);
            self.resize_project_pane(column);
            return Action::None;
        }

        if self.is_message_divider(column, row) {
            self.drag_target = Some(DragTarget::MessageDivider);
            self.resize_messages_pane(row);
            return Action::None;
        }

        if contains(self.geometry.project_inner, column, row) {
            self.close_completion();
            self.focus = Focus::Project;
            self.select_project_file_at(row);
        } else if contains(self.geometry.editor_inner, column, row) {
            self.focus = Focus::Editor;
            if self.is_breakpoint_gutter(column) {
                let file_row = self
                    .editor
                    .row_offset()
                    .saturating_add(row.saturating_sub(self.geometry.editor_inner.y) as usize);
                if let Some(path) = self.editor.path().map(Path::to_path_buf) {
                    self.toggle_breakpoint(path, file_row);
                }
                return Action::None;
            }
            self.place_cursor_at(column, row, false);
            self.drag_target = Some(DragTarget::EditorSelection);
        } else if contains(self.geometry.messages_inner, column, row) {
            self.close_completion();
            self.focus = Focus::Messages;
            self.select_message_at(row);
            self.status = "Focus: Messages".to_string();
        } else if contains(self.geometry.project_area, column, row) {
            self.close_completion();
            self.focus = Focus::Project;
            self.status = "Focus: Project".to_string();
        } else if contains(self.geometry.editor_area, column, row) {
            self.focus = Focus::Editor;
            self.status = "Focus: Edit".to_string();
        } else if contains(self.geometry.messages_area, column, row) {
            self.close_completion();
            self.focus = Focus::Messages;
            self.status = "Focus: Messages".to_string();
        }

        Action::None
    }

    fn handle_mouse_drag(&mut self, column: u16, row: u16) {
        match self.drag_target {
            Some(DragTarget::ProjectDivider) => self.resize_project_pane(column),
            Some(DragTarget::MessageDivider) => self.resize_messages_pane(row),
            Some(DragTarget::EditorSelection) => self.place_cursor_at(column, row, true),
            None => {}
        }
    }

    fn handle_mouse_scroll(&mut self, column: u16, row: u16, amount: isize) {
        if self.dialog.is_some() || self.help_open {
            return;
        }

        if contains(self.geometry.project_inner, column, row) {
            self.focus = Focus::Project;
            if amount < 0 {
                for _ in 0..amount.unsigned_abs() {
                    self.select_previous_file();
                }
            } else {
                for _ in 0..amount as usize {
                    self.select_next_file();
                }
            }
        } else if contains(self.geometry.messages_inner, column, row) {
            self.focus = Focus::Messages;
            if amount < 0 {
                self.select_message_delta(-(amount.unsigned_abs() as isize));
            } else {
                self.select_message_delta(amount);
            }
        } else if contains(self.geometry.editor_inner, column, row) {
            self.focus = Focus::Editor;
            if amount < 0 {
                self.editor.page_up(amount.unsigned_abs());
            } else {
                self.editor.page_down(amount as usize);
            }
        }
    }

    fn handle_project_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.select_previous_file(),
            KeyCode::Down => self.select_next_file(),
            KeyCode::Home => self.selected_file = 0,
            KeyCode::End => {
                self.selected_file = self.project_files.len().saturating_sub(1);
            }
            KeyCode::Enter => self.open_selected_file(),
            KeyCode::Backspace => {
                if self.browser_dir != self.root {
                    let parent = self
                        .browser_dir
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_else(|| self.root.clone());
                    self.navigate_to_project_dir(parent);
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                self.refresh_project();
                self.status = "Project tree refreshed".to_string();
            }
            _ => {}
        }
    }

    fn handle_editor_key(&mut self, key: KeyEvent) {
        if !is_selection_navigation_key(key) && self.handle_completion_popup_key(key) {
            return;
        }

        if key.modifiers.contains(KeyModifiers::ALT) {
            let handled = match key.code {
                KeyCode::Char('x') | KeyCode::Char('X') => {
                    self.editor.delete_line();
                    self.refresh_search_matches(true);
                    true
                }
                KeyCode::Char('u') | KeyCode::Char('U') => {
                    self.editor.duplicate_line();
                    self.refresh_search_matches(true);
                    true
                }
                _ => false,
            };

            if handled {
                self.close_completion();
                return;
            }
        }

        let selecting = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Left if selecting => {
                self.editor.extend_left();
                self.close_completion();
            }
            KeyCode::Left => {
                self.editor.move_left();
                self.close_completion();
            }
            KeyCode::Right if selecting => {
                self.editor.extend_right();
                self.close_completion();
            }
            KeyCode::Right => {
                self.editor.move_right();
                self.close_completion();
            }
            KeyCode::Up if selecting => {
                self.editor.extend_up();
                self.close_completion();
            }
            KeyCode::Up => {
                self.editor.move_up();
                self.close_completion();
            }
            KeyCode::Down if selecting => {
                self.editor.extend_down();
                self.close_completion();
            }
            KeyCode::Down => {
                self.editor.move_down();
                self.close_completion();
            }
            KeyCode::Home if selecting => {
                self.editor.extend_home();
                self.close_completion();
            }
            KeyCode::Home => {
                self.editor.home();
                self.close_completion();
            }
            KeyCode::End if selecting => {
                self.editor.extend_end();
                self.close_completion();
            }
            KeyCode::End => {
                self.editor.end();
                self.close_completion();
            }
            KeyCode::PageUp if selecting => {
                self.editor.extend_page_up(12);
                self.close_completion();
            }
            KeyCode::PageUp => {
                self.editor.page_up(12);
                self.close_completion();
            }
            KeyCode::PageDown if selecting => {
                self.editor.extend_page_down(12);
                self.close_completion();
            }
            KeyCode::PageDown => {
                self.editor.page_down(12);
                self.close_completion();
            }
            KeyCode::Backspace => {
                self.editor.backspace();
                self.refresh_search_matches(true);
                self.request_completion(false);
            }
            KeyCode::Delete => {
                self.editor.delete();
                self.refresh_search_matches(true);
                self.request_completion(false);
            }
            KeyCode::Enter => {
                self.editor.insert_newline();
                self.refresh_search_matches(true);
                self.close_completion();
            }
            KeyCode::BackTab => {
                self.editor.unindent();
                self.refresh_search_matches(true);
                self.close_completion();
            }
            KeyCode::Tab if selecting => {
                self.editor.unindent();
                self.refresh_search_matches(true);
                self.close_completion();
            }
            KeyCode::Tab => {
                self.editor.indent();
                self.refresh_search_matches(true);
                self.close_completion();
            }
            KeyCode::Char(character) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.editor.insert_char(character);
                    self.refresh_search_matches(true);
                    if character.is_alphanumeric() || matches!(character, '_' | '.') {
                        self.request_completion(false);
                    } else {
                        self.close_completion();
                    }
                }
            }
            _ => self.close_completion(),
        }
    }

    fn handle_completion_popup_key(&mut self, key: KeyEvent) -> bool {
        let visible_rows = self.completion_visible_rows();
        let Some(popup) = self.completion_popup.as_mut() else {
            return false;
        };

        match key.code {
            KeyCode::Up => {
                popup.selected = popup.selected.saturating_sub(1);
                popup.keep_selected_visible(visible_rows);
                true
            }
            KeyCode::Down => {
                if !popup.items.is_empty() {
                    popup.selected = (popup.selected + 1).min(popup.items.len() - 1);
                }
                popup.keep_selected_visible(visible_rows);
                true
            }
            KeyCode::PageUp => {
                popup.selected = popup.selected.saturating_sub(8);
                popup.keep_selected_visible(visible_rows);
                true
            }
            KeyCode::PageDown => {
                if !popup.items.is_empty() {
                    popup.selected = (popup.selected + 8).min(popup.items.len() - 1);
                }
                popup.keep_selected_visible(visible_rows);
                true
            }
            KeyCode::Enter => self.accept_completion(),
            KeyCode::Esc => {
                self.close_completion();
                true
            }
            _ => false,
        }
    }

    fn completion_visible_rows(&self) -> usize {
        usize::from(
            self.geometry
                .editor_inner
                .height
                .saturating_sub(2)
                .clamp(1, 8),
        )
    }

    fn handle_message_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.select_message_delta(-1),
            KeyCode::Down => self.select_message_delta(1),
            KeyCode::PageUp => self.select_message_delta(-8),
            KeyCode::PageDown => self.select_message_delta(8),
            KeyCode::Home => self.select_message(0),
            KeyCode::End => self.select_message(self.messages.len().saturating_sub(1)),
            _ => {}
        }
    }

    fn select_previous_file(&mut self) {
        self.selected_file = self.selected_file.saturating_sub(1);
    }

    fn select_next_file(&mut self) {
        if !self.project_files.is_empty() {
            self.selected_file = (self.selected_file + 1).min(self.project_files.len() - 1);
        }
    }

    fn select_message_at(&mut self, row: u16) {
        if self.messages.is_empty() {
            return;
        }

        let visible_rows = self.geometry.messages_inner.height as usize;
        let total = self.messages.len();
        let start = total.saturating_sub(visible_rows + self.message_scroll);
        let clicked = start + row.saturating_sub(self.geometry.messages_inner.y) as usize;
        self.select_message(clicked.min(total - 1));
    }

    fn select_message_delta(&mut self, amount: isize) {
        if self.messages.is_empty() {
            return;
        }

        let max = self.messages.len() - 1;
        let next = if amount < 0 {
            self.active_message.saturating_sub(amount.unsigned_abs())
        } else {
            self.active_message.saturating_add(amount as usize).min(max)
        };
        self.select_message(next);
    }

    fn select_message(&mut self, index: usize) {
        if self.messages.is_empty() {
            self.active_message = 0;
            self.message_scroll = 0;
            return;
        }

        self.active_message = index.min(self.messages.len() - 1);
        self.keep_active_message_visible();
    }

    fn keep_active_message_visible(&mut self) {
        let total = self.messages.len();
        if total == 0 {
            return;
        }

        self.active_message = self.active_message.min(total - 1);
        let visible_rows = (self.geometry.messages_inner.height as usize)
            .max(1)
            .min(total);
        let start = total.saturating_sub(visible_rows + self.message_scroll);
        let end = total.saturating_sub(self.message_scroll);

        if self.active_message < start {
            self.message_scroll = total.saturating_sub(self.active_message + visible_rows);
        } else if self.active_message >= end {
            self.message_scroll = total.saturating_sub(self.active_message + 1);
        }

        self.message_scroll = self.message_scroll.min(total.saturating_sub(1));
    }

    fn select_project_file_at(&mut self, row: u16) {
        if self.project_files.is_empty() {
            return;
        }

        let visible_rows = self.geometry.project_inner.height as usize;
        let start = self
            .selected_file
            .saturating_sub(visible_rows.saturating_sub(1));
        let clicked = start + row.saturating_sub(self.geometry.project_inner.y) as usize;

        if let Some(entry) = self.project_files.get(clicked) {
            self.selected_file = clicked;
            self.status = format!("Selected {} (F3/Enter opens)", entry.label);
            self.open_selected_file();
        }
    }

    fn place_cursor_at(&mut self, column: u16, row: u16, selecting: bool) {
        let inner = self.geometry.editor_inner;
        let text_x = inner.x.saturating_add(self.editor_gutter_width());
        let file_row = self
            .editor
            .row_offset()
            .saturating_add(row.saturating_sub(inner.y) as usize);
        let file_col = if column < text_x {
            self.editor.col_offset()
        } else {
            self.editor
                .col_offset()
                .saturating_add(column.saturating_sub(text_x) as usize)
        };

        if selecting {
            self.editor.select_to(file_row, file_col);
        } else {
            self.editor.set_cursor(file_row, file_col);
        }
        self.status = format!(
            "Cursor: line {}, column {}",
            self.editor.cursor_row() + 1,
            self.editor.cursor_col() + 1
        );
    }

    pub fn editor_line_number_width(&self) -> u16 {
        self.editor.lines().len().max(1).to_string().len().max(3) as u16
    }

    pub fn editor_gutter_width(&self) -> u16 {
        self.editor_line_number_width() + 3
    }

    pub fn paused_line(&self, path: &Path, line: usize) -> bool {
        self.debug_location
            .as_ref()
            .is_some_and(|location| location.path == path && location.line == line)
    }

    fn is_breakpoint_gutter(&self, column: u16) -> bool {
        let gutter_start = self.geometry.editor_inner.x;
        let gutter_end = gutter_start.saturating_add(2);
        column >= gutter_start && column <= gutter_end
    }

    fn is_project_divider(&self, column: u16, row: u16) -> bool {
        if !contains_y(self.geometry.project_area, row) {
            return false;
        }

        let right_edge = self
            .geometry
            .project_area
            .x
            .saturating_add(self.geometry.project_area.width.saturating_sub(1));

        column == right_edge || column == right_edge.saturating_add(1)
    }

    fn is_message_divider(&self, column: u16, row: u16) -> bool {
        if !contains_x(self.geometry.messages_area, column) {
            return false;
        }

        row == self.geometry.messages_area.y
            || row == self.geometry.messages_area.y.saturating_sub(1)
    }

    fn resize_project_pane(&mut self, column: u16) {
        let desktop = self.geometry.desktop_inner;
        if desktop.width == 0 {
            return;
        }

        let max_width = desktop.width.saturating_sub(MIN_EDITOR_PANE_WIDTH).max(1);
        let min_width = MIN_PROJECT_PANE_WIDTH.min(max_width);
        let width = column
            .saturating_sub(desktop.x)
            .saturating_add(1)
            .clamp(min_width, max_width);

        self.project_pane_width = width;
        self.status = format!("Project pane: {width} columns");
    }

    fn resize_messages_pane(&mut self, row: u16) {
        let status_y = self.geometry.status_area.y;
        let max_height = self
            .geometry
            .root
            .height
            .saturating_sub(MIN_DESKTOP_HEIGHT + 2)
            .max(1);
        let min_height = MIN_MESSAGES_PANE_HEIGHT.min(max_height);
        let height = status_y.saturating_sub(row).clamp(min_height, max_height);

        self.messages_pane_height = height;
        self.status = format!("Messages pane: {height} rows");
    }

    pub fn push_message(&mut self, message: impl Into<String>) {
        let stamp = timestamp();
        self.messages.push(format!("{stamp} {}", message.into()));
        if self.messages.len() > 600 {
            self.messages.drain(0..200);
            self.message_scroll = self.message_scroll.saturating_sub(200);
        }
        self.active_message = self.messages.len().saturating_sub(1);
        self.message_scroll = 0;
    }
}

fn output_lines(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|line| line.to_string())
        .collect()
}

fn relative_label(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|relative| relative.display().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn timestamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() % 86_400)
        .unwrap_or(0);
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub fn read_to_string(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

fn is_project_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn contains(area: Rect, column: u16, row: u16) -> bool {
    contains_x(area, column) && contains_y(area, row)
}

fn contains_x(area: Rect, column: u16) -> bool {
    column >= area.x && column < area.x.saturating_add(area.width)
}

fn contains_y(area: Rect, row: u16) -> bool {
    row >= area.y && row < area.y.saturating_add(area.height)
}

fn is_selection_navigation_key(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::SHIFT)
        && matches!(
            key.code,
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

fn first_selectable_item(menu_index: usize) -> usize {
    MENUS[menu_index]
        .items
        .iter()
        .position(|item| !item.separator)
        .unwrap_or(0)
}

fn menu_index_for_hotkey(character: char) -> Option<usize> {
    let needle = character.to_ascii_lowercase();
    MENUS.iter().position(|menu| {
        menu.title
            .chars()
            .next()
            .is_some_and(|candidate| candidate.to_ascii_lowercase() == needle)
    })
}

fn item_index_for_hotkey(menu_index: usize, character: char) -> Option<usize> {
    let needle = character.to_ascii_lowercase();
    MENUS[menu_index].items.iter().position(|item| {
        !item.separator
            && item
                .label
                .chars()
                .next()
                .is_some_and(|candidate| candidate.to_ascii_lowercase() == needle)
    })
}

fn menu_bar_index_at(menu: &MenuGeometry, column: u16, row: u16) -> Option<usize> {
    menu.bar_items
        .iter()
        .position(|area| contains(*area, column, row))
}

fn menu_dropdown_item_at(
    menu: &MenuGeometry,
    active_menu: usize,
    column: u16,
    row: u16,
) -> Option<(usize, usize)> {
    let area = menu.dropdown?;
    if !contains(area, column, row) {
        return None;
    }

    let inner_y = area.y.saturating_add(1);
    let inner_bottom = area.y.saturating_add(area.height).saturating_sub(1);
    if row < inner_y || row >= inner_bottom {
        return None;
    }

    let item_index = row.saturating_sub(inner_y) as usize;
    let item = MENUS[active_menu].items.get(item_index)?;
    (!item.separator).then_some((active_menu, item_index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ide::CompletionKind;

    #[test]
    fn saves_dirty_file_before_switching_files() {
        let root = temp_project("dirty-switch");
        let first = root.join("src").join("first.rs");
        let second = root.join("src").join("second.rs");
        fs::write(&first, "first").unwrap();
        fs::write(&second, "second").unwrap();

        let mut app = App::new_for_tests(root.clone());
        app.open_file_path(first.clone(), "first.rs");
        app.editor.insert_text("dirty ");
        app.open_file_path(second.clone(), "second.rs");

        assert_eq!(fs::read_to_string(&first).unwrap(), "dirty first");
        assert_eq!(app.editor.path(), Some(second.as_path()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn completion_popup_keeps_selected_item_visible() {
        let mut popup = CompletionPopup {
            items: (0..12)
                .map(|index| CompletionCandidate {
                    label: format!("item{index}"),
                    insert_text: format!("item{index}"),
                    detail: None,
                    kind: CompletionKind::Keyword,
                })
                .collect(),
            selected: 0,
            scroll: 0,
            replace_start: 0,
            replace_end: 0,
        };

        popup.selected = 8;
        popup.keep_selected_visible(8);
        assert_eq!(popup.scroll, 1);

        popup.selected = 11;
        popup.keep_selected_visible(8);
        assert_eq!(popup.scroll, 4);

        popup.selected = 2;
        popup.keep_selected_visible(8);
        assert_eq!(popup.scroll, 2);
    }

    #[test]
    fn debug_focus_keeps_current_file_when_target_open_fails() {
        let root = temp_project("debug-open-fails");
        let first = root.join("src").join("first.rs");
        let missing = root.join("src").join("missing.rs");
        fs::write(
            &first,
            [
                "fn main() {",
                "    println!(\"one\");",
                "    println!(\"two\");",
                "}",
            ]
            .join("\n"),
        )
        .unwrap();

        let mut app = App::new_for_tests(root.clone());
        app.open_file_path(first.clone(), "first.rs");
        app.focus_debug_location(&SourceLocation {
            path: missing,
            line: 2,
        });

        assert_eq!(app.editor.path(), Some(first.as_path()));
        assert_eq!(app.editor.cursor_row(), 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn editor_tab_indents_without_changing_focus() {
        let root = temp_project("editor-tab");
        let mut app = App::new_for_tests(root.clone());
        app.dialog = None;
        app.focus = Focus::Editor;
        app.editor.insert_text("letvalue");
        app.editor.set_cursor(0, 3);

        app.handle_active_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

        assert_eq!(app.focus, Focus::Editor);
        assert_eq!(app.editor.lines(), &["let    value".to_string()]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn editor_backtab_unindents() {
        let root = temp_project("editor-backtab");
        let mut app = App::new_for_tests(root.clone());
        app.dialog = None;
        app.focus = Focus::Editor;
        app.editor.insert_text("    let value");
        app.editor.set_cursor(0, 9);

        app.handle_active_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));

        assert_eq!(app.editor.lines(), &["let value".to_string()]);
        assert_eq!(app.editor.cursor_col(), 5);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shift_arrow_selects_text_in_editor() {
        let root = temp_project("shift-arrow-selects");
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

            app.handle_active_key(KeyEvent::new(code, KeyModifiers::SHIFT));

            assert_eq!(app.editor.selected_text().as_deref(), Some(expected));
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shift_navigation_selects_text_when_terminal_adds_alt_modifier() {
        let root = temp_project("shift-alt-navigation-selects");
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

            app.handle_active_key(KeyEvent::new(code, KeyModifiers::SHIFT | KeyModifiers::ALT));

            assert_eq!(app.editor.selected_text().as_deref(), Some(expected));
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn shift_up_down_select_even_when_completion_popup_is_visible() {
        let root = temp_project("shift-up-down-completion");
        for (code, expected) in [(KeyCode::Up, "bc\nd"), (KeyCode::Down, "ef\ng")] {
            let mut app = App::new_for_tests(root.clone());
            app.dialog = None;
            app.focus = Focus::Editor;
            app.editor.insert_text("abc\ndef\nghi");
            app.editor.set_cursor(1, 1);
            app.completion_popup = Some(CompletionPopup {
                items: vec![
                    CompletionCandidate {
                        label: "one".to_string(),
                        insert_text: "one".to_string(),
                        detail: None,
                        kind: CompletionKind::Keyword,
                    },
                    CompletionCandidate {
                        label: "two".to_string(),
                        insert_text: "two".to_string(),
                        detail: None,
                        kind: CompletionKind::Keyword,
                    },
                ],
                selected: 1,
                scroll: 0,
                replace_start: 0,
                replace_end: 0,
            });

            app.handle_active_key(KeyEvent::new(code, KeyModifiers::SHIFT));

            assert_eq!(app.editor.selected_text().as_deref(), Some(expected));
            assert!(!app.completion_visible());
        }

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn find_next_wraps_through_matches() {
        let root = temp_project("find-next-wraps");
        let mut app = App::new_for_tests(root.clone());
        app.dialog = None;
        app.focus = Focus::Editor;
        app.editor.insert_text("foo bar foo");
        app.find.query = "foo".to_string();
        app.refresh_search_matches(false);

        app.find_next();
        assert_eq!(app.editor.cursor_col(), 0);

        app.find_next();
        assert_eq!(app.editor.cursor_col(), 8);

        app.find_next();
        assert_eq!(app.editor.cursor_col(), 0);

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
