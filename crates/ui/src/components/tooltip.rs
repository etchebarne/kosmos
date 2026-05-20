use gpui::{
    AnchoredPositionMode, AnyElement, App, Corner, ElementId, Global, IntoElement, RenderOnce,
    SharedString, Window, anchored, deferred, div, prelude::*, rems,
};
use std::cell::RefCell;
use theme::ActiveTheme;

thread_local! {
    static TOOLTIP_NAMESPACE: RefCell<Vec<SharedString>> = const { RefCell::new(Vec::new()) };
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum TooltipPosition {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

#[derive(IntoElement)]
pub struct Tooltip {
    id: SharedString,
    label: SharedString,
    child: AnyElement,
    position: TooltipPosition,
    disabled: bool,
}

#[derive(Default)]
struct TooltipState {
    active: Option<SharedString>,
}

impl Global for TooltipState {}

impl Tooltip {
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        child: impl IntoElement,
    ) -> Self {
        let id = scoped_tooltip_id(id.into());
        Self {
            id,
            label: label.into(),
            child: child.into_any_element(),
            position: TooltipPosition::default(),
            disabled: false,
        }
    }

    pub fn position(mut self, position: TooltipPosition) -> Self {
        self.position = position;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

pub fn with_tooltip_namespace<R>(namespace: impl Into<SharedString>, f: impl FnOnce() -> R) -> R {
    let _guard = TooltipNamespaceGuard::new(namespace.into());
    f()
}

struct TooltipNamespaceGuard;

impl TooltipNamespaceGuard {
    fn new(namespace: SharedString) -> Self {
        TOOLTIP_NAMESPACE.with(|stack| stack.borrow_mut().push(namespace));
        Self
    }
}

impl Drop for TooltipNamespaceGuard {
    fn drop(&mut self) {
        TOOLTIP_NAMESPACE.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

fn scoped_tooltip_id(id: SharedString) -> SharedString {
    TOOLTIP_NAMESPACE.with(|stack| {
        stack
            .borrow()
            .last()
            .map(|namespace| SharedString::from(format!("{namespace}:{id}")))
            .unwrap_or(id)
    })
}

impl RenderOnce for Tooltip {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let is_active = cx
            .try_global::<TooltipState>()
            .and_then(|state| state.active.as_ref())
            .is_some_and(|active| active == &self.id);
        let id = self.id.clone();
        let tooltip_id = ElementId::Name(format!("{}-tooltip", self.id).into());
        let gap = rems(0.375);
        let tooltip_width = rems(11.0);

        div()
            .id(ElementId::Name(self.id))
            .relative()
            .flex()
            .on_hover(move |hovered, window, cx| {
                let state = cx.default_global::<TooltipState>();
                if *hovered {
                    state.active = Some(id.clone());
                } else if state.active.as_ref() == Some(&id) {
                    state.active = None;
                }
                window.refresh();
            })
            .child(self.child)
            .when(
                !self.disabled && !self.label.is_empty() && is_active,
                |this| {
                    this.child(
                        div()
                            .id(tooltip_id)
                            .absolute()
                            .when(self.position == TooltipPosition::Top, |this| {
                                this.bottom(gpui::relative(1.0))
                                    .mb(gap)
                                    .left(gpui::relative(0.5))
                                    .ml(-tooltip_width / 2.)
                            })
                            .when(self.position == TooltipPosition::Bottom, |this| {
                                this.top(gpui::relative(1.0))
                                    .mt(gap)
                                    .left(gpui::relative(0.5))
                                    .ml(-tooltip_width / 2.)
                            })
                            .when(self.position == TooltipPosition::Left, |this| {
                                this.right(gpui::relative(1.0)).mr(gap).top(rems(0.0))
                            })
                            .when(self.position == TooltipPosition::Right, |this| {
                                this.left(gpui::relative(1.0)).ml(gap).top(rems(0.0))
                            })
                            .child(
                                deferred(
                                    anchored()
                                        .position_mode(AnchoredPositionMode::Local)
                                        .anchor(Corner::TopLeft)
                                        .snap_to_window()
                                        .child(
                                            div()
                                                .w(tooltip_width)
                                                .px_2()
                                                .py_1()
                                                .rounded(rems(0.3125))
                                                .border_1()
                                                .border_color(theme.border_strong)
                                                .bg(theme.bg_elevated)
                                                .shadow_lg()
                                                .text_xs()
                                                .text_center()
                                                .text_color(theme.text_emphasis)
                                                .whitespace_nowrap()
                                                .child(self.label),
                                        ),
                                )
                                .with_priority(3),
                            ),
                    )
                },
            )
    }
}
