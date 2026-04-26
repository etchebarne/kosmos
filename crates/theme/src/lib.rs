use gpui::{App, Global, Rgba, rgb};

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
