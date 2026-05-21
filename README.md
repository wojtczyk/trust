# TRUST

TRUST is a retro TUI IDE for Rust projects inspired by classic blue-screen DOS
development environments.

Status: experimental nostalgia project. It edits files, browses Rust projects,
and runs Cargo commands.

## New IDE Features

- `rust-analyzer` powered autocomplete for Rust files, with a built-in fallback
  completion list if the language server is unavailable.
- Clickable top-bar `Run`, `Debug`, and `BP` controls.
- Source-level breakpoints in the editor gutter and a basic LLDB-backed debug
  loop with continue and stepping commands.

## Screenshots

Building and running "Hello World" in TRUST.

| Starting a project | Running a console program |
| --- | --- |
| ![Hello project in TRUST](doc/1_hello.png) | ![Hello project editor view](doc/2_hello.png) |

TRUST can build TRUST.

| TRUST Editor | Running Tests |
| --- | --- |
| ![TRUST project in TRUST](doc/3_trust.png) | ![TRUST menus and panes](doc/4_trust.png) |

## FAQ

**Why?**  
Because Rust deserves a blue-screen IDE from the olden days and someone had to do this.

**Does it save my files?**  
Yes. Use `F2` or `Ctrl+S`. TRUST marks dirty buffers with `*` in the editor title. Still, this is more of a fun project so use at your own risk.

**Is this affiliated with any classic DOS IDE vendor?**  
No. TRUST is an independent nostalgia project inspired by classic DOS development environments.

## Run

```sh
cargo run -- /path/to/rust/project
```

If no path is supplied, TRUST opens the current directory.

## Keys

- `F1`: help
- `F2` / `Ctrl+S`: save
- `F3` / `Ctrl+O`: open selected file
- `Backspace`: go to the parent directory in the project pane
- `F4` / `Ctrl+F`: cycle focus
- `F5` / `Ctrl+R`: `cargo run`
- `F6`: toggle breakpoint on the current line
- `F7`: `cargo check`
- `F8` / `Ctrl+T`: `cargo test`
- `F9` / `Ctrl+B`: `cargo build`
- `F10`: open the menu bar
- `Ctrl+Z`: undo
- `Ctrl+Y` / `Ctrl+Shift+Z`: redo
- `F11`: debugger step into
- `F12`: debugger step over
- `Shift+F11`: debugger step out
- `Shift+F5`: stop the active debug session
- `Ctrl+C`: copy selected text
- `Ctrl+D`: start or continue the debugger
- `Ctrl+Space`: request autocomplete suggestions
- `Ctrl+V`: paste clipboard text
- `Ctrl+X`: cut selected text
- `Esc` / `Ctrl+Q`: quit
- `Alt+X`: delete line
- `Alt+U`: duplicate line
- `Tab`: indent
- `Shift+Tab`: unindent
- `Shift+Navigation`: select text
- In the editor, `Enter` inserts an auto-indented new line.

## Menus

- `F10` opens the menu bar.
- Left/right arrows switch menus.
- Up/down arrows move through a dropdown.
- `Enter` activates the highlighted menu item.
- `Esc` closes the menu.
- Mouse clicks on the menu bar and dropdown items work too.
- `Edit` now includes `Undo` and `Redo`.
- `Debug` contains start/continue, breakpoint, step, and stop commands.
- `File > New` asks for a filename and creates it in the current project pane
  directory.
- `Project > New project` opens the Cargo project dialog with parent directory,
  project name, and `bin` / `lib` selector.
- `Window` switches between panes and contains the former focus option.

## Mouse

- Click inside the editor to move the cursor.
- Click inside the left editor gutter, or use `F6`, to toggle a breakpoint.
- Drag inside the editor to select text.
- Click the top bar `Run`, `Debug`, or `BP` buttons to trigger those actions.
- Click inside the project pane to open editable files or navigate directories.
- Click inside any pane to focus it.
- Drag the vertical divider between project and editor panes to resize them.
- Drag the top border of the compiler/message pane to resize it.
- Scroll inside the project, editor, or message pane to move through content.

The project pane lists directories plus editable Rust and Cargo-related files
such as `.rs`, `.toml`, and `.lock`, while skipping `.git`, `target`, and common
editor/build directories. Compiler output is captured in the bottom pane.
