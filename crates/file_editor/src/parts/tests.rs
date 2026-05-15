#[cfg(test)]
mod tests {
    use super::*;

    fn change(
        old_position: usize,
        old_text: &str,
        new_position: usize,
        new_text: &str,
    ) -> TextChange {
        TextChange {
            old_position,
            old_text: old_text.to_string(),
            new_position,
            new_text: new_text.to_string(),
        }
    }

    fn selection(range: Range<usize>) -> SelectionSnapshot {
        SelectionSnapshot {
            range,
            reversed: false,
        }
    }

    #[test]
    fn compresses_adjacent_insertions() {
        assert_eq!(
            compress_consecutive_text_changes(&[change(0, "", 0, "a")], &[change(1, "", 1, "b")]),
            vec![change(0, "", 0, "ab")]
        );
    }

    #[test]
    fn compresses_repeated_backspace_deletions() {
        assert_eq!(
            compress_consecutive_text_changes(&[change(2, "c", 2, "")], &[change(1, "b", 1, "")]),
            vec![change(1, "bc", 1, "")]
        );
    }

    #[test]
    fn compresses_repeated_forward_deletions() {
        assert_eq!(
            compress_consecutive_text_changes(&[change(0, "a", 0, "")], &[change(0, "b", 0, "")]),
            vec![change(0, "ab", 0, "")]
        );
    }

    #[test]
    fn edit_stack_element_appends_like_open_monaco_stack_element() {
        let mut element = EditStackElement::new(
            change(0, "", 0, "a"),
            selection(0..0),
            selection(1..1),
            0,
            1,
        );

        element.append(change(1, "", 1, "b"), selection(2..2), 2);

        assert_eq!(element.changes, vec![change(0, "", 0, "ab")]);
        assert_eq!(element.before_selection, selection(0..0));
        assert_eq!(element.after_selection, selection(2..2));
        assert_eq!(element.before_revision, 0);
        assert_eq!(element.after_revision, 2);
    }

    #[test]
    fn groups_only_adjacent_single_character_typing_and_deleting() {
        assert!(should_group_edit("abc", &(1..1), "x"));
        assert!(should_group_edit("abc", &(1..2), ""));
        assert!(!should_group_edit("abc", &(1..1), "xy"));
        assert!(!should_group_edit("abc", &(1..1), "\n"));
        assert!(!should_group_edit("abc", &(1..2), "x"));
    }

    #[test]
    fn line_range_for_empty_selection_includes_newline() {
        assert_eq!(line_range_for_selection("one\ntwo\n", &(1..1)), 0..4);
        assert_eq!(line_range_for_selection("one\ntwo", &(5..5)), 4..7);
        assert_eq!(line_range_for_selection("one\n", &(4..4)), 3..4);
    }

    #[test]
    fn line_range_for_selection_covers_touched_lines() {
        assert_eq!(line_range_for_selection("one\ntwo\nthree", &(1..6)), 0..8);
        assert_eq!(line_range_for_selection("one\ntwo\nthree", &(0..4)), 0..4);
    }

    #[test]
    fn duplicate_text_adds_missing_line_break_at_file_edges() {
        assert_eq!(
            duplicate_text_for_line_range("one", &(0..3), false),
            "\none"
        );
        assert_eq!(duplicate_text_for_line_range("one", &(0..3), true), "one\n");
        assert_eq!(
            duplicate_text_for_line_range("one\n", &(0..4), false),
            "one\n"
        );
    }
}
