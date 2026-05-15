pub mod registry;

use std::path::PathBuf;

pub use registry::TabKind;

#[derive(Clone)]
pub struct Tab {
    pub id: usize,
    pub kind: String,
    pub title: Option<String>,
    pub path: Option<PathBuf>,
}

impl Tab {
    pub fn new(id: usize, kind: &'static TabKind) -> Self {
        Self {
            id,
            kind: kind.id.to_string(),
            title: None,
            path: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
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

    pub fn title(&self) -> String {
        if let Some(title) = &self.title {
            return title.clone();
        }
        match self.kind() {
            Some(kind) => kind.name.to_string(),
            None => self.kind.clone(),
        }
    }
}
