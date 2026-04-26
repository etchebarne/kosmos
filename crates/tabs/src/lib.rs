pub mod registry;

use gpui::SharedString;
use icons::IconName;
use serde::{Deserialize, Serialize};

pub use registry::TabKind;

#[derive(Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: usize,
    pub kind: SharedString,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<SharedString>,
}

impl Tab {
    pub fn new(id: usize, kind: &'static TabKind) -> Self {
        Self {
            id,
            kind: SharedString::new_static(kind.id),
            title: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
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
        self.kind().map(|k| k.icon).unwrap_or(IconName::File)
    }
}
