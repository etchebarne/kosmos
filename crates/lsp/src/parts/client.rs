impl Client {
    fn start(entry: &'static RegistryEntry, root: PathBuf) -> Result<Self, Error> {
        let bin_path = installer::bin_path(entry).ok_or_else(|| Error::Start {
            server: entry.id,
            message: "no binary for the current platform".to_string(),
        })?;

        let mut command = Command::new(&bin_path);
        command
            .args(entry.launch.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(&root);
        for &(key, value) in entry.launch.env {
            command.env(key, value);
        }

        let mut child = command.spawn().map_err(|err| Error::Start {
            server: entry.id,
            message: format!("{}: {err}", bin_path.display()),
        })?;
        let stdin = child.stdin.take().ok_or_else(|| Error::Start {
            server: entry.id,
            message: "missing stdin pipe".to_string(),
        })?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Start {
            server: entry.id,
            message: "missing stdout pipe".to_string(),
        })?;

        let inner = Arc::new(ClientInner {
            server_id: entry.id,
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            documents: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        });

        spawn_reader(stdout, Arc::downgrade(&inner));

        let client = Self { inner };
        client.initialize(entry, &root)?;
        Ok(client)
    }

    fn initialize(&self, entry: &'static RegistryEntry, root: &Path) -> Result<(), Error> {
        let root_uri = file_uri(root)?;
        let root_path = root.to_string_lossy().to_string();
        self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {
                    "name": "Kosmos",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "rootUri": root_uri.as_str(),
                "rootPath": root_path.as_str(),
                "workspaceFolders": [{
                    "uri": root_uri.as_str(),
                    "name": root.file_name().and_then(|name| name.to_str()).unwrap_or("workspace"),
                }],
                "capabilities": {
                    "textDocument": {
                        "hover": {
                            "dynamicRegistration": false,
                            "contentFormat": ["markdown", "plaintext"],
                        },
                        "synchronization": {
                            "dynamicRegistration": false,
                            "willSave": false,
                            "willSaveWaitUntil": false,
                            "didSave": false,
                        },
                    },
                    "workspace": {
                        "configuration": false,
                        "workspaceFolders": true,
                    },
                },
            }),
            INITIALIZE_TIMEOUT,
        )?;
        self.notify("initialized", json!({}))?;

        eprintln!("kosmos: started LSP {} for {}", entry.id, root.display());
        Ok(())
    }

    fn hover(
        &self,
        entry: &'static RegistryEntry,
        request: &HoverRequest,
    ) -> Result<Option<String>, Error> {
        let uri = file_uri(&request.path)?;
        self.ensure_document(entry, request, &uri)?;

        let result = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": {
                    "line": request.position.line,
                    "character": request.position.character,
                },
            }),
            HOVER_TIMEOUT,
        )?;

        Ok(hover_text(&result))
    }

    fn ensure_document(
        &self,
        entry: &'static RegistryEntry,
        request: &HoverRequest,
        uri: &str,
    ) -> Result<(), Error> {
        enum SyncAction {
            Open { version: i32 },
            Change { version: i32 },
            None,
        }

        let action = {
            let mut documents = self.inner.documents.lock().unwrap();
            match documents.get_mut(uri) {
                Some(state) if state.content == request.content => SyncAction::None,
                Some(state) => {
                    state.version += 1;
                    state.content = request.content.clone();
                    SyncAction::Change {
                        version: state.version,
                    }
                }
                None => {
                    documents.insert(
                        uri.to_string(),
                        DocumentState {
                            version: 1,
                            content: request.content.clone(),
                        },
                    );
                    SyncAction::Open { version: 1 }
                }
            }
        };

        match action {
            SyncAction::Open { version } => self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": request.language_id.as_str(),
                        "version": version,
                        "text": request.content.as_str(),
                    },
                }),
            ),
            SyncAction::Change { version } => self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "version": version,
                    },
                    "contentChanges": [{ "text": request.content.as_str() }],
                }),
            ),
            SyncAction::None => Ok(()),
        }
        .map_err(|err| Error::Response {
            server: entry.id,
            message: err.to_string(),
        })
    }

    fn request(
        &self,
        method: &'static str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, Error> {
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::sync_channel(1);
        self.inner.pending.lock().unwrap().insert(id, tx);

        let send_result = self.send(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        if let Err(err) = send_result {
            self.inner.pending.lock().unwrap().remove(&id);
            return Err(err);
        }

        let response = match rx.recv_timeout(timeout) {
            Ok(response) => response,
            Err(_) => {
                self.inner.pending.lock().unwrap().remove(&id);
                return Err(Error::Timeout {
                    server: self.inner.server_id,
                    method,
                });
            }
        };

        if let Some(error) = response.get("error") {
            return Err(Error::Response {
                server: self.inner.server_id,
                message: response_error_message(error),
            });
        }

        Ok(response.get("result").cloned().unwrap_or(Value::Null))
    }

    fn notify(&self, method: &'static str, params: Value) -> Result<(), Error> {
        self.send(json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    fn send(&self, value: Value) -> Result<(), Error> {
        self.inner.send(value)
    }
}

