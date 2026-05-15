use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::app::read_to_string;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy)]
enum Movement {
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp(usize),
    PageDown(usize),
}

#[derive(Debug, Clone)]
struct EditorSnapshot {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    row_offset: usize,
    col_offset: usize,
    selection_anchor: Option<Position>,
    revision: usize,
}

#[derive(Debug, Clone)]
pub struct Editor {
    path: Option<PathBuf>,
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    row_offset: usize,
    col_offset: usize,
    viewport_rows: usize,
    viewport_cols: usize,
    selection_anchor: Option<Position>,
    dirty: bool,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    revision: usize,
    clean_revision: usize,
}

impl Editor {
    pub fn scratch() -> Self {
        Self {
            path: None,
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            row_offset: 0,
            col_offset: 0,
            viewport_rows: 18,
            viewport_cols: 72,
            selection_anchor: None,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            revision: 0,
            clean_revision: 0,
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let content = read_to_string(path)?;
        let mut lines: Vec<String> = content.lines().map(ToString::to_string).collect();
        if content.ends_with('\n') || lines.is_empty() {
            lines.push(String::new());
        }

        Ok(Self {
            path: Some(path.to_path_buf()),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            row_offset: 0,
            col_offset: 0,
            viewport_rows: 18,
            viewport_cols: 72,
            selection_anchor: None,
            dirty: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            revision: 0,
            clean_revision: 0,
        })
    }

    pub fn save(&mut self) -> io::Result<()> {
        let Some(path) = &self.path else {
            return Err(io::Error::other("scratch buffer has no path"));
        };

        fs::write(path, self.lines.join("\n"))?;
        self.clean_revision = self.revision;
        self.sync_dirty_flag();
        Ok(())
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    pub fn row_offset(&self) -> usize {
        self.row_offset
    }

    pub fn col_offset(&self) -> usize {
        self.col_offset
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn has_selection(&self) -> bool {
        self.selection_bounds().is_some()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn set_cursor(&mut self, row: usize, col: usize) {
        let position = self.clamp_position(Position { row, col });
        self.set_cursor_position(position);
        self.clear_selection();
        self.keep_cursor_visible();
    }

    pub fn begin_selection(&mut self) {
        self.selection_anchor = Some(self.current_position());
    }

    pub fn select_to(&mut self, row: usize, col: usize) {
        if self.selection_anchor.is_none() {
            self.begin_selection();
        }
        let position = self.clamp_position(Position { row, col });
        self.set_cursor_position(position);
        if self.selection_anchor == Some(self.current_position()) {
            self.clear_selection();
        }
        self.keep_cursor_visible();
    }

    pub fn selection_bounds(&self) -> Option<(Position, Position)> {
        let anchor = self.selection_anchor?;
        let cursor = self.current_position();
        if anchor == cursor {
            return None;
        }

        let start = anchor.min(cursor);
        let end = anchor.max(cursor);
        Some((start, end))
    }

    pub fn selection_range_for_line(&self, row: usize) -> Option<(usize, usize)> {
        let (start, end) = self.selection_bounds()?;
        if row < start.row || row > end.row {
            return None;
        }

        let line_len = self.line_len(row);
        let (from, to) = if start.row == end.row {
            (start.col, end.col)
        } else if row == start.row {
            (start.col, line_len)
        } else if row == end.row {
            (0, end.col)
        } else {
            (0, line_len)
        };

        let from = from.min(line_len);
        let to = to.min(line_len);
        (from < to).then_some((from, to))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_bounds()?;

        if start.row == end.row {
            return self
                .lines
                .get(start.row)
                .map(|line| slice_chars(line, start.col, end.col));
        }

        let mut text = String::new();
        let first_line = self.lines.get(start.row)?;
        text.push_str(&slice_chars(
            first_line,
            start.col,
            self.line_len(start.row),
        ));

        for row in start.row + 1..end.row {
            text.push('\n');
            if let Some(line) = self.lines.get(row) {
                text.push_str(line);
            }
        }

        text.push('\n');
        let last_line = self.lines.get(end.row)?;
        text.push_str(&slice_chars(last_line, 0, end.col));
        Some(text)
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.begin_edit();
        self.delete_selection_without_history();
        self.finish_edit();
        Some(text)
    }

    pub fn move_left(&mut self) {
        self.move_cursor(Movement::Left, false);
    }

    pub fn move_right(&mut self) {
        self.move_cursor(Movement::Right, false);
    }

    pub fn move_up(&mut self) {
        self.move_cursor(Movement::Up, false);
    }

    pub fn move_down(&mut self) {
        self.move_cursor(Movement::Down, false);
    }

    pub fn home(&mut self) {
        self.move_cursor(Movement::Home, false);
    }

    pub fn end(&mut self) {
        self.move_cursor(Movement::End, false);
    }

    pub fn page_up(&mut self, rows: usize) {
        self.move_cursor(Movement::PageUp(rows), false);
    }

    pub fn page_down(&mut self, rows: usize) {
        self.move_cursor(Movement::PageDown(rows), false);
    }

    pub fn extend_left(&mut self) {
        self.move_cursor(Movement::Left, true);
    }

    pub fn extend_right(&mut self) {
        self.move_cursor(Movement::Right, true);
    }

    pub fn extend_up(&mut self) {
        self.move_cursor(Movement::Up, true);
    }

    pub fn extend_down(&mut self) {
        self.move_cursor(Movement::Down, true);
    }

    pub fn extend_home(&mut self) {
        self.move_cursor(Movement::Home, true);
    }

    pub fn extend_end(&mut self) {
        self.move_cursor(Movement::End, true);
    }

    pub fn extend_page_up(&mut self, rows: usize) {
        self.move_cursor(Movement::PageUp(rows), true);
    }

    pub fn extend_page_down(&mut self, rows: usize) {
        self.move_cursor(Movement::PageDown(rows), true);
    }

    pub fn insert_char(&mut self, character: char) {
        self.begin_edit();
        self.delete_selection_without_history();
        let cursor_col = self.cursor_col;
        let byte_idx = char_to_byte(&self.lines[self.cursor_row], cursor_col);
        self.lines[self.cursor_row].insert(byte_idx, character);
        self.cursor_col += 1;
        self.finish_edit();
    }

    pub fn insert_text(&mut self, text: &str) {
        let text = normalize_newlines(text);
        if text.is_empty() {
            return;
        }

        self.begin_edit();
        self.delete_selection_without_history();
        let parts = text.split('\n').collect::<Vec<_>>();
        if parts.len() == 1 {
            let byte_idx = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].insert_str(byte_idx, parts[0]);
            self.cursor_col += parts[0].chars().count();
        } else {
            let byte_idx = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            let suffix = self.lines[self.cursor_row].split_off(byte_idx);
            self.lines[self.cursor_row].push_str(parts[0]);

            let mut insert_row = self.cursor_row + 1;
            for part in &parts[1..parts.len() - 1] {
                self.lines.insert(insert_row, (*part).to_string());
                insert_row += 1;
            }

            let last_part = parts.last().copied().unwrap_or_default();
            self.lines
                .insert(insert_row, format!("{last_part}{suffix}"));
            self.cursor_row = insert_row;
            self.cursor_col = last_part.chars().count();
        }

        self.finish_edit();
    }

    pub fn insert_newline(&mut self) {
        self.begin_edit();
        self.delete_selection_without_history();
        let cursor_col = self.cursor_col;
        let byte_idx = char_to_byte(&self.lines[self.cursor_row], cursor_col);
        let remainder = self.lines[self.cursor_row].split_off(byte_idx);
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.lines.insert(self.cursor_row, remainder);
        self.finish_edit();
    }

    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.begin_edit();
            self.delete_selection_without_history();
            self.finish_edit();
            return;
        }

        if self.cursor_col > 0 {
            self.begin_edit();
            let remove_at = char_to_byte(&self.lines[self.cursor_row], self.cursor_col - 1);
            self.lines[self.cursor_row].remove(remove_at);
            self.cursor_col -= 1;
            self.finish_edit();
        } else if self.cursor_row > 0 {
            self.begin_edit();
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
            self.lines[self.cursor_row].push_str(&current);
            self.finish_edit();
        }
    }

    pub fn delete(&mut self) {
        if self.has_selection() {
            self.begin_edit();
            self.delete_selection_without_history();
            self.finish_edit();
            return;
        }

        if self.cursor_col < self.line_len(self.cursor_row) {
            self.begin_edit();
            let remove_at = char_to_byte(&self.lines[self.cursor_row], self.cursor_col);
            self.lines[self.cursor_row].remove(remove_at);
            self.finish_edit();
        } else if self.cursor_row + 1 < self.lines.len() {
            self.begin_edit();
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
            self.finish_edit();
        }
    }

    pub fn delete_line(&mut self) {
        self.begin_edit();
        self.clear_selection();
        if self.lines.len() == 1 {
            self.lines[0].clear();
            self.cursor_col = 0;
        } else {
            self.lines.remove(self.cursor_row);
            self.cursor_row = self.cursor_row.min(self.lines.len() - 1);
            self.clamp_col();
        }
        self.finish_edit();
    }

    pub fn duplicate_line(&mut self) {
        self.begin_edit();
        self.clear_selection();
        let line = self.lines[self.cursor_row].clone();
        self.lines.insert(self.cursor_row + 1, line);
        self.cursor_row += 1;
        self.finish_edit();
    }

    pub fn undo(&mut self) -> bool {
        let Some(snapshot) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(snapshot) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.snapshot());
        self.restore_snapshot(snapshot);
        true
    }

    pub fn set_viewport(&mut self, rows: usize, cols: usize) {
        self.viewport_rows = rows;
        self.viewport_cols = cols;

        if self.cursor_row < self.row_offset {
            self.row_offset = self.cursor_row;
        } else if rows > 0 && self.cursor_row >= self.row_offset + rows {
            self.row_offset = self.cursor_row.saturating_sub(rows - 1);
        }

        if self.cursor_col < self.col_offset {
            self.col_offset = self.cursor_col;
        } else if cols > 0 && self.cursor_col >= self.col_offset + cols {
            self.col_offset = self.cursor_col.saturating_sub(cols - 1);
        }
    }

    fn move_cursor(&mut self, movement: Movement, selecting: bool) {
        if selecting {
            if self.selection_anchor.is_none() {
                self.begin_selection();
            }
        } else {
            self.clear_selection();
        }

        match movement {
            Movement::Left => self.step_left(),
            Movement::Right => self.step_right(),
            Movement::Up => self.step_up(),
            Movement::Down => self.step_down(),
            Movement::Home => self.cursor_col = 0,
            Movement::End => self.cursor_col = self.line_len(self.cursor_row),
            Movement::PageUp(rows) => {
                self.cursor_row = self.cursor_row.saturating_sub(rows);
                self.clamp_col();
            }
            Movement::PageDown(rows) => {
                self.cursor_row = self
                    .cursor_row
                    .saturating_add(rows)
                    .min(self.lines.len().saturating_sub(1));
                self.clamp_col();
            }
        }

        if selecting && self.selection_anchor == Some(self.current_position()) {
            self.clear_selection();
        }
        self.keep_cursor_visible();
    }

    fn step_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.line_len(self.cursor_row);
        }
    }

    fn step_right(&mut self) {
        let len = self.line_len(self.cursor_row);
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    fn step_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col();
        }
    }

    fn step_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.clamp_col();
        }
    }

    fn delete_selection_without_history(&mut self) -> bool {
        let Some((start, end)) = self.selection_bounds() else {
            return false;
        };

        if start.row == end.row {
            let start_byte = char_to_byte(&self.lines[start.row], start.col);
            let end_byte = char_to_byte(&self.lines[end.row], end.col);
            self.lines[start.row].replace_range(start_byte..end_byte, "");
        } else {
            let start_byte = char_to_byte(&self.lines[start.row], start.col);
            let end_byte = char_to_byte(&self.lines[end.row], end.col);
            let suffix = self.lines[end.row][end_byte..].to_string();
            self.lines[start.row].truncate(start_byte);
            self.lines[start.row].push_str(&suffix);
            self.lines.drain(start.row + 1..=end.row);
        }

        self.set_cursor_position(start);
        self.clear_selection();
        true
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            lines: self.lines.clone(),
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            row_offset: self.row_offset,
            col_offset: self.col_offset,
            selection_anchor: self.selection_anchor,
            revision: self.revision,
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.lines = snapshot.lines;
        self.cursor_row = snapshot.cursor_row;
        self.cursor_col = snapshot.cursor_col;
        self.row_offset = snapshot.row_offset;
        self.col_offset = snapshot.col_offset;
        self.selection_anchor = snapshot.selection_anchor;
        self.revision = snapshot.revision;
        self.sync_dirty_flag();
        self.keep_cursor_visible();
    }

    fn begin_edit(&mut self) {
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
        if self.undo_stack.len() > 512 {
            self.undo_stack.remove(0);
        }
    }

    fn finish_edit(&mut self) {
        self.revision += 1;
        self.sync_dirty_flag();
        self.keep_cursor_visible();
    }

    fn sync_dirty_flag(&mut self) {
        self.dirty = self.revision != self.clean_revision;
    }

    fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn current_position(&self) -> Position {
        Position {
            row: self.cursor_row,
            col: self.cursor_col,
        }
    }

    fn set_cursor_position(&mut self, position: Position) {
        self.cursor_row = position.row;
        self.cursor_col = position.col;
    }

    fn clamp_position(&self, position: Position) -> Position {
        let row = position.row.min(self.lines.len().saturating_sub(1));
        let col = position.col.min(self.line_len(row));
        Position { row, col }
    }

    fn keep_cursor_visible(&mut self) {
        self.set_viewport(self.viewport_rows, self.viewport_cols);
    }

    fn clamp_col(&mut self) {
        self.cursor_col = self.cursor_col.min(self.line_len(self.cursor_row));
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines
            .get(row)
            .map(|line| line.chars().count())
            .unwrap_or(0)
    }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    let start = char_to_byte(text, start);
    let end = char_to_byte(text, end);
    text[start..end].to_string()
}

fn char_to_byte(text: &str, char_idx: usize) -> usize {
    text.char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len())
}

#[cfg(test)]
mod tests {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};

    use super::Editor;

    #[test]
    fn selects_and_cuts_across_lines() {
        let mut editor = Editor::scratch();
        editor.insert_text("alpha\nbeta\ngamma");
        editor.set_cursor(0, 2);
        editor.begin_selection();
        editor.select_to(2, 2);

        assert_eq!(editor.selected_text().as_deref(), Some("pha\nbeta\nga"));
        assert_eq!(editor.cut_selection().as_deref(), Some("pha\nbeta\nga"));
        assert_eq!(editor.lines(), &["almma".to_string()]);
    }

    #[test]
    fn paste_replaces_selection() {
        let mut editor = Editor::scratch();
        editor.insert_text("hello world");
        editor.set_cursor(0, 6);
        editor.begin_selection();
        editor.select_to(0, 11);
        editor.insert_text("rust");

        assert_eq!(editor.lines(), &["hello rust".to_string()]);
        assert!(!editor.has_selection());
    }

    #[test]
    fn mouse_cursor_uses_current_viewport_height() {
        let mut editor = Editor::scratch();
        editor.insert_text(
            &(0..60)
                .map(|line| format!("line {line}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        editor.set_cursor(0, 0);
        editor.set_viewport(40, 72);

        editor.set_cursor(30, 0);

        assert_eq!(editor.cursor_row(), 30);
        assert_eq!(editor.row_offset(), 0);
    }

    #[test]
    fn undo_and_redo_restore_text() {
        let mut editor = Editor::scratch();
        editor.insert_text("hello");
        editor.insert_text(" world");

        assert!(editor.undo());
        assert_eq!(editor.lines(), &["hello".to_string()]);
        assert!(editor.redo());
        assert_eq!(editor.lines(), &["hello world".to_string()]);
    }

    #[test]
    fn undo_can_restore_saved_clean_state() {
        let path = temp_file_path("trust_undo_redo");
        fs::write(&path, "alpha\n").expect("write temp file");

        let mut editor = Editor::open(&path).expect("open temp file");
        editor.insert_text("beta");
        assert!(editor.is_dirty());

        editor.save().expect("save editor");
        assert!(!editor.is_dirty());

        editor.insert_text("!");
        assert!(editor.is_dirty());
        assert!(editor.undo());
        assert_eq!(editor.lines(), &["betaalpha".to_string(), "".to_string()]);
        assert!(!editor.is_dirty());

        fs::remove_file(path).expect("remove temp file");
    }

    fn temp_file_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{unique}.txt"))
    }
}
