impl TextArea {
    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf16_count = 0;
        let mut utf8_offset = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn line_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (index, ch) in self.content.char_indices() {
            if ch == '\n' {
                ranges.push(start..index);
                start = index + ch.len_utf8();
            }
        }
        ranges.push(start..self.content.len());
        ranges
    }

    fn line_for_offset(&self, offset: usize) -> (usize, Range<usize>) {
        let ranges = self.line_ranges();
        for (line_index, range) in ranges.iter().enumerate() {
            if offset <= range.end {
                return (line_index, range.clone());
            }
        }
        let last_index = ranges.len().saturating_sub(1);
        (last_index, ranges.get(last_index).cloned().unwrap_or(0..0))
    }

    fn wrapped_line_height(line: &WrappedLine) -> usize {
        line.wrap_boundaries().len() + 1
    }

    fn visual_top_for_line(
        lines: &[WrappedLine],
        line_index: usize,
        line_height: Pixels,
    ) -> Pixels {
        lines
            .iter()
            .take(line_index)
            .fold(Pixels::ZERO, |top, line| {
                top + line_height * Self::wrapped_line_height(line)
            })
    }

    fn wrapped_segments(line: &WrappedLine) -> Vec<Range<usize>> {
        let mut start = 0;
        let mut ranges = Vec::new();
        for boundary in line.wrap_boundaries() {
            let run = &line.runs()[boundary.run_ix];
            let end = run.glyphs[boundary.glyph_ix].index;
            ranges.push(start..end);
            start = end;
        }
        ranges.push(start..line.len());
        ranges
    }

    fn viewport_text_height(&self, window: &mut Window) -> Pixels {
        let scroll_bounds = self.scroll_handle.bounds();
        let viewport_height = if scroll_bounds.size.height > Pixels::ZERO {
            scroll_bounds.size.height
        } else {
            rems(self.height_rem).to_pixels(window.rem_size())
        };
        let padding_y =
            rems(self.padding_top_rem + self.padding_bottom_rem).to_pixels(window.rem_size());

        (viewport_height - padding_y).max(window.line_height())
    }
}
