mod assets;
mod language;
mod raster;

pub use assets::*;

use gpui::{
    App, IntoElement, RenderOnce, Rgba, SharedString, Window, canvas, prelude::*, rems, svg,
};
use icondata_core::Icon as IconData;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Add,
    Archive,
    ArrowDown,
    ArrowUp,
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
    Diff,
    Edit,
    Ellipsis,
    EmptyWindow,
    Error,
    File,
    FileAdd,
    Folder,
    FolderAdd,
    FolderOpened,
    Info,
    ListTree,
    Pass,
    Refresh,
    Remove,
    Search,
    Server,
    SettingsGear,
    SourceControl,
    SplitHorizontal,
    SplitVertical,
    Terminal,
    Tag,
    Trash,
    Warning,
    LangAstro,
    LangBash,
    LangBun,
    LangC,
    LangCpp,
    LangCsharp,
    LangCss,
    LangDart,
    LangDocker,
    LangDotenv,
    LangGit,
    LangGo,
    LangGraphql,
    LangHaskell,
    LangHtml,
    LangJava,
    LangJavascript,
    LangJson,
    LangJulia,
    LangKotlin,
    LangLua,
    LangMarkdown,
    LangPhp,
    LangPowershell,
    LangPython,
    LangR,
    LangReact,
    LangRuby,
    LangRust,
    LangSass,
    LangScala,
    LangSolidity,
    LangSql,
    LangSvelte,
    LangSwift,
    LangTerraform,
    LangTypescript,
    LangVue,
    LangZig,
}

impl IconName {
    pub const ALL: &'static [Self] = &[
        Self::Add,
        Self::Archive,
        Self::ArrowDown,
        Self::ArrowUp,
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
        Self::Diff,
        Self::Edit,
        Self::Ellipsis,
        Self::EmptyWindow,
        Self::Error,
        Self::File,
        Self::FileAdd,
        Self::Folder,
        Self::FolderAdd,
        Self::FolderOpened,
        Self::Info,
        Self::ListTree,
        Self::Pass,
        Self::Refresh,
        Self::Remove,
        Self::Search,
        Self::Server,
        Self::SettingsGear,
        Self::SourceControl,
        Self::SplitHorizontal,
        Self::SplitVertical,
        Self::Terminal,
        Self::Tag,
        Self::Trash,
        Self::Warning,
        Self::LangAstro,
        Self::LangBash,
        Self::LangBun,
        Self::LangC,
        Self::LangCpp,
        Self::LangCsharp,
        Self::LangCss,
        Self::LangDart,
        Self::LangDocker,
        Self::LangDotenv,
        Self::LangGit,
        Self::LangGo,
        Self::LangGraphql,
        Self::LangHaskell,
        Self::LangHtml,
        Self::LangJava,
        Self::LangJavascript,
        Self::LangJson,
        Self::LangJulia,
        Self::LangKotlin,
        Self::LangLua,
        Self::LangMarkdown,
        Self::LangPhp,
        Self::LangPowershell,
        Self::LangPython,
        Self::LangR,
        Self::LangReact,
        Self::LangRuby,
        Self::LangRust,
        Self::LangSass,
        Self::LangScala,
        Self::LangSolidity,
        Self::LangSql,
        Self::LangSvelte,
        Self::LangSwift,
        Self::LangTerraform,
        Self::LangTypescript,
        Self::LangVue,
        Self::LangZig,
    ];

    pub fn path(self) -> &'static str {
        match self {
            Self::Add => "icons/add.svg",
            Self::Archive => "icons/archive.svg",
            Self::ArrowDown => "icons/arrow-down.svg",
            Self::ArrowUp => "icons/arrow-up.svg",
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
            Self::Diff => "icons/diff.svg",
            Self::Edit => "icons/edit.svg",
            Self::Ellipsis => "icons/ellipsis.svg",
            Self::EmptyWindow => "icons/empty-window.svg",
            Self::Error => "icons/error.svg",
            Self::File => "icons/file.svg",
            Self::FileAdd => "icons/file-add.svg",
            Self::Folder => "icons/folder.svg",
            Self::FolderAdd => "icons/folder-add.svg",
            Self::FolderOpened => "icons/folder-opened.svg",
            Self::Info => "icons/info.svg",
            Self::ListTree => "icons/list-tree.svg",
            Self::Pass => "icons/pass.svg",
            Self::Refresh => "icons/refresh.svg",
            Self::Remove => "icons/remove.svg",
            Self::Search => "icons/search.svg",
            Self::Server => "icons/server.svg",
            Self::SettingsGear => "icons/settings-gear.svg",
            Self::SourceControl => "icons/source-control.svg",
            Self::SplitHorizontal => "icons/split-horizontal.svg",
            Self::SplitVertical => "icons/split-vertical.svg",
            Self::Terminal => "icons/terminal.svg",
            Self::Tag => "icons/tag.svg",
            Self::Trash => "icons/trash.svg",
            Self::Warning => "icons/warning.svg",
            Self::LangAstro => "langs/astro.svg",
            Self::LangBash => "langs/bash.svg",
            Self::LangBun => "langs/bun.svg",
            Self::LangC => "langs/c.svg",
            Self::LangCpp => "langs/cpp.svg",
            Self::LangCsharp => "langs/csharp.svg",
            Self::LangCss => "langs/css.svg",
            Self::LangDart => "langs/dart.svg",
            Self::LangDocker => "langs/docker.svg",
            Self::LangDotenv => "langs/dotenv.svg",
            Self::LangGit => "langs/git.svg",
            Self::LangGo => "langs/go.svg",
            Self::LangGraphql => "langs/graphql.svg",
            Self::LangHaskell => "langs/haskell.svg",
            Self::LangHtml => "langs/html.svg",
            Self::LangJava => "langs/java.svg",
            Self::LangJavascript => "langs/javascript.svg",
            Self::LangJson => "langs/json.svg",
            Self::LangJulia => "langs/julia.svg",
            Self::LangKotlin => "langs/kotlin.svg",
            Self::LangLua => "langs/lua.svg",
            Self::LangMarkdown => "langs/markdown.svg",
            Self::LangPhp => "langs/php.svg",
            Self::LangPowershell => "langs/powershell.svg",
            Self::LangPython => "langs/python.svg",
            Self::LangR => "langs/r.svg",
            Self::LangReact => "langs/react.svg",
            Self::LangRuby => "langs/ruby.svg",
            Self::LangRust => "langs/rust.svg",
            Self::LangSass => "langs/sass.svg",
            Self::LangScala => "langs/scala.svg",
            Self::LangSolidity => "langs/solidity.svg",
            Self::LangSql => "langs/sql.svg",
            Self::LangSvelte => "langs/svelte.svg",
            Self::LangSwift => "langs/swift.svg",
            Self::LangTerraform => "langs/terraform.svg",
            Self::LangTypescript => "langs/typescript.svg",
            Self::LangVue => "langs/vue.svg",
            Self::LangZig => "langs/zig.svg",
        }
    }

    pub fn from_path(path: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|icon| icon.path() == path)
    }

    pub fn for_language(language_id: &str) -> Option<Self> {
        language::icon_for_language(language_id)
    }

    /// Match well-known file names that should override extension-based icons
    /// (e.g. `Cargo.toml` → Rust, `bun.lock` → Bun).
    pub fn for_file_name(file_name: &str) -> Option<Self> {
        language::icon_for_file_name(file_name)
    }

    fn data(self) -> Option<IconData> {
        let data = match self {
            Self::Add => icondata_vs::VsAdd,
            Self::Archive => icondata_vs::VsArchive,
            Self::ArrowDown => icondata_vs::VsArrowDown,
            Self::ArrowUp => icondata_vs::VsArrowUp,
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
            Self::Diff => icondata_vs::VsDiff,
            Self::Edit => icondata_vs::VsEdit,
            Self::Ellipsis => icondata_vs::VsEllipsis,
            Self::EmptyWindow => icondata_vs::VsEmptyWindow,
            Self::Error => icondata_vs::VsError,
            Self::File => icondata_vs::VsFile,
            Self::FileAdd => icondata_vs::VsNewFile,
            Self::Folder => icondata_vs::VsFolder,
            Self::FolderAdd => icondata_vs::VsNewFolder,
            Self::FolderOpened => icondata_vs::VsFolderOpened,
            Self::Info => icondata_vs::VsInfo,
            Self::ListTree => icondata_vs::VsListTree,
            Self::Pass => icondata_vs::VsPass,
            Self::Refresh => icondata_vs::VsRefresh,
            Self::Remove => icondata_vs::VsRemove,
            Self::Search => icondata_vs::VsSearch,
            Self::Server => icondata_vs::VsServer,
            Self::SettingsGear => icondata_vs::VsSettingsGear,
            Self::SourceControl => icondata_vs::VsSourceControl,
            Self::SplitHorizontal => icondata_vs::VsSplitHorizontal,
            Self::SplitVertical => icondata_vs::VsSplitVertical,
            Self::Terminal => icondata_vs::VsTerminal,
            Self::Tag => icondata_vs::VsTag,
            Self::Trash => icondata_vs::VsTrash,
            Self::Warning => icondata_vs::VsWarning,
            Self::LangAstro
            | Self::LangBash
            | Self::LangBun
            | Self::LangC
            | Self::LangCpp
            | Self::LangCsharp
            | Self::LangCss
            | Self::LangDart
            | Self::LangDocker
            | Self::LangDotenv
            | Self::LangGit
            | Self::LangGo
            | Self::LangGraphql
            | Self::LangHaskell
            | Self::LangHtml
            | Self::LangJava
            | Self::LangJavascript
            | Self::LangJson
            | Self::LangJulia
            | Self::LangKotlin
            | Self::LangLua
            | Self::LangMarkdown
            | Self::LangPhp
            | Self::LangPowershell
            | Self::LangPython
            | Self::LangR
            | Self::LangReact
            | Self::LangRuby
            | Self::LangRust
            | Self::LangSass
            | Self::LangScala
            | Self::LangSolidity
            | Self::LangSql
            | Self::LangSvelte
            | Self::LangSwift
            | Self::LangTerraform
            | Self::LangTypescript
            | Self::LangVue
            | Self::LangZig => return None,
        };
        Some(data)
    }

    pub fn to_svg(self) -> Option<String> {
        let data = self.data()?;
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
        Some(svg)
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

    pub fn size_rem(mut self, size_rem: f32) -> Self {
        self.size = size_rem * 16.0;
        self
    }

    pub fn color(mut self, color: Rgba) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Icon {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let path = self.name.path();
        let size = rems(self.size / 16.0);
        // Multi-color SVGs (language icons) need per-paint rasterization at the
        // element's device-pixel bounds — gpui's `img()` caches a single bitmap
        // and stretches it, which produces visible aliasing/blur on zoom.
        // `svg()` already does per-paint rasterization but forces a single
        // tint color, so we can only use it for the monochrome icondata icons.
        if path.starts_with("langs/") {
            let asset_path = SharedString::from(path);
            canvas(
                |_bounds, _window, _cx| (),
                move |bounds, _, window, cx| {
                    raster::paint(asset_path, bounds, window, cx);
                },
            )
            .size(size)
            .flex_none()
            .into_any_element()
        } else {
            let mut element = svg().path(path).size(size).flex_none();
            if let Some(color) = self.color {
                element = element.text_color(color);
            }
            element.into_any_element()
        }
    }
}
