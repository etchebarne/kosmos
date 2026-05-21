use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{
    Anchor, AnchoredPositionMode, AnyElement, App, Bounds, Context, CursorStyle, DragMoveEvent,
    Element, ElementId, ElementInputHandler, Entity, FocusHandle, GlobalElementId, HighlightStyle,
    InteractiveText, IntoElement, LayoutId, ListHorizontalSizingBehavior, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, Rgba, SharedString, Style, StyledText,
    TextLayout, TextRun, Window, anchored, canvas, deferred, div, fill, point, prelude::*, px,
    relative, rems, uniform_list,
};

use file_editor::{
    BOTTOM_SPACER_LINES, Buffer, BufferStore, EditorHoverStatus, EditorInputLayout,
    EditorLineInputLayout, EditorView, EditorViewStore, soft_wrap_enabled, virtual_list,
};
use file_tree::ActiveFileTree;
use highlight::HighlightId;
use icons::{Icon, IconName};
use syntax::{SyntaxSnapshot, SyntaxStore};
use tabs::{Tab, registry};
use theme::{ActiveTheme, SyntaxStyles, Theme};

use crate::components::input::{
    Backspace, Copy, Cut, Delete, Down, DuplicateLineDown, DuplicateLineUp, End, Enter, Home,
    KEY_CONTEXT, Left, Paste, Redo, Right, SelectAll, SelectDown, SelectLeft, SelectRight,
    SelectUp, SelectWordLeft, SelectWordRight, Undo, Up, WordLeft, WordRight,
    should_paint_text_cursor, text_cursor_bounds,
};
use crate::components::scrollbar::{self, EditorScrollMetrics, ScrollbarDrag};

use self::markdown::render_markdown;

const GUTTER_WIDTH_REM: f32 = 3.5;
const GUTTER_PADDING_REM: f32 = 0.5;
const GUTTER_FOLD_COLUMN_REM: f32 = 0.5;
const GUTTER_TOTAL_WIDTH_REM: f32 = GUTTER_WIDTH_REM + GUTTER_FOLD_COLUMN_REM;
const GUTTER_HOVER_RIGHT_SLOP_REM: f32 = 1.25;
const GUTTER_FOLD_HOVER_LEFT_REM: f32 = GUTTER_WIDTH_REM - GUTTER_PADDING_REM;
const GUTTER_FOLD_HOVER_RIGHT_SLOP_REM: f32 = 0.5;
const GUTTER_FOLD_HOVER_WIDTH_REM: f32 =
    GUTTER_TOTAL_WIDTH_REM + GUTTER_FOLD_HOVER_RIGHT_SLOP_REM - GUTTER_FOLD_HOVER_LEFT_REM;
const BODY_PADDING_LEFT_REM: f32 = 0.75;
const FONT_FAMILY: &str = "DejaVu Sans Mono";
const DEFAULT_INDENT_GUIDE_COLUMNS: usize = 4;
const TAB_SIZE_COLUMNS: usize = 4;
const MONOSPACE_CHAR_WIDTH_REM: f32 = 0.525;
const INDENT_GUIDE_WIDTH_REM: f32 = 0.0625;
/// Fixed row height. Pinning this lets `uniform_list::measure_item` return a
/// stable row height regardless of how it lays out our flex_row at
/// MinContent — otherwise the reported content size jitters between renders.
const ROW_HEIGHT_REM: f32 = 1.4;
const HOVER_DEBOUNCE: Duration = Duration::from_millis(500);
const HOVER_HIDE_DELAY: Duration = Duration::from_millis(180);

#[derive(Clone)]
struct LineHover {
    line_index: usize,
    buffer: Entity<Buffer>,
    view: Entity<EditorView>,
    root: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct VisibleIndentRow {
    index: usize,
    top: Pixels,
    bottom: Pixels,
}

#[derive(Clone, Copy)]
struct ActiveIndentGuideRun {
    column: usize,
    top: Pixels,
}

#[derive(Clone, Default)]
struct EditLineState {
    selection: Option<LineSelection>,
    cursor: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LineSelection {
    range: Range<usize>,
    includes_line_break: bool,
}

#[derive(Clone, Copy, Default)]
struct SoftWrapLineMetrics {
    content_chars: usize,
    indent_columns: usize,
}

struct EditorInputElement {
    view: Entity<EditorView>,
}

struct CursorElement {
    text_layout: TextLayout,
    cursor: usize,
    color: Rgba,
    focus_handle: FocusHandle,
}

struct SoftWrapSelectionElement {
    line: SharedString,
    text_layout: TextLayout,
    display_byte_offset: usize,
    selection: LineSelection,
    color: gpui::Hsla,
}

impl IntoElement for CursorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for CursorElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = Pixels::ZERO.into();
        style.size.height = Pixels::ZERO.into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        if !self.focus_handle.is_focused(window) || !should_paint_text_cursor(window) {
            return;
        }

        let Some(position) = self.text_layout.position_for_index(self.cursor) else {
            return;
        };
        window.paint_quad(fill(
            text_cursor_bounds(
                point(position.x, position.y),
                self.text_layout.line_height(),
                window,
            ),
            self.color,
        ));
    }
}

impl IntoElement for SoftWrapSelectionElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SoftWrapSelectionElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = Pixels::ZERO.into();
        style.size.height = Pixels::ZERO.into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        for bounds in soft_wrap_selection_extra_bounds(
            self.line.as_ref(),
            &self.text_layout,
            self.display_byte_offset,
            &self.selection,
            window,
        ) {
            window.paint_quad(fill(bounds, self.color));
        }
    }
}

impl IntoElement for EditorInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.view.read(cx).focus_handle();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.view.clone()),
            cx,
        );
    }
}

fn wire_editor_actions<T: 'static, E: gpui::InteractiveElement + 'static>(
    element: E,
    view: &Entity<EditorView>,
    cx: &mut Context<T>,
) -> E {
    let backspace = view.clone();
    let delete = view.clone();
    let enter = view.clone();
    let left = view.clone();
    let right = view.clone();
    let up = view.clone();
    let down = view.clone();
    let word_left = view.clone();
    let word_right = view.clone();
    let select_left = view.clone();
    let select_right = view.clone();
    let select_up = view.clone();
    let select_down = view.clone();
    let select_word_left = view.clone();
    let select_word_right = view.clone();
    let select_all = view.clone();
    let home = view.clone();
    let end = view.clone();
    let copy = view.clone();
    let cut = view.clone();
    let paste = view.clone();
    let undo = view.clone();
    let redo = view.clone();
    let duplicate_line_up = view.clone();
    let duplicate_line_down = view.clone();

    element
        .on_action(cx.listener(move |_, _: &Backspace, window, cx| {
            backspace.update(cx, |view, cx| view.backspace(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Delete, window, cx| {
            delete.update(cx, |view, cx| view.delete(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Enter, window, cx| {
            enter.update(cx, |view, cx| view.enter(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Left, window, cx| {
            left.update(cx, |view, cx| view.left(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Right, window, cx| {
            right.update(cx, |view, cx| view.right(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Up, window, cx| {
            up.update(cx, |view, cx| view.up(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Down, window, cx| {
            down.update(cx, |view, cx| view.down(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &WordLeft, window, cx| {
            word_left.update(cx, |view, cx| view.word_left(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &WordRight, window, cx| {
            word_right.update(cx, |view, cx| view.word_right(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectLeft, window, cx| {
            select_left.update(cx, |view, cx| view.select_left(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectRight, window, cx| {
            select_right.update(cx, |view, cx| view.select_right(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectUp, window, cx| {
            select_up.update(cx, |view, cx| view.select_up(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectDown, window, cx| {
            select_down.update(cx, |view, cx| view.select_down(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectWordLeft, window, cx| {
            select_word_left.update(cx, |view, cx| view.select_word_left(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectWordRight, window, cx| {
            select_word_right.update(cx, |view, cx| view.select_word_right(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &SelectAll, window, cx| {
            select_all.update(cx, |view, cx| view.select_all(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Home, window, cx| {
            home.update(cx, |view, cx| view.home(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &End, window, cx| {
            end.update(cx, |view, cx| view.end(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Copy, window, cx| {
            copy.update(cx, |view, cx| view.copy(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Cut, window, cx| {
            cut.update(cx, |view, cx| view.cut(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Paste, window, cx| {
            paste.update(cx, |view, cx| view.paste(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Undo, window, cx| {
            undo.update(cx, |view, cx| view.undo(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &Redo, window, cx| {
            redo.update(cx, |view, cx| view.redo(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &DuplicateLineUp, window, cx| {
            duplicate_line_up.update(cx, |view, cx| view.duplicate_line_up(window, cx));
        }))
        .on_action(cx.listener(move |_, _: &DuplicateLineDown, window, cx| {
            duplicate_line_down.update(cx, |view, cx| view.duplicate_line_down(window, cx));
        }))
}
