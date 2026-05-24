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
