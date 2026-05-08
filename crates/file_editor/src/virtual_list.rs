//! Variable-height virtualized list element.
//!
//! gpui's stock `list` element solves the same problem but only by
//! synchronously walking every row to compute total content height
//! (`measure_all`); for a 34k-line file that's a multi-second freeze on
//! every viewport-width change. `VirtualList` sidesteps it by deriving
//! per-row heights from a pure-arithmetic closure (e.g. char-count ÷
//! chars-per-line for soft wrap) and maintaining its own cumulative-sum
//! table. The render closure is **only** invoked for the rows currently
//! on screen, so a pane-resize drag costs the visible-row work plus an
//! O(n) arithmetic refresh of the height table — both ~1ms territory
//! instead of seconds.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, DispatchPhase, Element, ElementId,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, IntoElement, IsZero, LayoutId,
    Pixels, Refineable, ScrollWheelEvent, Size, Style, StyleRefinement, Styled, Window, point,
    size,
};

/// Custom virtualized list with per-row heights driven by a closure.
///
/// `height_fn` is `(index, viewport_width, rem_size) -> Pixels`. It runs
/// once per row when the cumulative height table refreshes (on item count
/// or viewport-width change), so it must be cheap — pure arithmetic
/// against precomputed per-row inputs is the intended pattern.
pub struct VirtualList {
    id: ElementId,
    style: StyleRefinement,
    state: VirtualListState,
    item_count: usize,
    height_fn: Rc<dyn Fn(usize, Pixels, Pixels) -> Pixels>,
    render_item: Box<dyn FnMut(usize, &mut Window, &mut App) -> AnyElement>,
}

/// Cheaply cloneable handle to a [`VirtualList`]'s scroll + cumulative
/// height state. Exposed so the scrollbar component can read content
/// height + drive scroll position without a separate event channel.
#[derive(Clone, Default)]
pub struct VirtualListState(Rc<RefCell<VirtualListInner>>);

#[derive(Default)]
struct VirtualListInner {
    /// Y-axis scroll position in pixels (top of viewport).
    scroll_y: Pixels,
    /// Last viewport size we observed during layout. Used to clamp the
    /// scroll offset and feed the scrollbar its viewport dimension.
    viewport_size: Size<Pixels>,
    /// Prefix sum of per-row heights. `cumulative[i]` is the y-offset of
    /// the top of item `i`; `cumulative[count]` is total content height.
    /// Recomputed when `viewport_width` or `item_count` changes.
    cumulative: Vec<Pixels>,
    /// Per-row heights backing `cumulative`. Initialized from `height_fn`, then
    /// corrected for visible rows using GPUI's actual layout result.
    heights: Vec<Pixels>,
    cumulative_width: Option<Pixels>,
    cumulative_item_count: usize,
}

impl VirtualListState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn content_height(&self) -> Pixels {
        self.0
            .borrow()
            .cumulative
            .last()
            .copied()
            .unwrap_or(Pixels::ZERO)
    }

    pub fn viewport_size(&self) -> Size<Pixels> {
        self.0.borrow().viewport_size
    }

    pub fn scroll_y(&self) -> Pixels {
        self.0.borrow().scroll_y
    }

    pub fn max_scroll_y(&self) -> Pixels {
        let inner = self.0.borrow();
        let total = inner.cumulative.last().copied().unwrap_or(Pixels::ZERO);
        (total - inner.viewport_size.height).max(Pixels::ZERO)
    }

    pub fn visible_rows(&self) -> Vec<(usize, Pixels, Pixels)> {
        let inner = self.0.borrow();
        let item_count = inner.cumulative_item_count;
        if item_count == 0 || inner.viewport_size.height <= Pixels::ZERO {
            return Vec::new();
        }

        let scroll_y = inner.scroll_y;
        let first = inner
            .cumulative
            .partition_point(|&top| top <= scroll_y)
            .saturating_sub(1)
            .min(item_count - 1);
        let scroll_end = scroll_y + inner.viewport_size.height;
        let last = inner
            .cumulative
            .partition_point(|&top| top < scroll_end)
            .min(item_count);

        (first..last)
            .map(|index| {
                let top = inner.cumulative[index] - scroll_y;
                let bottom = inner.cumulative[index + 1] - scroll_y;
                (index, top, bottom)
            })
            .collect()
    }

    /// Set scroll position from an external driver (e.g. scrollbar drag).
    /// Clamped to the legal range.
    pub fn set_scroll_y(&self, y: Pixels) {
        let mut inner = self.0.borrow_mut();
        let total = inner.cumulative.last().copied().unwrap_or(Pixels::ZERO);
        let max = (total - inner.viewport_size.height).max(Pixels::ZERO);
        inner.scroll_y = y.max(Pixels::ZERO).min(max);
    }
}

fn update_measured_item_height(
    state: &VirtualListState,
    index: usize,
    measured_height: Pixels,
    viewport_height: Pixels,
) {
    let mut inner = state.0.borrow_mut();
    let Some(current_height) = inner.heights.get(index).copied() else {
        return;
    };
    if current_height == measured_height {
        return;
    }

    let delta = measured_height - current_height;
    inner.heights[index] = measured_height;
    for top in &mut inner.cumulative[index + 1..] {
        *top += delta;
    }

    let total = inner.cumulative.last().copied().unwrap_or(Pixels::ZERO);
    let max = (total - viewport_height).max(Pixels::ZERO);
    inner.scroll_y = inner.scroll_y.max(Pixels::ZERO).min(max);
}

pub fn virtual_list<I: Into<ElementId>>(
    id: I,
    state: VirtualListState,
    item_count: usize,
    height_fn: impl Fn(usize, Pixels, Pixels) -> Pixels + 'static,
    render_item: impl FnMut(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> VirtualList {
    VirtualList {
        id: id.into(),
        style: StyleRefinement::default(),
        state,
        item_count,
        height_fn: Rc::new(height_fn),
        render_item: Box::new(render_item),
    }
}

impl Styled for VirtualList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl IntoElement for VirtualList {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for VirtualList {
    type RequestLayoutState = Vec<AnyElement>;
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, None, cx);
        (layout_id, Vec::new())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        items: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let viewport_w = bounds.size.width;
        let viewport_h = bounds.size.height;

        let needs_refresh = {
            let inner = self.state.0.borrow();
            inner.cumulative_width != Some(viewport_w)
                || inner.cumulative_item_count != self.item_count
                || inner.cumulative.len() != self.item_count + 1
                || inner.heights.len() != self.item_count
        };
        if needs_refresh {
            let rem_size = window.rem_size();
            let mut inner = self.state.0.borrow_mut();
            inner.cumulative.clear();
            inner.heights.clear();
            inner.cumulative.reserve(self.item_count + 1);
            inner.heights.reserve(self.item_count);
            inner.cumulative.push(Pixels::ZERO);
            let mut acc = Pixels::ZERO;
            for i in 0..self.item_count {
                let height = (self.height_fn)(i, viewport_w, rem_size);
                inner.heights.push(height);
                acc += height;
                inner.cumulative.push(acc);
            }
            inner.cumulative_width = Some(viewport_w);
            inner.cumulative_item_count = self.item_count;
        }

        // Update viewport + clamp scroll to the legal range now that we
        // know the latest content/viewport sizes.
        {
            let mut inner = self.state.0.borrow_mut();
            inner.viewport_size = bounds.size;
            let total = inner.cumulative.last().copied().unwrap_or(Pixels::ZERO);
            let max = (total - viewport_h).max(Pixels::ZERO);
            inner.scroll_y = inner.scroll_y.max(Pixels::ZERO).min(max);
        }

        let mut i = {
            let inner = self.state.0.borrow();
            if self.item_count == 0 {
                0
            } else {
                inner
                    .cumulative
                    .partition_point(|&p| p <= inner.scroll_y)
                    .saturating_sub(1)
                    .min(self.item_count - 1)
            }
        };

        while i < self.item_count {
            let row_top = {
                let inner = self.state.0.borrow();
                inner.cumulative[i] - inner.scroll_y
            };
            if row_top >= viewport_h {
                break;
            }

            let mut element = (self.render_item)(i, window, cx);
            let measured = element.layout_as_root(
                size(
                    AvailableSpace::Definite(viewport_w),
                    AvailableSpace::MinContent,
                ),
                window,
                cx,
            );
            update_measured_item_height(&self.state, i, measured.height, viewport_h);

            let item_top = {
                let inner = self.state.0.borrow();
                bounds.origin.y + inner.cumulative[i] - inner.scroll_y
            };
            element.layout_as_root(
                size(
                    AvailableSpace::Definite(viewport_w),
                    AvailableSpace::Definite(measured.height),
                ),
                window,
                cx,
            );
            element.prepaint_at(point(bounds.origin.x, item_top), window, cx);
            items.push(element);
            i += 1;
        }

        // Hitbox for routing scroll-wheel events to our scroll state.
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        let state = self.state.clone();
        let line_height = window.line_height();
        let view_id = window.current_view();
        let hb = hitbox.clone();
        window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || !hb.is_hovered(window) {
                return;
            }
            let delta = event.delta.pixel_delta(line_height);
            if delta.y.is_zero() {
                return;
            }
            let mut inner = state.0.borrow_mut();
            let total = inner.cumulative.last().copied().unwrap_or(Pixels::ZERO);
            let max = (total - inner.viewport_size.height).max(Pixels::ZERO);
            let next = (inner.scroll_y - delta.y).max(Pixels::ZERO).min(max);
            if next != inner.scroll_y {
                inner.scroll_y = next;
                cx.notify(view_id);
            }
        });

        hitbox
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        items: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mask = ContentMask { bounds };
        window.with_content_mask(Some(mask), |window| {
            for element in items.iter_mut() {
                element.paint(window, cx);
            }
        });
    }
}
