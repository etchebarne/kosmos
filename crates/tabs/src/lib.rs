pub mod registry;

use std::path::PathBuf;

use gpui::SharedString;
use icons::IconName;

pub use registry::TabKind;

#[derive(Clone)]
pub struct Tab {
    pub id: usize,
    pub kind: SharedString,
    pub title: Option<SharedString>,
    pub path: Option<PathBuf>,
}

impl Tab {
    pub fn new(id: usize, kind: &'static TabKind) -> Self {
        Self {
            id,
            kind: SharedString::new_static(kind.id),
            title: None,
            path: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Some(path);
        self
    }

    pub fn kind(&self) -> Option<&'static TabKind> {
        registry::get(&self.kind)
    }

    pub fn title(&self) -> SharedString {
        if let Some(title) = &self.title {
            return title.clone();
        }
        match self.kind() {
            Some(kind) => SharedString::new_static(kind.name),
            None => self.kind.clone(),
        }
    }

    pub fn icon(&self) -> IconName {
        if let Some(path) = &self.path
            && let Some(icon) = icon_for_path(path)
        {
            return icon;
        }
        self.kind().map(|k| k.icon).unwrap_or(IconName::File)
    }
}

fn icon_for_path(path: &std::path::Path) -> Option<IconName> {
    if let Some(name) = path.file_name().and_then(|n| n.to_str())
        && let Some(icon) = IconName::for_file_name(name)
    {
        return Some(icon);
    }
    language::from_path(path).and_then(|id| IconName::for_language(id.as_str()))
}
