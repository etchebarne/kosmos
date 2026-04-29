use gpui::{App, Global, Rgba, rgb};
use highlight::HighlightId;

pub const SETTING_ID: &str = "appearance.theme";
pub const DEFAULT_ID: &str = "dark";

#[derive(Clone, Copy)]
pub struct Theme {
    pub is_dark: bool,
    pub bg_root: Rgba,
    pub bg_surface: Rgba,
    pub bg_elevated: Rgba,
    pub bg_hover: Rgba,
    pub bg_selected: Rgba,
    pub bg_hover_strong: Rgba,
    pub bg_close_hover: Rgba,

    pub border: Rgba,
    pub border_subtle: Rgba,
    pub border_strong: Rgba,

    pub text: Rgba,
    pub text_muted: Rgba,
    pub text_subtle: Rgba,
    pub text_emphasis: Rgba,
    pub text_header: Rgba,

    pub accent: Rgba,
    pub danger: Rgba,

    pub syntax: SyntaxStyles,
}

/// Color palette for syntax highlighting, keyed by [`HighlightId`]. Every
/// variant must have an entry; consumers expect [`Self::color`] to be total.
#[derive(Clone, Copy)]
pub struct SyntaxStyles {
    pub attribute: Rgba,
    pub boolean: Rgba,
    pub comment: Rgba,
    pub constant: Rgba,
    pub constructor: Rgba,
    pub escape: Rgba,
    pub function: Rgba,
    pub function_macro: Rgba,
    pub keyword: Rgba,
    pub label: Rgba,
    pub markup_code: Rgba,
    pub markup_emphasis: Rgba,
    pub markup_heading: Rgba,
    pub markup_link: Rgba,
    pub markup_strong: Rgba,
    pub method: Rgba,
    pub namespace: Rgba,
    pub number: Rgba,
    pub operator: Rgba,
    pub parameter: Rgba,
    pub property: Rgba,
    pub punctuation: Rgba,
    pub string: Rgba,
    pub tag: Rgba,
    pub r#type: Rgba,
    pub type_builtin: Rgba,
    pub variable: Rgba,
}

impl SyntaxStyles {
    pub fn color(&self, id: HighlightId) -> Rgba {
        match id {
            HighlightId::Attribute => self.attribute,
            HighlightId::Boolean => self.boolean,
            HighlightId::Comment => self.comment,
            HighlightId::Constant => self.constant,
            HighlightId::Constructor => self.constructor,
            HighlightId::Escape => self.escape,
            HighlightId::Function => self.function,
            HighlightId::FunctionMacro => self.function_macro,
            HighlightId::Keyword => self.keyword,
            HighlightId::Label => self.label,
            HighlightId::MarkupCode => self.markup_code,
            HighlightId::MarkupEmphasis => self.markup_emphasis,
            HighlightId::MarkupHeading => self.markup_heading,
            HighlightId::MarkupLink => self.markup_link,
            HighlightId::MarkupStrong => self.markup_strong,
            HighlightId::Method => self.method,
            HighlightId::Namespace => self.namespace,
            HighlightId::Number => self.number,
            HighlightId::Operator => self.operator,
            HighlightId::Parameter => self.parameter,
            HighlightId::Property => self.property,
            HighlightId::Punctuation => self.punctuation,
            HighlightId::String => self.string,
            HighlightId::Tag => self.tag,
            HighlightId::Type => self.r#type,
            HighlightId::TypeBuiltin => self.type_builtin,
            HighlightId::Variable => self.variable,
        }
    }

    // Palette tracks VS Code's Default Dark+ / Light+ themes so familiar code
    // colorings carry over: variables/parameters/properties/attributes share
    // a single identifier color, functions are warm yellow, types are teal,
    // strings warm orange, keywords purple, numbers light green.
    fn dark() -> Self {
        Self {
            attribute: rgb(0x9cdcfe),
            boolean: rgb(0x569cd6),
            comment: rgb(0x6a9955),
            constant: rgb(0x4fc1ff),
            constructor: rgb(0x4ec9b0),
            escape: rgb(0xd7ba7d),
            function: rgb(0xdcdcaa),
            function_macro: rgb(0xdcdcaa),
            keyword: rgb(0xc586c0),
            label: rgb(0xc8c8c8),
            markup_code: rgb(0xce9178),
            markup_emphasis: rgb(0xdcdcaa),
            markup_heading: rgb(0x4ec9b0),
            markup_link: rgb(0x9cdcfe),
            markup_strong: rgb(0xdcdcaa),
            method: rgb(0xdcdcaa),
            namespace: rgb(0x4ec9b0),
            number: rgb(0xb5cea8),
            operator: rgb(0xd4d4d4),
            parameter: rgb(0x9cdcfe),
            property: rgb(0x9cdcfe),
            punctuation: rgb(0xd4d4d4),
            string: rgb(0xce9178),
            tag: rgb(0x569cd6),
            r#type: rgb(0x4ec9b0),
            type_builtin: rgb(0x569cd6),
            variable: rgb(0x9cdcfe),
        }
    }

    fn light() -> Self {
        Self {
            attribute: rgb(0x001080),
            boolean: rgb(0x0000ff),
            comment: rgb(0x008000),
            constant: rgb(0x0070c1),
            constructor: rgb(0x267f99),
            escape: rgb(0xee0000),
            function: rgb(0x795e26),
            function_macro: rgb(0x795e26),
            keyword: rgb(0xaf00db),
            label: rgb(0x000000),
            markup_code: rgb(0xa31515),
            markup_emphasis: rgb(0x795e26),
            markup_heading: rgb(0x267f99),
            markup_link: rgb(0x001080),
            markup_strong: rgb(0x795e26),
            method: rgb(0x795e26),
            namespace: rgb(0x267f99),
            number: rgb(0x098658),
            operator: rgb(0x000000),
            parameter: rgb(0x001080),
            property: rgb(0x001080),
            punctuation: rgb(0x000000),
            string: rgb(0xa31515),
            tag: rgb(0x800000),
            r#type: rgb(0x267f99),
            type_builtin: rgb(0x0000ff),
            variable: rgb(0x001080),
        }
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            is_dark: true,
            bg_root: rgb(0x0d0d0d),
            bg_surface: rgb(0x161616),
            bg_elevated: rgb(0x1c1c1c),
            bg_hover: rgb(0x252525),
            bg_selected: rgb(0x2e2e2e),
            bg_hover_strong: rgb(0x383838),
            bg_close_hover: rgb(0x404040),

            border: rgb(0x262626),
            border_subtle: rgb(0x1f1f1f),
            border_strong: rgb(0x363636),

            text: rgb(0xe5e5e5),
            text_muted: rgb(0xb8b8b8),
            text_subtle: rgb(0x8a8a8a),
            text_emphasis: rgb(0xffffff),
            text_header: rgb(0xd4d4d4),

            accent: rgb(0x3b82f6),
            danger: rgb(0xdc2626),

            syntax: SyntaxStyles::dark(),
        }
    }

    pub fn light() -> Self {
        Self {
            is_dark: false,
            bg_root: rgb(0xf5f5f5),
            bg_surface: rgb(0xffffff),
            bg_elevated: rgb(0xfafafa),
            bg_hover: rgb(0xeaeaea),
            bg_selected: rgb(0xdcdcdc),
            bg_hover_strong: rgb(0xcfcfcf),
            bg_close_hover: rgb(0xc2c2c2),

            border: rgb(0xd9d9d9),
            border_subtle: rgb(0xe4e4e4),
            border_strong: rgb(0xbfbfbf),

            text: rgb(0x1a1a1a),
            text_muted: rgb(0x4a4a4a),
            text_subtle: rgb(0x6b6b6b),
            text_emphasis: rgb(0x000000),
            text_header: rgb(0x2a2a2a),

            accent: rgb(0x2563eb),
            danger: rgb(0xdc2626),

            syntax: SyntaxStyles::light(),
        }
    }

    pub fn by_id(id: &str) -> Self {
        match id {
            "light" => Self::light(),
            _ => Self::dark(),
        }
    }
}

impl Global for Theme {}

pub trait ActiveTheme {
    fn theme(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn theme(&self) -> &Theme {
        self.global::<Theme>()
    }
}

/// A selectable option for a string-valued setting (id is what gets persisted,
/// label is what the user sees). Lives here so foundational crates can declare
/// their own option lists without depending on `settings`.
pub struct DropdownOption {
    pub id: &'static str,
    pub label: &'static str,
}

pub const REGISTRY: &[DropdownOption] = &[
    DropdownOption {
        id: "dark",
        label: "Dark",
    },
    DropdownOption {
        id: "light",
        label: "Light",
    },
];
