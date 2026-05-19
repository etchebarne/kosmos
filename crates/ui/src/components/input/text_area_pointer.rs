impl TextArea {
    fn index_for_mouse_position(&self, position: Point<Pixels>, window: &mut Window) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        if self.last_bounds.is_none() {
            return self.content.len();
        }
        let line_height = window.line_height();
        let scroll_bounds = self.scroll_handle.bounds();
        let padding_x = rems(self.padding_x_rem).to_pixels(window.rem_size());
        let padding_top = rems(self.padding_top_rem).to_pixels(window.rem_size());
        let scroll_offset = self.scroll_handle.offset();
        let x = position.x - scroll_bounds.left() - padding_x - scroll_offset.x;
        let y =
            (position.y - scroll_bounds.top() - padding_top - scroll_offset.y).max(Pixels::ZERO);
        let ranges = self.line_ranges();
        let mut visual_top = Pixels::ZERO;
        for (line_index, line) in self.last_lines.iter().enumerate() {
            let visual_height = line_height * Self::wrapped_line_height(line);
            if y <= visual_top + visual_height {
                let local = point(x, y - visual_top);
                let offset = line
                    .closest_index_for_position(local, line_height)
                    .unwrap_or_else(|offset| offset)
                    .min(line.len());
                return ranges
                    .get(line_index)
                    .map_or(self.content.len(), |range| range.start + offset);
            }
            visual_top += visual_height;
        }
        self.content.len()
    }

    fn vertical_target_offset(&self, direction: isize, window: &mut Window) -> usize {
        if self.content.is_empty() || self.last_lines.is_empty() {
            return 0;
        }

        let ranges = self.line_ranges();
        if ranges.len() != self.last_lines.len() {
            return self.cursor_offset().min(self.content.len());
        }

        let line_height = window.line_height();
        let cursor = self.cursor_offset().min(self.content.len());
        let (line_index, line_range) = self.line_for_offset(cursor);
        let Some(line) = self.last_lines.get(line_index) else {
            return cursor;
        };
        let Some(position) = line.position_for_index(
            cursor.saturating_sub(line_range.start).min(line.len()),
            line_height,
        ) else {
            return cursor;
        };

        let local_visual_line = (position.y / line_height).floor() as usize;
        let current_visual_line = self
            .last_lines
            .iter()
            .take(line_index)
            .fold(local_visual_line, |line_count, line| {
                line_count + Self::wrapped_line_height(line)
            });
        let total_visual_lines = self
            .last_lines
            .iter()
            .map(Self::wrapped_line_height)
            .sum::<usize>();
        let target_visual_line = if direction < 0 {
            current_visual_line.saturating_sub(direction.unsigned_abs())
        } else {
            (current_visual_line + direction as usize).min(total_visual_lines.saturating_sub(1))
        };

        let mut visual_line_start = 0;
        for (line_index, line) in self.last_lines.iter().enumerate() {
            let visual_line_count = Self::wrapped_line_height(line);
            if target_visual_line < visual_line_start + visual_line_count {
                let local_visual_line = target_visual_line - visual_line_start;
                let local_offset = line
                    .closest_index_for_position(
                        point(position.x, line_height * local_visual_line),
                        line_height,
                    )
                    .unwrap_or_else(|offset| offset)
                    .min(line.len());
                return ranges
                    .get(line_index)
                    .map_or(self.content.len(), |range| range.start + local_offset)
                    .min(self.content.len());
            }
            visual_line_start += visual_line_count;
        }

        self.content.len()
    }

    fn reveal_cursor(&mut self, _: &mut Window) {
        self.pending_reveal_cursor = true;
    }

    fn scroll_top_to_reveal_cursor(&self, lines: &[WrappedLine], window: &mut Window) -> Pixels {
        let visual_line_count = lines.iter().map(Self::wrapped_line_height).sum::<usize>();
        let cursor = self.cursor_offset();
        let (line_index, line_range) = self.line_for_offset(cursor);
        let line_height = window.line_height();
        let viewport_height = self.viewport_text_height(window);
        let content_height = line_height * visual_line_count;
        let max_scroll_top = (content_height - viewport_height).max(Pixels::ZERO);
        let current_scroll_top =
            (-self.scroll_handle.offset().y).clamp(Pixels::ZERO, max_scroll_top);
        let mut cursor_top = Self::visual_top_for_line(lines, line_index, line_height);
        if let Some(line) = lines.get(line_index)
            && let Some(position) =
                line.position_for_index(cursor.saturating_sub(line_range.start), line_height)
        {
            cursor_top += position.y;
        }
        let cursor_bottom = cursor_top + line_height;
        let mut target_scroll_top = current_scroll_top;

        if cursor_top < current_scroll_top {
            target_scroll_top = cursor_top;
        } else if cursor_bottom > current_scroll_top + viewport_height {
            target_scroll_top = cursor_bottom - viewport_height;
        }

        target_scroll_top = target_scroll_top.clamp(Pixels::ZERO, max_scroll_top);
        target_scroll_top
    }
}
