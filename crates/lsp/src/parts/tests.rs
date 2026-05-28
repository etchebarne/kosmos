#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markup_content_hover() {
        let result = json!({
            "contents": {
                "kind": "markdown",
                "value": "```rust\nfn main()\n```",
            },
        });

        assert_eq!(hover_text(&result).unwrap(), "```rust\nfn main()\n```");
    }

    #[test]
    fn extracts_marked_string_arrays() {
        let result = json!({
            "contents": [
                { "language": "rust", "value": "struct Foo" },
                "docs",
            ],
        });

        assert_eq!(
            hover_text(&result).unwrap(),
            "```rust\nstruct Foo\n```\n\ndocs"
        );
    }

    #[test]
    fn ignores_empty_hover() {
        let result = json!({ "contents": [] });

        assert!(hover_text(&result).is_none());
    }

    #[test]
    fn parses_completion_arrays() {
        let result = json!([
            { "label": "println!", "kind": 3 },
            { "label": "print!", "detail": "macro" },
        ]);

        let response = completion_response(result).unwrap().unwrap();
        match response {
            lsp_types::CompletionResponse::Array(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].label, "println!");
                assert_eq!(items[1].detail.as_deref(), Some("macro"));
            }
            lsp_types::CompletionResponse::List(_) => panic!("expected completion array"),
        }
    }

    #[test]
    fn ignores_null_completion() {
        assert!(completion_response(Value::Null).unwrap().is_none());
    }

    #[test]
    fn builds_incremental_content_change() {
        let old = "fn main() {\n    let value = Ve::new();\n}\n";
        let new = "fn main() {\n    let value = Vec::new();\n}\n";

        let change = text_document_content_change(
            lsp_types::TextDocumentSyncKind::INCREMENTAL,
            old,
            new,
        );

        assert_eq!(change.text, "c");
        assert_eq!(change.range_length, Some(0));
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(1, 18),
                lsp_types::Position::new(1, 18),
            ))
        );
    }

    #[test]
    fn incremental_content_change_uses_utf16_positions() {
        let old = "fn main() {\n    let value = \"🦀\";\n}\n";
        let new = "fn main() {\n    let value = \"🦀x\";\n}\n";

        let change = text_document_content_change(
            lsp_types::TextDocumentSyncKind::INCREMENTAL,
            old,
            new,
        );

        assert_eq!(change.text, "x");
        assert_eq!(
            change.range,
            Some(lsp_types::Range::new(
                lsp_types::Position::new(1, 19),
                lsp_types::Position::new(1, 19),
            ))
        );
    }

    #[test]
    fn builds_full_content_change_for_full_sync_servers() {
        let change = text_document_content_change(
            lsp_types::TextDocumentSyncKind::FULL,
            "old",
            "new",
        );

        assert_eq!(change.text, "new");
        assert_eq!(change.range, None);
        assert_eq!(change.range_length, None);
    }

    #[test]
    fn parses_text_document_sync_kind_from_capabilities() {
        let result = json!({
            "capabilities": {
                "textDocumentSync": {
                    "openClose": true,
                    "change": 2,
                },
            },
        });

        assert_eq!(
            text_document_sync_kind(&result),
            lsp_types::TextDocumentSyncKind::INCREMENTAL
        );
    }
}
