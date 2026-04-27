use gpui::{App, Global, Rgba, rgb};

pub const SETTING_ID: &str = "appearance.theme";
pub const DEFAULT_ID: &str = "dark";

#[derive(Clone, Copy)]
pub struct Theme {
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
}

impl Theme {
    pub fn dark() -> Self {
        Self {
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
        }
    }

    pub fn light() -> Self {
        Self {
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
