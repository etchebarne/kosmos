impl FileTree {
    fn deselect_path(&mut self, path: &Path) {
        self.selected.remove(path);
        if self.selection_anchor.as_deref() == Some(path) {
            self.selection_anchor = None;
        }
    }

    pub fn start_rename(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let original = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        // Renames operate on a single target — narrow the selection to it so
        // the active row matches what's being renamed.
        self.selected.clear();
        self.selected.insert(path.clone());
        self.selection_anchor = Some(path.clone());
        self.rename = Some(RenameTarget {
            path,
            original_name: original.into(),
        });
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        if self.rename.take().is_some() {
            cx.notify();
        }
    }

    pub fn apply_rename(&mut self, new_name: String, cx: &mut Context<Self>) {
        let Some(target) = self.rename.take() else {
            return;
        };
        let trimmed = new_name.trim();
        if trimmed.is_empty() || trimmed == target.original_name.as_ref() {
            cx.notify();
            return;
        }
        let Some(parent) = target.path.parent() else {
            return;
        };
        let new_path = parent.join(trimmed);
        match ops::rename(&target.path, &new_path) {
            Ok(_) => {
                if self.children.contains_key(parent) {
                    self.reload_dir(parent);
                }
                if self.selected.remove(&target.path) {
                    self.selected.insert(new_path.clone());
                }
                if self.selection_anchor.as_deref() == Some(target.path.as_path()) {
                    self.selection_anchor = Some(new_path);
                }
                cx.notify();
            }
            Err(err) => self.set_error(format!("Rename failed: {err}"), cx),
        }
    }

    pub fn start_new_entry(
        &mut self,
        anchor: Option<&Path>,
        kind: NodeKind,
        cx: &mut Context<Self>,
    ) {
        let parent = match anchor {
            Some(path) if path.is_dir() => path.to_path_buf(),
            Some(path) => path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.root.clone().unwrap_or_default()),
            None => self.root.clone().unwrap_or_default(),
        };
        self.expand(&parent, cx);
        self.new_entry = Some(NewEntryDraft { parent, kind });
        cx.notify();
    }

    pub fn cancel_new_entry(&mut self, cx: &mut Context<Self>) {
        if self.new_entry.take().is_some() {
            cx.notify();
        }
    }

    pub fn apply_new_entry(&mut self, name: String, cx: &mut Context<Self>) {
        let Some(draft) = self.new_entry.take() else {
            return;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() {
            cx.notify();
            return;
        }
        let path = draft.parent.join(trimmed);
        let result = match draft.kind {
            NodeKind::File => ops::create_file(&path),
            NodeKind::Directory => ops::create_dir(&path),
        };
        match result {
            Ok(_) => {
                self.reload_dir(&draft.parent);
                self.selected.clear();
                self.selected.insert(path.clone());
                self.selection_anchor = Some(path);
                cx.notify();
            }
            Err(err) => self.set_error(format!("Create failed: {err}"), cx),
        }
    }

    fn set_error(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }
}
