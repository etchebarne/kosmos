impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let border_color = if self.focus_handle.is_focused(window) {
            theme.border_strong
        } else {
            theme.border
        };
        let framed = self.framed;
        let height_rem = self.height_rem;
        let padding_x_rem = self.padding_x_rem;
        let padding_top_rem = self.padding_top_rem;
        let padding_bottom_rem = self.padding_bottom_rem;

        div()
            .id(SharedString::from(format!(
                "text-area:{:?}",
                cx.entity().entity_id()
            )))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::enter))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::word_left))
            .on_action(cx.listener(Self::word_right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_action(cx.listener(Self::select_word_left))
            .on_action(cx.listener(Self::select_word_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::paste))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .relative()
            .min_w(rems(13.75))
            .h(rems(height_rem))
            .px(rems(padding_x_rem))
            .pt(rems(padding_top_rem))
            .pb(rems(padding_bottom_rem))
            .flex()
            .items_start()
            .when(framed, |this| {
                this.rounded(rems(0.3125))
                    .bg(theme.bg_elevated)
                    .border_1()
                    .border_color(border_color)
            })
            .when(!framed, |this| this.bg(theme.bg_surface))
            .text_sm()
            .text_color(theme.text)
            .child(
                div()
                    .id(("text-area-scroll", cx.entity().entity_id()))
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll_handle)
                    .child(TextAreaElement { input: cx.entity() }),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .child(
                        Scrollbar::vertical(&self.scroll_handle)
                            .id(("text-area-scrollbar", cx.entity().entity_id()))
                            .scrollbar_show(ScrollbarShow::Always),
                    ),
            )
    }
}
