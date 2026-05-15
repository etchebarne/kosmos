struct TextAreaElement {
    input: Entity<TextArea>,
}

struct TextAreaPrepaintState {
    lines: Vec<WrappedLine>,
    cursor: Option<PaintQuad>,
    selections: Vec<PaintQuad>,
    target_scroll_top: Option<Pixels>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextAreaElement {
    type RequestLayoutState = ();
    type PrepaintState = TextAreaPrepaintState;

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
        let line_count = self.input.read(cx).last_visual_line_count.max(3);
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * line_count).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = *cx.theme();
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let ranges = input.line_ranges();
        let wrap_width = Some(bounds.size.width);

        let mut lines = Vec::new();
        for range in &ranges {
            let (display_text, text_color) = if content.is_empty() {
                (input.placeholder.clone(), hsla(0., 0., 0.55, 0.6))
            } else {
                (
                    SharedString::from(content[range.clone()].to_string()),
                    style.color,
                )
            };
            let run = TextRun {
                len: display_text.len(),
                font: style.font(),
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            if let Ok(mut wrapped) =
                window
                    .text_system()
                    .shape_text(display_text, font_size, &[run], wrap_width, None)
            {
                lines.extend(wrapped.drain(..));
            }
            if content.is_empty() {
                break;
            }
        }

        let mut selections = Vec::new();
        if !selected_range.is_empty() {
            for (hard_line_index, line_range) in ranges.iter().enumerate() {
                let Some(line) = lines.get(hard_line_index) else {
                    continue;
                };
                let visual_top =
                    TextArea::visual_top_for_line(&lines, hard_line_index, line_height);
                let segments = TextArea::wrapped_segments(line);
                let last_segment_index = segments.len().saturating_sub(1);
                for (segment_index, segment) in segments.into_iter().enumerate() {
                    let segment_start = line_range.start + segment.start;
                    let segment_end = line_range.start + segment.end;
                    let start = selected_range.start.max(segment_start);
                    let end = selected_range.end.min(segment_end);
                    if start < end {
                        let start_x = if start == segment_start {
                            Pixels::ZERO
                        } else {
                            line.position_for_index(start - line_range.start, line_height)
                                .map_or(Pixels::ZERO, |position| position.x)
                        };
                        let end_x = if end == segment_end && segment_index < last_segment_index {
                            bounds.size.width
                        } else {
                            line.position_for_index(end - line_range.start, line_height)
                                .map_or(bounds.size.width, |position| position.x)
                        };
                        let segment_top = visual_top + line_height * segment_index;
                        selections.push(fill(
                            Bounds::from_corners(
                                point(bounds.left() + start_x, bounds.top() + segment_top),
                                point(
                                    bounds.left() + end_x,
                                    bounds.top() + segment_top + line_height,
                                ),
                            ),
                            gpui::Hsla::from(theme.accent).opacity(0.35),
                        ));
                    }
                }
            }
        }

        let cursor_quad = if selected_range.is_empty() {
            let (line_index, line_range) = input.line_for_offset(cursor);
            let line = lines.get(line_index);
            let visual_top = TextArea::visual_top_for_line(&lines, line_index, line_height);
            line.and_then(|line| {
                line.position_for_index(cursor.saturating_sub(line_range.start), line_height)
                    .map(|position| {
                        fill(
                            text_cursor_bounds(
                                point(
                                    bounds.left() + position.x,
                                    bounds.top() + visual_top + position.y,
                                ),
                                line_height,
                                window,
                            ),
                            theme.text,
                        )
                    })
            })
        } else {
            None
        };
        let target_scroll_top = input
            .pending_reveal_cursor
            .then(|| input.scroll_top_to_reveal_cursor(&lines, window));

        TextAreaPrepaintState {
            lines,
            cursor: cursor_quad,
            selections,
            target_scroll_top,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        for selection in prepaint.selections.drain(..) {
            window.paint_quad(selection);
        }
        let line_height = window.line_height();
        let mut visual_line_index = 0;
        for line in prepaint.lines.iter() {
            line.paint(
                point(
                    bounds.left(),
                    bounds.top() + line_height * visual_line_index,
                ),
                line_height,
                TextAlign::default(),
                Some(bounds),
                window,
                cx,
            )
            .ok();
            visual_line_index += TextArea::wrapped_line_height(line);
        }
        if focus_handle.is_focused(window)
            && should_paint_text_cursor(window)
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }
        let lines = std::mem::take(&mut prepaint.lines);
        let target_scroll_top = prepaint.target_scroll_top.take();
        let mut refresh = false;
        self.input.update(cx, |input, _cx| {
            input.last_visual_line_count = visual_line_index;
            input.last_lines = lines;
            input.last_bounds = Some(bounds);
            if let Some(target_scroll_top) = target_scroll_top {
                input.pending_reveal_cursor = false;
                if target_scroll_top != -input.scroll_handle.offset().y {
                    input
                        .scroll_handle
                        .set_offset(point(Pixels::ZERO, -target_scroll_top));
                    refresh = true;
                }
            }
        });
        if refresh {
            cx.refresh_windows();
        }
    }
}
