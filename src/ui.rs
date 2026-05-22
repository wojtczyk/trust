use crate::app::{
    App, Dialog, Focus, Geometry, MENUS, MIN_DESKTOP_HEIGHT, MIN_EDITOR_PANE_WIDTH,
    MIN_MESSAGES_PANE_HEIGHT, MIN_PROJECT_PANE_WIDTH, MenuGeometry,
};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

const DOS_BLUE: Color = Color::Rgb(0, 0, 170);
const DOS_CYAN: Color = Color::Rgb(0, 170, 170);
const DOS_BRIGHT_CYAN: Color = Color::Rgb(85, 255, 255);
const DOS_GRAY: Color = Color::Rgb(170, 170, 170);
const DOS_DARK_GRAY: Color = Color::Rgb(85, 85, 85);
const DOS_GREEN: Color = Color::Rgb(0, 170, 0);
const DOS_RED: Color = Color::Rgb(170, 0, 0);
const DOS_BRIGHT_RED: Color = Color::Rgb(255, 85, 85);
const DOS_YELLOW: Color = Color::Rgb(255, 255, 85);
const DOS_WHITE: Color = Color::Rgb(255, 255, 255);
const DOS_BLACK: Color = Color::Rgb(0, 0, 0);

pub fn draw(frame: &mut Frame, app: &mut App) {
    let root = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(DOS_BLUE)), root);

    app.messages_pane_height = clamp_messages_height(root.height, app.messages_pane_height);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(app.messages_pane_height),
            Constraint::Length(1),
        ])
        .split(root);

    app.geometry = Geometry {
        root,
        menu_area: vertical[0],
        messages_area: vertical[2],
        status_area: vertical[3],
        ..Geometry::default()
    };

    draw_menu(frame, vertical[0], app);
    draw_desktop(frame, vertical[1], app);
    draw_messages(frame, vertical[2], app);
    draw_status(frame, vertical[3], app);

    if app.menu_open {
        draw_menu_dropdown(frame, app);
    }

    if app.completion_visible() {
        draw_completion_popup(frame, app);
    }

    if app.help_open {
        draw_help(frame, centered(root, 72, 17));
    } else if let Some(dialog) = app.dialog {
        match dialog {
            Dialog::Find => draw_find_dialog(frame, app, centered(root, 72, 9)),
            Dialog::NewFile => draw_new_file_dialog(frame, app, centered(root, 66, 10)),
            Dialog::NewProject => draw_new_project_dialog(frame, app, centered(root, 74, 16)),
            Dialog::About | Dialog::CompileResult => {
                draw_dialog(frame, app, dialog, centered(root, 66, 13));
            }
        }
    }
}

fn draw_menu(frame: &mut Frame, area: Rect, app: &mut App) {
    app.geometry.menu = MenuGeometry::default();
    let mut spans = vec![Span::styled(
        " TRUST ",
        Style::default()
            .fg(DOS_YELLOW)
            .bg(DOS_DARK_GRAY)
            .add_modifier(Modifier::BOLD),
    )];
    let mut x = area.x + " TRUST ".len() as u16;

    for (index, menu) in MENUS.iter().enumerate() {
        let active = app.menu_open && app.active_menu == index;
        let title = menu.title;
        let hot = &title[..1];
        let rest = &title[1..];
        let menu_width = title.len() as u16 + 1;
        app.geometry.menu.bar_items[index] = Rect {
            x,
            y: area.y,
            width: menu_width,
            height: 1,
        };
        x = x.saturating_add(menu_width);

        let style = if active {
            Style::default()
                .fg(DOS_BLACK)
                .bg(DOS_CYAN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DOS_BLACK).bg(DOS_GRAY)
        };
        let hot_style = if active {
            Style::default()
                .fg(DOS_RED)
                .bg(DOS_CYAN)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(DOS_RED)
                .bg(DOS_GRAY)
                .add_modifier(Modifier::BOLD)
        };

        spans.push(Span::styled(" ", style));
        spans.push(Span::styled(hot, hot_style));
        spans.push(Span::styled(rest, style));
    }

    let base = Style::default().fg(DOS_BLACK).bg(DOS_GRAY);
    let button_style = Style::default()
        .fg(DOS_WHITE)
        .bg(DOS_BLUE)
        .add_modifier(Modifier::BOLD);
    let debug_style = if app.debug_active() {
        Style::default()
            .fg(DOS_BLACK)
            .bg(DOS_BRIGHT_CYAN)
            .add_modifier(Modifier::BOLD)
    } else {
        button_style
    };
    let breakpoint_style = Style::default()
        .fg(DOS_YELLOW)
        .bg(DOS_DARK_GRAY)
        .add_modifier(Modifier::BOLD);

    let button_labels = [
        ("[ Run ]", button_style),
        (
            if app.debug_active() {
                "[ Continue ]"
            } else {
                "[ Debug ]"
            },
            debug_style,
        ),
        ("[ BP ]", breakpoint_style),
    ];
    let buttons_width = button_labels
        .iter()
        .map(|(label, _)| label.chars().count())
        .sum::<usize>();
    let left_width = Line::from(spans.clone()).width();
    let padding = (area.width as usize).saturating_sub(left_width + buttons_width);
    spans.push(Span::styled(" ".repeat(padding), base));

    let mut button_x = area.x.saturating_add((left_width + padding) as u16);
    for (index, (label, style)) in button_labels.into_iter().enumerate() {
        let width = label.chars().count() as u16;
        let rect = Rect {
            x: button_x,
            y: area.y,
            width,
            height: 1,
        };
        match index {
            0 => app.geometry.menu.run_button = rect,
            1 => app.geometry.menu.debug_button = rect,
            _ => app.geometry.menu.breakpoint_button = rect,
        }
        spans.push(Span::styled(label, style));
        button_x = button_x.saturating_add(width);
    }

    frame.render_widget(Paragraph::new(Line::from(spans)).style(base), area);
}

fn draw_menu_dropdown(frame: &mut Frame, app: &mut App) {
    let menu = MENUS[app.active_menu];
    let bar = app.geometry.menu.bar_items[app.active_menu];
    let width = menu
        .items
        .iter()
        .map(|item| {
            if item.separator {
                8
            } else {
                item.label.len() + item.shortcut.len() + 5
            }
        })
        .max()
        .unwrap_or(14)
        .max(menu.title.len() + 4) as u16;
    let height = menu.items.len() as u16 + 2;
    let max_x = app.geometry.root.x + app.geometry.root.width.saturating_sub(width);
    let area = Rect {
        x: bar.x.min(max_x),
        y: app.geometry.menu_area.y.saturating_add(1),
        width,
        height: height.min(app.geometry.root.height.saturating_sub(1)),
    };
    app.geometry.menu.dropdown = Some(area);

    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = menu
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            if item.separator {
                Line::from(Span::styled(
                    "─".repeat(inner.width as usize),
                    Style::default().fg(DOS_DARK_GRAY).bg(DOS_GRAY),
                ))
            } else {
                let active = index == app.active_menu_item;
                let style = if active {
                    Style::default()
                        .fg(DOS_WHITE)
                        .bg(DOS_BLUE)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(DOS_BLACK).bg(DOS_GRAY)
                };
                let hot_style = if active {
                    Style::default()
                        .fg(DOS_BRIGHT_RED)
                        .bg(DOS_BLUE)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(DOS_RED)
                        .bg(DOS_GRAY)
                        .add_modifier(Modifier::BOLD)
                };
                let shortcut_gap = inner
                    .width
                    .saturating_sub(item.label.len() as u16)
                    .saturating_sub(item.shortcut.len() as u16)
                    .saturating_sub(2) as usize;
                let hot = &item.label[..1];
                let rest = &item.label[1..];
                Line::from(vec![
                    Span::styled(" ", style),
                    Span::styled(hot, hot_style),
                    Span::styled(rest, style),
                    Span::styled(
                        format!("{}{} ", " ".repeat(shortcut_gap), item.shortcut),
                        style,
                    ),
                ])
            }
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
        inner,
    );
}

fn draw_desktop(frame: &mut Frame, area: Rect, app: &mut App) {
    let desktop = area.inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    app.project_pane_width = clamp_project_width(desktop.width, app.project_pane_width);
    app.geometry.desktop_inner = desktop;

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(app.project_pane_width),
            Constraint::Min(MIN_EDITOR_PANE_WIDTH),
        ])
        .split(desktop);

    app.geometry.project_area = columns[0];
    app.geometry.editor_area = columns[1];
    draw_project(frame, columns[0], app);
    draw_editor(frame, columns[1], app);
}

fn draw_project(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = format!(" Project: {} ", app.browser_label());
    let block = retro_block(
        &title,
        app.focus == Focus::Project,
        "Enter Open",
        Some("Backspace Up"),
    );
    let inner = block.inner(area);
    app.geometry.project_inner = inner;
    frame.render_widget(block, area);

    if app.project_files.is_empty() {
        let text = Text::from(vec![
            Line::from("No editable files here."),
            Line::from(""),
            Line::from("Use File > New"),
            Line::from("or enter another directory."),
        ]);
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(DOS_WHITE).bg(DOS_BLUE))
                .alignment(Alignment::Center),
            inner,
        );
        return;
    }

    let height = inner.height as usize;
    let selected = app.selected_file;
    let start = selected.saturating_sub(height.saturating_sub(1));
    let items = app
        .project_files
        .iter()
        .enumerate()
        .skip(start)
        .take(height)
        .map(|(index, entry)| {
            let style = if index == selected {
                Style::default()
                    .fg(DOS_BLACK)
                    .bg(DOS_CYAN)
                    .add_modifier(Modifier::BOLD)
            } else if entry.is_directory() {
                Style::default().fg(DOS_YELLOW).bg(DOS_BLUE)
            } else {
                Style::default().fg(DOS_WHITE).bg(DOS_BLUE)
            };
            let label = if entry.is_directory() {
                format!("[D] {}", entry.label)
            } else {
                format!("    {}", entry.label)
            };
            ListItem::new(Line::from(Span::styled(
                truncate(&label, inner.width),
                style,
            )))
        })
        .collect::<Vec<_>>();

    frame.render_widget(List::new(items).style(Style::default().bg(DOS_BLUE)), inner);
}

fn draw_editor(frame: &mut Frame, area: Rect, app: &mut App) {
    let dirty = if app.editor.is_dirty() { " *" } else { "" };
    let title = format!(" {}{} ", app.current_file_label(), dirty);
    let footer = if app.completion_visible() {
        "Ctrl+Space Complete"
    } else if app.search_summary().is_some() {
        "Ctrl+G Next"
    } else {
        "F2 Save"
    };
    let block = retro_block(&title, app.focus == Focus::Editor, "Edit", Some(footer));
    let inner = block.inner(area);
    app.geometry.editor_inner = inner;
    frame.render_widget(block, area);

    let gutter_width = app.editor_gutter_width();
    let line_number_width = app.editor_line_number_width();
    let text_cols = inner.width.saturating_sub(gutter_width) as usize;
    let text_rows = inner.height as usize;
    app.editor.set_viewport(text_rows, text_cols);

    let mut lines = Vec::with_capacity(text_rows);
    let row_offset = app.editor.row_offset();
    let col_offset = app.editor.col_offset();
    let current_path = app.editor.path().map(|path| path.to_path_buf());

    for screen_row in 0..text_rows {
        let file_row = row_offset + screen_row;
        let mut spans = Vec::new();
        if let Some(line) = app.editor.lines().get(file_row) {
            let marker = current_path
                .as_deref()
                .map(|path| app.has_breakpoint(path, file_row))
                .unwrap_or(false);
            let paused = current_path
                .as_deref()
                .map(|path| app.paused_line(path, file_row))
                .unwrap_or(false);
            let number = format!(
                "{} {:>width$} ",
                if marker { "●" } else { " " },
                file_row + 1,
                width = line_number_width as usize
            );
            spans.push(Span::styled(number, editor_gutter_style(marker, paused)));

            spans.extend(render_editor_line(
                line,
                col_offset,
                text_cols,
                app.editor.selection_range_for_line(file_row),
                &app.search_match_ranges_for_line(file_row),
                paused,
            ));
        } else {
            spans.push(Span::styled(
                format!("  {}", "~".repeat(line_number_width as usize)),
                Style::default().fg(DOS_DARK_GRAY).bg(DOS_BLUE),
            ));
        }
        lines.push(Line::from(spans));
    }

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(DOS_WHITE).bg(DOS_BLUE)),
        inner,
    );

    if app.focus == Focus::Editor {
        let cursor_x = inner.x
            + gutter_width
            + app
                .editor
                .cursor_col()
                .saturating_sub(app.editor.col_offset()) as u16;
        let cursor_y = inner.y
            + app
                .editor
                .cursor_row()
                .saturating_sub(app.editor.row_offset()) as u16;
        if cursor_x < inner.x + inner.width && cursor_y < inner.y + inner.height {
            frame.set_cursor_position((cursor_x, cursor_y));
        }
    }
}

fn draw_messages(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .title(" Compiler Messages ")
        .title_style(Style::default().fg(DOS_BLUE).bg(DOS_CYAN))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_BLUE).bg(DOS_CYAN).add_modifier(
            if app.focus == Focus::Messages {
                Modifier::BOLD
            } else {
                Modifier::empty()
            },
        ))
        .style(Style::default().fg(DOS_BLUE).bg(DOS_CYAN))
        .title_bottom(Line::from(Span::styled(
            "Cargo",
            Style::default().fg(DOS_BLUE).bg(DOS_CYAN),
        )))
        .title_bottom(
            Line::from(Span::styled(
                "F7 Check",
                Style::default().fg(DOS_BLUE).bg(DOS_CYAN),
            ))
            .right_aligned(),
        );
    let inner = block.inner(area);
    app.geometry.messages_inner = inner;
    frame.render_widget(block, area);

    let visible_rows = inner.height as usize;
    let total = app.messages.len();
    let start = total.saturating_sub(visible_rows + app.message_scroll);
    let end = total.saturating_sub(app.message_scroll);

    let active_message = app.active_message.min(total.saturating_sub(1));
    let items = app.messages[start..end]
        .iter()
        .enumerate()
        .map(|(offset, message)| {
            let index = start + offset;
            let style = if index == active_message {
                Style::default()
                    .fg(DOS_BRIGHT_CYAN)
                    .bg(DOS_GREEN)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DOS_BLUE).bg(DOS_CYAN)
            };
            Line::from(Span::styled(truncate(message, inner.width), style))
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(items).style(Style::default().fg(DOS_BLUE).bg(DOS_CYAN)),
        inner,
    );
}

fn draw_completion_popup(frame: &mut Frame, app: &App) {
    let Some(popup) = &app.completion_popup else {
        return;
    };

    let inner = app.geometry.editor_inner;
    if inner.width < 12 || inner.height < 3 {
        return;
    }

    let max_label = popup
        .items
        .iter()
        .map(|item| {
            let detail = item.detail.as_deref().unwrap_or_default();
            item.label.chars().count()
                + usize::from(!detail.is_empty()) * (detail.chars().count() + 3)
        })
        .max()
        .unwrap_or(12)
        .min(inner.width.saturating_sub(2) as usize);
    let width = (max_label + 2).max(16) as u16;
    let height = (popup.items.len().min(8) + 2) as u16;
    let cursor_x = inner
        .x
        .saturating_add(app.editor_gutter_width())
        .saturating_add(
            app.editor
                .cursor_col()
                .saturating_sub(app.editor.col_offset()) as u16,
        );
    let cursor_y = inner.y.saturating_add(
        app.editor
            .cursor_row()
            .saturating_sub(app.editor.row_offset()) as u16,
    );
    let max_x = inner.x.saturating_add(inner.width.saturating_sub(width));
    let max_y = inner.y.saturating_add(inner.height.saturating_sub(height));
    let area = Rect {
        x: cursor_x.min(max_x),
        y: cursor_y.saturating_add(1).min(max_y),
        width,
        height,
    };

    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Complete ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items = popup
        .items
        .iter()
        .skip(popup.scroll)
        .take(inner.height as usize)
        .enumerate()
        .map(|(index, item)| {
            let active = popup.scroll + index == popup.selected;
            let style = if active {
                Style::default()
                    .fg(DOS_WHITE)
                    .bg(DOS_BLUE)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DOS_BLACK).bg(DOS_GRAY)
            };
            let detail = item.detail.as_deref().unwrap_or_default();
            let detail_text = if detail.is_empty() {
                String::new()
            } else {
                format!(
                    " - {}",
                    truncate(
                        detail,
                        inner.width.saturating_sub(item.label.len() as u16 + 3)
                    )
                )
            };
            Line::from(Span::styled(
                truncate(&format!("{}{}", item.label, detail_text), inner.width),
                style,
            ))
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Paragraph::new(items).style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
        inner,
    );
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let base = Style::default()
        .fg(DOS_BLACK)
        .bg(DOS_GRAY)
        .add_modifier(Modifier::BOLD);
    let key = Style::default()
        .fg(DOS_RED)
        .bg(DOS_GRAY)
        .add_modifier(Modifier::BOLD);
    let position = format!(
        "Ln {}, Col {}",
        app.editor.cursor_row() + 1,
        app.editor.cursor_col() + 1
    );
    let selection = if app.editor.has_selection() {
        "  Sel"
    } else {
        ""
    };
    let completion = if app.completion_visible() {
        "  Complete"
    } else {
        ""
    };
    let search = app
        .search_summary()
        .map(|summary| format!("  {summary}"))
        .unwrap_or_default();
    let debug = if app.debug_active() { "  Debug" } else { "" };
    let suffix = format!(
        "  {position}{selection}{completion}{search}{debug}  {} ",
        app.status
    );
    let mut line = Line::from(vec![
        Span::styled(" ", base),
        Span::styled("F1", key),
        Span::styled(" Help  ", base),
        Span::styled("F2", key),
        Span::styled(" Save  ", base),
        Span::styled("F3", key),
        Span::styled(" Open  ", base),
        Span::styled("F5", key),
        Span::styled(" Run  ", base),
        Span::styled("F6", key),
        Span::styled(" BP  ", base),
        Span::styled("F7", key),
        Span::styled(" Check  ", base),
        Span::styled("F9", key),
        Span::styled(" Build  ", base),
        Span::styled("F10", key),
        Span::styled(" Menu  ", base),
        Span::styled("^Z", key),
        Span::styled(" Undo  ", base),
        Span::styled("^Y", key),
        Span::styled(" Redo", base),
        Span::styled(suffix, base),
    ]);
    let width = line.width();
    if width < area.width as usize {
        line.spans
            .push(Span::styled(" ".repeat(area.width as usize - width), base));
    }
    frame.render_widget(Paragraph::new(line).style(base), area);
}

fn clamp_messages_height(root_height: u16, current: u16) -> u16 {
    let max_height = root_height.saturating_sub(MIN_DESKTOP_HEIGHT + 2).max(1);
    let min_height = MIN_MESSAGES_PANE_HEIGHT.min(max_height);
    current.clamp(min_height, max_height)
}

fn clamp_project_width(total_width: u16, current: u16) -> u16 {
    if total_width <= 1 {
        return 1;
    }

    let editor_reserve = MIN_EDITOR_PANE_WIDTH.min(total_width.saturating_sub(1));
    let max_width = total_width.saturating_sub(editor_reserve).max(1);
    let min_width = MIN_PROJECT_PANE_WIDTH.min(max_width);
    current.clamp(min_width, max_width)
}

fn draw_help(frame: &mut Frame, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = Text::from(vec![
        Line::from(vec![Span::styled(
            "TRUST keys",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("F2 Save     F3 Open selected file    F4 Cycle focus"),
        Line::from("File > New creates a file"),
        Line::from("Project > New project runs cargo new"),
        Line::from("F5 Run      F6 Toggle breakpoint     F7 Cargo check"),
        Line::from("F8 Test     F9 Build                 F10 Menu"),
        Line::from("Ctrl+D Debug/Continue  F11 Step Into  F12 Step Over"),
        Line::from("Shift+F11 Step Out     Shift+F5 Stop debug"),
        Line::from("Ctrl+Z Undo Ctrl+Y Redo Ctrl+C Copy Ctrl+Q Quit"),
        Line::from("Ctrl+X Cut  Ctrl+V Paste Ctrl+S Save Ctrl+F Find"),
        Line::from("Ctrl+G Find next  Shift+Enter Find previous"),
        Line::from("Ctrl+Space Complete"),
        Line::from("Tab Indent  Shift+Tab Unindent"),
        Line::from("Alt+U Duplicate line   Alt+X Delete line"),
        Line::from("Shift+Arrows/Home/End/Page selects text"),
        Line::from("Menu: F10 opens, arrows move, Enter activates."),
        Line::from(""),
        Line::from("Mouse: click panes to focus, click source to move cursor,"),
        Line::from("click gutter or [BP] to toggle breakpoints,"),
        Line::from("click [Run] or [Debug] in the top bar to execute,"),
        Line::from("click files/directories to open or browse,"),
        Line::from("drag pane borders to resize."),
        Line::from(""),
        Line::from("The colors are intentionally loud. History had opinions."),
        Line::from("Press any key to return."),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_dialog(frame: &mut Frame, app: &App, dialog: Dialog, area: Rect) {
    frame.render_widget(Clear, area);
    let title = match dialog {
        Dialog::About => " About TRUST ",
        Dialog::CompileResult => " Cargo Result ",
        Dialog::Find => " Find ",
        Dialog::NewFile => " New File ",
        Dialog::NewProject => " New Project ",
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let text = match dialog {
        Dialog::About => Text::from(vec![
            Line::from(""),
            Line::from(vec![Span::styled(
                "TRUST 0.1",
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("Retro DOS-style Rust IDE"),
            Line::from("a terminal IDE for Rust projects."),
            Line::from(""),
            Line::from("Press any key to begin."),
        ]),
        Dialog::CompileResult => Text::from(vec![
            Line::from(vec![Span::styled(
                app.status.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )]),
            Line::from(""),
            Line::from("Compiler output is in the message window."),
            Line::from("Use F7/F8/F9/F5 for check/test/build/run."),
            Line::from(""),
            Line::from("Press any key to return."),
        ]),
        Dialog::Find => Text::from(""),
        Dialog::NewFile => Text::from(""),
        Dialog::NewProject => Text::from(""),
    };

    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_find_dialog(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" Find ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let query = truncate(&app.find.query, inner.width.saturating_sub(14).max(1));
    let match_summary = if app.find.query.is_empty() {
        "Type to search in the current file.".to_string()
    } else if app.search_summary().is_some() {
        app.search_summary().unwrap_or_default()
    } else {
        "Find 0".to_string()
    };

    let text = Text::from(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" Query ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(
                format!(" {query} "),
                Style::default()
                    .fg(DOS_WHITE)
                    .bg(DOS_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(format!(" {match_summary}")),
        Line::from(""),
        Line::from(" Enter next  Shift+Enter previous  Esc close "),
    ]);

    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY))
            .wrap(Wrap { trim: true }),
        inner,
    );

    if app.dialog == Some(Dialog::Find) {
        let cursor_x = inner
            .x
            .saturating_add(8 + app.find.query.chars().count() as u16);
        let cursor_x = cursor_x.min(inner.x.saturating_add(inner.width.saturating_sub(1)));
        let cursor_y = inner.y.saturating_add(1);
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn draw_new_file_dialog(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" New File ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let directory = truncate(&app.browser_label(), inner.width.saturating_sub(16).max(1));
    let filename = truncate(&app.new_file.name, inner.width.saturating_sub(16).max(1));

    let text = Text::from(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" Directory ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(
                format!(" {directory} "),
                Style::default().fg(DOS_BLUE).bg(DOS_CYAN),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Filename  ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(format!(" {filename} "), new_project_field_style(true)),
        ]),
        Line::from(""),
        Line::from(" Enter creates. Esc cancels."),
    ]);

    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn draw_new_project_dialog(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(" New Cargo Project ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(DOS_YELLOW).bg(DOS_CYAN))
        .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let directory_style =
        new_project_field_style(app.new_project.field == crate::app::NewProjectField::Directory);
    let name_style =
        new_project_field_style(app.new_project.field == crate::app::NewProjectField::Name);
    let kind_style =
        new_project_field_style(app.new_project.field == crate::app::NewProjectField::Kind);
    let create_style =
        new_project_field_style(app.new_project.field == crate::app::NewProjectField::Create);

    let kind = app.new_project.kind;
    let bin_style = if kind == crate::app::NewProjectKind::Bin {
        kind_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DOS_BLACK).bg(DOS_GRAY)
    };
    let lib_style = if kind == crate::app::NewProjectKind::Lib {
        kind_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DOS_BLACK).bg(DOS_GRAY)
    };

    let directory = truncate(
        &app.new_project.directory,
        inner.width.saturating_sub(18).max(1),
    );
    let name = truncate(&app.new_project.name, inner.width.saturating_sub(18).max(1));

    let text = Text::from(vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" Directory ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(format!(" {directory} "), directory_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Project   ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(format!(" {name} "), name_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Type      ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(" bin ", bin_style),
            Span::styled("  ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(" lib ", lib_style),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("           ", Style::default().fg(DOS_BLACK).bg(DOS_GRAY)),
            Span::styled(" Create ", create_style),
        ]),
        Line::from(""),
        Line::from(" Tab moves fields. Enter creates. Esc cancels."),
    ]);

    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(DOS_BLACK).bg(DOS_GRAY))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn new_project_field_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(DOS_WHITE)
            .bg(DOS_BLUE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DOS_BLUE).bg(DOS_CYAN)
    }
}

fn retro_block<'a>(
    title: &'a str,
    active: bool,
    left: &'a str,
    right: Option<&'a str>,
) -> Block<'a> {
    let border = if active {
        Style::default()
            .fg(DOS_YELLOW)
            .bg(DOS_BLUE)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DOS_WHITE).bg(DOS_BLUE)
    };

    let mut block = Block::default()
        .title(title)
        .title_style(Style::default().fg(DOS_YELLOW).bg(DOS_BLUE))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(border)
        .style(Style::default().bg(DOS_BLUE))
        .title_bottom(Line::from(Span::styled(
            left,
            Style::default().fg(DOS_YELLOW).bg(DOS_BLUE),
        )));

    if let Some(right) = right {
        block = block.title_bottom(
            Line::from(Span::styled(
                right,
                Style::default().fg(DOS_CYAN).bg(DOS_BLUE),
            ))
            .right_aligned(),
        );
    }

    block
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(4));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn truncate(value: &str, width: u16) -> String {
    let width = width as usize;
    let mut result = value.chars().take(width).collect::<String>();
    if result.chars().count() == width && value.chars().count() > width && width > 1 {
        result.pop();
        result.push('>');
    }
    result
}

fn render_editor_line(
    line: &str,
    col_offset: usize,
    text_cols: usize,
    selection: Option<(usize, usize)>,
    search_matches: &[(usize, usize, bool)],
    paused: bool,
) -> Vec<Span<'static>> {
    let chars = line
        .chars()
        .skip(col_offset)
        .take(text_cols)
        .collect::<Vec<_>>();
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_kind = None;

    for (screen_col, character) in chars.into_iter().enumerate() {
        let absolute_col = col_offset + screen_col;
        let selected = selection
            .map(|(start, end)| absolute_col >= start && absolute_col < end)
            .unwrap_or(false);
        let search_kind = if selected {
            HighlightKind::Selected
        } else if search_matches
            .iter()
            .any(|(start, end, active)| *active && absolute_col >= *start && absolute_col < *end)
        {
            HighlightKind::ActiveSearch
        } else if search_matches
            .iter()
            .any(|(start, end, _)| absolute_col >= *start && absolute_col < *end)
        {
            HighlightKind::Search
        } else {
            HighlightKind::Normal
        };

        if run_kind == Some(search_kind) || run_kind.is_none() {
            run.push(character);
            run_kind = Some(search_kind);
        } else {
            push_editor_run(
                &mut spans,
                &run,
                run_kind.unwrap_or(HighlightKind::Normal),
                paused,
            );
            run.clear();
            run.push(character);
            run_kind = Some(search_kind);
        }
    }

    if !run.is_empty() {
        push_editor_run(
            &mut spans,
            &run,
            run_kind.unwrap_or(HighlightKind::Normal),
            paused,
        );
    }

    spans
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HighlightKind {
    Normal,
    Search,
    ActiveSearch,
    Selected,
}

fn push_editor_run(spans: &mut Vec<Span<'static>>, run: &str, kind: HighlightKind, paused: bool) {
    match kind {
        HighlightKind::Selected => spans.push(Span::styled(
            run.to_string(),
            Style::default()
                .fg(DOS_BLACK)
                .bg(DOS_CYAN)
                .add_modifier(Modifier::BOLD),
        )),
        HighlightKind::ActiveSearch => spans.push(Span::styled(
            run.to_string(),
            Style::default()
                .fg(DOS_BLACK)
                .bg(DOS_YELLOW)
                .add_modifier(Modifier::BOLD),
        )),
        HighlightKind::Search => spans.push(Span::styled(
            run.to_string(),
            Style::default().fg(DOS_YELLOW).bg(DOS_DARK_GRAY),
        )),
        HighlightKind::Normal => {
            spans.extend(highlight_rust(run, paused));
        }
    }
}

fn highlight_rust(line: &str, paused: bool) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut in_string = false;

    for character in line.chars() {
        if character == '"' {
            if !current.is_empty() {
                spans.push(classify_token(&current, paused));
                current.clear();
            }
            spans.push(Span::styled("\"", syntax_style(DOS_YELLOW, paused)));
            in_string = !in_string;
        } else if in_string {
            spans.push(Span::styled(
                character.to_string(),
                syntax_style(DOS_YELLOW, paused),
            ));
        } else if character.is_alphanumeric() || character == '_' {
            current.push(character);
        } else {
            if !current.is_empty() {
                spans.push(classify_token(&current, paused));
                current.clear();
            }
            spans.push(Span::styled(
                character.to_string(),
                syntax_style(DOS_WHITE, paused),
            ));
        }
    }

    if !current.is_empty() {
        spans.push(classify_token(&current, paused));
    }

    spans
}

fn classify_token(token: &str, paused: bool) -> Span<'static> {
    let style = if is_keyword(token) {
        syntax_style(DOS_YELLOW, paused).add_modifier(Modifier::BOLD)
    } else if matches!(token, "self" | "Self" | "crate" | "super") {
        syntax_style(DOS_CYAN, paused)
    } else {
        syntax_style(DOS_WHITE, paused)
    };
    Span::styled(token.to_string(), style)
}

fn syntax_style(fg: Color, paused: bool) -> Style {
    Style::default()
        .fg(fg)
        .bg(if paused { DOS_RED } else { DOS_BLUE })
}

fn editor_gutter_style(breakpoint: bool, paused: bool) -> Style {
    let fg = if breakpoint {
        DOS_BRIGHT_RED
    } else {
        DOS_YELLOW
    };
    Style::default()
        .fg(fg)
        .bg(if paused { DOS_RED } else { DOS_BLUE })
        .add_modifier(if breakpoint || paused {
            Modifier::BOLD
        } else {
            Modifier::empty()
        })
}

fn is_keyword(token: &str) -> bool {
    matches!(
        token,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
    )
}
