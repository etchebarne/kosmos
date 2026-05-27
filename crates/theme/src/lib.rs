use std::sync::Arc;

use gpui::{Anchor, App, Global, Hsla, Rgba, rgb};

pub const SETTING_ID: &str = "appearance.theme";
pub const DARK_ID: &str = "dark";
pub const NEUTRAL_ID: &str = "neutral";
pub const LIGHT_ID: &str = "light";
pub const DEFAULT_ID: &str = DARK_ID;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub id: &'static str,
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
    pub accent_secondary: Rgba,
    pub danger: Rgba,
    pub success: Rgba,
    pub warning: Rgba,

    pub dirty: Rgba,
    pub terminal_foreground: Rgba,
    pub terminal_background: Rgba,
    pub terminal_separator: Rgba,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            id: DARK_ID,
            is_dark: true,
            bg_root: rgb(0x000000),
            bg_surface: rgb(0x000000),
            bg_elevated: rgb(0x000000),
            bg_hover: rgb(0x171717),
            bg_selected: rgb(0x202020),
            bg_hover_strong: rgb(0x282828),
            bg_close_hover: rgb(0x303030),

            border: rgb(0x1a1a1a),
            border_subtle: rgb(0x141414),
            border_strong: rgb(0x292929),

            text: rgb(0xe5e5e5),
            text_muted: rgb(0xb8b8b8),
            text_subtle: rgb(0x8a8a8a),
            text_emphasis: rgb(0xffffff),
            text_header: rgb(0xd4d4d4),

            accent: rgb(0x3b82f6),
            accent_secondary: rgb(0xa855f7),
            danger: rgb(0xdc2626),
            success: rgb(0x22c55e),
            warning: rgb(0xf59e0b),

            dirty: rgb(0xffffff),
            terminal_foreground: rgb(0xe5e5e5),
            terminal_background: rgb(0x000000),
            terminal_separator: rgb(0x242424),
        }
    }

    pub fn light() -> Self {
        Self {
            id: LIGHT_ID,
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
            accent_secondary: rgb(0x9333ea),
            danger: rgb(0xdc2626),
            success: rgb(0x16a34a),
            warning: rgb(0xd97706),

            dirty: rgb(0x111827),
            terminal_foreground: rgb(0x1a1a1a),
            terminal_background: rgb(0xffffff),
            terminal_separator: rgb(0xbfbfbf),
        }
    }

    pub fn neutral() -> Self {
        Self {
            id: NEUTRAL_ID,
            is_dark: true,
            bg_root: rgb(0x262626),
            bg_surface: rgb(0x262626),
            bg_elevated: rgb(0x262626),
            bg_hover: rgb(0x303030),
            bg_selected: rgb(0x383838),
            bg_hover_strong: rgb(0x424242),
            bg_close_hover: rgb(0x4a4a4a),

            border: rgb(0x3a3a3a),
            border_subtle: rgb(0x303030),
            border_strong: rgb(0x4a4a4a),

            text: rgb(0xe5e5e5),
            text_muted: rgb(0xb8b8b8),
            text_subtle: rgb(0x8f8f8f),
            text_emphasis: rgb(0xffffff),
            text_header: rgb(0xd4d4d4),

            accent: rgb(0x3b82f6),
            accent_secondary: rgb(0xa855f7),
            danger: rgb(0xdc2626),
            success: rgb(0x22c55e),
            warning: rgb(0xf59e0b),

            dirty: rgb(0xffffff),
            terminal_foreground: rgb(0xe5e5e5),
            terminal_background: rgb(0x262626),
            terminal_separator: rgb(0x404040),
        }
    }

    pub fn by_id(id: &str) -> Self {
        match Self::normalize_id(id) {
            LIGHT_ID => Self::light(),
            NEUTRAL_ID => Self::neutral(),
            _ => Self::dark(),
        }
    }

    pub fn normalize_id(id: &str) -> &'static str {
        match id {
            LIGHT_ID => LIGHT_ID,
            NEUTRAL_ID => NEUTRAL_ID,
            DARK_ID => DARK_ID,
            _ => DEFAULT_ID,
        }
    }
}

impl Global for Theme {}

pub fn install(id: &str, cx: &mut App) {
    set_active_theme(Theme::by_id(id), cx);
}

pub fn apply(id: &str, cx: &mut App) -> bool {
    let next = Theme::by_id(id);
    if cx.has_global::<Theme>() && *cx.global::<Theme>() == next {
        return false;
    }

    set_active_theme(next, cx);
    cx.refresh_windows();
    true
}

fn set_active_theme(theme: Theme, cx: &mut App) {
    cx.set_global(theme);
    sync_component_theme(theme, cx);
}

fn sync_component_theme(theme: Theme, cx: &mut App) {
    let mode = if theme.is_dark {
        gpui_component::ThemeMode::Dark
    } else {
        gpui_component::ThemeMode::Light
    };
    gpui_component::Theme::change(mode, None, cx);

    let component_theme = gpui_component::Theme::global_mut(cx);
    component_theme.notification.placement = Anchor::BottomRight;

    component_theme.colors.background = hsla(theme.bg_surface);
    component_theme.colors.foreground = hsla(theme.text);
    component_theme.colors.border = hsla(theme.border);
    component_theme.colors.input = hsla(theme.border_strong);
    component_theme.colors.ring = hsla(theme.accent);

    component_theme.colors.muted = hsla(theme.bg_elevated);
    component_theme.colors.muted_foreground = hsla(theme.text_subtle);
    component_theme.colors.secondary = hsla(theme.bg_hover);
    component_theme.colors.secondary_foreground = hsla(theme.text);
    component_theme.colors.secondary_hover = hsla(theme.bg_selected);
    component_theme.colors.secondary_active = hsla(theme.bg_hover_strong);

    component_theme.colors.primary = hsla(theme.accent);
    component_theme.colors.primary_foreground = hsla(rgb(0xffffff));
    component_theme.colors.primary_hover = hsla(theme.accent);
    component_theme.colors.primary_active = hsla(theme.accent);
    component_theme.colors.button_primary = hsla(theme.accent);
    component_theme.colors.button_primary_foreground = hsla(rgb(0xffffff));
    component_theme.colors.button_primary_hover = hsla(theme.accent);
    component_theme.colors.button_primary_active = hsla(theme.accent);

    component_theme.colors.accent = hsla(theme.bg_hover);
    component_theme.colors.accent_foreground = hsla(theme.text_emphasis);
    component_theme.colors.selection =
        hsla(theme.accent).opacity(if theme.is_dark { 0.28 } else { 0.25 });

    component_theme.colors.danger = hsla(theme.danger);
    component_theme.colors.danger_foreground = hsla(rgb(0xffffff));
    component_theme.colors.danger_hover = hsla(theme.danger);
    component_theme.colors.danger_active = hsla(theme.danger);
    component_theme.colors.success = hsla(theme.success);
    component_theme.colors.success_foreground = hsla(rgb(0xffffff));
    component_theme.colors.success_hover = hsla(theme.success);
    component_theme.colors.success_active = hsla(theme.success);
    component_theme.colors.warning = hsla(theme.warning);
    component_theme.colors.warning_foreground = hsla(rgb(0xffffff));
    component_theme.colors.warning_hover = hsla(theme.warning);
    component_theme.colors.warning_active = hsla(theme.warning);

    component_theme.colors.link = hsla(theme.accent);
    component_theme.colors.link_active = hsla(theme.accent);
    component_theme.colors.link_hover = hsla(theme.accent);

    component_theme.colors.list = hsla(theme.bg_surface);
    component_theme.colors.list_head = hsla(theme.bg_elevated);
    component_theme.colors.list_even = hsla(theme.bg_surface);
    component_theme.colors.list_hover = hsla(theme.bg_hover);
    component_theme.colors.list_active =
        hsla(theme.accent).opacity(if theme.is_dark { 0.18 } else { 0.16 });
    component_theme.colors.list_active_border =
        hsla(theme.accent).opacity(if theme.is_dark { 0.42 } else { 0.55 });
    component_theme.colors.table = hsla(theme.bg_surface);
    component_theme.colors.table_head = hsla(theme.bg_elevated);
    component_theme.colors.table_head_foreground = hsla(theme.text_subtle);
    component_theme.colors.table_even = hsla(theme.bg_surface);
    component_theme.colors.table_hover = hsla(theme.bg_hover);
    component_theme.colors.table_active = component_theme.colors.list_active;
    component_theme.colors.table_active_border = component_theme.colors.list_active_border;
    component_theme.colors.table_row_border = hsla(theme.border_subtle);

    component_theme.colors.popover = hsla(theme.bg_elevated);
    component_theme.colors.popover_foreground = hsla(theme.text);
    component_theme.colors.sidebar = hsla(theme.bg_surface);
    component_theme.colors.sidebar_foreground = hsla(theme.text);
    component_theme.colors.sidebar_border = hsla(theme.border);
    component_theme.colors.sidebar_accent = hsla(theme.bg_hover);
    component_theme.colors.sidebar_accent_foreground = hsla(theme.text_emphasis);
    component_theme.colors.sidebar_primary = hsla(theme.accent);
    component_theme.colors.sidebar_primary_foreground = hsla(rgb(0xffffff));

    component_theme.colors.tab = hsla(theme.bg_surface);
    component_theme.colors.tab_active = hsla(theme.bg_elevated);
    component_theme.colors.tab_foreground = hsla(theme.text_muted);
    component_theme.colors.tab_active_foreground = hsla(theme.text_emphasis);
    component_theme.colors.tab_bar = hsla(theme.bg_root);
    component_theme.colors.tab_bar_segmented = hsla(theme.bg_surface);

    component_theme.colors.scrollbar = hsla(theme.bg_surface);
    component_theme.colors.scrollbar_thumb = hsla(theme.border_strong);
    component_theme.colors.scrollbar_thumb_hover = hsla(theme.text_subtle);
    component_theme.colors.switch = hsla(theme.bg_selected);
    component_theme.colors.switch_thumb = hsla(theme.bg_surface);
    component_theme.colors.skeleton = hsla(theme.bg_hover);
    component_theme.colors.overlay =
        hsla(rgb(0x000000)).opacity(if theme.is_dark { 0.55 } else { 0.28 });
    component_theme.colors.window_border = hsla(theme.border);
    component_theme.colors.title_bar = hsla(theme.bg_surface);
    component_theme.colors.title_bar_border = hsla(theme.border);

    let mut highlight_theme = (*component_theme.highlight_theme).clone();
    highlight_theme.style.editor_background = Some(gpui::transparent_black());
    component_theme.highlight_theme = Arc::new(highlight_theme);
}

fn hsla(color: Rgba) -> Hsla {
    Hsla::from(color)
}

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
        id: DARK_ID,
        label: "Kosmos Dark",
    },
    DropdownOption {
        id: NEUTRAL_ID,
        label: "Kosmos Neutral",
    },
    DropdownOption {
        id: LIGHT_ID,
        label: "Kosmos Light",
    },
];
