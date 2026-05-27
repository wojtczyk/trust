use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::app::read_to_string;

const INDENT: &str = "    ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub row: usize,
    pub start_col: usize,
    pub end_col: usize,
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
    next_revision: usize,
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
            next_revision: 1,
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
            next_revision: 1,
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

    pub fn line(&self, row: usize) -> Option<&str> {
        self.lines.get(row).map(String::as_str)
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn find_matches(&self, query: &str) -> Vec<SearchMatch> {
        if query.is_empty() {
            return Vec::new();
        }

        let needle = query.chars().collect::<Vec<_>>();
        let needle_len = needle.len();
        let mut matches = Vec::new();

        for (row, line) in self.lines.iter().enumerate() {
            let chars = line.chars().collect::<Vec<_>>();
            if chars.len() < needle_len {
                continue;
            }

            for start_col in 0..=chars.len() - needle_len {
                if chars[start_col..start_col + needle_len] == needle[..] {
                    matches.push(SearchMatch {
                        row,
                        start_col,
                        end_col: start_col + needle_len,
                    });
                }
            }
        }

        matches
    }

    pub fn cursor_row(&self) -> usize {
        self.cursor_row
    }

    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    pub fn char_before_cursor(&self) -> Option<char> {
        if self.cursor_col == 0 {
            return None;
        }

        self.lines
            .get(self.cursor_row)
            .and_then(|line| line.chars().nth(self.cursor_col - 1))
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
        let replaced_selection = self.delete_selection_without_history();
        if !replaced_selection && is_closing_delimiter(character) {
            self.outdent_current_line_once();
        }
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
        let current = self.lines[self.cursor_row].clone();
        let before = current[..byte_idx].to_string();
        let after = current[byte_idx..].to_string();
        let base_indent = leading_indent(&before);
        let line_indent = newline_indent(&before);

        self.lines[self.cursor_row] = before.clone();
        self.cursor_row += 1;
        self.cursor_col = line_indent.chars().count();

        if closes_opening_delimiter(&before, &after) {
            self.lines.insert(self.cursor_row, line_indent);
            self.lines.insert(
                self.cursor_row + 1,
                format!("{base_indent}{}", after.trim_start()),
            );
        } else {
            self.lines
                .insert(self.cursor_row, format!("{line_indent}{after}"));
        }
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

    pub fn indent(&mut self) {
        let Some((start_row, end_row)) = self.selected_line_range() else {
            self.insert_text(INDENT);
            return;
        };

        self.begin_edit();
        for row in start_row..=end_row {
            self.lines[row].insert_str(0, INDENT);
        }
        self.adjust_positions_for_indent(start_row, end_row);
        self.finish_edit();
    }

    pub fn unindent(&mut self) {
        let (start_row, end_row) = self
            .selected_line_range()
            .unwrap_or((self.cursor_row, self.cursor_row));
        let removals = (start_row..=end_row)
            .map(|row| line_outdent_width(&self.lines[row]))
            .collect::<Vec<_>>();

        if removals.iter().all(|width| *width == 0) {
            return;
        }

        self.begin_edit();
        for (offset, width) in removals.iter().copied().enumerate() {
            if width == 0 {
                continue;
            }
            let line = &mut self.lines[start_row + offset];
            let end_byte = char_to_byte(line, width);
            line.replace_range(0..end_byte, "");
        }
        self.adjust_positions_for_unindent(start_row, &removals);
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

    pub fn completion_prefix_bounds(&self) -> (usize, usize, String) {
        let Some(line) = self.lines.get(self.cursor_row) else {
            return (0, 0, String::new());
        };
        let chars = line.chars().collect::<Vec<_>>();
        let mut start = self.cursor_col.min(chars.len());
        while start > 0 && is_identifier_char(chars[start - 1]) {
            start -= 1;
        }

        let mut end = self.cursor_col.min(chars.len());
        while end < chars.len() && is_identifier_char(chars[end]) {
            end += 1;
        }

        let prefix = chars[start..self.cursor_col.min(chars.len())]
            .iter()
            .collect::<String>();
        (start, end, prefix)
    }

    pub fn replace_range_in_current_line(
        &mut self,
        start_col: usize,
        end_col: usize,
        replacement: &str,
    ) {
        self.clear_selection();
        let row = self.cursor_row;
        let line_len = self.line_len(row);
        let start_col = start_col.min(line_len);
        let end_col = end_col.min(line_len);
        let (start_col, end_col) = if start_col <= end_col {
            (start_col, end_col)
        } else {
            (end_col, start_col)
        };

        self.set_cursor_position(Position {
            row,
            col: start_col,
        });
        if start_col != end_col {
            self.selection_anchor = Some(Position {
                row,
                col: start_col,
            });
            self.set_cursor_position(Position { row, col: end_col });
            self.begin_edit();
            self.delete_selection_without_history();
            self.insert_text_without_history(replacement);
            self.finish_edit();
            return;
        }
        self.begin_edit();
        self.insert_text_without_history(replacement);
        self.finish_edit();
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

    fn selected_line_range(&self) -> Option<(usize, usize)> {
        let (start, end) = self.selection_bounds()?;
        let end_row = if end.col == 0 && end.row > start.row {
            end.row - 1
        } else {
            end.row
        };
        Some((start.row, end_row))
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

    fn insert_text_without_history(&mut self, text: &str) {
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
        self.next_revision = self.next_revision.max(self.revision + 1);
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
        self.revision = self.next_revision;
        self.next_revision += 1;
        self.sync_dirty_flag();
        self.keep_cursor_visible();
    }

    fn sync_dirty_flag(&mut self) {
        self.dirty = self.revision != self.clean_revision;
    }

    fn adjust_positions_for_indent(&mut self, start_row: usize, end_row: usize) {
        self.cursor_col =
            adjust_col_for_indent(self.cursor_row, self.cursor_col, start_row, end_row);
        if let Some(anchor) = self.selection_anchor.as_mut() {
            anchor.col = adjust_col_for_indent(anchor.row, anchor.col, start_row, end_row);
        }
    }

    fn adjust_positions_for_unindent(&mut self, start_row: usize, removals: &[usize]) {
        self.cursor_col =
            adjust_col_for_unindent(self.cursor_row, self.cursor_col, start_row, removals);
        if let Some(anchor) = self.selection_anchor.as_mut() {
            anchor.col = adjust_col_for_unindent(anchor.row, anchor.col, start_row, removals);
        }
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

    fn outdent_current_line_once(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let before_cursor = slice_chars(line, 0, self.cursor_col);
        if !before_cursor.chars().all(char::is_whitespace) {
            return;
        }

        let remove_cols = outdent_width(&before_cursor);
        if remove_cols == 0 {
            return;
        }

        let start_col = self.cursor_col - remove_cols;
        let start_byte = char_to_byte(line, start_col);
        let end_byte = char_to_byte(line, self.cursor_col);
        line.replace_range(start_byte..end_byte, "");
        self.cursor_col = start_col;
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

fn leading_indent(text: &str) -> String {
    text.chars()
        .take_while(|character| matches!(character, ' ' | '\t'))
        .collect()
}

fn newline_indent(before_cursor: &str) -> String {
    let mut indent = leading_indent(before_cursor);
    if increases_indent(before_cursor) {
        indent.push_str(INDENT);
    }
    indent
}

fn increases_indent(before_cursor: &str) -> bool {
    let trimmed = before_cursor.trim_end();
    trimmed.ends_with('{')
        || trimmed.ends_with('(')
        || trimmed.ends_with('[')
        || trimmed.ends_with("=>")
}

fn closes_opening_delimiter(before_cursor: &str, after_cursor: &str) -> bool {
    let Some(opening) = before_cursor.trim_end().chars().last() else {
        return false;
    };
    let expected = match opening {
        '{' => '}',
        '(' => ')',
        '[' => ']',
        _ => return false,
    };

    after_cursor.trim_start().starts_with(expected)
}

fn is_closing_delimiter(character: char) -> bool {
    matches!(character, '}' | ')' | ']')
}

fn outdent_width(before_cursor: &str) -> usize {
    if before_cursor.ends_with('\t') {
        return 1;
    }

    before_cursor
        .chars()
        .rev()
        .take(INDENT.chars().count())
        .take_while(|character| *character == ' ')
        .count()
}

fn line_outdent_width(line: &str) -> usize {
    if line.starts_with('\t') {
        return 1;
    }

    line.chars()
        .take(INDENT.chars().count())
        .take_while(|character| *character == ' ')
        .count()
}

fn adjust_col_for_indent(row: usize, col: usize, start_row: usize, end_row: usize) -> usize {
    if (start_row..=end_row).contains(&row) {
        col + INDENT.chars().count()
    } else {
        col
    }
}

fn adjust_col_for_unindent(row: usize, col: usize, start_row: usize, removals: &[usize]) -> usize {
    let Some(offset) = row.checked_sub(start_row) else {
        return col;
    };
    let Some(width) = removals.get(offset).copied() else {
        return col;
    };
    col.saturating_sub(width.min(col))
}

fn is_identifier_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{Editor, SearchMatch};

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
    fn finds_matches_across_multiple_lines() {
        let mut editor = Editor::scratch();
        editor.insert_text("alpha beta\nbeta alpha\nalphabet");

        assert_eq!(
            editor.find_matches("alpha"),
            vec![
                SearchMatch {
                    row: 0,
                    start_col: 0,
                    end_col: 5,
                },
                SearchMatch {
                    row: 1,
                    start_col: 5,
                    end_col: 10,
                },
                SearchMatch {
                    row: 2,
                    start_col: 0,
                    end_col: 5,
                },
            ]
        );
    }

    #[test]
    fn tab_inserts_indent_at_cursor_without_selection() {
        let mut editor = Editor::scratch();
        editor.insert_text("letvalue");
        editor.set_cursor(0, 3);

        editor.indent();

        assert_eq!(editor.lines(), &["let    value".to_string()]);
        assert_eq!(editor.cursor_col(), 7);
    }

    #[test]
    fn tab_indents_selected_lines() {
        let mut editor = Editor::scratch();
        editor.insert_text("one\ntwo\nthree");
        editor.set_cursor(0, 1);
        editor.begin_selection();
        editor.select_to(1, 2);

        editor.indent();

        assert_eq!(
            editor.lines(),
            &[
                "    one".to_string(),
                "    two".to_string(),
                "three".to_string(),
            ]
        );
        assert_eq!(editor.selected_text().as_deref(), Some("ne\n    tw"));
    }

    #[test]
    fn backtab_unindents_current_line() {
        let mut editor = Editor::scratch();
        editor.insert_text("    let value");
        editor.set_cursor(0, 9);

        editor.unindent();

        assert_eq!(editor.lines(), &["let value".to_string()]);
        assert_eq!(editor.cursor_col(), 5);
    }

    #[test]
    fn backtab_unindents_selected_lines() {
        let mut editor = Editor::scratch();
        editor.insert_text("    one\n  two\n\tthree");
        editor.set_cursor(0, 4);
        editor.begin_selection();
        editor.select_to(2, 6);

        editor.unindent();

        assert_eq!(
            editor.lines(),
            &["one".to_string(), "two".to_string(), "three".to_string()]
        );
        assert_eq!(editor.selected_text().as_deref(), Some("one\ntwo\nthree"));
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

    #[test]
    fn edit_after_undoing_from_saved_state_stays_dirty() {
        let path = temp_file_path("trust_undo_dirty");
        fs::write(&path, "").expect("write temp file");

        let mut editor = Editor::open(&path).expect("open temp file");
        editor.insert_text("a");
        editor.insert_text("b");
        editor.save().expect("save editor");
        assert!(!editor.is_dirty());

        assert!(editor.undo());
        assert!(editor.is_dirty());
        editor.insert_text("c");

        assert_eq!(editor.lines(), &["ac".to_string()]);
        assert!(editor.is_dirty());

        fs::remove_file(path).expect("remove temp file");
    }

    fn temp_file_path(prefix: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{unique}.txt"))
    }

    #[test]
    fn replaces_completion_range_on_current_line() {
        let mut editor = Editor::scratch();
        editor.insert_text("pri");
        editor.replace_range_in_current_line(0, 3, "println!");

        assert_eq!(editor.lines(), &["println!".to_string()]);
        assert_eq!(editor.cursor_col(), 8);
    }

    #[test]
    fn replaces_completion_range_with_multiline_snippet() {
        let mut editor = Editor::scratch();
        editor.insert_text("ma");
        editor.replace_range_in_current_line(0, 2, "match value {\n    _ => {}\n}");

        assert_eq!(
            editor.lines(),
            &[
                "match value {".to_string(),
                "    _ => {}".to_string(),
                "}".to_string(),
            ]
        );
        assert_eq!(editor.cursor_row(), 2);
        assert_eq!(editor.cursor_col(), 1);
    }

    #[test]
    fn newline_carries_current_indent() {
        let mut editor = Editor::scratch();
        editor.insert_text("    let value = 1;");

        editor.insert_newline();

        assert_eq!(
            editor.lines(),
            &["    let value = 1;".to_string(), "    ".to_string()]
        );
        assert_eq!(editor.cursor_row(), 1);
        assert_eq!(editor.cursor_col(), 4);
    }

    #[test]
    fn newline_indents_after_opening_brace() {
        let mut editor = Editor::scratch();
        editor.insert_text("fn main() {");

        editor.insert_newline();

        assert_eq!(
            editor.lines(),
            &["fn main() {".to_string(), "    ".to_string()]
        );
        assert_eq!(editor.cursor_row(), 1);
        assert_eq!(editor.cursor_col(), 4);
    }

    #[test]
    fn newline_between_matching_braces_creates_inner_line() {
        let mut editor = Editor::scratch();
        editor.insert_text("    if ready {}");
        editor.set_cursor(0, 14);

        editor.insert_newline();

        assert_eq!(
            editor.lines(),
            &[
                "    if ready {".to_string(),
                "        ".to_string(),
                "    }".to_string(),
            ]
        );
        assert_eq!(editor.cursor_row(), 1);
        assert_eq!(editor.cursor_col(), 8);
    }

    #[test]
    fn closing_brace_dedents_indented_blank_line() {
        let mut editor = Editor::scratch();
        editor.insert_text("        ");

        editor.insert_char('}');

        assert_eq!(editor.lines(), &["    }".to_string()]);
        assert_eq!(editor.cursor_col(), 5);
    }
}
