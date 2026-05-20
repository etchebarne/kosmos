use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    mpsc::{Receiver, Sender, channel},
};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb};
use gpui::{
    App, AppContext, BorrowAppContext, Context, Entity, EntityInputHandler, FocusHandle, Focusable,
    Global, Task, UTF16Selection, Window,
};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

const DEFAULT_COLUMNS: usize = 80;
const DEFAULT_ROWS: usize = 24;
const DEFAULT_CELL_WIDTH_PX: u16 = 8;
const DEFAULT_CELL_HEIGHT_PX: u16 = 18;
const OUTPUT_POLL_INTERVAL: Duration = Duration::from_millis(16);
const CWD_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const READ_BUFFER_SIZE: usize = 8192;
const MIN_ZOOM_PERCENT: i64 = 50;
const MAX_ZOOM_PERCENT: i64 = 200;
const ZOOM_STEP_PERCENT: i64 = 5;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct TerminalKey {
    pub workspace_id: usize,
    pub tab_id: usize,
}

impl TerminalKey {
    pub fn new(workspace_id: usize, tab_id: usize) -> Self {
        Self {
            workspace_id,
            tab_id,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalPalette {
    Dark,
    Light,
}

impl TerminalPalette {
    pub fn for_dark_theme(is_dark: bool) -> Self {
        match is_dark {
            true => Self::Dark,
            false => Self::Light,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalMouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalMouseModifiers {
    pub shift: bool,
    pub alt: bool,
    pub control: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellProfile {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

impl ShellProfile {
    fn new(path: PathBuf) -> Option<Self> {
        if !path.is_absolute() || !is_executable_file(&path) {
            return None;
        }
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("shell")
            .to_string();
        let id = path.to_string_lossy().into_owned();
        Some(Self { id, label, path })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl TerminalColor {
    pub const fn rgb(value: u32) -> Self {
        Self {
            r: ((value >> 16) & 0xff) as u8,
            g: ((value >> 8) & 0xff) as u8,
            b: (value & 0xff) as u8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalTheme {
    pub palette: TerminalPalette,
    pub foreground: TerminalColor,
    pub background: TerminalColor,
}

impl TerminalTheme {
    pub fn new(
        palette: TerminalPalette,
        foreground: TerminalColor,
        background: TerminalColor,
    ) -> Self {
        Self {
            palette,
            foreground,
            background,
        }
    }

    pub fn for_palette(palette: TerminalPalette) -> Self {
        match palette {
            TerminalPalette::Dark => Self::new(
                palette,
                rgb_to_terminal(DARK_FALLBACK_FOREGROUND),
                rgb_to_terminal(DARK_FALLBACK_BACKGROUND),
            ),
            TerminalPalette::Light => Self::new(
                palette,
                rgb_to_terminal(LIGHT_FALLBACK_FOREGROUND),
                rgb_to_terminal(LIGHT_FALLBACK_BACKGROUND),
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalStyle {
    pub foreground: TerminalColor,
    pub background: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
    pub strikeout: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalTextRun {
    pub range: Range<usize>,
    pub style: TerminalStyle,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCellRun {
    pub text: String,
    pub style: TerminalStyle,
    pub column: usize,
    pub width: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCell {
    pub text: String,
    pub style: TerminalStyle,
    pub column: usize,
    pub width: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRow {
    pub text: String,
    pub runs: Vec<TerminalTextRun>,
    pub cell_runs: Vec<TerminalCellRun>,
    pub cells: Vec<TerminalCell>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelectionRange {
    pub row: usize,
    pub column: usize,
    pub width: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCursorShape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalCursor {
    pub row: usize,
    pub column: usize,
    pub shape: TerminalCursorShape,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSnapshot {
    pub rows: Vec<TerminalRow>,
    pub cursor: Option<TerminalCursor>,
    pub cursor_color: TerminalColor,
    pub selection_ranges: Vec<TerminalSelectionRange>,
    pub columns: usize,
    pub screen_rows: usize,
    pub display_offset: usize,
    pub title: Option<String>,
    pub selected_shell_id: String,
    pub selected_shell_label: String,
    pub shells: Vec<ShellProfile>,
    pub zoom_percent: i64,
    pub status: TerminalStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalStatus {
    Running,
    Restarting,
    Failed(String),
    Exited,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalKeyInput {
    pub key: String,
    pub text: Option<String>,
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    pub platform: bool,
}

#[derive(Default)]
pub struct TerminalStore {
    sessions: HashMap<TerminalKey, Entity<TerminalSession>>,
}

impl TerminalStore {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    pub fn for_tab(key: TerminalKey, cwd: PathBuf, cx: &mut App) -> Entity<TerminalSession> {
        if let Some(existing) = cx
            .try_global::<Self>()
            .and_then(|store| store.sessions.get(&key).cloned())
        {
            return existing;
        }

        let entity = cx.new(|cx| TerminalSession::new(key, cwd.clone(), cx));
        cx.update_global::<Self, _>(|store, _| {
            store.sessions.insert(key, entity.clone());
        });
        entity
    }

    pub fn get(key: TerminalKey, cx: &App) -> Option<Entity<TerminalSession>> {
        cx.try_global::<Self>()
            .and_then(|store| store.sessions.get(&key).cloned())
    }

    pub fn cwd(key: TerminalKey, cx: &mut App) -> Option<PathBuf> {
        let session = cx
            .try_global::<Self>()
            .and_then(|store| store.sessions.get(&key).cloned())?;
        Some(session.update(cx, |session, _| {
            session.refresh_current_directory();
            session.cwd.clone()
        }))
    }

    pub fn drop_tab(key: TerminalKey, cx: &mut App) {
        let Some(session) = cx
            .try_global::<Self>()
            .and_then(|store| store.sessions.get(&key).cloned())
        else {
            return;
        };
        session.update(cx, |session, _| session.shutdown());
        cx.update_global::<Self, _>(|store, _| {
            store.sessions.remove(&key);
        });
    }

    pub fn drop_workspace(workspace_id: usize, cx: &mut App) {
        let keys = cx
            .try_global::<Self>()
            .map(|store| {
                store
                    .sessions
                    .keys()
                    .copied()
                    .filter(|key| key.workspace_id == workspace_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for key in keys {
            Self::drop_tab(key, cx);
        }
    }
}

impl Global for TerminalStore {}

pub struct TerminalSession {
    key: TerminalKey,
    cwd: PathBuf,
    shells: Vec<ShellProfile>,
    selected_shell_id: String,
    parser: Processor,
    term: Term<TerminalEventProxy>,
    size: TerminalSize,
    process: Option<TerminalProcess>,
    output_rx: Option<Receiver<Vec<u8>>>,
    event_rx: Receiver<Event>,
    event_tx: Sender<Event>,
    focus_handle: FocusHandle,
    poll_task: Option<Task<()>>,
    observed_by_ui: bool,
    theme: TerminalTheme,
    selection: Option<TerminalSelection>,
    selecting: bool,
    mouse_pressed_button: Option<TerminalMouseButton>,
    zoom_percent: i64,
    status: TerminalStatus,
    title: Option<String>,
    last_cwd_refresh: Instant,
    snapshot_cache: Option<Arc<TerminalSnapshot>>,
    snapshot_dirty: bool,
}

impl TerminalSession {
    pub fn new(key: TerminalKey, cwd: PathBuf, cx: &mut Context<Self>) -> Self {
        let shells = discover_shells();
        let selected_shell_id = shells
            .first()
            .map(|shell| shell.id.clone())
            .unwrap_or_else(|| fallback_shell_profile().id);
        let (event_tx, event_rx) = channel();
        let size = TerminalSize::default();
        let term = new_term(size, event_tx.clone());
        let mut session = Self {
            key,
            cwd,
            shells,
            selected_shell_id,
            parser: Processor::new(),
            term,
            size,
            process: None,
            output_rx: None,
            event_rx,
            event_tx,
            focus_handle: cx.focus_handle(),
            poll_task: None,
            observed_by_ui: false,
            theme: TerminalTheme::for_palette(TerminalPalette::Dark),
            selection: None,
            selecting: false,
            mouse_pressed_button: None,
            zoom_percent: 100,
            status: TerminalStatus::Restarting,
            title: None,
            last_cwd_refresh: Instant::now(),
            snapshot_cache: None,
            snapshot_dirty: true,
        };
        session.start_process();
        session.start_poll_task(cx);
        session
    }

    pub fn key(&self) -> TerminalKey {
        self.key
    }

    pub fn focus_handle(&self) -> FocusHandle {
        self.focus_handle.clone()
    }

    pub fn zoom_percent(&self) -> i64 {
        self.zoom_percent
    }

    pub fn observed_by_ui(&self) -> bool {
        self.observed_by_ui
    }

    pub fn mark_observed_by_ui(&mut self) {
        self.observed_by_ui = true;
    }

    pub fn set_theme(&mut self, theme: TerminalTheme) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        self.invalidate_snapshot();
    }

    pub fn snapshot(&mut self) -> Arc<TerminalSnapshot> {
        if !self.snapshot_dirty
            && let Some(snapshot) = &self.snapshot_cache
        {
            return snapshot.clone();
        }

        let snapshot = Arc::new(self.build_snapshot());
        self.snapshot_cache = Some(snapshot.clone());
        self.snapshot_dirty = false;
        snapshot
    }

    fn build_snapshot(&self) -> TerminalSnapshot {
        let content = self.term.renderable_content();
        let display_offset = content.display_offset;
        let cursor_color = snapshot_cursor_color(content.colors, self.theme);
        let cursor = snapshot_cursor(content.cursor, display_offset, self.size);
        let selection_ranges = selection_ranges(self.selection, self.size);
        let rows = snapshot_rows(content, self.size, self.theme);
        let selected_shell = self.selected_shell();
        TerminalSnapshot {
            rows,
            cursor,
            cursor_color,
            selection_ranges,
            columns: self.size.columns,
            screen_rows: self.size.rows,
            display_offset,
            title: self.title.clone(),
            selected_shell_id: selected_shell.id.clone(),
            selected_shell_label: selected_shell.label.clone(),
            shells: self.shells.clone(),
            zoom_percent: self.zoom_percent,
            status: self.status.clone(),
        }
    }

    pub fn resize(&mut self, columns: usize, rows: usize, cell_width: u16, cell_height: u16) {
        let next = TerminalSize {
            columns: columns.max(alacritty_terminal::term::MIN_COLUMNS),
            rows: rows.max(alacritty_terminal::term::MIN_SCREEN_LINES),
            cell_width,
            cell_height,
        };
        if self.size == next {
            return;
        }
        self.size = next;
        self.term.resize(next);
        self.invalidate_snapshot();
        if let Some(process) = &mut self.process {
            process.resize(next);
        }
    }

    pub fn write_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.write_to_pty(text.as_bytes());
    }

    pub fn write_key(&mut self, input: TerminalKeyInput) -> bool {
        if let Some(bytes) = encode_key_input(&input, *self.term.mode()) {
            self.write_to_pty(bytes.as_bytes());
            return true;
        }
        false
    }

    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.term.mode().contains(TermMode::BRACKETED_PASTE) {
            self.write_text("\x1b[200~");
            self.write_text(text);
            self.write_text("\x1b[201~");
        } else {
            self.write_text(text);
        }
    }

    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.feed_bytes(b"\x1b[2J\x1b[3J\x1b[H");
        cx.notify();
    }

    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.restart(cx);
    }

    pub fn select_shell(&mut self, shell_id: &str, cx: &mut Context<Self>) {
        if self.selected_shell_id == shell_id {
            return;
        }
        if !self.shells.iter().any(|shell| shell.id == shell_id) {
            return;
        }
        self.selected_shell_id = shell_id.to_string();
        self.restart(cx);
    }

    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom_percent + ZOOM_STEP_PERCENT, cx);
    }

    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom_percent - ZOOM_STEP_PERCENT, cx);
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(100, cx);
    }

    pub fn scroll_lines(&mut self, lines: i32, cx: &mut Context<Self>) {
        if lines == 0 {
            return;
        }
        if self
            .term
            .mode()
            .contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
        {
            let key = if lines > 0 { "up" } else { "down" };
            for _ in 0..lines.unsigned_abs().min(128) {
                self.write_key(TerminalKeyInput {
                    key: key.to_string(),
                    ..Default::default()
                });
            }
            return;
        }
        self.term.scroll_display(Scroll::Delta(lines));
        self.invalidate_snapshot();
        cx.notify();
    }

    pub fn mouse_down(
        &mut self,
        row: usize,
        column: usize,
        button: TerminalMouseButton,
        modifiers: TerminalMouseModifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let position = TerminalSelectionPoint::new(row, column, self.size);
        if self.should_report_mouse(modifiers, button) {
            self.selecting = false;
            self.mouse_pressed_button = Some(button);
            self.write_mouse_event(TerminalMouseEventKind::Press(button), position, modifiers);
            return true;
        }

        if button != TerminalMouseButton::Left {
            return false;
        }

        self.selection = Some(TerminalSelection::new(position));
        self.selecting = true;
        self.invalidate_snapshot();
        cx.notify();
        true
    }

    pub fn mouse_move(
        &mut self,
        row: usize,
        column: usize,
        modifiers: TerminalMouseModifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let position = TerminalSelectionPoint::new(row, column, self.size);
        if self.mouse_reporting_enabled()
            && self.mouse_move_reporting_enabled()
            && let Some(button) = self.mouse_pressed_button
        {
            self.write_mouse_event(TerminalMouseEventKind::Move(button), position, modifiers);
            return true;
        }

        if !self.selecting {
            return false;
        }
        let Some(selection) = self.selection.as_mut() else {
            return false;
        };
        if selection.update_active(position) {
            self.invalidate_snapshot();
            cx.notify();
        }
        true
    }

    pub fn mouse_up(
        &mut self,
        row: usize,
        column: usize,
        button: TerminalMouseButton,
        modifiers: TerminalMouseModifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        let position = TerminalSelectionPoint::new(row, column, self.size);
        if self.mouse_reporting_enabled() && self.mouse_pressed_button.take().is_some() {
            self.write_mouse_event(TerminalMouseEventKind::Release(button), position, modifiers);
            return true;
        }

        if !self.selecting {
            return false;
        }
        self.selecting = false;
        let Some(selection) = self.selection.as_mut() else {
            return false;
        };
        selection.update_active(position);
        if selection.is_empty() {
            self.selection = None;
        }
        self.invalidate_snapshot();
        cx.notify();
        true
    }

    pub fn scroll_lines_at(
        &mut self,
        row: usize,
        column: usize,
        lines: i32,
        modifiers: TerminalMouseModifiers,
        cx: &mut Context<Self>,
    ) -> bool {
        if lines == 0 {
            return false;
        }
        if self.mouse_reporting_enabled() && !modifiers.shift {
            let position = TerminalSelectionPoint::new(row, column, self.size);
            let direction = if lines > 0 {
                TerminalMouseWheelDirection::Up
            } else {
                TerminalMouseWheelDirection::Down
            };
            for _ in 0..lines.unsigned_abs().min(128) {
                self.write_mouse_event(
                    TerminalMouseEventKind::Wheel(direction),
                    position,
                    modifiers,
                );
            }
            return true;
        }

        self.scroll_lines(lines, cx);
        true
    }

    pub fn selected_text(&mut self) -> Option<String> {
        let selection = self.selection?.normalized(self.size)?;
        let snapshot = self.snapshot();
        let mut lines = Vec::new();

        for row_index in selection.start.row..=selection.end.row {
            let row = snapshot.rows.get(row_index)?;
            let start_column = if row_index == selection.start.row {
                selection.start.column
            } else {
                0
            };
            let end_column = if row_index == selection.end.row {
                selection
                    .end
                    .column
                    .saturating_add(1)
                    .min(self.size.columns)
            } else {
                self.size.columns
            };
            lines.push(text_for_columns(row, start_column, end_column));
        }

        let text = lines.join("\n");
        (!text.is_empty()).then_some(text)
    }

    pub fn shutdown(&mut self) {
        self.process = None;
        self.output_rx = None;
        self.poll_task = None;
        self.status = TerminalStatus::Exited;
        self.invalidate_snapshot();
    }

    fn set_zoom(&mut self, percent: i64, cx: &mut Context<Self>) {
        let clamped = percent.clamp(MIN_ZOOM_PERCENT, MAX_ZOOM_PERCENT);
        if self.zoom_percent == clamped {
            return;
        }
        self.zoom_percent = clamped;
        self.invalidate_snapshot();
        cx.notify();
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        self.refresh_current_directory();
        self.status = TerminalStatus::Restarting;
        self.process = None;
        self.output_rx = None;
        self.poll_task = None;
        self.parser = Processor::new();
        self.term = new_term(self.size, self.event_tx.clone());
        self.last_cwd_refresh = Instant::now();
        self.start_process();
        self.start_poll_task(cx);
        self.invalidate_snapshot();
        cx.notify();
    }

    fn start_process(&mut self) {
        let Some(shell) = self
            .shells
            .iter()
            .find(|shell| shell.id == self.selected_shell_id)
            .cloned()
            .or_else(|| self.shells.first().cloned())
        else {
            self.status = TerminalStatus::Failed("No shell found".to_string());
            return;
        };

        self.selected_shell_id = shell.id.clone();
        let pty_system = native_pty_system();
        let Ok(pair) = pty_system.openpty(pty_size(self.size)) else {
            self.status = TerminalStatus::Failed("Failed to open PTY".to_string());
            return;
        };

        let reader = match pair.master.try_clone_reader() {
            Ok(reader) => reader,
            Err(err) => {
                self.status = TerminalStatus::Failed(format!("Failed to read PTY: {err}"));
                return;
            }
        };
        let writer = match pair.master.take_writer() {
            Ok(writer) => writer,
            Err(err) => {
                self.status = TerminalStatus::Failed(format!("Failed to write PTY: {err}"));
                return;
            }
        };

        let mut command = CommandBuilder::new(&shell.path);
        command.cwd(self.cwd.as_os_str());
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env("TERM_PROGRAM", "kosmos");

        let child = match pair.slave.spawn_command(command) {
            Ok(child) => child,
            Err(err) => {
                self.status = TerminalStatus::Failed(format!("Failed to spawn shell: {err}"));
                return;
            }
        };
        drop(pair.slave);

        let (output_tx, output_rx) = channel();
        spawn_reader(reader, output_tx);
        self.output_rx = Some(output_rx);
        self.process = Some(TerminalProcess::new(pair.master, writer, child));
        self.status = TerminalStatus::Running;
    }

    fn start_poll_task(&mut self, cx: &mut Context<Self>) {
        if self.poll_task.is_some() {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(OUTPUT_POLL_INTERVAL).await;
                let Ok(keep_running) = this.update(cx, |session, cx| session.poll(cx)) else {
                    break;
                };
                if !keep_running {
                    let _ = this.update(cx, |session, _| {
                        session.poll_task = None;
                    });
                    break;
                }
            }
        });
        self.poll_task = Some(task);
    }

    fn poll(&mut self, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        let mut disconnected = false;
        let mut output: Option<Vec<u8>> = None;

        if let Some(rx) = &self.output_rx {
            loop {
                match rx.try_recv() {
                    Ok(chunk) if chunk.is_empty() => {
                        disconnected = true;
                        break;
                    }
                    Ok(chunk) => match output.as_mut() {
                        Some(bytes) => bytes.extend_from_slice(&chunk),
                        None => output = Some(chunk),
                    },
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        if let Some(output) = output {
            self.feed_bytes(&output);
            changed = true;
        }
        if self.refresh_current_directory_if_due() {
            changed = true;
        }
        if disconnected {
            self.process = None;
            self.output_rx = None;
            if matches!(
                self.status,
                TerminalStatus::Running | TerminalStatus::Restarting
            ) {
                self.status = TerminalStatus::Exited;
                self.invalidate_snapshot();
                changed = true;
            }
        }
        if self.process.as_mut().is_some_and(TerminalProcess::try_wait) {
            self.process = None;
            if !matches!(self.status, TerminalStatus::Exited) {
                self.status = TerminalStatus::Exited;
                self.invalidate_snapshot();
                changed = true;
            }
        }
        if self.drain_events() {
            changed = true;
        }
        if changed {
            cx.notify();
        }
        self.process.is_some() || self.output_rx.is_some()
    }

    fn feed_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.parser.advance(&mut self.term, bytes);
        self.invalidate_snapshot();
    }

    fn invalidate_snapshot(&mut self) {
        self.snapshot_dirty = true;
    }

    fn refresh_current_directory(&mut self) -> bool {
        let Some(cwd) = self
            .process
            .as_ref()
            .and_then(TerminalProcess::current_directory)
        else {
            return false;
        };
        if self.cwd == cwd {
            return false;
        }
        self.cwd = cwd;
        true
    }

    fn refresh_current_directory_if_due(&mut self) -> bool {
        if self.last_cwd_refresh.elapsed() < CWD_REFRESH_INTERVAL {
            return false;
        }
        self.last_cwd_refresh = Instant::now();
        self.refresh_current_directory()
    }

    fn drain_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                Event::PtyWrite(text) => self.write_text(&text),
                Event::Title(title) => {
                    self.title = Some(title);
                    self.invalidate_snapshot();
                    changed = true;
                }
                Event::ResetTitle => {
                    self.title = None;
                    self.invalidate_snapshot();
                    changed = true;
                }
                Event::ColorRequest(index, formatter) => {
                    let rgb = rgb_for_index(index, self.term.colors(), self.theme)
                        .unwrap_or_else(|| default_foreground(self.theme));
                    self.write_text(&formatter(rgb));
                }
                Event::TextAreaSizeRequest(formatter) => {
                    self.write_text(&formatter(WindowSize {
                        num_lines: self.size.rows as u16,
                        num_cols: self.size.columns as u16,
                        cell_width: self.size.cell_width,
                        cell_height: self.size.cell_height,
                    }));
                }
                Event::ClipboardStore(_, _) | Event::ClipboardLoad(_, _) => {}
                Event::ChildExit(_) => {
                    self.status = TerminalStatus::Exited;
                    self.invalidate_snapshot();
                    changed = true;
                }
                Event::Exit => {
                    self.status = TerminalStatus::Exited;
                    self.invalidate_snapshot();
                    changed = true;
                }
                Event::MouseCursorDirty
                | Event::CursorBlinkingChange
                | Event::Wakeup
                | Event::Bell => {
                    self.invalidate_snapshot();
                    changed = true;
                }
            }
        }
        changed
    }

    fn write_to_pty(&mut self, bytes: &[u8]) {
        if let Some(process) = &mut self.process {
            process.write(bytes);
        }
    }

    fn selected_shell(&self) -> ShellProfile {
        self.shells
            .iter()
            .find(|shell| shell.id == self.selected_shell_id)
            .cloned()
            .or_else(|| self.shells.first().cloned())
            .unwrap_or_else(fallback_shell_profile)
    }

    fn mouse_reporting_enabled(&self) -> bool {
        self.term.mode().intersects(TermMode::MOUSE_MODE)
    }

    fn mouse_move_reporting_enabled(&self) -> bool {
        self.term
            .mode()
            .intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
    }

    fn should_report_mouse(
        &self,
        modifiers: TerminalMouseModifiers,
        button: TerminalMouseButton,
    ) -> bool {
        self.mouse_reporting_enabled() && !(modifiers.shift && button == TerminalMouseButton::Left)
    }

    fn write_mouse_event(
        &mut self,
        kind: TerminalMouseEventKind,
        position: TerminalSelectionPoint,
        modifiers: TerminalMouseModifiers,
    ) {
        if let Some(bytes) = encode_mouse_event(kind, position, modifiers, *self.term.mode()) {
            self.write_to_pty(&bytes);
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Focusable for TerminalSession {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EntityInputHandler for TerminalSession {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        actual_range.replace(range_utf16);
        Some(String::new())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        None
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}

    fn replace_text_in_range(
        &mut self,
        _range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        self.write_text(new_text);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_text_in_range(range_utf16, new_text, window, cx);
    }

    fn bounds_for_range(
        &mut self,
        _range_utf16: Range<usize>,
        bounds: gpui::Bounds<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(
        &mut self,
        _point: gpui::Point<gpui::Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(0)
    }
}

struct TerminalProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    exited: bool,
}

impl TerminalProcess {
    fn new(
        master: Box<dyn MasterPty + Send>,
        writer: Box<dyn Write + Send>,
        child: Box<dyn Child + Send + Sync>,
    ) -> Self {
        Self {
            master,
            writer,
            child,
            exited: false,
        }
    }

    fn resize(&mut self, size: TerminalSize) {
        let _ = self.master.resize(pty_size(size));
    }

    fn write(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    fn current_directory(&self) -> Option<PathBuf> {
        process_current_directory(self.child.process_id()?)
    }

    fn try_wait(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_)) => {
                self.exited = true;
                true
            }
            Ok(None) | Err(_) => false,
        }
    }

    fn terminate(&mut self) {
        if self.exited {
            return;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.exited = true;
    }
}

#[cfg(target_os = "linux")]
fn process_current_directory(pid: u32) -> Option<PathBuf> {
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd")).ok()?;
    (cwd.is_absolute() && cwd.is_dir()).then_some(cwd)
}

#[cfg(not(target_os = "linux"))]
fn process_current_directory(_pid: u32) -> Option<PathBuf> {
    None
}

impl Drop for TerminalProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[derive(Clone)]
struct TerminalEventProxy {
    tx: Sender<Event>,
}

impl EventListener for TerminalEventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(event);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSize {
    columns: usize,
    rows: usize,
    cell_width: u16,
    cell_height: u16,
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self {
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            cell_width: DEFAULT_CELL_WIDTH_PX,
            cell_height: DEFAULT_CELL_HEIGHT_PX,
        }
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSelectionPoint {
    row: usize,
    column: usize,
}

impl TerminalSelectionPoint {
    fn new(row: usize, column: usize, size: TerminalSize) -> Self {
        Self {
            row: row.min(size.rows.saturating_sub(1)),
            column: column.min(size.columns.saturating_sub(1)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalSelection {
    anchor: TerminalSelectionPoint,
    active: TerminalSelectionPoint,
}

impl TerminalSelection {
    fn new(position: TerminalSelectionPoint) -> Self {
        Self {
            anchor: position,
            active: position,
        }
    }

    fn update_active(&mut self, position: TerminalSelectionPoint) -> bool {
        if self.active == position {
            return false;
        }
        self.active = position;
        true
    }

    fn is_empty(&self) -> bool {
        self.anchor == self.active
    }

    fn normalized(self, size: TerminalSize) -> Option<TerminalNormalizedSelection> {
        if self.is_empty() || size.rows == 0 || size.columns == 0 {
            return None;
        }
        let (start, end) =
            if (self.anchor.row, self.anchor.column) <= (self.active.row, self.active.column) {
                (self.anchor, self.active)
            } else {
                (self.active, self.anchor)
            };
        Some(TerminalNormalizedSelection { start, end })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TerminalNormalizedSelection {
    start: TerminalSelectionPoint,
    end: TerminalSelectionPoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalMouseWheelDirection {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalMouseEventKind {
    Press(TerminalMouseButton),
    Move(TerminalMouseButton),
    Release(TerminalMouseButton),
    Wheel(TerminalMouseWheelDirection),
}

fn new_term(size: TerminalSize, event_tx: Sender<Event>) -> Term<TerminalEventProxy> {
    Term::new(
        Config {
            scrolling_history: 10_000,
            ..Config::default()
        },
        &size,
        TerminalEventProxy { tx: event_tx },
    )
}

fn pty_size(size: TerminalSize) -> PtySize {
    PtySize {
        rows: size.rows as u16,
        cols: size.columns as u16,
        pixel_width: size.columns.saturating_mul(size.cell_width as usize) as u16,
        pixel_height: size.rows.saturating_mul(size.cell_height as usize) as u16,
    }
}

fn spawn_reader(mut reader: Box<dyn Read + Send>, output_tx: Sender<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut buffer = [0u8; READ_BUFFER_SIZE];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if output_tx.send(buffer[..count].to_vec()).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = output_tx.send(Vec::new());
    });
}

pub fn discover_shells() -> Vec<ShellProfile> {
    let mut candidates = Vec::new();
    candidates.extend(environment_shell_candidates());
    candidates.extend(platform_shell_candidates());
    candidates.push(fallback_shell_profile().path);

    let mut shells = candidates
        .into_iter()
        .filter_map(ShellProfile::new)
        .filter(is_selectable_shell_profile)
        .collect::<Vec<_>>();
    dedupe_shell_profiles_by_label(&mut shells);
    hide_redundant_sh_profile(&mut shells);
    shells
}

fn dedupe_shell_profiles_by_label(shells: &mut Vec<ShellProfile>) {
    let mut seen_labels = HashSet::new();
    shells.retain(|shell| seen_labels.insert(shell.label.clone()));
}

fn hide_redundant_sh_profile(shells: &mut Vec<ShellProfile>) {
    if shells.iter().any(|shell| shell.label != "sh") {
        shells.retain(|shell| shell.label != "sh");
    }
}

fn is_selectable_shell_profile(shell: &ShellProfile) -> bool {
    is_selectable_shell_name(&shell.label)
}

#[cfg(unix)]
fn is_selectable_shell_name(name: &str) -> bool {
    !matches!(
        name,
        "false"
            | "git-shell"
            | "nologin"
            | "rbash"
            | "rksh"
            | "rsh"
            | "rzsh"
            | "systemd-home-fallback-shell"
    )
}

#[cfg(not(unix))]
fn is_selectable_shell_name(_name: &str) -> bool {
    true
}

fn environment_shell_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(shell) = std::env::var_os("SHELL") {
        candidates.push(PathBuf::from(shell));
    }
    #[cfg(windows)]
    if let Some(shell) = std::env::var_os("COMSPEC") {
        candidates.push(PathBuf::from(shell));
    }
    candidates
}

#[cfg(unix)]
fn platform_shell_candidates() -> Vec<PathBuf> {
    let mut candidates = shells_from_etc_shells();
    candidates.extend(common_unix_shell_candidates());
    candidates
}

#[cfg(windows)]
fn platform_shell_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        candidates.push(PathBuf::from(program_files).join("PowerShell/7/pwsh.exe"));
    }
    let system_root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    candidates.push(system_root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
    candidates.push(system_root.join(r"System32\cmd.exe"));
    candidates
}

#[cfg(not(any(unix, windows)))]
fn platform_shell_candidates() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(unix)]
fn shells_from_etc_shells() -> Vec<PathBuf> {
    let Ok(contents) = std::fs::read_to_string("/etc/shells") else {
        return Vec::new();
    };
    contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

#[cfg(unix)]
fn common_unix_shell_candidates() -> Vec<PathBuf> {
    [
        "/bin/zsh",
        "/usr/bin/zsh",
        "/bin/fish",
        "/usr/bin/fish",
        "/bin/bash",
        "/usr/bin/bash",
        "/bin/sh",
        "/usr/bin/sh",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(unix)]
fn fallback_shell_profile() -> ShellProfile {
    let path = PathBuf::from("/bin/sh");
    ShellProfile {
        id: path.to_string_lossy().into_owned(),
        label: "sh".to_string(),
        path,
    }
}

#[cfg(windows)]
fn fallback_shell_profile() -> ShellProfile {
    let path = std::env::var_os("COMSPEC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"));
    ShellProfile {
        id: path.to_string_lossy().into_owned(),
        label: path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("cmd")
            .to_string(),
        path,
    }
}

#[cfg(not(any(unix, windows)))]
fn fallback_shell_profile() -> ShellProfile {
    let path = PathBuf::from("sh");
    ShellProfile {
        id: path.to_string_lossy().into_owned(),
        label: "sh".to_string(),
        path,
    }
}

fn snapshot_rows(
    content: alacritty_terminal::term::RenderableContent<'_>,
    size: TerminalSize,
    theme: TerminalTheme,
) -> Vec<TerminalRow> {
    let mut cells = vec![vec![Cell::default(); size.columns]; size.rows];
    let display_offset = content.display_offset;
    let colors = content.colors;
    for indexed in content.display_iter {
        let row = indexed.point.line.0 + display_offset as i32;
        if row < 0 || row as usize >= size.rows {
            continue;
        }
        let column = indexed.point.column.0;
        if column >= size.columns {
            continue;
        }
        cells[row as usize][column] = indexed.cell.clone();
    }

    cells
        .into_iter()
        .map(|row| snapshot_row(row, colors, theme))
        .collect()
}

fn snapshot_row(
    cells: Vec<Cell>,
    colors: &alacritty_terminal::term::color::Colors,
    theme: TerminalTheme,
) -> TerminalRow {
    let mut text = String::new();
    let mut runs: Vec<TerminalTextRun> = Vec::new();
    let mut cell_runs: Vec<TerminalCellRun> = Vec::new();
    let mut terminal_cells: Vec<TerminalCell> = Vec::new();
    let total_columns = cells.len();
    let mut occupied_columns = 0;
    let mut previous_cell_was_wide = false;

    for cell in cells {
        let is_wide_spacer = cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
        let style = style_for_cell(&cell, colors, theme);
        if is_wide_spacer {
            if previous_cell_was_wide {
                previous_cell_was_wide = false;
                continue;
            }
            push_terminal_cell(
                &mut terminal_cells,
                " ".to_string(),
                style,
                occupied_columns,
                1,
            );
            push_cell_run(&mut cell_runs, " ".to_string(), style, occupied_columns, 1);
            occupied_columns += 1;
            previous_cell_was_wide = false;
            continue;
        }

        let start = text.len();
        let cell_text = text_for_cell(&cell);
        text.push_str(&cell_text);
        let end = text.len();
        push_text_run(&mut runs, start..end, style);

        let is_wide = cell.flags.contains(Flags::WIDE_CHAR);
        let width = if is_wide && occupied_columns + 1 < total_columns {
            2
        } else {
            1
        };
        push_terminal_cell(
            &mut terminal_cells,
            cell_text.clone(),
            style,
            occupied_columns,
            width,
        );
        push_cell_run(&mut cell_runs, cell_text, style, occupied_columns, width);
        occupied_columns += width;
        previous_cell_was_wide = is_wide;
    }

    TerminalRow {
        text,
        runs,
        cell_runs,
        cells: terminal_cells,
    }
}

fn push_terminal_cell(
    cells: &mut Vec<TerminalCell>,
    text: String,
    style: TerminalStyle,
    column: usize,
    width: usize,
) {
    cells.push(TerminalCell {
        text,
        style,
        column,
        width,
    });
}

fn push_cell_run(
    cell_runs: &mut Vec<TerminalCellRun>,
    text: String,
    style: TerminalStyle,
    column: usize,
    width: usize,
) {
    if let Some(last) = cell_runs.last_mut()
        && last.style == style
        && last.column + last.width == column
    {
        last.text.push_str(&text);
        last.width += width;
        return;
    }

    cell_runs.push(TerminalCellRun {
        text,
        style,
        column,
        width,
    });
}

fn text_for_cell(cell: &Cell) -> String {
    if cell.flags.contains(Flags::HIDDEN) {
        return " ".to_string();
    }

    let mut text = String::new();
    text.push(cell.c);
    for ch in cell.zerowidth().into_iter().flatten() {
        text.push(*ch);
    }
    text
}

fn push_text_run(runs: &mut Vec<TerminalTextRun>, range: Range<usize>, style: TerminalStyle) {
    if range.is_empty() {
        return;
    }
    if let Some(last) = runs.last_mut()
        && last.style == style
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }
    runs.push(TerminalTextRun { range, style });
}

fn snapshot_cursor(
    cursor: alacritty_terminal::term::RenderableCursor,
    display_offset: usize,
    size: TerminalSize,
) -> Option<TerminalCursor> {
    let row = cursor.point.line.0 + display_offset as i32;
    if row < 0 || row as usize >= size.rows {
        return None;
    }
    let shape = match cursor.shape {
        CursorShape::Block => TerminalCursorShape::Block,
        CursorShape::Underline => TerminalCursorShape::Underline,
        CursorShape::Beam => TerminalCursorShape::Beam,
        CursorShape::HollowBlock => TerminalCursorShape::HollowBlock,
        CursorShape::Hidden => TerminalCursorShape::Hidden,
    };
    if shape == TerminalCursorShape::Hidden {
        return None;
    }
    Some(TerminalCursor {
        row: row as usize,
        column: cursor.point.column.0.min(size.columns.saturating_sub(1)),
        shape,
    })
}

fn selection_ranges(
    selection: Option<TerminalSelection>,
    size: TerminalSize,
) -> Vec<TerminalSelectionRange> {
    let Some(selection) = selection.and_then(|selection| selection.normalized(size)) else {
        return Vec::new();
    };

    (selection.start.row..=selection.end.row)
        .filter_map(|row| {
            let start_column = if row == selection.start.row {
                selection.start.column
            } else {
                0
            };
            let end_column = if row == selection.end.row {
                selection.end.column.saturating_add(1).min(size.columns)
            } else {
                size.columns
            };
            let width = end_column.saturating_sub(start_column);
            (width > 0).then_some(TerminalSelectionRange {
                row,
                column: start_column,
                width,
            })
        })
        .collect()
}

fn text_for_columns(row: &TerminalRow, start_column: usize, end_column: usize) -> String {
    if start_column >= end_column {
        return String::new();
    }

    let mut text = String::new();
    for cell in &row.cells {
        let cell_end = cell.column + cell.width;
        if cell.column >= end_column || cell_end <= start_column {
            continue;
        }
        text.push_str(&cell.text);
    }
    text.trim_end_matches(' ').to_string()
}

fn style_for_cell(
    cell: &Cell,
    colors: &alacritty_terminal::term::color::Colors,
    theme: TerminalTheme,
) -> TerminalStyle {
    let mut foreground = color_for(cell.fg, colors, theme)
        .unwrap_or_else(|| rgb_to_terminal(default_foreground(theme)));
    let mut background = color_for(cell.bg, colors, theme)
        .unwrap_or_else(|| rgb_to_terminal(default_background(theme)));
    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }
    TerminalStyle {
        foreground,
        background,
        bold: cell.flags.contains(Flags::BOLD),
        italic: cell.flags.contains(Flags::ITALIC),
        underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
        dim: cell.flags.contains(Flags::DIM),
        strikeout: cell.flags.contains(Flags::STRIKEOUT),
    }
}

fn color_for(
    color: AnsiColor,
    colors: &alacritty_terminal::term::color::Colors,
    theme: TerminalTheme,
) -> Option<TerminalColor> {
    match color {
        AnsiColor::Spec(rgb) => Some(rgb_to_terminal(rgb)),
        AnsiColor::Indexed(index) => {
            rgb_for_index(index as usize, colors, theme).map(rgb_to_terminal)
        }
        AnsiColor::Named(named) => rgb_for_named_color(named, colors, theme).map(rgb_to_terminal),
    }
}

const DARK_FALLBACK_FOREGROUND: Rgb = Rgb {
    r: 0xff,
    g: 0xff,
    b: 0xff,
};
const DARK_FALLBACK_BACKGROUND: Rgb = Rgb {
    r: 0x1c,
    g: 0x1c,
    b: 0x1f,
};
const LIGHT_FALLBACK_FOREGROUND: Rgb = Rgb {
    r: 0x00,
    g: 0x00,
    b: 0x00,
};
const LIGHT_FALLBACK_BACKGROUND: Rgb = Rgb {
    r: 0xff,
    g: 0xff,
    b: 0xff,
};

fn snapshot_cursor_color(
    colors: &alacritty_terminal::term::color::Colors,
    theme: TerminalTheme,
) -> TerminalColor {
    rgb_to_terminal(
        rgb_for_named_color(NamedColor::Cursor, colors, theme)
            .unwrap_or_else(|| default_foreground(theme)),
    )
}

fn rgb_for_index(
    index: usize,
    colors: &alacritty_terminal::term::color::Colors,
    theme: TerminalTheme,
) -> Option<Rgb> {
    if index < alacritty_terminal::term::color::COUNT
        && let Some(color) = colors[index]
    {
        return Some(color);
    }

    default_color_for_index(index, theme)
}

fn rgb_for_named_color(
    color: NamedColor,
    colors: &alacritty_terminal::term::color::Colors,
    theme: TerminalTheme,
) -> Option<Rgb> {
    colors[color].or_else(|| default_named_color(color, theme))
}

fn default_foreground(theme: TerminalTheme) -> Rgb {
    terminal_to_rgb(theme.foreground)
}

fn default_background(theme: TerminalTheme) -> Rgb {
    terminal_to_rgb(theme.background)
}

fn default_named_color(color: NamedColor, theme: TerminalTheme) -> Option<Rgb> {
    let value = match color {
        NamedColor::Black => 0x241f31,
        NamedColor::Red => 0xc01c28,
        NamedColor::Green => 0x2ec27e,
        NamedColor::Yellow => 0xf5c211,
        NamedColor::Blue => 0x1e78e4,
        NamedColor::Magenta => 0x9841bb,
        NamedColor::Cyan => 0x0ab9dc,
        NamedColor::White => 0xc0bfbc,
        NamedColor::BrightBlack => 0x5e5c64,
        NamedColor::BrightRed => 0xed333b,
        NamedColor::BrightGreen => 0x57e389,
        NamedColor::BrightYellow => 0xf8e45c,
        NamedColor::BrightBlue => 0x51a1ff,
        NamedColor::BrightMagenta => 0xc061cb,
        NamedColor::BrightCyan => 0x4fd2fd,
        NamedColor::BrightWhite => 0xf6f5f4,
        NamedColor::Foreground | NamedColor::BrightForeground => {
            return Some(default_foreground(theme));
        }
        NamedColor::Background => return Some(default_background(theme)),
        NamedColor::Cursor => return Some(default_foreground(theme)),
        NamedColor::DimForeground => 0xa6a6a6,
        NamedColor::DimBlack => 0x17141f,
        NamedColor::DimRed => 0x7d121a,
        NamedColor::DimGreen => 0x1e7e52,
        NamedColor::DimYellow => 0x9f7e0b,
        NamedColor::DimBlue => 0x144e94,
        NamedColor::DimMagenta => 0x632a7a,
        NamedColor::DimCyan => 0x07788f,
        NamedColor::DimWhite => 0x7d7c7a,
    };
    Some(rgb_from_u32(value))
}

fn default_color_for_index(index: usize, theme: TerminalTheme) -> Option<Rgb> {
    if index < 16 {
        return default_named_color(
            match index {
                0 => NamedColor::Black,
                1 => NamedColor::Red,
                2 => NamedColor::Green,
                3 => NamedColor::Yellow,
                4 => NamedColor::Blue,
                5 => NamedColor::Magenta,
                6 => NamedColor::Cyan,
                7 => NamedColor::White,
                8 => NamedColor::BrightBlack,
                9 => NamedColor::BrightRed,
                10 => NamedColor::BrightGreen,
                11 => NamedColor::BrightYellow,
                12 => NamedColor::BrightBlue,
                13 => NamedColor::BrightMagenta,
                14 => NamedColor::BrightCyan,
                _ => NamedColor::BrightWhite,
            },
            theme,
        );
    }

    if (16..=231).contains(&index) {
        let index = index - 16;
        let r = index / 36;
        let g = (index % 36) / 6;
        let b = index % 6;
        let channel = |value: usize| if value == 0 { 0 } else { 55 + value as u8 * 40 };
        return Some(Rgb {
            r: channel(r),
            g: channel(g),
            b: channel(b),
        });
    }

    if (232..=255).contains(&index) {
        let value = 8 + (index as u8 - 232) * 10;
        return Some(Rgb {
            r: value,
            g: value,
            b: value,
        });
    }

    match index {
        256 => Some(default_foreground(theme)),
        257 => Some(default_background(theme)),
        258 => Some(default_foreground(theme)),
        _ => None,
    }
}

fn rgb_to_terminal(rgb: Rgb) -> TerminalColor {
    TerminalColor {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    }
}

fn terminal_to_rgb(color: TerminalColor) -> Rgb {
    Rgb {
        r: color.r,
        g: color.g,
        b: color.b,
    }
}

fn rgb_from_u32(value: u32) -> Rgb {
    Rgb {
        r: ((value >> 16) & 0xff) as u8,
        g: ((value >> 8) & 0xff) as u8,
        b: (value & 0xff) as u8,
    }
}

fn encode_mouse_event(
    kind: TerminalMouseEventKind,
    position: TerminalSelectionPoint,
    modifiers: TerminalMouseModifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    let code = mouse_event_code(kind, modifiers, mode.contains(TermMode::SGR_MOUSE));
    let column = position.column + 1;
    let row = position.row + 1;

    if mode.contains(TermMode::SGR_MOUSE) {
        let final_byte = match kind {
            TerminalMouseEventKind::Release(_) => 'm',
            TerminalMouseEventKind::Press(_)
            | TerminalMouseEventKind::Move(_)
            | TerminalMouseEventKind::Wheel(_) => 'M',
        };
        return Some(format!("\x1b[<{code};{column};{row}{final_byte}").into_bytes());
    }

    encode_normal_mouse_event(code, column, row)
}

fn mouse_event_code(
    kind: TerminalMouseEventKind,
    modifiers: TerminalMouseModifiers,
    sgr_mouse: bool,
) -> u8 {
    let mut code = match kind {
        TerminalMouseEventKind::Press(button) => mouse_button_code(button),
        TerminalMouseEventKind::Move(button) => mouse_button_code(button) + 32,
        TerminalMouseEventKind::Release(button) if sgr_mouse => mouse_button_code(button),
        TerminalMouseEventKind::Release(_) => 3,
        TerminalMouseEventKind::Wheel(TerminalMouseWheelDirection::Up) => 64,
        TerminalMouseEventKind::Wheel(TerminalMouseWheelDirection::Down) => 65,
    };
    if modifiers.shift {
        code += 4;
    }
    if modifiers.alt {
        code += 8;
    }
    if modifiers.control {
        code += 16;
    }
    code
}

fn mouse_button_code(button: TerminalMouseButton) -> u8 {
    match button {
        TerminalMouseButton::Left => 0,
        TerminalMouseButton::Middle => 1,
        TerminalMouseButton::Right => 2,
    }
}

fn encode_normal_mouse_event(code: u8, column: usize, row: usize) -> Option<Vec<u8>> {
    let encoded_code = code.checked_add(32)?;
    let encoded_column = u8::try_from(column).ok()?.checked_add(32)?;
    let encoded_row = u8::try_from(row).ok()?.checked_add(32)?;

    Some(vec![
        0x1b,
        b'[',
        b'M',
        encoded_code,
        encoded_column,
        encoded_row,
    ])
}

fn encode_key_input(input: &TerminalKeyInput, mode: TermMode) -> Option<String> {
    if input.platform {
        return None;
    }

    let key = input.key.as_str();
    let encoded = match key {
        "enter" => "\r".to_string(),
        "tab" if input.shift => "\x1b[Z".to_string(),
        "tab" => "\t".to_string(),
        "escape" | "esc" => "\x1b".to_string(),
        "backspace" => {
            if input.alt {
                "\x1b\x7f".to_string()
            } else {
                "\x7f".to_string()
            }
        }
        "left" => cursor_key('D', input, mode),
        "right" => cursor_key('C', input, mode),
        "up" => cursor_key('A', input, mode),
        "down" => cursor_key('B', input, mode),
        "home" => cursor_key('H', input, mode),
        "end" => cursor_key('F', input, mode),
        "delete" => csi_tilde_key(3, input),
        "insert" => csi_tilde_key(2, input),
        "pageup" | "page_up" => csi_tilde_key(5, input),
        "pagedown" | "page_down" => csi_tilde_key(6, input),
        "f1" => function_key("P", 11, input),
        "f2" => function_key("Q", 12, input),
        "f3" => function_key("R", 13, input),
        "f4" => function_key("S", 14, input),
        "f5" => csi_tilde_key(15, input),
        "f6" => csi_tilde_key(17, input),
        "f7" => csi_tilde_key(18, input),
        "f8" => csi_tilde_key(19, input),
        "f9" => csi_tilde_key(20, input),
        "f10" => csi_tilde_key(21, input),
        "f11" => csi_tilde_key(23, input),
        "f12" => csi_tilde_key(24, input),
        _ => text_key(input)?,
    };
    Some(encoded)
}

fn cursor_key(final_byte: char, input: &TerminalKeyInput, mode: TermMode) -> String {
    if let Some(modifier) = modifier_parameter(input) {
        return format!("\x1b[1;{modifier}{final_byte}");
    }
    if mode.contains(TermMode::APP_CURSOR) {
        format!("\x1bO{final_byte}")
    } else {
        format!("\x1b[{final_byte}")
    }
}

fn csi_tilde_key(number: u8, input: &TerminalKeyInput) -> String {
    if let Some(modifier) = modifier_parameter(input) {
        format!("\x1b[{number};{modifier}~")
    } else {
        format!("\x1b[{number}~")
    }
}

fn function_key(ss3_final: &str, csi_number: u8, input: &TerminalKeyInput) -> String {
    if let Some(modifier) = modifier_parameter(input) {
        format!("\x1b[{csi_number};{modifier}~")
    } else {
        format!("\x1bO{ss3_final}")
    }
}

fn modifier_parameter(input: &TerminalKeyInput) -> Option<u8> {
    let mut value = 1;
    if input.shift {
        value += 1;
    }
    if input.alt {
        value += 2;
    }
    if input.control {
        value += 4;
    }
    (value > 1).then_some(value)
}

fn text_key(input: &TerminalKeyInput) -> Option<String> {
    if input.control {
        return control_key(input).map(|ch| ch.to_string());
    }
    let mut text = input.text.clone()?;
    if input.alt {
        text.insert(0, '\x1b');
    }
    Some(text)
}

fn control_key(input: &TerminalKeyInput) -> Option<char> {
    let key = input.key.as_bytes();
    if key.len() != 1 {
        return match input.key.as_str() {
            "space" => Some('\0'),
            "[" => Some('\x1b'),
            "\\" => Some('\x1c'),
            "]" => Some('\x1d'),
            "^" => Some('\x1e'),
            "_" => Some('\x1f'),
            _ => None,
        };
    }
    let byte = key[0].to_ascii_lowercase();
    if byte.is_ascii_lowercase() {
        return Some((byte - b'a' + 1) as char);
    }
    match byte {
        b'@' | b'`' | b' ' => Some('\0'),
        b'[' => Some('\x1b'),
        b'\\' => Some('\x1c'),
        b']' => Some('\x1d'),
        b'^' => Some('\x1e'),
        b'_' | b'/' => Some('\x1f'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;

    fn test_shell_profile(path: &str, label: &str) -> ShellProfile {
        ShellProfile {
            id: path.to_string(),
            label: label.to_string(),
            path: PathBuf::from(path),
        }
    }

    fn shell_labels(shells: &[ShellProfile]) -> Vec<String> {
        shells.iter().map(|shell| shell.label.clone()).collect()
    }

    #[test]
    fn encodes_common_control_keys() {
        let input = TerminalKeyInput {
            key: "c".to_string(),
            control: true,
            ..Default::default()
        };
        assert_eq!(
            encode_key_input(&input, TermMode::default()),
            Some("\x03".to_string())
        );
    }

    #[test]
    fn encodes_modified_cursor_keys() {
        let input = TerminalKeyInput {
            key: "up".to_string(),
            shift: true,
            control: true,
            ..Default::default()
        };
        assert_eq!(
            encode_key_input(&input, TermMode::default()),
            Some("\x1b[1;6A".to_string())
        );
    }

    #[test]
    fn dedupes_shell_profiles() {
        let shells = discover_shells();
        let mut ids = HashSet::new();
        let mut labels = HashSet::new();
        for shell in shells {
            assert!(ids.insert(shell.id));
            assert!(labels.insert(shell.label));
        }
    }

    #[test]
    fn dedupes_shell_profiles_by_display_label() {
        let mut shells = vec![
            test_shell_profile("/usr/bin/bash", "bash"),
            test_shell_profile("/bin/bash", "bash"),
            test_shell_profile("/usr/bin/fish", "fish"),
        ];

        dedupe_shell_profiles_by_label(&mut shells);

        assert_eq!(shell_labels(&shells), ["bash", "fish"]);
    }

    #[test]
    fn hides_sh_when_named_shells_exist() {
        let mut shells = vec![
            test_shell_profile("/bin/sh", "sh"),
            test_shell_profile("/usr/bin/bash", "bash"),
        ];

        hide_redundant_sh_profile(&mut shells);

        assert_eq!(shell_labels(&shells), ["bash"]);
    }

    #[cfg(unix)]
    #[test]
    fn filters_non_interactive_unix_shell_names() {
        assert!(!is_selectable_shell_name("rbash"));
        assert!(!is_selectable_shell_name("git-shell"));
        assert!(!is_selectable_shell_name("systemd-home-fallback-shell"));
        assert!(is_selectable_shell_name("bash"));
        assert!(is_selectable_shell_name("fish"));
    }

    #[test]
    fn default_palette_matches_gnome_console_standard_livery() {
        let theme = TerminalTheme::for_palette(TerminalPalette::Dark);

        assert_eq!(
            default_named_color(NamedColor::Green, theme),
            Some(rgb_from_u32(0x2ec27e))
        );
        assert_eq!(
            default_named_color(NamedColor::BrightGreen, theme),
            Some(rgb_from_u32(0x57e389))
        );
    }

    #[test]
    fn default_foreground_and_background_follow_terminal_theme() {
        let theme = TerminalTheme::new(
            TerminalPalette::Dark,
            TerminalColor::rgb(0xe5e5e5),
            TerminalColor::rgb(0x161616),
        );

        assert_eq!(
            default_named_color(NamedColor::Background, theme),
            Some(rgb_from_u32(0x161616))
        );
        assert_eq!(
            default_named_color(NamedColor::Foreground, theme),
            Some(rgb_from_u32(0xe5e5e5))
        );
        assert_eq!(
            default_named_color(NamedColor::Cursor, theme),
            Some(rgb_from_u32(0xe5e5e5))
        );
        assert_eq!(
            default_color_for_index(257, theme),
            Some(rgb_from_u32(0x161616))
        );
    }

    #[test]
    fn current_palette_overrides_defaults() {
        let theme = TerminalTheme::for_palette(TerminalPalette::Dark);
        let mut colors = alacritty_terminal::term::color::Colors::default();
        let custom_green = rgb_from_u32(0x00ff7f);
        colors[NamedColor::Green] = Some(custom_green);
        colors[2] = Some(custom_green);

        assert_eq!(
            rgb_for_named_color(NamedColor::Green, &colors, theme),
            Some(custom_green)
        );
        assert_eq!(rgb_for_index(2, &colors, theme), Some(custom_green));
    }

    #[test]
    fn encodes_sgr_mouse_events() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let position = TerminalSelectionPoint { row: 2, column: 4 };

        assert_eq!(
            encode_mouse_event(
                TerminalMouseEventKind::Press(TerminalMouseButton::Left),
                position,
                TerminalMouseModifiers::default(),
                mode,
            ),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            encode_mouse_event(
                TerminalMouseEventKind::Release(TerminalMouseButton::Left),
                position,
                TerminalMouseModifiers {
                    control: true,
                    ..Default::default()
                },
                mode,
            ),
            Some(b"\x1b[<16;5;3m".to_vec())
        );
        assert_eq!(
            encode_mouse_event(
                TerminalMouseEventKind::Wheel(TerminalMouseWheelDirection::Down),
                position,
                TerminalMouseModifiers::default(),
                mode,
            ),
            Some(b"\x1b[<65;5;3M".to_vec())
        );
    }

    #[test]
    fn computes_selection_ranges_across_rows() {
        let size = TerminalSize {
            columns: 5,
            rows: 3,
            cell_width: DEFAULT_CELL_WIDTH_PX,
            cell_height: DEFAULT_CELL_HEIGHT_PX,
        };
        let selection = TerminalSelection {
            anchor: TerminalSelectionPoint { row: 0, column: 3 },
            active: TerminalSelectionPoint { row: 2, column: 1 },
        };

        assert_eq!(
            selection_ranges(Some(selection), size),
            vec![
                TerminalSelectionRange {
                    row: 0,
                    column: 3,
                    width: 2,
                },
                TerminalSelectionRange {
                    row: 1,
                    column: 0,
                    width: 5,
                },
                TerminalSelectionRange {
                    row: 2,
                    column: 0,
                    width: 2,
                },
            ]
        );
    }

    #[test]
    fn snapshot_keeps_wide_and_combining_text() {
        let size = TerminalSize {
            columns: 8,
            rows: 2,
            cell_width: DEFAULT_CELL_WIDTH_PX,
            cell_height: DEFAULT_CELL_HEIGHT_PX,
        };
        let mut term = Term::new(Config::default(), &size, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, "中e\u{301}".as_bytes());

        let content = term.renderable_content();
        let rows = snapshot_rows(
            content,
            size,
            TerminalTheme::for_palette(TerminalPalette::Dark),
        );
        assert!(rows[0].text.contains('中'));
        assert!(rows[0].text.contains("e\u{301}"));
    }

    #[test]
    fn snapshot_uses_theme_defaults() {
        let size = TerminalSize {
            columns: 2,
            rows: 1,
            cell_width: DEFAULT_CELL_WIDTH_PX,
            cell_height: DEFAULT_CELL_HEIGHT_PX,
        };
        let theme = TerminalTheme::new(
            TerminalPalette::Light,
            TerminalColor::rgb(0x1a1a1a),
            TerminalColor::rgb(0xffffff),
        );
        let term = Term::new(Config::default(), &size, VoidListener);
        let content = term.renderable_content();
        let cursor_color = snapshot_cursor_color(content.colors, theme);
        let rows = snapshot_rows(content, size, theme);
        let style = rows[0].cells[0].style;

        assert_eq!(style.foreground, theme.foreground);
        assert_eq!(style.background, theme.background);
        assert_eq!(cursor_color, theme.foreground);
    }
}
