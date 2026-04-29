//! Shared highlight vocabulary. Held in its own crate so both producers
//! (`syntax` from tree-sitter, `lsp` from semantic tokens) and consumers
//! (`theme` for the color palette, `ui` for rendering) can depend on a tiny
//! enum without dragging each other into the compile graph.

/// Categorical token class used for syntax highlighting. Producers map their
/// own vocabularies onto this enum; the theme maps each variant to a concrete
/// color. Add variants here when a new class needs distinct visual treatment;
/// removing one is a breaking change for every consumer.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Debug)]
pub enum HighlightId {
    Attribute,
    Boolean,
    Comment,
    Constant,
    Constructor,
    Escape,
    Function,
    FunctionMacro,
    Keyword,
    Label,
    MarkupCode,
    MarkupEmphasis,
    MarkupHeading,
    MarkupLink,
    MarkupStrong,
    Method,
    Namespace,
    Number,
    Operator,
    Parameter,
    Property,
    Punctuation,
    String,
    Tag,
    Type,
    TypeBuiltin,
    Variable,
}
