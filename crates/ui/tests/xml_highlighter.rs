use gpui::HighlightStyle;
use gpui_component::{
    highlighter::{HighlightTheme, SyntaxHighlighter},
    input::Rope,
};

fn has_colored_token(
    styles: &[(std::ops::Range<usize>, HighlightStyle)],
    source: &str,
    token: &str,
) -> bool {
    let start = source.find(token).expect("token should exist in source");
    let end = start + token.len();

    styles
        .iter()
        .any(|(range, style)| range.start <= start && range.end >= end && style.color.is_some())
}

#[test]
fn xml_highlighter_styles_tags_and_attributes() {
    let source = r#"<?xml version="1.0"?>
<note priority="high">
  <to>Tove</to>
</note>
"#;

    let rope = Rope::from_str(source);
    let mut highlighter = SyntaxHighlighter::new("xml");
    assert_eq!(highlighter.language().as_ref(), "xml");

    assert!(highlighter.update(None, &rope, None));
    let styles = highlighter.styles(&(0..source.len()), &HighlightTheme::default_dark());

    assert!(has_colored_token(&styles, source, "note"));
    assert!(has_colored_token(&styles, source, "priority"));
    assert!(has_colored_token(&styles, source, "high"));
}

#[test]
fn svg_alias_uses_xml_highlighter() {
    let highlighter = SyntaxHighlighter::new("svg");

    assert_eq!(highlighter.language().as_ref(), "xml");
}
