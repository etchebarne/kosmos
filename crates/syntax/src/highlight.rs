use std::ops::Range;

pub use highlight::HighlightId;

/// A contiguous byte range in a buffer's text that should render with the
/// given highlight class. Produced by [`crate::SyntaxSnapshot::highlights`]
/// and consumed by the renderer, which clips spans to the visible line and
/// turns each into a styled text run.
///
/// Two fields drive overlap resolution. `specificity` is the dot-count of the
/// originating capture name (`@string.special.key` → 2, `@string` → 0); a
/// more-specific capture wins regardless of where it sits in the query, so
/// JSON `@string.special.key` beats the broader `(string) @string` even
/// though the latter has a higher pattern index. `pattern_index` breaks ties
/// at equal specificity, with later patterns winning — that's how the JSX
/// overlay overrides the JS `@variable` blanket capture.
#[derive(Clone, Debug)]
pub struct HighlightSpan {
    pub range: Range<usize>,
    pub id: HighlightId,
    pub pattern_index: usize,
    pub specificity: u8,
}
