mod assets;

pub use assets::*;

use gpui::{App, IntoElement, RenderOnce, Rgba, Window, prelude::*, rems, svg};
use icondata_core::Icon as IconData;
use theme::ActiveTheme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Add,
    Blank,
    ChromeClose,
    ChromeMaximize,
    ChromeMinimize,
    ChromeRestore,
    Close,
    EmptyWindow,
    File,
    ListTree,
    Search,
    SettingsGear,
    SourceControl,
    SplitHorizontal,
    SplitVertical,
    Terminal,
}

impl IconName {
    pub const ALL: &'static [Self] = &[
        Self::Add,
        Self::Blank,
        Self::ChromeClose,
        Self::ChromeMaximize,
        Self::ChromeMinimize,
        Self::ChromeRestore,
        Self::Close,
        Self::EmptyWindow,
        Self::File,
        Self::ListTree,
        Self::Search,
        Self::SettingsGear,
        Self::SourceControl,
        Self::SplitHorizontal,
        Self::SplitVertical,
        Self::Terminal,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Add => "icons/add.svg",
            Self::Blank => "icons/blank.svg",
            Self::ChromeClose => "icons/chrome-close.svg",
            Self::ChromeMaximize => "icons/chrome-maximize.svg",
            Self::ChromeMinimize => "icons/chrome-minimize.svg",
            Self::ChromeRestore => "icons/chrome-restore.svg",
            Self::Close => "icons/close.svg",
            Self::EmptyWindow => "icons/empty-window.svg",
            Self::File => "icons/file.svg",
            Self::ListTree => "icons/list-tree.svg",
            Self::Search => "icons/search.svg",
            Self::SettingsGear => "icons/settings-gear.svg",
            Self::SourceControl => "icons/source-control.svg",
            Self::SplitHorizontal => "icons/split-horizontal.svg",
            Self::SplitVertical => "icons/split-vertical.svg",
            Self::Terminal => "icons/terminal.svg",
        }
    }

    pub fn file_name(self) -> &'static str {
        self.path().strip_prefix("icons/").unwrap_or(self.path())
    }

    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|icon| icon.path() == path)
    }

    fn data(self) -> IconData {
        match self {
            Self::Add => icondata_vs::VsAdd,
            Self::Blank => icondata_vs::VsBlank,
            Self::ChromeClose => icondata_vs::VsChromeClose,
            Self::ChromeMaximize => icondata_vs::VsChromeMaximize,
            Self::ChromeMinimize => icondata_vs::VsChromeMinimize,
            Self::ChromeRestore => icondata_vs::VsChromeRestore,
            Self::Close => icondata_vs::VsClose,
            Self::EmptyWindow => icondata_vs::VsEmptyWindow,
            Self::File => icondata_vs::VsFile,
            Self::ListTree => icondata_vs::VsListTree,
            Self::Search => icondata_vs::VsSearch,
            Self::SettingsGear => icondata_vs::VsSettingsGear,
            Self::SourceControl => icondata_vs::VsSourceControl,
            Self::SplitHorizontal => icondata_vs::VsSplitHorizontal,
            Self::SplitVertical => icondata_vs::VsSplitVertical,
            Self::Terminal => icondata_vs::VsTerminal,
        }
    }

    pub fn to_svg(self) -> String {
        let data = self.data();
        let mut svg = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg""#);

        push_attr(&mut svg, "x", data.x);
        push_attr(&mut svg, "y", data.y);
        push_attr(&mut svg, "width", data.width);
        push_attr(&mut svg, "height", data.height);
        push_attr(&mut svg, "viewBox", data.view_box);
        push_attr(&mut svg, "style", data.style);
        push_attr(&mut svg, "stroke-linecap", data.stroke_linecap);
        push_attr(&mut svg, "stroke-linejoin", data.stroke_linejoin);
        push_attr(&mut svg, "stroke-width", data.stroke_width);
        push_attr(&mut svg, "stroke", data.stroke);
        push_attr(&mut svg, "fill", data.fill.or(Some("currentColor")));

        svg.push('>');
        svg.push_str(data.data);
        svg.push_str("</svg>");
        svg
    }
}

fn push_attr(svg: &mut String, name: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };

    svg.push(' ');
    svg.push_str(name);
    svg.push_str(r#"=""#);
    svg.push_str(value);
    svg.push('"');
}

#[derive(IntoElement)]
pub struct Icon {
    name: IconName,
    size: f32,
    color: Option<Rgba>,
}

impl Icon {
    pub fn new(name: IconName) -> Self {
        Self {
            name,
            size: 16.0,
            color: None,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Rgba) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = self.color.unwrap_or_else(|| cx.theme().text_muted);
        svg()
            .path(self.name.path())
            .size(rems(self.size / 16.0))
            .flex_none()
            .text_color(color)
    }
}
