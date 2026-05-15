use std::{
    fs, io,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::{
    editor::Editor,
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
            MenuItem::action("Find", "", MenuAction::NotImplemented("Find")),
            MenuItem::action("Find next", "", MenuAction::NotImplemented("Find next")),
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
        items: &[MenuItem::action(
            "Breakpoints",
            "",
            MenuAction::NotImplemented("Debug"),
        )],
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
    CargoRun,
    CargoTest,
    CargoCheck,
    CargoBuild,
    ToggleFocus,
    FocusProject,
    FocusEditor,
    FocusMessages,
    RefreshProject,
    Help,
    About,
    NotImplemented(&'static str),
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MenuGeometry {
    pub bar_items: [Rect; MENUS.len()],
    pub dropdown: Option<Rect>,
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
    pub new_project: NewProjectForm,
    pub status: String,
    pub project_pane_width: u16,
    pub messages_pane_height: u16,
    pub geometry: Geometry,
    drag_target: Option<DragTarget>,
}

impl App {
    pub fn new(root: PathBuf) -> Self {
        let mut app = Self {
            browser_dir: root.clone(),
            new_project: NewProjectForm::new(&root),
            root,
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
            status: "Welcome to TRUST".to_string(),
            project_pane_width: 30,
            messages_pane_height: 6,
            geometry: Geometry::default(),
            drag_target: None,
        };
        app.messages.push("Ready.".to_string());
        app
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
        self.focus = match self.focus {
            Focus::Project => Focus::Editor,
            Focus::Editor => Focus::Messages,
            Focus::Messages => Focus::Project,
        };
        self.status = format!("Focus: {}", self.focus_name());
    }

    fn set_focus(&mut self, focus: Focus) {
        self.close_menu();
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

    fn open_file_path(&mut self, path: PathBuf, label: impl Into<String>) {
        let label = label.into();
        match Editor::open(&path) {
            Ok(editor) => {
                self.editor = editor;
                self.focus = Focus::Editor;
                self.status = format!("Opened {label}");
                self.push_message(format!("Opened {}", path.display()));
            }
            Err(error) => self.status = format!("Open failed: {error}"),
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
        self.selected_file = 0;
        self.refresh_project();
        self.focus = Focus::Project;
        self.status = format!("Browsing {}", self.browser_label());
    }

    pub fn save_current(&mut self) {
        self.close_menu();
        match self.editor.save() {
            Ok(()) => {
                self.status = format!("Saved {}", self.current_file_label());
                self.push_message(format!("Saved {}", self.current_file_label()));
            }
            Err(error) => self.status = format!("Save failed: {error}"),
        }
    }

    pub fn run_cargo(&mut self, command: &str) {
        self.close_menu();
        if self.editor.is_dirty() {
            self.save_current();
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
        self.new_file = NewFileForm::new();
        self.dialog = Some(Dialog::NewFile);
        self.status = format!("New file in {}", self.browser_label());
    }

    fn open_new_project_dialog(&mut self) {
        self.close_menu();
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
                    self.root = target.canonicalize().unwrap_or(target);
                    self.browser_dir = self.root.clone();
                    self.selected_file = 0;
                    self.editor = Editor::scratch();
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
            MenuAction::Save => self.save_current(),
            MenuAction::Quit => return Action::Quit,
            MenuAction::Undo => self.undo_editor(),
            MenuAction::Redo => self.redo_editor(),
            MenuAction::Copy => self.copy_selection(),
            MenuAction::Cut => self.cut_selection(),
            MenuAction::Paste => self.paste_from_clipboard(),
            MenuAction::DeleteLine => self.editor.delete_line(),
            MenuAction::DuplicateLine => self.editor.duplicate_line(),
            MenuAction::CargoRun => self.run_cargo("run"),
            MenuAction::CargoTest => self.run_cargo("test"),
            MenuAction::CargoCheck => self.run_cargo("check"),
            MenuAction::CargoBuild => self.run_cargo("build"),
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
            MenuAction::NotImplemented(name) => {
                self.status = format!("{name} is not implemented yet");
            }
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
            self.focus = Focus::Project;
            self.select_project_file_at(row);
        } else if contains(self.geometry.editor_inner, column, row) {
            self.focus = Focus::Editor;
            self.place_cursor_at(column, row, false);
            self.drag_target = Some(DragTarget::EditorSelection);
        } else if contains(self.geometry.messages_inner, column, row) {
            self.focus = Focus::Messages;
            self.select_message_at(row);
            self.status = "Focus: Messages".to_string();
        } else if contains(self.geometry.project_area, column, row) {
            self.focus = Focus::Project;
            self.status = "Focus: Project".to_string();
        } else if contains(self.geometry.editor_area, column, row) {
            self.focus = Focus::Editor;
            self.status = "Focus: Edit".to_string();
        } else if contains(self.geometry.messages_area, column, row) {
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
        if key.modifiers.contains(KeyModifiers::ALT) {
            match key.code {
                KeyCode::Char('x') | KeyCode::Char('X') => self.editor.delete_line(),
                KeyCode::Char('u') | KeyCode::Char('U') => self.editor.duplicate_line(),
                _ => {}
            }
            return;
        }

        let selecting = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::Left if selecting => self.editor.extend_left(),
            KeyCode::Left => self.editor.move_left(),
            KeyCode::Right if selecting => self.editor.extend_right(),
            KeyCode::Right => self.editor.move_right(),
            KeyCode::Up if selecting => self.editor.extend_up(),
            KeyCode::Up => self.editor.move_up(),
            KeyCode::Down if selecting => self.editor.extend_down(),
            KeyCode::Down => self.editor.move_down(),
            KeyCode::Home if selecting => self.editor.extend_home(),
            KeyCode::Home => self.editor.home(),
            KeyCode::End if selecting => self.editor.extend_end(),
            KeyCode::End => self.editor.end(),
            KeyCode::PageUp if selecting => self.editor.extend_page_up(12),
            KeyCode::PageUp => self.editor.page_up(12),
            KeyCode::PageDown if selecting => self.editor.extend_page_down(12),
            KeyCode::PageDown => self.editor.page_down(12),
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Delete => self.editor.delete(),
            KeyCode::Enter => self.editor.insert_newline(),
            KeyCode::Char(character) => {
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT {
                    self.editor.insert_char(character);
                }
            }
            _ => {}
        }
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
        let line_number_width = self.editor_line_number_width();
        let text_x = inner.x.saturating_add(line_number_width);
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
        (self.editor.lines().len().max(1).to_string().len().max(3) + 1) as u16
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
