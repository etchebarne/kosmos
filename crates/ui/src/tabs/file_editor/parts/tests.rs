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

    #[test]
    fn component_completion_query_uses_identifier_suffix() {
        assert_eq!(completion_filter_query(".pri"), "pri");
        assert_eq!(completion_filter_query("PROFILE.pri"), "pri");
        assert_eq!(completion_filter_query("::pri"), "pri");
        assert_eq!(completion_filter_query("background-co"), "background-co");
    }

    #[test]
    fn component_completion_raw_query_uses_current_expression() {
        let content = "const test = PROFILE.na";
        assert_eq!(completion_raw_query_for_offset(content, content.len()), "PROFILE.na");
    }

    #[test]
    fn component_completion_line_for_offset_tracks_document_line() {
        assert_eq!(completion_line_for_offset("one\ntwo\nthree", 0), 0);
        assert_eq!(completion_line_for_offset("one\ntwo\nthree", 5), 1);
        assert_eq!(completion_line_for_offset("one\ntwo\nthree", 9), 2);
    }

    #[test]
    fn component_completion_hides_after_query_terminator() {
        assert!(completion_should_request(".description"));
        assert!(completion_should_request("."));
        assert!(!completion_should_request(".description,"));
        assert!(!completion_should_request(" "));
    }

    #[test]
    fn component_completion_ranking_tracks_query() {
        let mut items = vec![
            completion_item("zoo"),
            completion_item("apricot"),
            completion_item("std::print"),
            completion_item("print"),
        ];

        rank_completion_items(&mut items, "pri");

        let labels = items.into_iter().map(|item| item.label).collect::<Vec<_>>();
        assert_eq!(labels, ["print", "std::print", "apricot", "zoo"]);
    }

    #[test]
    fn component_completion_adds_compact_detail_and_full_documentation() {
        let mut item = completion_item("promise");
        item.kind = Some(CompletionItemKind::FUNCTION);
        item.detail = Some("astro:src/content/loaders/glob promise helper with a very long source path".to_string());

        enhance_completion_item(&mut item);
        let entry = ComponentCompletionItem::new(item.clone());

        assert_eq!(entry.label, "promise");
        assert_eq!(entry.detail.as_deref(), Some("(function)"));
        match item.documentation {
            Some(Documentation::String(documentation)) => {
                assert!(documentation.contains("astro:src/content/loaders/glob"));
            }
            _ => panic!("expected full detail documentation"),
        }
    }

    #[test]
    fn component_completion_keeps_useful_short_detail() {
        let mut item = completion_item("name");
        item.kind = Some(CompletionItemKind::PROPERTY);
        item.detail = Some("(property) name: string".to_string());

        enhance_completion_item(&mut item);
        let entry = ComponentCompletionItem::new(item.clone());

        assert_eq!(entry.label, "name");
        assert_eq!(entry.detail.as_deref(), Some("(property) name: string"));
        match item.documentation {
            Some(Documentation::String(documentation)) => {
                assert_eq!(documentation, "(property) name: string");
            }
            _ => panic!("expected detail documentation"),
        }
    }

    #[test]
    fn component_completion_avoids_kind_only_documentation() {
        let mut item = completion_item("Promise");
        item.kind = Some(CompletionItemKind::CLASS);

        enhance_completion_item(&mut item);
        let entry = ComponentCompletionItem::new(item.clone());

        assert_eq!(entry.label, "Promise");
        assert_eq!(entry.detail.as_deref(), Some("(class)"));
        assert!(item.documentation.is_none());
    }

    #[test]
    fn component_completion_fallback_edit_inserts_label() {
        let item = completion_item("name");
        let (range, new_text) = completion_edit_for_item(&item, "PROFILE.n", 8..9);

        assert_eq!(range, 8..9);
        assert_eq!(new_text, "name");
    }

    #[test]
    fn component_completion_text_edit_uses_server_range() {
        let mut item = completion_item("name");
        item.kind = Some(CompletionItemKind::FIELD);
        item.text_edit = Some(CompletionTextEdit::Edit(lsp_types::TextEdit {
            range: test_completion_range(),
            new_text: "name".to_string(),
        }));

        let (range, new_text) = completion_edit_for_item(&item, "  PROF", 0..0);

        assert_eq!(range, 2..5);
        assert_eq!(new_text, "name");
    }

    fn completion_item(label: &str) -> CompletionItem {
        CompletionItem {
            label: label.to_string(),
            ..Default::default()
        }
    }

    fn test_completion_range() -> lsp_types::Range {
        lsp_types::Range::new(lsp_types::Position::new(0, 2), lsp_types::Position::new(0, 5))
    }
}
