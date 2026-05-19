#[cfg(test)]
mod tests {
    use super::markdown::{MarkdownBlock, code_block_language_id, parse_markdown};
    use super::*;

    #[test]
    fn parses_fenced_code_blocks() {
        let blocks = parse_markdown("```rust\nfn main() {}\n```");

        let MarkdownBlock::CodeBlock { language, text } = &blocks[0] else {
            panic!("expected code block");
        };
        assert_eq!(language.as_deref(), Some("rust"));
        assert_eq!(text, "fn main() {}\n");
    }

    #[test]
    fn parses_inline_code_and_emphasis() {
        let blocks = parse_markdown("Use `hover` for *details*.");

        let MarkdownBlock::Paragraph(inline) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(inline.text, "Use hover for details.");
        assert!(inline.ranges.iter().any(|(_, style)| style.code));
        assert!(inline.ranges.iter().any(|(_, style)| style.emphasis));
    }

    #[test]
    fn normalizes_code_block_language_aliases() {
        assert_eq!(
            code_block_language_id("tsx").map(|id| id.to_string()),
            Some("typescriptreact".to_string())
        );
        assert_eq!(
            code_block_language_id("bash").map(|id| id.to_string()),
            Some("shellscript".to_string())
        );
        assert_eq!(
            code_block_language_id("rust ignore").map(|id| id.to_string()),
            Some("rust".to_string())
        );
    }

    #[test]
    fn selection_visual_columns_include_spaces_and_tabs() {
        assert_eq!(
            selection_visual_columns(
                "\tlet value",
                &LineSelection {
                    range: 0..5,
                    includes_line_break: false,
                },
            ),
            (0, 8)
        );
    }

    #[test]
    fn selection_visual_columns_include_line_break_gap() {
        assert_eq!(
            selection_visual_columns(
                "    ",
                &LineSelection {
                    range: 0..4,
                    includes_line_break: true,
                },
            ),
            (0, 5)
        );
    }

    #[test]
    fn symbol_range_covers_entire_identifier() {
        assert_eq!(symbol_range_at("let declaration_name = 1", 6), Some(4..20));
        assert_eq!(symbol_range_at("let declaration_name = 1", 19), Some(4..20));
    }

    #[test]
    fn symbol_range_ignores_whitespace() {
        assert_eq!(symbol_range_at("let value = 1", 3), None);
    }

    #[test]
    fn symbol_range_ignores_punctuation() {
        assert_eq!(symbol_range_at("call(value);", 4), None);
        assert_eq!(symbol_range_at("call(value);", 10), None);
        assert_eq!(symbol_range_at("foo.bar", 3), None);
        assert_eq!(symbol_range_at("a / b", 2), None);
    }

    #[test]
    fn symbol_range_covers_entire_double_quoted_string() {
        assert_eq!(
            symbol_range_at("let value = \"hello world\";", 13),
            Some(12..25)
        );
        assert_eq!(
            symbol_range_at("let value = \"hello world\";", 18),
            Some(12..25)
        );
        assert_eq!(
            symbol_range_at("let value = \"hello world\";", 24),
            Some(12..25)
        );
    }

    #[test]
    fn symbol_range_keeps_escaped_quotes_inside_string() {
        assert_eq!(
            symbol_range_at(r#"let value = "hello \"world\"";"#, 24),
            Some(12..29)
        );
        assert_eq!(
            symbol_range_at(r#"let value = "hello \"world\"";"#, 20),
            Some(12..29)
        );
    }

    #[test]
    fn line_highlights_combines_syntax_and_hover_source() {
        let theme = Theme::dark();
        let highlights = line_highlights(
            10,
            vec![(0..10, HighlightId::Variable)],
            &theme.syntax,
            Some(4..8),
            None,
            theme,
        );

        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0].0, 0..4);
        assert_eq!(highlights[1].0, 4..8);
        assert_eq!(highlights[2].0, 8..10);
        assert_eq!(
            highlights[1].1.background_color,
            Some(theme.bg_hover_strong.into())
        );
        assert_eq!(highlights[1].1.color, highlights[0].1.color);
    }

    #[test]
    fn line_highlights_supports_hover_source_without_syntax() {
        let theme = Theme::dark();
        let highlights = line_highlights(10, Vec::new(), &theme.syntax, Some(2..5), None, theme);

        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].0, 2..5);
        assert_eq!(
            highlights[0].1.background_color,
            Some(theme.bg_hover_strong.into())
        );
    }

    #[test]
    fn line_highlights_supports_selection_without_syntax() {
        let theme = Theme::dark();
        let highlights = line_highlights(10, Vec::new(), &theme.syntax, None, Some(2..5), theme);

        assert_eq!(highlights.len(), 1);
        assert_eq!(highlights[0].0, 2..5);
        assert_eq!(
            highlights[0].1.background_color,
            Some(gpui::Hsla::from(theme.accent).opacity(0.35))
        );
    }

    #[test]
    fn hover_popup_only_renders_after_lsp_result() {
        assert!(!hover_status_has_popup(&EditorHoverStatus::Loading));
        assert!(!hover_status_has_popup(&EditorHoverStatus::Empty));
        assert!(hover_status_has_popup(&EditorHoverStatus::Ready(
            "details".to_string()
        )));
        assert!(hover_status_has_popup(&EditorHoverStatus::Error(
            "failed".to_string()
        )));
    }

    #[test]
    fn indent_guides_follow_four_column_indents() {
        assert_eq!(indent_guide_columns(0, 4), Vec::<usize>::new());
        assert_eq!(indent_guide_columns(3, 4), Vec::<usize>::new());
        assert_eq!(indent_guide_columns(4, 4), vec![0]);
        assert_eq!(indent_guide_columns(8, 4), vec![0, 4]);
    }

    #[test]
    fn indent_guides_treat_tabs_as_tab_stops() {
        assert_eq!(indentation_columns("\tlet value = 1;"), Some(4));
        assert_eq!(indentation_columns("\t\tlet value = 1;"), Some(8));
        assert_eq!(indentation_columns("  \tlet value = 1;"), Some(4));
    }

    #[test]
    fn foldable_lines_follow_deeper_content() {
        let indents = [Some(0), Some(4), Some(8), Some(4), Some(0)];

        assert_eq!(
            foldable_lines_for_indents(&indents),
            vec![true, true, false, false, false]
        );
    }

    #[test]
    fn foldable_lines_skip_blank_lines() {
        let indents = [Some(0), None, Some(4), Some(0)];

        assert_eq!(
            foldable_lines_for_indents(&indents),
            vec![true, false, false, false]
        );
    }

    #[test]
    fn visible_lines_skip_folded_descendants() {
        let indents = [Some(0), Some(4), Some(8), Some(4), Some(0)];
        let foldable = foldable_lines_for_indents(&indents);
        let folded = HashSet::from([0usize]);

        assert_eq!(
            visible_lines_for_indents(&indents, &foldable, &folded),
            vec![0, 4]
        );
    }

    #[test]
    fn visible_lines_keep_blank_lines_inside_fold() {
        let indents = [Some(0), Some(4), None, Some(4), Some(0)];
        let foldable = foldable_lines_for_indents(&indents);
        let folded = HashSet::from([0usize]);

        assert_eq!(
            visible_lines_for_indents(&indents, &foldable, &folded),
            vec![0, 4]
        );
    }

    #[test]
    fn indent_guides_merge_adjacent_rows_into_runs() {
        let rows: &[(usize, &[usize])] = &[
            (0, &[0]),
            (1, &[0, 2]),
            (2, &[0, 2]),
            (3, &[0]),
            (4, &[]),
            (5, &[0]),
        ];

        assert_eq!(
            indent_guide_runs(rows),
            vec![(0, 0, 4), (0, 5, 6), (2, 1, 3)]
        );
    }

    #[test]
    fn soft_wrap_height_uses_text_area_width_after_gutter_padding() {
        let rem_size = px(16.0);
        let height = soft_wrap_row_height(
            SoftWrapLineMetrics {
                content_chars: 40,
                indent_columns: 0,
            },
            rems(24.0).to_pixels(rem_size),
            rem_size,
        );

        assert_eq!(height, rems(ROW_HEIGHT_REM).to_pixels(rem_size) * 2.0);
    }

    #[test]
    fn soft_wrap_height_accounts_for_hanging_indent_width() {
        let rem_size = px(16.0);
        let height = soft_wrap_row_height(
            SoftWrapLineMetrics {
                content_chars: 20,
                indent_columns: 8,
            },
            rems(18.0).to_pixels(rem_size),
            rem_size,
        );

        assert_eq!(height, rems(ROW_HEIGHT_REM).to_pixels(rem_size) * 2.0);
    }

    #[test]
    fn display_ranges_are_shifted_after_stripping_indent() {
        assert_eq!(shift_range_for_display(4..10, 4, 12), Some(0..6));
        assert_eq!(shift_range_for_display(0..4, 4, 12), None);
        assert_eq!(shift_range_for_display(2..8, 4, 12), Some(0..4));
    }

    #[test]
    fn indent_guides_infer_two_space_indents() {
        let indents = [Some(0), Some(2), Some(4), Some(2), Some(0)];

        assert_eq!(infer_indent_width(&indents), 2);
        assert_eq!(
            indent_guides_for_indents(&indents),
            vec![vec![], vec![0], vec![0, 2], vec![0], vec![]]
        );
    }

    #[test]
    fn indent_guides_continue_through_blank_lines() {
        let indents = [Some(0), Some(2), None, Some(2), Some(0)];

        assert_eq!(
            indent_guides_for_indents(&indents),
            vec![vec![], vec![0], vec![0], vec![0], vec![]]
        );
    }
}
