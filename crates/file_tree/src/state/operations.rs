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
        let Some(target_dir) = Self::operation_target_dir(dest_dir) else {
            return;
        };
        let mut errors: Vec<String> = Vec::new();
        let mut src_parents: HashSet<PathBuf> = HashSet::new();
        for src in &srcs {
            if let Err(err) = ops::paste(src, &target_dir, op) {
                errors.push(format!("{}: {err}", src.display()));
            }
            Self::collect_parent(&mut src_parents, src);
        }
        if op == ClipboardOp::Cut {
            self.clipboard = None;
        }
        self.reload_dir(&target_dir);
        self.reload_known_parents(src_parents);
        self.finish_fs_operation("Paste", errors, cx);
    }

    pub fn move_into(&mut self, srcs: Vec<PathBuf>, dest_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(target_dir) = Self::operation_target_dir(dest_dir) else {
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
            Self::collect_parent(&mut parents, src);
        }
        self.reload_dir(&target_dir);
        self.reload_known_parents(parents);
        self.finish_fs_operation("Move", errors, cx);
    }

    pub fn trash(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.remove_paths(paths, "Trash", ops::trash, cx);
    }

    pub fn delete(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.remove_paths(paths, "Delete", ops::delete, cx);
    }

    fn remove_paths(
        &mut self,
        paths: Vec<PathBuf>,
        label: &'static str,
        operation: impl Fn(&Path) -> std::io::Result<()>,
        cx: &mut Context<Self>,
    ) {
        let mut errors: Vec<String> = Vec::new();
        let mut parents: HashSet<PathBuf> = HashSet::new();
        for path in &paths {
            match operation(path) {
                Ok(_) => {
                    Self::collect_parent(&mut parents, path);
                    self.deselect_path(path);
                }
                Err(err) => errors.push(format!("{}: {err}", path.display())),
            }
        }
        self.reload_known_parents(parents);
        self.finish_fs_operation(label, errors, cx);
    }

    fn operation_target_dir(dest_dir: PathBuf) -> Option<PathBuf> {
        if dest_dir.is_dir() {
            Some(dest_dir)
        } else {
            dest_dir.parent().map(Path::to_path_buf)
        }
    }

    fn collect_parent(parents: &mut HashSet<PathBuf>, path: &Path) {
        if let Some(parent) = path.parent() {
            parents.insert(parent.to_path_buf());
        }
    }

    fn reload_known_parents(&mut self, parents: HashSet<PathBuf>) {
        for parent in parents {
            if self.children.contains_key(&parent) {
                self.reload_dir(&parent);
            }
        }
    }

    fn finish_fs_operation(
        &mut self,
        label: &'static str,
        errors: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if !errors.is_empty() {
            self.set_error(format!("{label} failed: {}", errors.join("; ")), cx);
        } else {
            cx.notify();
        }
    }

}
