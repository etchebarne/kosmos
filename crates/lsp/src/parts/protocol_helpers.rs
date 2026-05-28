impl ClientInner {
    fn send(&self, value: Value) -> Result<(), Error> {
        let body = serde_json::to_vec(&value)?;
        let mut stdin = self.stdin.lock().unwrap();
        write!(stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        stdin.write_all(&body)?;
        stdin.flush()?;
        Ok(())
    }

    fn respond(&self, id: Value, result: Value) {
        let _ = self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }));
    }

    fn set_sync_kind(&self, sync_kind: lsp_types::TextDocumentSyncKind) {
        *self.sync_kind.lock().unwrap() = sync_kind;
    }

    fn sync_kind(&self) -> lsp_types::TextDocumentSyncKind {
        *self.sync_kind.lock().unwrap()
    }

    fn publish_diagnostics(&self, params: lsp_types::PublishDiagnosticsParams) {
        let uri = params.uri.to_string();
        let mut diagnostics = self.diagnostics.lock().unwrap();
        let epoch = diagnostics.get(&uri).map_or(1, |state| state.epoch + 1);
        diagnostics.insert(
            uri,
            DiagnosticState {
                version: params.version,
                diagnostics: params.diagnostics,
                epoch,
            },
        );
        drop(diagnostics);
        self.diagnostics_changed.notify_all();
    }

    fn diagnostics_epoch(&self, uri: &str) -> u64 {
        self.diagnostics
            .lock()
            .unwrap()
            .get(uri)
            .map_or(0, |state| state.epoch)
    }

    fn wait_for_diagnostics(
        &self,
        uri: &str,
        version: i32,
        previous_epoch: u64,
        timeout: Duration,
    ) {
        let deadline = Instant::now() + timeout;
        let mut diagnostics = self.diagnostics.lock().unwrap();
        while !diagnostics_are_fresh(diagnostics.get(uri), version, previous_epoch) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }

            let (next_diagnostics, result) = self
                .diagnostics_changed
                .wait_timeout(diagnostics, remaining)
                .unwrap();
            diagnostics = next_diagnostics;
            if result.timed_out() {
                break;
            }
        }
    }

    fn diagnostics_for(
        &self,
        uri: &str,
        version: i32,
        minimum_epoch: Option<u64>,
    ) -> DiagnosticsResponse {
        let diagnostics = self.diagnostics.lock().unwrap();
        let Some(state) = diagnostics.get(uri) else {
            return DiagnosticsResponse {
                diagnostics: Vec::new(),
                epoch: 0,
                fresh: false,
            };
        };

        let fresh = diagnostics_are_fresh(Some(state), version, minimum_epoch.unwrap_or(0));
        DiagnosticsResponse {
            diagnostics: if fresh {
                state.diagnostics.clone()
            } else {
                Vec::new()
            },
            epoch: state.epoch,
            fresh,
        }
    }
}

fn spawn_reader(stdout: ChildStdout, client: Weak<ClientInner>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let message = match read_message(&mut reader) {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(err) => {
                    eprintln!("kosmos: LSP read error: {err}");
                    break;
                }
            };

            let value: Value = match serde_json::from_str(&message) {
                Ok(value) => value,
                Err(err) => {
                    eprintln!("kosmos: LSP JSON parse error: {err}");
                    continue;
                }
            };

            let Some(client) = client.upgrade() else {
                break;
            };

            if let Some(method) = value.get("method").and_then(Value::as_str) {
                if let Some(id) = value.get("id").cloned() {
                    let result = match method {
                        "workspace/configuration" => {
                            workspace_configuration_result(value.get("params").unwrap_or(&Value::Null))
                        }
                        _ => Value::Null,
                    };
                    client.respond(id, result);
                    continue;
                }

                if method == "textDocument/publishDiagnostics" {
                    if let Some(params) = value.get("params").cloned() {
                        match serde_json::from_value::<lsp_types::PublishDiagnosticsParams>(params)
                        {
                            Ok(params) => client.publish_diagnostics(params),
                            Err(err) => eprintln!("kosmos: LSP diagnostics parse error: {err}"),
                        }
                    } else {
                        eprintln!("kosmos: LSP diagnostics notification missing params");
                    }
                    continue;
                }
            }

            let Some(id) = value.get("id").and_then(Value::as_u64) else {
                continue;
            };
            if let Some(tx) = client.pending.lock().unwrap().remove(&id) {
                let _ = tx.send(value);
            }
        }
    });
}

fn workspace_configuration_result(params: &Value) -> Value {
    let Some(items) = params.get("items").and_then(Value::as_array) else {
        return json!([]);
    };

    Value::Array(items.iter().map(|_| Value::Null).collect())
}

fn text_document_sync_kind(result: &Value) -> lsp_types::TextDocumentSyncKind {
    let Some(sync) = result.get("capabilities").and_then(|capabilities| {
        capabilities.get("textDocumentSync")
    }) else {
        return lsp_types::TextDocumentSyncKind::FULL;
    };

    if let Some(kind) = sync.as_i64() {
        return match kind {
            2 => lsp_types::TextDocumentSyncKind::INCREMENTAL,
            1 => lsp_types::TextDocumentSyncKind::FULL,
            _ => lsp_types::TextDocumentSyncKind::NONE,
        };
    }

    sync.get("change")
        .and_then(Value::as_i64)
        .map(|kind| match kind {
            2 => lsp_types::TextDocumentSyncKind::INCREMENTAL,
            1 => lsp_types::TextDocumentSyncKind::FULL,
            _ => lsp_types::TextDocumentSyncKind::NONE,
        })
        .unwrap_or(lsp_types::TextDocumentSyncKind::FULL)
}

fn text_document_content_change(
    sync_kind: lsp_types::TextDocumentSyncKind,
    old: &str,
    new: &str,
) -> lsp_types::TextDocumentContentChangeEvent {
    if sync_kind != lsp_types::TextDocumentSyncKind::INCREMENTAL {
        return lsp_types::TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: new.to_string(),
        };
    }

    let prefix = common_prefix_len(old, new);
    let (old_end, new_end) = common_suffix_offsets(old, new, prefix);
    let range = lsp_types::Range::new(
        lsp_position_for_offset(old, prefix),
        lsp_position_for_offset(old, old_end),
    );
    let range_length = old[prefix..old_end].encode_utf16().count() as u32;

    lsp_types::TextDocumentContentChangeEvent {
        range: Some(range),
        range_length: Some(range_length),
        text: new[prefix..new_end].to_string(),
    }
}

fn common_prefix_len(a: &str, b: &str) -> usize {
    let mut len = 0;
    let mut a_chars = a.char_indices();
    let mut b_chars = b.char_indices();
    loop {
        match (a_chars.next(), b_chars.next()) {
            (Some((a_index, a_ch)), Some((_, b_ch))) if a_ch == b_ch => {
                len = a_index + a_ch.len_utf8();
            }
            _ => return len,
        }
    }
}

fn common_suffix_offsets(a: &str, b: &str, prefix: usize) -> (usize, usize) {
    let mut a_end = a.len();
    let mut b_end = b.len();
    while a_end > prefix && b_end > prefix {
        let Some(a_ch) = a[..a_end].chars().next_back() else {
            break;
        };
        let Some(b_ch) = b[..b_end].chars().next_back() else {
            break;
        };
        if a_ch != b_ch {
            break;
        }

        a_end -= a_ch.len_utf8();
        b_end -= b_ch.len_utf8();
    }
    (a_end, b_end)
}

fn lsp_position_for_offset(text: &str, offset: usize) -> lsp_types::Position {
    let mut line = 0;
    let mut character = 0;
    for ch in text[..offset].chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    lsp_types::Position::new(line, character)
}

fn diagnostics_are_fresh(
    state: Option<&DiagnosticState>,
    version: i32,
    minimum_epoch: u64,
) -> bool {
    let Some(state) = state else {
        return false;
    };

    if state.epoch <= minimum_epoch {
        return false;
    }

    match state.version {
        Some(diagnostic_version) => diagnostic_version >= version,
        None => true,
    }
}

fn read_message(reader: &mut BufReader<ChildStdout>) -> io::Result<Option<String>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            content_length = value.trim().parse::<usize>().ok();
        }
    }

    let Some(content_length) = content_length else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length header",
        ));
    };

    let mut body = vec![0; content_length];
    reader.read_exact(&mut body)?;
    Ok(Some(String::from_utf8_lossy(&body).into_owned()))
}

fn file_uri(path: &Path) -> Result<String, Error> {
    Url::from_file_path(path)
        .map(|url| url.to_string())
        .map_err(|_| Error::Url(path.to_path_buf()))
}

fn response_error_message(error: &Value) -> String {
    error
        .get("message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| error.to_string())
}

fn hover_text(result: &Value) -> Option<String> {
    if result.is_null() {
        return None;
    }

    let contents = result.get("contents")?;
    let mut parts = Vec::new();
    collect_hover_parts(contents, &mut parts);
    let text = normalize_hover_text(parts.join("\n\n"));
    (!text.is_empty()).then_some(text)
}

fn completion_response(result: Value) -> Result<Option<lsp_types::CompletionResponse>, Error> {
    if result.is_null() {
        return Ok(None);
    }

    serde_json::from_value(result).map(Some).map_err(Error::from)
}

fn collect_hover_parts(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) => parts.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_hover_parts(item, parts);
            }
        }
        Value::Object(map) => {
            if let (Some(language), Some(value)) = (
                map.get("language").and_then(Value::as_str),
                map.get("value").and_then(Value::as_str),
            ) {
                parts.push(format!("```{language}\n{value}\n```"));
            } else if let Some(value) = map.get("value").and_then(Value::as_str) {
                parts.push(value.to_string());
            }
        }
        _ => {}
    }
}

fn normalize_hover_text(text: String) -> String {
    let mut text = text.replace("\r\n", "\n").replace('\r', "\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_string()
}
