use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, MouseButton, RenderOnce, SharedString,
    Window, deferred, div, prelude::*, px,
};

use theme::ActiveTheme;

type ToggleHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type SelectHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App) + 'static>;

#[derive(Clone)]
pub struct DropdownOption {
    pub id: SharedString,
    pub label: SharedString,
}

impl DropdownOption {
    pub fn new(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

#[derive(IntoElement)]
pub struct Dropdown {
    id: SharedString,
    value: SharedString,
    options: Vec<DropdownOption>,
    is_open: bool,
    on_toggle: Option<ToggleHandler>,
    on_select: Option<SelectHandler>,
}

impl Dropdown {
    pub fn new(
        id: impl Into<SharedString>,
        value: impl Into<SharedString>,
        options: Vec<DropdownOption>,
    ) -> Self {
        Self {
            id: id.into(),
            value: value.into(),
            options,
            is_open: false,
            on_toggle: None,
            on_select: None,
        }
    }

    pub fn open(mut self, is_open: bool) -> Self {
        self.is_open = is_open;
        self
    }

    pub fn on_toggle<F>(mut self, f: F) -> Self
    where
        F: Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    pub fn on_select<F>(mut self, f: F) -> Self
    where
        F: Fn(&SharedString, &mut Window, &mut App) + 'static,
    {
        self.on_select = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for Dropdown {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let label = self
            .options
            .iter()
            .find(|o| o.id == self.value)
            .map(|o| o.label.clone())
            .unwrap_or_default();

        let menu = self.is_open.then(|| {
            render_menu(
                self.id.clone(),
                self.options.clone(),
                self.on_select.clone(),
                self.on_toggle.clone(),
            )
        });

        let on_toggle = self.on_toggle.clone();
        div()
            .id(ElementId::Name(format!("{}-trigger", self.id).into()))
            .relative()
            .h(px(28.0))
            .min_w(px(180.0))
            .px_2()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded(px(5.0))
            .bg(theme.bg_elevated)
            .border_1()
            .border_color(if self.is_open {
                theme.border_strong
            } else {
                theme.border
            })
            .text_sm()
            .text_color(theme.text)
            .hover(move |this| this.bg(theme.bg_hover))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |event, window, cx| {
                cx.stop_propagation();
                if let Some(handler) = &on_toggle {
                    handler(event, window, cx);
                }
            })
            .child(div().child(label))
            .child(div().text_color(theme.text_subtle).child("▾"))
            .children(menu)
    }
}

fn render_menu(
    id: SharedString,
    options: Vec<DropdownOption>,
    on_select: Option<SelectHandler>,
    on_toggle: Option<ToggleHandler>,
) -> impl IntoElement {
    DropdownMenu {
        id,
        options,
        on_select,
        on_toggle,
    }
}

#[derive(IntoElement)]
struct DropdownMenu {
    id: SharedString,
    options: Vec<DropdownOption>,
    on_select: Option<SelectHandler>,
    on_toggle: Option<ToggleHandler>,
}

impl RenderOnce for DropdownMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();
        let mut items: Vec<AnyElement> = Vec::with_capacity(self.options.len());
        for (index, option) in self.options.into_iter().enumerate() {
            let item_id =
                ElementId::Name(format!("{}-item-{}", self.id, index).into());
            let option_id = option.id.clone();
            let on_select = self.on_select.clone();
            let on_toggle = self.on_toggle.clone();
            items.push(
                div()
                    .id(item_id)
                    .h(px(28.0))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded(px(4.0))
                    .text_sm()
                    .text_color(theme.text)
                    .hover(move |this| {
                        this.bg(theme.bg_selected).text_color(theme.text_emphasis)
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |event, window, cx| {
                        cx.stop_propagation();
                        if let Some(handler) = &on_select {
                            handler(&option_id, window, cx);
                        }
                        if let Some(handler) = &on_toggle {
                            handler(event, window, cx);
                        }
                    })
                    .child(option.label)
                    .into_any_element(),
            );
        }

        deferred(
            div()
                .id(ElementId::Name(format!("{}-menu", self.id).into()))
                .absolute()
                .top(px(32.0))
                .left(px(0.0))
                .min_w_full()
                .p_1()
                .flex()
                .flex_col()
                .gap_1()
                .rounded(px(6.0))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_elevated)
                .shadow_lg()
                .block_mouse_except_scroll()
                .children(items),
        )
    }
}
