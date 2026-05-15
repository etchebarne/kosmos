impl FileTree {
    pub fn cut(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.clipboard = Some((ClipboardOp::Cut, paths));
        cx.notify();
    }

    pub fn copy(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        self.clipboard = Some((ClipboardOp::Copy, paths));
        cx.notify();
    }

    pub fn paste_into(&mut self, dest_dir: PathBuf, cx: &mut Context<Self>) {
        let Some((op, srcs)) = self.clipboard.clone() else {
            return;
        };
        let target_dir = if dest_dir.is_dir() {
            dest_dir
        } else if let Some(parent) = dest_dir.parent() {
            parent.to_path_buf()
        } else {
            return;
        };
        let mut errors: Vec<String> = Vec::new();
        let mut src_parents: HashSet<PathBuf> = HashSet::new();
        for src in &srcs {
            if let Err(err) = ops::paste(src, &target_dir, op) {
                errors.push(format!("{}: {err}", src.display()));
            }
            if let Some(parent) = src.parent() {
                src_parents.insert(parent.to_path_buf());
            }
        }
        if op == ClipboardOp::Cut {
            self.clipboard = None;
        }
        self.reload_dir(&target_dir);
        for parent in src_parents {
            if self.children.contains_key(&parent) {
                self.reload_dir(&parent);
            }
        }
        if !errors.is_empty() {
            self.set_error(format!("Paste failed: {}", errors.join("; ")), cx);
        } else {
            cx.notify();
        }
    }

    pub fn move_into(&mut self, srcs: Vec<PathBuf>, dest_dir: PathBuf, cx: &mut Context<Self>) {
        let target_dir = if dest_dir.is_dir() {
            dest_dir
        } else if let Some(parent) = dest_dir.parent() {
            parent.to_path_buf()
        } else {
            return;
        };
        let mut errors: Vec<String> = Vec::new();
        let mut parents: HashSet<PathBuf> = HashSet::new();
        for src in &srcs {
            // Skip no-ops and pathological cases instead of erroring.
            if *src == target_dir
                || target_dir.ancestors().any(|a| a == src.as_path())
                || src.parent() == Some(target_dir.as_path())
            {
                continue;
            }
            if let Err(err) = ops::move_into(src, &target_dir) {
                errors.push(format!("{}: {err}", src.display()));
                continue;
            }
            if let Some(parent) = src.parent() {
                parents.insert(parent.to_path_buf());
            }
        }
        self.reload_dir(&target_dir);
        for parent in parents {
            if self.children.contains_key(&parent) {
                self.reload_dir(&parent);
            }
        }
        if !errors.is_empty() {
            self.set_error(format!("Move failed: {}", errors.join("; ")), cx);
        } else {
            cx.notify();
        }
    }

    pub fn trash(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut errors: Vec<String> = Vec::new();
        let mut parents: HashSet<PathBuf> = HashSet::new();
        for path in &paths {
            match ops::trash(path) {
                Ok(_) => {
                    if let Some(parent) = path.parent() {
                        parents.insert(parent.to_path_buf());
                    }
                    self.deselect_path(path);
                }
                Err(err) => errors.push(format!("{}: {err}", path.display())),
            }
        }
        for parent in parents {
            if self.children.contains_key(&parent) {
                self.reload_dir(&parent);
            }
        }
        if !errors.is_empty() {
            self.set_error(format!("Trash failed: {}", errors.join("; ")), cx);
        } else {
            cx.notify();
        }
    }

    pub fn delete(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut errors: Vec<String> = Vec::new();
        let mut parents: HashSet<PathBuf> = HashSet::new();
        for path in &paths {
            match ops::delete(path) {
                Ok(_) => {
                    if let Some(parent) = path.parent() {
                        parents.insert(parent.to_path_buf());
                    }
                    self.deselect_path(path);
                }
                Err(err) => errors.push(format!("{}: {err}", path.display())),
            }
        }
        for parent in parents {
            if self.children.contains_key(&parent) {
                self.reload_dir(&parent);
            }
        }
        if !errors.is_empty() {
            self.set_error(format!("Delete failed: {}", errors.join("; ")), cx);
        } else {
            cx.notify();
        }
    }

}
