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
