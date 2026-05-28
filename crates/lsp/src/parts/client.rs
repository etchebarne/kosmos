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
            sync_kind: Mutex::new(lsp_types::TextDocumentSyncKind::FULL),
            diagnostics: Mutex::new(HashMap::new()),
            diagnostics_changed: Condvar::new(),
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
        let result = self.request(
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
                        "completion": {
                            "dynamicRegistration": false,
                            "contextSupport": true,
                            "completionItem": {
                                "documentationFormat": ["markdown", "plaintext"],
                                "deprecatedSupport": true,
                                "insertReplaceSupport": true,
                                "labelDetailsSupport": true,
                                "preselectSupport": true,
                                "snippetSupport": false,
                            },
                        },
                        "synchronization": {
                            "dynamicRegistration": false,
                            "willSave": false,
                            "willSaveWaitUntil": false,
                            "didSave": true,
                        },
                        "publishDiagnostics": {
                            "dynamicRegistration": false,
                            "relatedInformation": true,
                            "versionSupport": true,
                            "tagSupport": { "valueSet": [1, 2] },
                            "codeDescriptionSupport": true,
                            "dataSupport": true,
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
        self.inner
            .set_sync_kind(text_document_sync_kind(&result));
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
        self.ensure_document(entry, &request.language_id, &request.content, &uri)?;

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

    fn completion(
        &self,
        entry: &'static RegistryEntry,
        request: &CompletionRequest,
    ) -> Result<Option<lsp_types::CompletionResponse>, Error> {
        let uri = file_uri(&request.path)?;
        self.ensure_document(entry, &request.language_id, &request.content, &uri)?;

        let result = self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": {
                    "line": request.position.line,
                    "character": request.position.character,
                },
            }),
            COMPLETION_TIMEOUT,
        )?;

        completion_response(result)
    }

    fn diagnostics(
        &self,
        entry: &'static RegistryEntry,
        request: &DiagnosticsRequest,
    ) -> Result<DiagnosticsResponse, Error> {
        let uri = file_uri(&request.path)?;
        let previous_epoch = self.inner.diagnostics_epoch(&uri);
        let sync = self.ensure_document(entry, &request.language_id, &request.content, &uri)?;
        let minimum_epoch = if sync.changed || previous_epoch == 0 {
            Some(previous_epoch)
        } else {
            request.previous_epoch
        };

        if let Some(minimum_epoch) = minimum_epoch {
            self.inner
                .wait_for_diagnostics(&uri, sync.version, minimum_epoch, DIAGNOSTIC_TIMEOUT);
        }

        Ok(self.inner.diagnostics_for(&uri, sync.version, minimum_epoch))
    }

    fn did_save(
        &self,
        entry: &'static RegistryEntry,
        request: &SaveRequest,
    ) -> Result<(), Error> {
        let uri = file_uri(&request.path)?;
        self.ensure_document(entry, &request.language_id, &request.content, &uri)?;
        self.notify(
            "textDocument/didSave",
            json!({
                "textDocument": { "uri": uri },
                "text": request.content,
            }),
        )
        .map_err(|err| Error::Response {
            server: entry.id,
            message: err.to_string(),
        })
    }

    fn ensure_document(
        &self,
        entry: &'static RegistryEntry,
        language_id: &str,
        content: &str,
        uri: &str,
    ) -> Result<DocumentSync, Error> {
        enum SyncAction {
            Open { version: i32 },
            Change {
                version: i32,
                change: lsp_types::TextDocumentContentChangeEvent,
            },
            None { version: i32 },
        }

        let sync_kind = self.inner.sync_kind();
        let action = {
            let mut documents = self.inner.documents.lock().unwrap();
            match documents.get_mut(uri) {
                Some(state) if state.content == content => SyncAction::None {
                    version: state.version,
                },
                Some(state) => {
                    state.version += 1;
                    let change = text_document_content_change(sync_kind, &state.content, content);
                    state.content = content.to_string();
                    SyncAction::Change {
                        version: state.version,
                        change,
                    }
                }
                None => {
                    documents.insert(
                        uri.to_string(),
                        DocumentState {
                            version: 1,
                            content: content.to_string(),
                        },
                    );
                    SyncAction::Open { version: 1 }
                }
            }
        };

        let result = match action {
            SyncAction::Open { version } => self.notify(
                "textDocument/didOpen",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "languageId": language_id,
                        "version": version,
                        "text": content,
                    },
                }),
            ),
            SyncAction::Change { version, ref change } => self.notify(
                "textDocument/didChange",
                json!({
                    "textDocument": {
                        "uri": uri,
                        "version": version,
                    },
                    "contentChanges": [change],
                }),
            ),
            SyncAction::None { .. } => Ok(()),
        }
        .map_err(|err| Error::Response {
            server: entry.id,
            message: err.to_string(),
        });

        result?;

        Ok(match action {
            SyncAction::Open { version } | SyncAction::Change { version, .. } => DocumentSync {
                version,
                changed: true,
            },
            SyncAction::None { version } => DocumentSync {
                version,
                changed: false,
            },
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
