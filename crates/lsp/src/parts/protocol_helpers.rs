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

            if let Some(method) = value.get("method").and_then(Value::as_str)
                && let Some(id) = value.get("id").cloned()
            {
                let result = match method {
                    "workspace/configuration" => json!([]),
                    _ => Value::Null,
                };
                client.respond(id, result);
                continue;
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

