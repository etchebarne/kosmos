use std::ops::Range;

use gpui::HighlightStyle;
use gpui_component::{
    highlighter::{HighlightTheme, SyntaxHighlighter},
    input::Rope,
};

fn has_styled_token(
    styles: &[(Range<usize>, HighlightStyle)],
    source: &str,
    token: &str,
) -> bool {
    let start = source.find(token).expect("token should exist in source");
    let end = start + token.len();

    styles.iter().any(|(range, style)| {
        range.start <= start && range.end >= end && style.color.is_some()
    })
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
