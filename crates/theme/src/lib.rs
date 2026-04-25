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
    pub bg_drag_over: Rgba,

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
            bg_root: rgb(0x0b1120),
            bg_surface: rgb(0x0f172a),
            bg_elevated: rgb(0x111827),
            bg_hover: rgb(0x1f2937),
            bg_selected: rgb(0x263244),
            bg_hover_strong: rgb(0x334155),
            bg_close_hover: rgb(0x374151),
            bg_drag_over: rgb(0x1e3a5f),

            border: rgb(0x263244),
            border_subtle: rgb(0x2d3748),
            border_strong: rgb(0x334155),

            text: rgb(0xe5e7eb),
            text_muted: rgb(0xcbd5e1),
            text_subtle: rgb(0x94a3b8),
            text_emphasis: rgb(0xffffff),
            text_header: rgb(0xdbe4ef),

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
