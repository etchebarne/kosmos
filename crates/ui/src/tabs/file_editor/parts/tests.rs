#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_line_copy_range_includes_trailing_newline() {
        assert_eq!(component_line_range_for_offset("one\ntwo\nthree", 5), 4..8);
    }

    #[test]
    fn component_utf16_range_counts_wide_characters() {
        assert_eq!(component_range_to_utf16("a💝b", &(0..5)), 0..3);
    }
}
