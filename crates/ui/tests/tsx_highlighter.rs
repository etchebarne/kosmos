use std::ops::Range;

use gpui::HighlightStyle;
use gpui_component::{
    highlighter::{HighlightTheme, SyntaxHighlighter},
    input::Rope,
};

fn has_styled_token(styles: &[(Range<usize>, HighlightStyle)], source: &str, token: &str) -> bool {
    let start = source.find(token).expect("token should exist in source");
    let end = start + token.len();

    styles
        .iter()
        .any(|(range, style)| range.start <= start && range.end >= end && style.color.is_some())
}

fn style_for_token(
    styles: &[(Range<usize>, HighlightStyle)],
    source: &str,
    token: &str,
) -> HighlightStyle {
    let start = source.find(token).expect("token should exist in source");
    let end = start + token.len();

    styles
        .iter()
        .find(|(range, style)| range.start <= start && range.end >= end && style.color.is_some())
        .map(|(_, style)| *style)
        .unwrap_or_else(|| panic!("token {token:?} should have a colored style"))
}

#[test]
fn tsx_highlighter_uses_full_typescript_and_jsx_queries() {
    let source = r#"import { useState } from "react";

export default function Home() {
  const [isReady, setReady] = useState(false);
  return <section className="hero">Ready</section>;
}
"#;

    let rope = Rope::from_str(source);
    let mut highlighter = SyntaxHighlighter::new("tsx");
    assert_eq!(highlighter.language().as_ref(), "tsx");

    assert!(highlighter.update(None, &rope, None));
    let styles = highlighter.styles(&(0..source.len()), &HighlightTheme::default_dark());

    assert!(has_styled_token(&styles, source, "import"));
    assert!(has_styled_token(&styles, source, "react"));
    assert!(has_styled_token(&styles, source, "className"));
    assert!(has_styled_token(&styles, source, "section"));
}

#[test]
fn tsx_highlighter_styles_custom_components_as_jsx_tags() {
    let source = r#"export function Balance({ kycRequired }) {
  return (
    <Button>
      {kycRequired ? <TriangleAlert className="size-5" /> : <Plus className="size-5" />}
      <span>Balance</span>
    </Button>
  );
}
"#;

    let rope = Rope::from_str(source);
    let mut highlighter = SyntaxHighlighter::new("tsx");

    assert!(highlighter.update(None, &rope, None));
    let styles = highlighter.styles(&(0..source.len()), &HighlightTheme::default_dark());
    let tag_style = style_for_token(&styles, source, "span");

    assert_eq!(style_for_token(&styles, source, "Button"), tag_style);
    assert_eq!(style_for_token(&styles, source, "TriangleAlert"), tag_style);
    assert_eq!(style_for_token(&styles, source, "Plus"), tag_style);
}
