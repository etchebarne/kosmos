/// Single pass over `content` that produces the line-start byte offsets,
/// per-line character counts, and the index of the line with the most
/// characters.
fn analyze_content(content: &str) -> (Vec<usize>, Vec<usize>, usize) {
    let line_count_estimate = content.bytes().filter(|b| *b == b'\n').count() + 1;
    let mut starts = Vec::with_capacity(line_count_estimate);
    let mut chars_per_line = Vec::with_capacity(line_count_estimate);
    starts.push(0);
    let mut longest_index = 0usize;
    let mut longest_chars = 0usize;
    let mut current_line_index = 0usize;
    let mut current_chars = 0usize;
    for (byte_idx, ch) in content.char_indices() {
        if ch == '\n' {
            if current_chars > longest_chars {
                longest_chars = current_chars;
                longest_index = current_line_index;
            }
            chars_per_line.push(current_chars);
            starts.push(byte_idx + 1);
            current_line_index += 1;
            current_chars = 0;
        } else {
            current_chars += 1;
        }
    }
    chars_per_line.push(current_chars);
    if current_chars > longest_chars {
        longest_index = current_line_index;
    }
    (starts, chars_per_line, longest_index)
}

fn end_point(content: &str) -> Point {
    let mut row = 0usize;
    let mut column = 0usize;
    for byte in content.bytes() {
        if byte == b'\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn clamp_to_char_boundary(content: &str, mut offset: usize) -> usize {
    offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn clamp_range_to_char_boundaries(content: &str, range: Range<usize>) -> Range<usize> {
    let start = clamp_to_char_boundary(content, range.start);
    let end = clamp_to_char_boundary(content, range.end);
    start.min(end)..end.max(start)
}

fn point_for_offset(content: &str, offset: usize) -> Point {
    let mut row = 0usize;
    let mut column = 0usize;
    for byte in content.bytes().take(offset.min(content.len())) {
        if byte == b'\n' {
            row += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    Point { row, column }
}

fn advance_point(start: Point, text: &str) -> Point {
    let mut point = start;
    for byte in text.bytes() {
        if byte == b'\n' {
            point.row += 1;
            point.column = 0;
        } else {
            point.column += 1;
        }
    }
    point
}

fn offset_from_utf16(content: &str, offset: usize) -> usize {
    let mut utf8_offset = 0usize;
    let mut utf16_count = 0usize;
    for ch in content.chars() {
        if utf16_count >= offset {
            break;
        }
        utf16_count += ch.len_utf16();
        utf8_offset += ch.len_utf8();
    }
    utf8_offset
}

fn offset_to_utf16(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    let mut utf16_offset = 0usize;
    let mut utf8_count = 0usize;
    for ch in content.chars() {
        if utf8_count >= offset {
            break;
        }
        utf8_count += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

fn range_from_utf16(content: &str, range_utf16: &Range<usize>) -> Range<usize> {
    offset_from_utf16(content, range_utf16.start)..offset_from_utf16(content, range_utf16.end)
}

fn range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    offset_to_utf16(content, range.start)..offset_to_utf16(content, range.end)
}

fn previous_char_boundary(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[..offset]
        .char_indices()
        .next_back()
        .map_or(0, |(index, _)| index)
}

fn next_char_boundary(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[offset..]
        .char_indices()
        .nth(1)
        .map_or(content.len(), |(index, _)| offset + index)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CharacterClass {
    Whitespace,
    Word,
    Punctuation,
}

fn character_class(ch: char) -> CharacterClass {
    if ch.is_whitespace() {
        CharacterClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        CharacterClass::Word
    } else {
        CharacterClass::Punctuation
    }
}

fn char_at(content: &str, offset: usize) -> Option<char> {
    content.get(offset..)?.chars().next()
}

fn previous_word_boundary(content: &str, offset: usize) -> usize {
    let mut offset = clamp_to_char_boundary(content, offset);
    while offset > 0 {
        let previous = previous_char_boundary(content, offset);
        if char_at(content, previous).is_none_or(|ch| !ch.is_whitespace()) {
            break;
        }
        offset = previous;
    }

    let Some(class) =
        char_at(content, previous_char_boundary(content, offset)).map(character_class)
    else {
        return 0;
    };
    while offset > 0 {
        let previous = previous_char_boundary(content, offset);
        if char_at(content, previous).map(character_class) != Some(class) {
            break;
        }
        offset = previous;
    }
    offset
}

fn next_word_boundary(content: &str, offset: usize) -> usize {
    let mut offset = clamp_to_char_boundary(content, offset);
    while offset < content.len() {
        if char_at(content, offset).is_none_or(|ch| !ch.is_whitespace()) {
            break;
        }
        offset = next_char_boundary(content, offset);
    }

    let Some(class) = char_at(content, offset).map(character_class) else {
        return content.len();
    };
    while offset < content.len() {
        if char_at(content, offset).map(character_class) != Some(class) {
            break;
        }
        offset = next_char_boundary(content, offset);
    }
    offset
}

fn word_range_at_offset(content: &str, offset: usize) -> Option<Range<usize>> {
    if content.is_empty() {
        return None;
    }
    let mut target = clamp_to_char_boundary(content, offset.min(content.len()));
    if target == content.len() {
        target = previous_char_boundary(content, target);
    }
    let class = char_at(content, target).map(character_class)?;

    let mut start = target;
    while start > 0 {
        let previous = previous_char_boundary(content, start);
        if char_at(content, previous).map(character_class) != Some(class) {
            break;
        }
        start = previous;
    }

    let mut end = target;
    while end < content.len() {
        if char_at(content, end).map(character_class) != Some(class) {
            break;
        }
        let next = next_char_boundary(content, end);
        if next == end {
            break;
        }
        end = next;
    }
    (start < end).then_some(start..end)
}

fn line_start_for_offset(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end_for_offset(content: &str, offset: usize) -> usize {
    let offset = clamp_to_char_boundary(content, offset);
    content[offset..]
        .find('\n')
        .map_or(content.len(), |index| offset + index)
}

fn line_range_including_newline_for_offset(content: &str, offset: usize) -> Range<usize> {
    let offset = clamp_to_char_boundary(content, offset);
    if offset == content.len() && offset > 0 && content.ends_with('\n') {
        let previous = previous_char_boundary(content, offset);
        return previous..offset;
    }

    let start = line_start_for_offset(content, offset);
    let end = line_end_for_offset(content, offset);
    let end = if end < content.len() {
        next_char_boundary(content, end)
    } else {
        end
    };
    start..end
}

fn line_range_for_selection(content: &str, selection: &Range<usize>) -> Range<usize> {
    if selection.is_empty() {
        return line_range_including_newline_for_offset(content, selection.start);
    }

    let start = line_start_for_offset(content, selection.start);
    let end_anchor = previous_char_boundary(content, selection.end);
    start..line_range_including_newline_for_offset(content, end_anchor).end
}

fn duplicate_text_for_line_range(content: &str, range: &Range<usize>, above: bool) -> String {
    let text = content.get(range.clone()).unwrap_or_default();
    if text.is_empty() {
        return "\n".to_string();
    }

    if text.ends_with('\n') {
        return text.to_string();
    }

    if above {
        format!("{text}\n")
    } else {
        format!("\n{text}")
    }
}

fn shift_selection(selection: &SelectionSnapshot, offset: usize) -> SelectionSnapshot {
    SelectionSnapshot {
        range: selection.range.start + offset..selection.range.end + offset,
        reversed: selection.reversed,
    }
}

fn line_column_for_offset(content: &str, offset: usize) -> (usize, usize) {
    let offset = clamp_to_char_boundary(content, offset);
    let mut row = 0usize;
    let mut line_start = 0usize;
    for (index, ch) in content.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            row += 1;
            line_start = index + ch.len_utf8();
        }
    }
    (row, content[line_start..offset].chars().count())
}

fn offset_for_line_column(content: &str, target_row: usize, target_column: usize) -> usize {
    let mut row = 0usize;
    let mut line_start = 0usize;
    for (index, ch) in content.char_indices() {
        if row == target_row {
            break;
        }
        if ch == '\n' {
            row += 1;
            line_start = index + ch.len_utf8();
        }
    }
    if row < target_row {
        return content.len();
    }
    let line_end = content[line_start..]
        .find('\n')
        .map_or(content.len(), |index| line_start + index);
    content[line_start..line_end]
        .char_indices()
        .nth(target_column)
        .map_or(line_end, |(index, _)| line_start + index)
}

fn byte_for_visual_column(line: &str, target_column: usize) -> usize {
    let mut column = 0usize;
    for (index, ch) in line.char_indices() {
        if column >= target_column {
            return index;
        }
        column += if ch == '\t' { 4 - (column % 4) } else { 1 };
        if column > target_column {
            return index + ch.len_utf8();
        }
    }
    line.len()
}

