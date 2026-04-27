mod assets;

pub use assets::*;

use gpui::{App, IntoElement, RenderOnce, Rgba, Window, prelude::*, rems, svg};
use icondata_core::Icon as IconData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Add,
    Blank,
    ChevronDown,
    ChevronRight,
    ChromeClose,
    ChromeMaximize,
    ChromeMinimize,
    ChromeRestore,
    Clippy,
    Close,
    CollapseAll,
    Copy,
    Edit,
    EmptyWindow,
    File,
    FileAdd,
    Folder,
    FolderAdd,
    FolderOpened,
    ListTree,
    Refresh,
    Search,
    SettingsGear,
    SourceControl,
    SplitHorizontal,
    SplitVertical,
    Terminal,
    Trash,
}

impl IconName {
    pub const ALL: &'static [Self] = &[
        Self::Add,
        Self::Blank,
        Self::ChevronDown,
        Self::ChevronRight,
        Self::ChromeClose,
        Self::ChromeMaximize,
        Self::ChromeMinimize,
        Self::ChromeRestore,
        Self::Clippy,
        Self::Close,
        Self::CollapseAll,
        Self::Copy,
        Self::Edit,
        Self::EmptyWindow,
        Self::File,
        Self::FileAdd,
        Self::Folder,
        Self::FolderAdd,
        Self::FolderOpened,
        Self::ListTree,
        Self::Refresh,
        Self::Search,
        Self::SettingsGear,
        Self::SourceControl,
        Self::SplitHorizontal,
        Self::SplitVertical,
        Self::Terminal,
        Self::Trash,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Add => "icons/add.svg",
            Self::Blank => "icons/blank.svg",
            Self::ChevronDown => "icons/chevron-down.svg",
            Self::ChevronRight => "icons/chevron-right.svg",
            Self::ChromeClose => "icons/chrome-close.svg",
            Self::ChromeMaximize => "icons/chrome-maximize.svg",
            Self::ChromeMinimize => "icons/chrome-minimize.svg",
            Self::ChromeRestore => "icons/chrome-restore.svg",
            Self::Clippy => "icons/clippy.svg",
            Self::Close => "icons/close.svg",
            Self::CollapseAll => "icons/collapse-all.svg",
            Self::Copy => "icons/copy.svg",
            Self::Edit => "icons/edit.svg",
            Self::EmptyWindow => "icons/empty-window.svg",
            Self::File => "icons/file.svg",
            Self::FileAdd => "icons/file-add.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderAdd => "icons/folder-add.svg",
            Self::FolderOpened => "icons/folder-opened.svg",
            Self::ListTree => "icons/list-tree.svg",
            Self::Refresh => "icons/refresh.svg",
            Self::Search => "icons/search.svg",
            Self::SettingsGear => "icons/settings-gear.svg",
            Self::SourceControl => "icons/source-control.svg",
            Self::SplitHorizontal => "icons/split-horizontal.svg",
            Self::SplitVertical => "icons/split-vertical.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Trash => "icons/trash.svg",
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
            Self::ChevronDown => icondata_vs::VsChevronDown,
            Self::ChevronRight => icondata_vs::VsChevronRight,
            Self::ChromeClose => icondata_vs::VsChromeClose,
            Self::ChromeMaximize => icondata_vs::VsChromeMaximize,
            Self::ChromeMinimize => icondata_vs::VsChromeMinimize,
            Self::ChromeRestore => icondata_vs::VsChromeRestore,
            Self::Clippy => icondata_vs::VsClippy,
            Self::Close => icondata_vs::VsClose,
            Self::CollapseAll => icondata_vs::VsCollapseAll,
            Self::Copy => icondata_vs::VsCopy,
            Self::Edit => icondata_vs::VsEdit,
            Self::EmptyWindow => icondata_vs::VsEmptyWindow,
            Self::File => icondata_vs::VsFile,
            Self::FileAdd => icondata_vs::VsNewFile,
            Self::Folder => icondata_vs::VsFolder,
            Self::FolderAdd => icondata_vs::VsNewFolder,
            Self::FolderOpened => icondata_vs::VsFolderOpened,
            Self::ListTree => icondata_vs::VsListTree,
            Self::Refresh => icondata_vs::VsRefresh,
            Self::Search => icondata_vs::VsSearch,
            Self::SettingsGear => icondata_vs::VsSettingsGear,
            Self::SourceControl => icondata_vs::VsSourceControl,
            Self::SplitHorizontal => icondata_vs::VsSplitHorizontal,
            Self::SplitVertical => icondata_vs::VsSplitVertical,
            Self::Terminal => icondata_vs::VsTerminal,
            Self::Trash => icondata_vs::VsTrash,
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
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut element = svg()
            .path(self.name.path())
            .size(rems(self.size / 16.0))
            .flex_none();
        if let Some(color) = self.color {
            element = element.text_color(color);
        }
        element
    }
}
