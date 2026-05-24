impl Buffer {
    pub(crate) fn new(id: BufferId, path: PathBuf, cx: &mut Context<Self>) -> Self {
        let content = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        let language = language::from_path(&path);
        Self {
            id,
            path,
            language,
            saved_content: content.clone(),
            content,
            dirty: false,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn id(&self) -> BufferId {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn language(&self) -> Option<&LanguageId> {
        self.language.as_ref()
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn replace_range(
        &mut self,
        range: Range<usize>,
        new_text: &str,
        cx: &mut Context<Self>,
    ) -> Range<usize> {
        let range = clamp_range_to_char_boundaries(&self.content, range);
        let new_text = normalize_newlines(new_text);
        let new_end = range.start + new_text.len();
        if self.content[range.clone()] == new_text {
            return range.start..new_end;
        }

        self.content.replace_range(range.clone(), &new_text);
        self.dirty = self.content != self.saved_content;
        cx.notify();
        range.start..new_end
    }

    pub fn save(&mut self, cx: &mut Context<Self>) -> std::io::Result<()> {
        std::fs::write(&self.path, &self.content)?;
        self.saved_content = self.content.clone();
        if self.dirty {
            self.dirty = false;
            cx.notify();
        }
        Ok(())
    }

    fn reload_from_disk(&mut self, cx: &mut Context<Self>) {
        if self.dirty {
            return;
        }
        let Ok(content) = std::fs::read_to_string(&self.path) else {
            return;
        };
        let content = content.replace("\r\n", "\n");
        if content == self.content {
            return;
        }

        self.content = content;
        self.saved_content = self.content.clone();
        self.dirty = false;
        cx.notify();
    }
}

impl Focusable for Buffer {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
