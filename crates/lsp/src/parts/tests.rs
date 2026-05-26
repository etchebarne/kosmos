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
}
