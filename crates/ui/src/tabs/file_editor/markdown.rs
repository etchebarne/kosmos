use std::ops::Range;

use gpui::{
    AnyElement, App, FontStyle, FontWeight, HighlightStyle, IntoElement, SharedString, StyledText,
    div, prelude::*, rems,
};
use highlight::HighlightId;
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use syntax::SyntaxRegistry;
use theme::Theme;

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) struct MarkdownStyle {
    pub(super) emphasis: bool,
    pub(super) strong: bool,
    pub(super) code: bool,
    pub(super) link: bool,
}

#[derive(Clone, Copy)]
enum MarkdownStyleKind {
    Emphasis,
    Strong,
    Code,
    Link,
}

#[derive(Default)]
pub(super) struct InlineMarkdown {
    pub(super) text: String,
    pub(super) ranges: Vec<(Range<usize>, MarkdownStyle)>,
    stack: Vec<MarkdownStyleKind>,
}

pub(super) enum MarkdownBlock {
    Paragraph(InlineMarkdown),
    Heading(HeadingLevel, InlineMarkdown),
    ListItem(InlineMarkdown),
    CodeBlock {
        language: Option<String>,
        text: String,
    },
    Rule,
}

enum ActiveMarkdownBlock {
    Paragraph(InlineMarkdown),
    Heading(HeadingLevel, InlineMarkdown),
    ListItem(InlineMarkdown),
}

impl InlineMarkdown {
    fn push(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(text);
        let end = self.text.len();
        let style = self.current_style();
        if style != MarkdownStyle::default() {
            self.ranges.push((start..end, style));
        }
    }

    fn push_with(&mut self, text: &str, kind: MarkdownStyleKind) {
        self.stack.push(kind);
        self.push(text);
        self.stack.pop();
    }

    fn push_style(&mut self, kind: MarkdownStyleKind) {
        self.stack.push(kind);
    }

    fn pop_style(&mut self, kind: MarkdownStyleKind) {
        if let Some(index) = self
            .stack
            .iter()
            .rposition(|existing| std::mem::discriminant(existing) == std::mem::discriminant(&kind))
        {
            self.stack.remove(index);
        }
    }

    fn current_style(&self) -> MarkdownStyle {
        let mut style = MarkdownStyle::default();
        for kind in &self.stack {
            match kind {
                MarkdownStyleKind::Emphasis => style.emphasis = true,
                MarkdownStyleKind::Strong => style.strong = true,
                MarkdownStyleKind::Code => style.code = true,
                MarkdownStyleKind::Link => style.link = true,
            }
        }
        style
    }
}

impl ActiveMarkdownBlock {
    fn inline_mut(&mut self) -> &mut InlineMarkdown {
        match self {
            Self::Paragraph(inline) | Self::Heading(_, inline) | Self::ListItem(inline) => inline,
        }
    }

    fn finish(self) -> MarkdownBlock {
        match self {
            Self::Paragraph(inline) => MarkdownBlock::Paragraph(inline),
            Self::Heading(level, inline) => MarkdownBlock::Heading(level, inline),
            Self::ListItem(inline) => MarkdownBlock::ListItem(inline),
        }
    }
}

pub(super) fn render_markdown(
    text: &str,
    theme: Theme,
    muted: bool,
    cx: &mut App,
) -> Vec<AnyElement> {
    let blocks = parse_markdown(text);
    if blocks.is_empty() {
        return vec![
            div()
                .child(SharedString::from(text.to_string()))
                .into_any_element(),
        ];
    }

    blocks
        .into_iter()
        .map(|block| render_markdown_block(block, theme, muted, cx))
        .collect()
}

pub(super) fn parse_markdown(text: &str) -> Vec<MarkdownBlock> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut blocks = Vec::new();
    let mut active: Option<ActiveMarkdownBlock> = None;
    let mut code_block: Option<(Option<String>, String)> = None;
    let mut list_depth = 0usize;

    for event in Parser::new_ext(text, options) {
        match event {
            Event::Start(Tag::Paragraph) if active.is_none() => {
                active = Some(ActiveMarkdownBlock::Paragraph(InlineMarkdown::default()));
            }
            Event::Start(Tag::Paragraph) => {}
            Event::Start(Tag::Heading { level, .. }) => {
                active = Some(ActiveMarkdownBlock::Heading(
                    level,
                    InlineMarkdown::default(),
                ));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        Some(language.to_string())
                    }
                    _ => None,
                };
                code_block = Some((language, String::new()));
            }
            Event::Start(Tag::List(_)) => {
                list_depth += 1;
            }
            Event::Start(Tag::Item) => {
                active = Some(ActiveMarkdownBlock::ListItem(InlineMarkdown::default()));
            }
            Event::Start(Tag::Emphasis) => {
                push_active_style(&mut active, MarkdownStyleKind::Emphasis)
            }
            Event::Start(Tag::Strong) => push_active_style(&mut active, MarkdownStyleKind::Strong),
            Event::Start(Tag::Link { .. }) => {
                push_active_style(&mut active, MarkdownStyleKind::Link)
            }
            Event::End(TagEnd::Paragraph) => {
                if !matches!(active, Some(ActiveMarkdownBlock::ListItem(_)))
                    && let Some(active) = active.take()
                {
                    blocks.push(active.finish());
                }
            }
            Event::End(TagEnd::Heading(_)) | Event::End(TagEnd::Item) => {
                if let Some(active) = active.take() {
                    blocks.push(active.finish());
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((language, text)) = code_block.take() {
                    blocks.push(MarkdownBlock::CodeBlock { language, text });
                }
            }
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
            }
            Event::End(TagEnd::Emphasis) => {
                pop_active_style(&mut active, MarkdownStyleKind::Emphasis)
            }
            Event::End(TagEnd::Strong) => pop_active_style(&mut active, MarkdownStyleKind::Strong),
            Event::End(TagEnd::Link) => pop_active_style(&mut active, MarkdownStyleKind::Link),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if let Some((_, code)) = code_block.as_mut() {
                    code.push_str(&text);
                } else {
                    push_active_text(&mut active, &text);
                }
            }
            Event::Code(text) => {
                if let Some(active) = active.as_mut() {
                    active
                        .inline_mut()
                        .push_with(&text, MarkdownStyleKind::Code);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some((_, code)) = code_block.as_mut() {
                    code.push('\n');
                } else {
                    push_active_text(&mut active, "\n");
                }
            }
            Event::Rule => blocks.push(MarkdownBlock::Rule),
            Event::TaskListMarker(checked) => {
                push_active_text(&mut active, if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(reference) => {
                push_active_text(&mut active, &format!("[{reference}]"));
            }
            _ => {}
        }

        if list_depth == 0
            && matches!(active, Some(ActiveMarkdownBlock::ListItem(_)))
            && let Some(active) = active.take()
        {
            blocks.push(active.finish());
        }
    }

    if let Some(active) = active.take() {
        blocks.push(active.finish());
    }
    if let Some((language, text)) = code_block.take() {
        blocks.push(MarkdownBlock::CodeBlock { language, text });
    }

    blocks
}

fn push_active_text(active: &mut Option<ActiveMarkdownBlock>, text: &str) {
    if active.is_none() {
        *active = Some(ActiveMarkdownBlock::Paragraph(InlineMarkdown::default()));
    }
    if let Some(active) = active.as_mut() {
        active.inline_mut().push(text);
    }
}

fn push_active_style(active: &mut Option<ActiveMarkdownBlock>, style: MarkdownStyleKind) {
    if let Some(active) = active.as_mut() {
        active.inline_mut().push_style(style);
    }
}

fn pop_active_style(active: &mut Option<ActiveMarkdownBlock>, style: MarkdownStyleKind) {
    if let Some(active) = active.as_mut() {
        active.inline_mut().pop_style(style);
    }
}

fn render_markdown_block(
    block: MarkdownBlock,
    theme: Theme,
    muted: bool,
    cx: &mut App,
) -> AnyElement {
    match block {
        MarkdownBlock::Paragraph(inline) => render_inline_markdown(inline, theme, muted)
            .mb(rems(0.125))
            .into_any_element(),
        MarkdownBlock::Heading(level, inline) => render_inline_markdown(inline, theme, muted)
            .text_size(match level {
                HeadingLevel::H1 | HeadingLevel::H2 => rems(0.95),
                _ => rems(0.875),
            })
            .font_weight(FontWeight::BOLD)
            .text_color(theme.text_emphasis)
            .into_any_element(),
        MarkdownBlock::ListItem(inline) => div()
            .flex()
            .flex_row()
            .gap(rems(0.375))
            .child(div().flex_none().text_color(theme.text_subtle).child("•"))
            .child(
                render_inline_markdown(inline, theme, muted)
                    .flex_1()
                    .min_w_0(),
            )
            .into_any_element(),
        MarkdownBlock::CodeBlock { language, text } => render_code_block(language, text, theme, cx),
        MarkdownBlock::Rule => div()
            .h(rems(0.0625))
            .w_full()
            .bg(theme.border_subtle)
            .into_any_element(),
    }
}

fn render_inline_markdown(inline: InlineMarkdown, theme: Theme, muted: bool) -> gpui::Div {
    let highlights = inline
        .ranges
        .into_iter()
        .map(|(range, style)| (range, markdown_highlight(style, theme)));
    div()
        .text_color(if muted {
            theme.text_muted
        } else {
            theme.text_emphasis
        })
        .child(StyledText::new(SharedString::from(inline.text)).with_highlights(highlights))
}

fn markdown_highlight(style: MarkdownStyle, theme: Theme) -> HighlightStyle {
    HighlightStyle {
        color: if style.code {
            Some(theme.syntax.markup_code.into())
        } else if style.link {
            Some(theme.syntax.markup_link.into())
        } else {
            None
        },
        font_weight: style.strong.then_some(FontWeight::BOLD),
        font_style: style.emphasis.then_some(FontStyle::Italic),
        background_color: style.code.then_some(theme.bg_hover.into()),
        ..Default::default()
    }
}

fn render_code_block(
    language: Option<String>,
    text: String,
    theme: Theme,
    cx: &mut App,
) -> AnyElement {
    let code = text.trim_end_matches('\n');
    let highlighted = language
        .as_deref()
        .and_then(code_block_language_id)
        .and_then(|language| SyntaxRegistry::load(&language, cx))
        .map(|grammar| syntax::highlight_content(&grammar, code));
    let mut block = div()
        .w_full()
        .flex()
        .flex_col()
        .gap(rems(0.25))
        .rounded(rems(0.3125))
        .border_1()
        .border_color(theme.border_subtle)
        .bg(theme.bg_hover)
        .px(rems(0.625))
        .py(rems(0.5))
        .text_color(theme.text_emphasis);

    if let Some(language) = language.filter(|language| !language.is_empty()) {
        block = block.child(
            div()
                .text_color(theme.text_subtle)
                .font_weight(FontWeight::BOLD)
                .child(language),
        );
    }

    let mut line_start = 0usize;
    for line in code.lines() {
        let spans = highlighted
            .as_deref()
            .map(|raw| super::clip_spans_to_line(line, line_start, raw))
            .unwrap_or_default();
        block = block.child(render_code_line(line, spans, theme));
        line_start += line.len() + 1;
    }

    block.into_any_element()
}

pub(super) fn code_block_language_id(language: &str) -> Option<language::LanguageId> {
    let raw = language
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .next()?
        .trim()
        .to_ascii_lowercase();
    if raw.is_empty() {
        return None;
    }

    let canonical = match raw.as_str() {
        "bash" | "sh" | "shell" | "zsh" => Some("shellscript"),
        "js" => Some("javascript"),
        "jsx" => Some("javascriptreact"),
        "ts" => Some("typescript"),
        "tsx" => Some("typescriptreact"),
        "py" => Some("python"),
        "rs" => Some("rust"),
        "yml" => Some("yaml"),
        "md" => Some("markdown"),
        "c++" | "cc" | "cxx" => Some("cpp"),
        _ => None,
    };
    if let Some(canonical) = canonical {
        return Some(language::LanguageId::from(canonical));
    }
    if language::info(&raw).is_some() {
        return Some(language::LanguageId::new(raw));
    }
    language::from_extension(&raw)
}

fn render_code_line(
    line: &str,
    spans: Vec<(Range<usize>, HighlightId)>,
    theme: Theme,
) -> AnyElement {
    let line = SharedString::from(line.to_string());
    let mut row = div().text_color(theme.syntax.markup_code);
    if spans.is_empty() {
        row = row.child(line);
    } else {
        let highlights = spans
            .into_iter()
            .map(|(range, id)| (range, theme.syntax.style(id)));
        row = row.child(StyledText::new(line).with_highlights(highlights));
    }
    row.into_any_element()
}
