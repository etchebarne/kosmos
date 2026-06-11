impl FileTree {
    /// Re-read a directory's contents from disk into our cache.
    pub fn reload_dir(&mut self, path: &Path) {
        match ops::read_dir(path) {
            Ok(nodes) => {
                self.children.insert(path.to_path_buf(), nodes);
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // The directory was removed (by us or externally). Drop it
                // from the cache so we don't keep trying to read it.
                self.children.remove(path);
                self.expanded.remove(path);
            }
            Err(err) => {
                self.error = Some(format!("Failed to read {}: {err}", path.display()).into());
                self.children.insert(path.to_path_buf(), Vec::new());
            }
        }
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let mut expanded = HashSet::new();
        if let Some(root) = self.root.clone() {
            expanded.insert(root);
        }
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.emit_expanded_changed(cx);
        cx.notify();
    }

    pub fn toggle_expand(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
        } else {
            self.expanded.insert(path.to_path_buf());
            if !self.children.contains_key(path) {
                self.reload_dir(path);
            }
        }
        self.emit_expanded_changed(cx);
        cx.notify();
    }

    pub fn expand(&mut self, path: &Path, cx: &mut Context<Self>) {
        if !self.expanded.contains(path) {
            self.expanded.insert(path.to_path_buf());
            if !self.children.contains_key(path) {
                self.reload_dir(path);
            }
            self.emit_expanded_changed(cx);
            cx.notify();
        }
    }

    pub fn select(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected.clear();
        self.selected.insert(path.clone());
        self.selection_anchor = Some(path);
        cx.notify();
    }

    /// Extend the selection to cover the visible range from the anchor to
    /// `target`. If there's no anchor yet, behaves like a plain select.
    pub fn extend_selection_to(&mut self, target: PathBuf, cx: &mut Context<Self>) {
        let anchor = match self.selection_anchor.clone() {
            Some(a) => a,
            None => {
                self.select(target, cx);
                return;
            }
        };
        let visible = self.visible_paths();
        let i_anchor = visible.iter().position(|p| p == &anchor);
        let i_target = visible.iter().position(|p| p == &target);
        match (i_anchor, i_target) {
            (Some(a), Some(b)) => {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                self.selected.clear();
                for p in &visible[lo..=hi] {
                    self.selected.insert(p.clone());
                }
                cx.notify();
            }
            _ => self.select(target, cx),
        }
    }

    /// Flatten the visible tree into a top-to-bottom path list, matching the
    /// rendering order in the UI (root → expanded children, dirs before files).
    pub fn visible_paths(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            out.push(root.clone());
            if self.expanded.contains(root) {
                self.append_visible_children(root, &mut out);
            }
        }
        out
    }

    fn append_visible_children(&self, dir: &Path, out: &mut Vec<PathBuf>) {
        let Some(children) = self.children.get(dir) else {
            return;
        };
        for node in children
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Directory))
        {
            out.push(node.path.clone());
            if self.expanded.contains(&node.path) {
                self.append_visible_children(&node.path, out);
            }
        }
        for node in children.iter().filter(|n| matches!(n.kind, NodeKind::File)) {
            out.push(node.path.clone());
        }
    }

}
