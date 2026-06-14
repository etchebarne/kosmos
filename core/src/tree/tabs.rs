#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TabId(u64);

impl TabId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tab {
    id: TabId,
    title: String,
    kind: TabKind,
}

impl Tab {
    pub fn new(id: TabId, title: impl Into<String>, kind: TabKind) -> Self {
        Self {
            id,
            title: title.into(),
            kind,
        }
    }

    pub fn id(&self) -> TabId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn rename(&mut self, title: impl Into<String>) {
        self.title = title.into();
    }

    pub fn kind(&self) -> &TabKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TabKind {
    Blank,
    FileTree,
    Editor,
    Git,
    Search,
    Terminal,
    Settings,
}
