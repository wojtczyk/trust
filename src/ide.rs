use std::{
    collections::{BTreeSet, HashSet},
    io::{self, BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use serde_json::{Value, json};

use crate::{editor::Editor, project::ProjectEntry};

const COMPLETION_LIMIT: usize = 24;

#[derive(Debug, Clone)]
pub struct CompletionCandidate {
    pub label: String,
    pub insert_text: String,
    pub detail: Option<String>,
    pub kind: CompletionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    LanguageServer,
    Project,
    Keyword,
    Snippet,
}

#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub items: Vec<CompletionCandidate>,
    pub replace_start: usize,
    pub replace_end: usize,
}

#[derive(Debug)]
pub struct CompletionEngine {
    client: Option<LanguageClient>,
}

impl CompletionEngine {
    pub fn new(root: &Path) -> Self {
        let client = LanguageClient::new(root).ok();
        Self { client }
    }

    #[cfg(test)]
    pub fn disabled_for_tests() -> Self {
        Self { client: None }
    }

    pub fn refresh_root(&mut self, root: &Path) {
        self.client = LanguageClient::new(root).ok();
    }

    pub fn complete(
        &mut self,
        _root: &Path,
        editor: &Editor,
        project_files: &[ProjectEntry],
        force: bool,
    ) -> Option<CompletionResponse> {
        let path = editor.path()?;
        let line = editor.line(editor.cursor_row())?;
        let full_text = editor.text();
        let (replace_start, replace_end, prefix) = editor.completion_prefix_bounds();
        let trigger = editor.char_before_cursor();
        let after_dot = trigger == Some('.');

        if !force && prefix.chars().count() < 2 && !after_dot {
            return None;
        }

        let mut items = if force && path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            self.client
                .as_mut()
                .and_then(|client| {
                    client
                        .completion(path, &full_text, editor.cursor_row(), editor.cursor_col())
                        .ok()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        if items.is_empty() || force {
            let fallback = fallback_items(
                &full_text,
                line,
                prefix.as_str(),
                project_files,
                after_dot || force,
            );
            merge_completion_items(&mut items, fallback);
        } else if !prefix.is_empty() {
            items.retain(|item| matches_prefix(item, &prefix));
        }

        items.truncate(COMPLETION_LIMIT);

        if items.is_empty() {
            return None;
        }

        Some(CompletionResponse {
            items,
            replace_start,
            replace_end,
        })
    }

    pub fn is_language_server_available(&self) -> bool {
        self.client.is_some()
    }
}

fn fallback_items(
    text: &str,
    line: &str,
    prefix: &str,
    project_files: &[ProjectEntry],
    show_all: bool,
) -> Vec<CompletionCandidate> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    let normalized_prefix = prefix.to_ascii_lowercase();

    for keyword in rust_keywords() {
        if show_all || keyword.starts_with(&normalized_prefix) {
            insert_candidate(
                &mut items,
                &mut seen,
                CompletionCandidate {
                    label: keyword.to_string(),
                    insert_text: keyword.to_string(),
                    detail: Some("Rust keyword".to_string()),
                    kind: CompletionKind::Keyword,
                },
            );
        }
    }

    for snippet in rust_snippets() {
        if show_all || snippet.0.starts_with(&normalized_prefix) {
            insert_candidate(
                &mut items,
                &mut seen,
                CompletionCandidate {
                    label: snippet.0.to_string(),
                    insert_text: strip_snippet_placeholders(snippet.1),
                    detail: Some("Snippet".to_string()),
                    kind: CompletionKind::Snippet,
                },
            );
        }
    }

    for token in project_tokens(text, line, project_files) {
        let lower = token.to_ascii_lowercase();
        if (show_all || lower.starts_with(&normalized_prefix)) && lower != normalized_prefix {
            insert_candidate(
                &mut items,
                &mut seen,
                CompletionCandidate {
                    label: token.clone(),
                    insert_text: token,
                    detail: Some("Project symbol".to_string()),
                    kind: CompletionKind::Project,
                },
            );
        }
    }

    items.sort_by_key(|item| completion_rank(item, prefix));
    items
}

fn completion_rank(item: &CompletionCandidate, prefix: &str) -> (u8, u8, u8, String) {
    let label = item.label.to_ascii_lowercase();
    let prefix = prefix.to_ascii_lowercase();
    let exact = (label == prefix) as u8;
    let starts_with = (label.starts_with(&prefix)) as u8;
    let kind_rank = match item.kind {
        CompletionKind::LanguageServer => 0,
        CompletionKind::Project => 1,
        CompletionKind::Snippet => 2,
        CompletionKind::Keyword => 3,
    };
    (u8::MAX - exact, u8::MAX - starts_with, kind_rank, label)
}

fn merge_completion_items(
    items: &mut Vec<CompletionCandidate>,
    additions: Vec<CompletionCandidate>,
) {
    let mut seen = items
        .iter()
        .map(|item| item.label.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    for item in additions {
        if seen.insert(item.label.to_ascii_lowercase()) {
            items.push(item);
        }
    }
}

fn insert_candidate(
    items: &mut Vec<CompletionCandidate>,
    seen: &mut HashSet<String>,
    candidate: CompletionCandidate,
) {
    let key = candidate.label.to_ascii_lowercase();
    if seen.insert(key) {
        items.push(candidate);
    }
}

fn matches_prefix(item: &CompletionCandidate, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }

    item.label
        .to_ascii_lowercase()
        .starts_with(&prefix.to_ascii_lowercase())
}

fn project_tokens(text: &str, line: &str, project_files: &[ProjectEntry]) -> BTreeSet<String> {
    let mut tokens = BTreeSet::new();
    for token in split_identifiers(text) {
        if token.len() > 1 {
            tokens.insert(token.to_string());
        }
    }
    for token in split_identifiers(line) {
        if token.len() > 1 {
            tokens.insert(token.to_string());
        }
    }

    for entry in project_files {
        if let Some(stem) = entry.path.file_stem().and_then(|stem| stem.to_str())
            && is_identifier_like(stem)
        {
            tokens.insert(stem.to_string());
        }
    }

    tokens
}

fn split_identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| !character.is_alphanumeric() && character != '_')
        .filter(|token| !token.is_empty())
}

fn is_identifier_like(token: &str) -> bool {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn rust_keywords() -> &'static [&'static str] {
    &[
        "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
        "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move",
        "mut", "pub", "ref", "return", "Self", "self", "static", "struct", "super", "trait",
        "true", "type", "unsafe", "use", "where", "while",
    ]
}

fn rust_snippets() -> &'static [(&'static str, &'static str)] {
    &[
        ("dbg!", "dbg!($0)"),
        ("println!", "println!(\"$0\");"),
        ("eprintln!", "eprintln!(\"$0\");"),
        ("todo!", "todo!()"),
        ("unimplemented!", "unimplemented!()"),
        ("match", "match $0 {\n    _ => {}\n}"),
        ("if let", "if let $0 = $1 {\n    \n}"),
    ]
}

#[derive(Debug)]
struct LanguageClient {
    child: Child,
    writer: Arc<Mutex<ChildStdin>>,
    incoming: mpsc::Receiver<Value>,
    next_id: u64,
    versions: Vec<(PathBuf, i32)>,
}

impl LanguageClient {
    fn new(root: &Path) -> io::Result<Self> {
        let mut child = Command::new("rust-analyzer")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("rust-analyzer stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("rust-analyzer stdout unavailable"))?;

        let writer = Arc::new(Mutex::new(stdin));
        let (sender, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_lsp_message(&mut reader) {
                    Ok(Some(message)) => {
                        if let Ok(value) = serde_json::from_str::<Value>(&message)
                            && sender.send(value).is_err()
                        {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        });

        let mut client = Self {
            child,
            writer,
            incoming,
            next_id: 1,
            versions: Vec::new(),
        };

        let id = client.send_request("initialize", initialize_params(root))?;
        let _ = client.wait_for_response(id, Duration::from_millis(1500))?;
        client.send_notification("initialized", json!({}))?;

        Ok(client)
    }

    fn completion(
        &mut self,
        path: &Path,
        text: &str,
        row: usize,
        col: usize,
    ) -> io::Result<Vec<CompletionCandidate>> {
        self.sync_document(path, text)?;
        let line = text.lines().nth(row).unwrap_or_default();
        let params = json!({
            "textDocument": {
                "uri": path_to_uri(path),
            },
            "position": {
                "line": row,
                "character": utf16_col(line, col),
            },
            "context": {
                "triggerKind": 1
            }
        });
        let id = self.send_request("textDocument/completion", params)?;
        let value = self.wait_for_response(id, Duration::from_millis(450))?;
        Ok(parse_completion_response(value))
    }

    fn sync_document(&mut self, path: &Path, text: &str) -> io::Result<()> {
        let version = self.bump_version(path.to_path_buf());
        let uri = path_to_uri(path);
        if version == 1 {
            self.send_notification(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": "rust",
                        "version": version,
                        "text": text,
                    }
                }),
            )?;
        } else {
            self.send_notification(
                "textDocument/didChange",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "version": version,
                    },
                    "contentChanges": [
                        {
                            "text": text
                        }
                    ]
                }),
            )?;
        }
        Ok(())
    }

    fn bump_version(&mut self, path: PathBuf) -> i32 {
        for (known_path, version) in &mut self.versions {
            if *known_path == path {
                *version += 1;
                return *version;
            }
        }
        self.versions.push((path, 1));
        1
    }

    fn send_request(&mut self, method: &str, params: Value) -> io::Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_message(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        Ok(id)
    }

    fn send_notification(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.send_message(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send_message(&mut self, value: Value) -> io::Result<()> {
        let payload = serde_json::to_vec(&value)?;
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("rust-analyzer writer lock poisoned"))?;
        write!(writer, "Content-Length: {}\r\n\r\n", payload.len())?;
        writer.write_all(&payload)?;
        writer.flush()
    }

    fn wait_for_response(&mut self, id: u64, timeout: Duration) -> io::Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "timed out waiting for rust-analyzer",
                ));
            };

            let message = self.incoming.recv_timeout(remaining).map_err(|_| {
                io::Error::new(io::ErrorKind::TimedOut, "rust-analyzer did not reply")
            })?;

            let message_id = message.get("id").and_then(Value::as_u64);
            if message_id == Some(id) {
                if let Some(error) = message.get("error") {
                    return Err(io::Error::other(error.to_string()));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }
}

impl Drop for LanguageClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn initialize_params(root: &Path) -> Value {
    let root_uri = path_to_uri(root);
    json!({
        "processId": null,
        "rootUri": root_uri,
        "workspaceFolders": [
            {
                "uri": path_to_uri(root),
                "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("project"),
            }
        ],
        "capabilities": {
            "textDocument": {
                "completion": {
                    "completionItem": {
                        "snippetSupport": true,
                        "documentationFormat": ["markdown", "plaintext"]
                    }
                }
            }
        },
        "initializationOptions": {
            "cargo": {
                "allFeatures": true
            }
        }
    })
}

fn parse_completion_response(value: Value) -> Vec<CompletionCandidate> {
    let items = if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else {
        value
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };

    let mut result = Vec::new();
    for item in items {
        let Some(label) = item.get("label").and_then(Value::as_str) else {
            continue;
        };
        let insert_text = item
            .get("textEdit")
            .and_then(|edit| edit.get("newText"))
            .and_then(Value::as_str)
            .or_else(|| item.get("insertText").and_then(Value::as_str))
            .unwrap_or(label);
        let insert_text = strip_snippet_placeholders(insert_text);
        let detail = item
            .get("detail")
            .and_then(Value::as_str)
            .map(ToString::to_string);
        result.push(CompletionCandidate {
            label: label.to_string(),
            insert_text,
            detail,
            kind: CompletionKind::LanguageServer,
        });
    }
    result
}

fn strip_snippet_placeholders(text: &str) -> String {
    let mut result = String::new();
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '$' {
            if index + 1 < chars.len() && chars[index + 1].is_ascii_digit() {
                index += 2;
                continue;
            }
            if index + 1 < chars.len() && chars[index + 1] == '{' {
                index += 2;
                let mut placeholder = String::new();
                while index < chars.len() && chars[index] != '}' {
                    placeholder.push(chars[index]);
                    index += 1;
                }
                if let Some((_, default)) = placeholder.split_once(':') {
                    result.push_str(default);
                }
                index += usize::from(index < chars.len());
                continue;
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

fn read_lsp_message(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut content_length = None;
    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 {
            return Ok(None);
        }

        let trimmed = header.trim();
        if trimmed.is_empty() {
            break;
        }

        if let Some(length) = trimmed.strip_prefix("Content-Length:") {
            content_length = length.trim().parse::<usize>().ok();
        }
    }

    let Some(content_length) = content_length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing LSP content length",
        ));
    };

    let mut payload = vec![0; content_length];
    reader.read_exact(&mut payload)?;
    Ok(Some(String::from_utf8_lossy(&payload).to_string()))
}

fn utf16_col(line: &str, col: usize) -> usize {
    line.chars().take(col).map(char::len_utf16).sum::<usize>()
}

fn path_to_uri(path: &Path) -> String {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(*byte as char)
            }
            _ => uri.push_str(&format!("%{:02X}", byte)),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::{CompletionKind, fallback_items, strip_snippet_placeholders};

    #[test]
    fn strips_lsp_snippet_placeholders() {
        assert_eq!(
            strip_snippet_placeholders("println!(\"${1:value}\");$0"),
            "println!(\"value\");"
        );
        assert_eq!(strip_snippet_placeholders("dbg!($0)"), "dbg!()");
    }

    #[test]
    fn fallback_snippets_do_not_insert_placeholders() {
        let items = fallback_items("", "", "if", &[], false);
        let snippet = items
            .iter()
            .find(|item| item.kind == CompletionKind::Snippet && item.label == "if let")
            .expect("if let snippet");

        assert!(!snippet.insert_text.contains('$'));
    }
}
