impl FileTree {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let _ = cx;
        Self {
            root: None,
            children: HashMap::new(),
            expanded: HashSet::new(),
            selected: HashSet::new(),
            selection_anchor: None,
            clipboard: None,
            error: None,
            rename: None,
            new_entry: None,
            context_menu: None,
            watcher: None,
            watcher_rx: None,
            watcher_task: None,
        }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// Returns the anchor of the current selection — the last item clicked
    /// without a shift modifier. Useful for callers that need a single
    /// reference point (e.g. "new file" anchored on the active item).
    pub fn selected(&self) -> Option<&Path> {
        self.selection_anchor.as_deref()
    }

    pub fn is_selected(&self, path: &Path) -> bool {
        self.selected.contains(path)
    }

    pub fn selected_paths(&self) -> &HashSet<PathBuf> {
        &self.selected
    }

    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn clipboard(&self) -> Option<(ClipboardOp, &[PathBuf])> {
        self.clipboard
            .as_ref()
            .map(|(op, paths)| (*op, paths.as_slice()))
    }

    pub fn rename_target(&self) -> Option<&RenameTarget> {
        self.rename.as_ref()
    }

    pub fn new_entry_draft(&self) -> Option<&NewEntryDraft> {
        self.new_entry.as_ref()
    }

    pub fn context_menu(&self) -> Option<&ContextMenuState> {
        self.context_menu.as_ref()
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    pub fn children_of(&self, path: &Path) -> Option<&[Node]> {
        self.children.get(path).map(|v| v.as_slice())
    }

    /// Set or replace the root path. Drops any existing watcher and reloads
    /// the top-level directory listing, then installs a recursive watcher.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if Some(&root) == self.root.as_ref() {
            return;
        }
        self.children.clear();
        self.expanded.clear();
        self.selected.clear();
        self.selection_anchor = None;
        self.error = None;
        self.rename = None;
        self.new_entry = None;
        self.context_menu = None;
        // Drop the previous watcher off the main thread — releasing inotify
        // watches for a large tree can take seconds.
        if let Some(old) = self.watcher.take() {
            cx.background_executor()
                .spawn(async move { drop(old) })
                .detach();
        }
        self.watcher_rx = None;
        self.watcher_task = None;

        self.root = Some(root.clone());
        self.expanded.insert(root.clone());
        self.reload_dir(&root);

        // Set up the events channel and start polling synchronously, but
        // install the watcher itself off the main thread — walking a big tree
        // and adding an inotify watch per directory can take several seconds.
        let (events_tx, events_rx) = channel::<FsEvents>();
        self.watcher_rx = Some(events_rx);
        self.spawn_poll_task(cx);
        self.spawn_watcher_install(root, events_tx, cx);

        cx.notify();
    }

    fn spawn_watcher_install(
        &mut self,
        root: PathBuf,
        events_tx: Sender<FsEvents>,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { watcher::start(&root, events_tx) })
                .await;
            let _ = this.update(cx, |tree, cx| match result {
                Ok(w) => tree.watcher = Some(w),
                Err(err) => {
                    tree.error = Some(format!("Watcher failed: {err}").into());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn spawn_poll_task(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this, cx| {
            loop {
                let timer = cx.background_executor().timer(Duration::from_millis(150));
                timer.await;

                let mut paths_to_refresh: HashSet<PathBuf> = HashSet::new();
                let mut affected_paths: HashSet<PathBuf> = HashSet::new();
                let drained = this
                    .update(cx, |tree, _| {
                        let Some(rx) = tree.watcher_rx.as_ref() else {
                            return false;
                        };
                        while let Ok(events) = rx.try_recv() {
                            for path in events.affected_paths {
                                affected_paths.insert(path.clone());
                                if let Some(parent) = path.parent() {
                                    paths_to_refresh.insert(parent.to_path_buf());
                                }
                                if path.is_dir() {
                                    paths_to_refresh.insert(path);
                                }
                            }
                        }
                        true
                    })
                    .ok();

                if drained.is_none() {
                    break;
                }

                if !affected_paths.is_empty() {
                    let paths = affected_paths.into_iter().collect::<Vec<_>>();
                    let _ = this.update(cx, |_, cx| cx.emit(FileTreeEvent::FsChanged { paths }));
                }

                if !paths_to_refresh.is_empty() {
                    let _ = this.update(cx, |tree, cx| {
                        let mut changed = false;
                        for path in paths_to_refresh {
                            if tree.children.contains_key(&path) {
                                tree.reload_dir(&path);
                                changed = true;
                            }
                        }
                        if changed {
                            cx.notify();
                        }
                    });
                }
            }
        });
        self.watcher_task = Some(task);
    }
}
