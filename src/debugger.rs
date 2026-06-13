use std::{
    env::consts::EXE_SUFFIX,
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
};

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub path: PathBuf,
    pub line: usize,
}

#[derive(Debug, Clone)]
pub enum DebuggerEvent {
    Output(String),
    Stopped(SourceLocation),
    Exited(Option<i32>),
}

#[derive(Debug)]
pub struct DebuggerSession {
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    events: mpsc::Receiver<DebuggerEvent>,
}

impl DebuggerSession {
    pub fn start(root: &Path, breakpoints: &[SourceLocation]) -> io::Result<Self> {
        let binary = build_debug_binary(root)?;
        let mut child = Command::new("lldb")
            .arg(&binary)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("lldb stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("lldb stdout unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("lldb stderr unavailable"))?;
        let writer = Arc::new(Mutex::new(stdin));
        let (sender, events) = mpsc::channel();

        spawn_reader(stdout, sender.clone(), root.to_path_buf());
        spawn_reader(stderr, sender.clone(), root.to_path_buf());

        let mut session = Self {
            child,
            writer,
            events,
        };
        session.send("settings set stop-disassembly-display never")?;
        session.send(
            "settings set target.process.thread.step-avoid-regexp '^std::|^core::|^alloc::'",
        )?;
        for breakpoint in breakpoints {
            session.send(&breakpoint_command(breakpoint)?)?;
        }
        session.send("run")?;
        Ok(session)
    }

    pub fn send(&mut self, command: &str) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("lldb writer lock poisoned"))?;
        writeln!(writer, "{command}")?;
        writer.flush()
    }

    pub fn try_recv(&mut self) -> Option<DebuggerEvent> {
        self.events.try_recv().ok()
    }

    pub fn stop(&mut self) -> io::Result<()> {
        let _ = self.send("process kill");
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
        Ok(())
    }
}

impl Drop for DebuggerSession {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn breakpoint_command(breakpoint: &SourceLocation) -> io::Result<String> {
    let file = breakpoint.path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "debugger breakpoint path is not valid UTF-8",
        )
    })?;
    let file = lldb_quote_argument(file)?;
    Ok(format!(
        "breakpoint set --file {file} --line {}",
        breakpoint.line + 1
    ))
}

fn lldb_quote_argument(value: &str) -> io::Result<String> {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            _ if character.is_control() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "debugger command argument contains a control character",
                ));
            }
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    Ok(quoted)
}

fn spawn_reader(
    stream: impl std::io::Read + Send + 'static,
    sender: mpsc::Sender<DebuggerEvent>,
    root: PathBuf,
) {
    std::thread::spawn(move || {
        let mut waiting_for_location = false;
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if line.contains("stop reason =")
                || line.contains("Process ") && line.contains("stopped")
            {
                waiting_for_location = true;
            }

            if waiting_for_location && let Some(location) = parse_frame_location(&line, &root) {
                let _ = sender.send(DebuggerEvent::Stopped(location));
                waiting_for_location = false;
            }

            if let Some(code) = parse_exit_code(&line) {
                let _ = sender.send(DebuggerEvent::Exited(code));
            }

            let _ = sender.send(DebuggerEvent::Output(line));
        }
    });
}

fn parse_exit_code(line: &str) -> Option<Option<i32>> {
    if !line.contains("Process ") || !line.contains("exited") {
        return None;
    }

    let status = line
        .split("status =")
        .nth(1)
        .and_then(|tail| tail.split(',').next())
        .map(str::trim)
        .and_then(|digits| digits.parse::<i32>().ok());
    Some(status)
}

fn parse_frame_location(line: &str, root: &Path) -> Option<SourceLocation> {
    let (_, tail) = line.split_once(" at ")?;
    let path = tail.rsplit_once(':')?.0.rsplit_once(':')?.0;
    let line_number = tail
        .rsplit_once(':')?
        .1
        .split(':')
        .next()?
        .parse::<usize>()
        .ok()?;
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    Some(SourceLocation {
        path,
        line: line_number.saturating_sub(1),
    })
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    target_directory: PathBuf,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    manifest_path: PathBuf,
    name: String,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Deserialize)]
struct CargoTarget {
    kind: Vec<String>,
    name: String,
}

fn build_debug_binary(root: &Path) -> io::Result<PathBuf> {
    let output = Command::new("cargo")
        .arg("build")
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!("cargo build failed:\n{stderr}")));
    }

    let metadata = cargo_metadata(root)?;
    let manifest = root.join("Cargo.toml");
    let package = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path == manifest)
        .or_else(|| metadata.packages.first())
        .ok_or_else(|| io::Error::other("no cargo package metadata found"))?;

    let target = package
        .targets
        .iter()
        .find(|target| target.kind.iter().any(|kind| kind == "bin"))
        .ok_or_else(|| {
            io::Error::other(format!(
                "project {} has no binary target to debug",
                package.name
            ))
        })?;

    Ok(metadata
        .target_directory
        .join("debug")
        .join(format!("{}{}", target.name, EXE_SUFFIX)))
}

fn cargo_metadata(root: &Path) -> io::Result<CargoMetadata> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(io::Error::other(format!(
            "cargo metadata failed:\n{stderr}"
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|error| io::Error::other(format!("invalid cargo metadata: {error}")))
}

#[cfg(test)]
mod tests {
    use std::{io::ErrorKind, path::PathBuf};

    use super::{SourceLocation, breakpoint_command, lldb_quote_argument};

    #[test]
    fn quotes_lldb_command_arguments() {
        assert_eq!(
            lldb_quote_argument("src/main.rs").unwrap(),
            "\"src/main.rs\""
        );
        assert_eq!(
            lldb_quote_argument("src/file name.rs").unwrap(),
            "\"src/file name.rs\""
        );
        assert_eq!(
            lldb_quote_argument("src/quote\"name.rs").unwrap(),
            "\"src/quote\\\"name.rs\""
        );
        assert_eq!(
            lldb_quote_argument("src/back\\slash.rs").unwrap(),
            "\"src/back\\\\slash.rs\""
        );
        assert_eq!(
            lldb_quote_argument("src/single'quote.rs").unwrap(),
            "\"src/single'quote.rs\""
        );
    }

    #[test]
    fn rejects_control_characters_in_lldb_arguments() {
        let error = lldb_quote_argument("src/main.rs\nscript print('owned')")
            .expect_err("newline should be rejected");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }

    #[test]
    fn builds_breakpoint_command_with_quoted_path() {
        let breakpoint = SourceLocation {
            path: PathBuf::from("src/quote\"name.rs"),
            line: 2,
        };

        assert_eq!(
            breakpoint_command(&breakpoint).unwrap(),
            "breakpoint set --file \"src/quote\\\"name.rs\" --line 3"
        );
    }
}
