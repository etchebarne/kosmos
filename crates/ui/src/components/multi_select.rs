use std::collections::HashSet;
use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, ElementId, IntoElement, MouseButton, RenderOnce, SharedString,
    Window, deferred, div, prelude::*, rems,
};

use theme::ActiveTheme;

use crate::components::DropdownOption;

type ToggleHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;
type ChangeHandler = Rc<dyn Fn(&Vec<SharedString>, &mut Window, &mut App) + 'static>;

#[derive(IntoElement)]
pub struct MultiSelect {
    id: SharedString,
    selected: Vec<SharedString>,
    options: Vec<DropdownOption>,
    ordered: bool,
    is_open: bool,
    on_toggle: Option<ToggleHandler>,
    on_change: Option<ChangeHandler>,
}

impl MultiSelect {
    pub fn new(
        id: impl Into<SharedString>,
        selected: Vec<SharedString>,
        options: Vec<DropdownOption>,
    ) -> Self {
        Self {
            id: id.into(),
            selected,
            options,
            ordered: false,
            is_open: false,
            on_toggle: None,
            on_change: None,
        }
    }

    pub fn ordered(mut self, ordered: bool) -> Self {
        self.ordered = ordered;
        self
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

    pub fn on_change<F>(mut self, f: F) -> Self
    where
        F: Fn(&Vec<SharedString>, &mut Window, &mut App) + 'static,
    {
        self.on_change = Some(Rc::new(f));
        self
    }
}

impl RenderOnce for MultiSelect {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();

        let labels: Vec<SharedString> = self
            .selected
            .iter()
            .map(|id| {
                self.options
                    .iter()
                    .find(|o| o.id == *id)
                    .map(|o| o.label.clone())
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        let summary: SharedString = if labels.is_empty() {
            "None".into()
        } else {
            labels
                .iter()
                .map(|l| l.as_ref())
                .collect::<Vec<_>>()
                .join(", ")
                .into()
        };

        let menu = self.is_open.then(|| MultiSelectMenu {
            id: self.id.clone(),
            selected: self.selected.clone(),
            options: self.options.clone(),
            ordered: self.ordered,
            on_change: self.on_change.clone(),
        });

        let on_toggle = self.on_toggle.clone();
        div()
            .id(ElementId::Name(format!("{}-trigger", self.id).into()))
            .relative()
            .h(rems(1.75))
            .min_w(rems(13.75))
            .px_2()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .rounded(rems(0.3125))
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
            .child(div().overflow_hidden().child(summary))
            .child(div().text_color(theme.text_subtle).child("▾"))
            .children(menu)
    }
}

#[derive(IntoElement)]
struct MultiSelectMenu {
    id: SharedString,
    selected: Vec<SharedString>,
    options: Vec<DropdownOption>,
    ordered: bool,
    on_change: Option<ChangeHandler>,
}

impl RenderOnce for MultiSelectMenu {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = *cx.theme();

        let selected_set: HashSet<SharedString> = self.selected.iter().cloned().collect();
        let mut rows: Vec<(SharedString, SharedString, bool, Option<usize>)> =
            Vec::with_capacity(self.options.len());
        for (sel_idx, sel_id) in self.selected.iter().enumerate() {
            if let Some(opt) = self.options.iter().find(|o| o.id == *sel_id) {
                rows.push((opt.id.clone(), opt.label.clone(), true, Some(sel_idx)));
            }
        }
        for opt in self.options.iter() {
            if !selected_set.contains(&opt.id) {
                rows.push((opt.id.clone(), opt.label.clone(), false, None));
            }
        }

        let last_selected_idx = self.selected.len().saturating_sub(1);

        let mut items: Vec<AnyElement> = Vec::with_capacity(rows.len());
        for (row_idx, (option_id, option_label, is_selected, sel_idx)) in
            rows.into_iter().enumerate()
        {
            let item_id = ElementId::Name(format!("{}-item-{}", self.id, row_idx).into());

            let on_change_row = self.on_change.clone();
            let selected_for_row = self.selected.clone();
            let opt_id_for_row = option_id.clone();

            let mut row = div()
                .id(item_id)
                .h(rems(1.75))
                .min_w_full()
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(rems(0.25))
                .text_sm()
                .text_color(if is_selected {
                    theme.text_emphasis
                } else {
                    theme.text
                })
                .hover(move |this| this.bg(theme.bg_selected))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    if let Some(handler) = &on_change_row {
                        let mut next = selected_for_row.clone();
                        if let Some(pos) = next.iter().position(|s| *s == opt_id_for_row) {
                            next.remove(pos);
                        } else {
                            next.push(opt_id_for_row.clone());
                        }
                        handler(&next, window, cx);
                    }
                });

            row = row.child(div().flex_1().child(option_label));

            if self.ordered && is_selected {
                let sel_idx = sel_idx.expect("selected row carries its position");
                let is_first = sel_idx == 0;
                let is_last = sel_idx == last_selected_idx;

                if !is_first {
                    let on_change_up = self.on_change.clone();
                    let selected_up = self.selected.clone();
                    row = row.child(
                        div()
                            .id(ElementId::Name(
                                format!("{}-up-{}", self.id, row_idx).into(),
                            ))
                            .px_1()
                            .text_color(theme.text_subtle)
                            .hover(move |this| this.text_color(theme.text_emphasis))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(handler) = &on_change_up {
                                    let mut next = selected_up.clone();
                                    next.swap(sel_idx, sel_idx - 1);
                                    handler(&next, window, cx);
                                }
                            })
                            .child("↑"),
                    );
                }

                if !is_last {
                    let on_change_down = self.on_change.clone();
                    let selected_down = self.selected.clone();
                    row = row.child(
                        div()
                            .id(ElementId::Name(
                                format!("{}-down-{}", self.id, row_idx).into(),
                            ))
                            .px_1()
                            .text_color(theme.text_subtle)
                            .hover(move |this| this.text_color(theme.text_emphasis))
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                if let Some(handler) = &on_change_down {
                                    let mut next = selected_down.clone();
                                    next.swap(sel_idx, sel_idx + 1);
                                    handler(&next, window, cx);
                                }
                            })
                            .child("↓"),
                    );
                }
            }

            if is_selected {
                row = row.child(div().text_color(theme.accent).child("✓"));
            }

            items.push(row.into_any_element());
        }

        deferred(
            div()
                .id(ElementId::Name(format!("{}-menu", self.id).into()))
                .absolute()
                .top(rems(2.0))
                .left(rems(0.0))
                .min_w_full()
                .p_1()
                .flex()
                .flex_col()
                .gap_0p5()
                .rounded(rems(0.375))
                .border_1()
                .border_color(theme.border_strong)
                .bg(theme.bg_elevated)
                .shadow_lg()
                .block_mouse_except_scroll()
                .children(items),
        )
    }
}
