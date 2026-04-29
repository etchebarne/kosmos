use gpui::{
    AnyElement, App, Empty, EntityId, IntoElement, Pixels, UniformListScrollHandle, div,
    prelude::*, px, rems,
};
use file_editor::VirtualListState;
use theme::ActiveTheme;

const TRACK_THICKNESS_REM: f32 = 0.625;
const MIN_THUMB_LENGTH_PX: f32 = 24.0;

/// Snapshot of one axis's scroll state. `viewport` is the visible extent,
/// `content` the total scrollable extent, `scrolled` the current offset (≥ 0,
/// growing as the user moves away from the start).
#[derive(Clone, Copy)]
pub struct AxisScrollbar {
    pub viewport: Pixels,
    pub content: Pixels,
    pub scrolled: Pixels,
}

impl AxisScrollbar {
    /// Build a scrollbar snapshot, returning `None` when the axis isn't
    /// scrollable (viewport unknown or content fits).
    pub fn new(viewport: Pixels, content: Pixels, scrolled: Pixels) -> Option<Self> {
        if viewport <= px(0.0) || content <= viewport {
            return None;
        }
        Some(Self {
            viewport,
            content,
            scrolled,
        })
    }

    pub fn max_scroll(&self) -> Pixels {
        (self.content - self.viewport).max(px(0.0))
    }

    pub fn thumb_length(&self) -> Pixels {
        if self.content <= px(0.0) {
            return self.viewport;
        }
        let ratio = self.viewport / self.content;
        (self.viewport * ratio)
            .max(px(MIN_THUMB_LENGTH_PX))
            .min(self.viewport)
    }

    pub fn thumb_position(&self) -> Pixels {
        let max_thumb_pos = (self.viewport - self.thumb_length()).max(px(0.0));
        let max_scroll = self.max_scroll();
        if max_scroll <= px(0.0) || max_thumb_pos <= px(0.0) {
            return px(0.0);
        }
        let ratio = (self.scrolled / max_scroll).clamp(0.0, 1.0);
        max_thumb_pos * ratio
    }

    pub fn scroll_for_thumb_position(&self, thumb_pos: Pixels) -> Pixels {
        let max_thumb_pos = (self.viewport - self.thumb_length()).max(px(0.0));
        if max_thumb_pos <= px(0.0) {
            return px(0.0);
        }
        let ratio = (thumb_pos / max_thumb_pos).clamp(0.0, 1.0);
        self.max_scroll() * ratio
    }

    /// Translate a mouse coordinate measured from the start of the track into
    /// the scroll offset that puts the thumb's center under the cursor.
    pub fn scroll_for_mouse_position(&self, mouse_in_track: Pixels) -> Pixels {
        let half_thumb = self.thumb_length() * 0.5;
        let target = (mouse_in_track - half_thumb).max(px(0.0));
        self.scroll_for_thumb_position(target)
    }
}

#[derive(Clone, Copy, Default)]
pub struct EditorScrollMetrics {
    pub vertical: Option<AxisScrollbar>,
    pub horizontal: Option<AxisScrollbar>,
}

impl EditorScrollMetrics {
    pub fn from_uniform(handle: &UniformListScrollHandle) -> Self {
        let state = handle.0.borrow();
        let Some(sizes) = state.last_item_size else {
            return Self::default();
        };
        let offset = state.base_handle.offset();
        Self {
            vertical: AxisScrollbar::new(sizes.item.height, sizes.contents.height, -offset.y),
            horizontal: AxisScrollbar::new(sizes.item.width, sizes.contents.width, -offset.x),
        }
    }

    pub fn from_virtual(state: &VirtualListState) -> Self {
        let viewport_h = state.viewport_size().height;
        if viewport_h <= px(0.0) {
            return Self::default();
        }
        let content_h = state.content_height();
        Self {
            vertical: AxisScrollbar::new(viewport_h, content_h, state.scroll_y()),
            // Soft-wrap mode never scrolls horizontally — lines wrap.
            horizontal: None,
        }
    }
}

/// Drag marker that identifies which axis is being scrubbed. Carries the
/// owning editor's [`EntityId`] so editors with multiple side-by-side instances
/// can ignore drag-move events that didn't originate in their own scrollbar —
/// gpui dispatches `on_drag_move` to every listener of a given drag type, not
/// just the element the drag started on.
#[derive(Clone, Copy)]
pub enum ScrollbarDrag {
    Vertical(EntityId),
    Horizontal(EntityId),
}

impl ScrollbarDrag {
    pub fn owner(self) -> EntityId {
        match self {
            Self::Vertical(id) | Self::Horizontal(id) => id,
        }
    }
}

pub fn render(metrics: EditorScrollMetrics, owner: EntityId, cx: &App) -> AnyElement {
    let theme = *cx.theme();
    let thumb_bg = gpui::Hsla::from(theme.text).opacity(0.3);
    let thumb_hover_bg = gpui::Hsla::from(theme.text).opacity(0.55);

    let mut overlays: Vec<AnyElement> = Vec::new();
    if let Some(v) = metrics.vertical {
        overlays.push(render_vertical(v, owner, thumb_bg, thumb_hover_bg));
    }
    if let Some(h) = metrics.horizontal {
        overlays.push(render_horizontal(h, owner, thumb_bg, thumb_hover_bg));
    }

    if overlays.is_empty() {
        return div().into_any_element();
    }

    div()
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .left_0()
        .children(overlays)
        .into_any_element()
}

fn render_vertical(
    metrics: AxisScrollbar,
    owner: EntityId,
    thumb_bg: gpui::Hsla,
    thumb_hover_bg: gpui::Hsla,
) -> AnyElement {
    let thumb_top = metrics.thumb_position();
    let thumb_height = metrics.thumb_length();
    div()
        .id("editor-scrollbar-vertical")
        .absolute()
        .top_0()
        .right_0()
        .bottom_0()
        .w(rems(TRACK_THICKNESS_REM))
        .child(
            div()
                .id("editor-scrollbar-vertical-thumb")
                .absolute()
                .top(thumb_top)
                .left_0()
                .right_0()
                .h(thumb_height)
                .rounded(rems(0.25))
                .bg(thumb_bg)
                .hover(move |this| this.bg(thumb_hover_bg))
                .on_drag(ScrollbarDrag::Vertical(owner), |_, _, _, cx| {
                    cx.new(|_| Empty)
                }),
        )
        .into_any_element()
}

fn render_horizontal(
    metrics: AxisScrollbar,
    owner: EntityId,
    thumb_bg: gpui::Hsla,
    thumb_hover_bg: gpui::Hsla,
) -> AnyElement {
    let thumb_left = metrics.thumb_position();
    let thumb_width = metrics.thumb_length();
    div()
        .id("editor-scrollbar-horizontal")
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(rems(TRACK_THICKNESS_REM))
        .child(
            div()
                .id("editor-scrollbar-horizontal-thumb")
                .absolute()
                .left(thumb_left)
                .top_0()
                .bottom_0()
                .w(thumb_width)
                .rounded(rems(0.25))
                .bg(thumb_bg)
                .hover(move |this| this.bg(thumb_hover_bg))
                .on_drag(ScrollbarDrag::Horizontal(owner), |_, _, _, cx| {
                    cx.new(|_| Empty)
                }),
        )
        .into_any_element()
}
