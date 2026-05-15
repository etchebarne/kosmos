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
}
