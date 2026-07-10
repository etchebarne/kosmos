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

    pub fn set_kind(&mut self, kind: TabKind) {
        self.title = kind.default_title().to_owned();
        self.kind = kind;
    }

    pub fn kind(&self) -> &TabKind {
        &self.kind
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TabKind {
    Blank,
    Diff,
    FileTree,
    Editor,
    Git,
    Search,
    Terminal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TabLifecycle {
    Ephemeral,
    KeepAlive,
}

impl TabKind {
    pub fn default_title(&self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::Diff => "Diff",
            Self::FileTree => "File Tree",
            Self::Editor => "Editor",
            Self::Git => "Git",
            Self::Search => "Search",
            Self::Terminal => "Terminal",
        }
    }

    pub fn lifecycle(&self) -> TabLifecycle {
        match self {
            Self::Diff | Self::Editor | Self::Terminal => TabLifecycle::KeepAlive,
            Self::Blank | Self::FileTree | Self::Git | Self::Search => TabLifecycle::Ephemeral,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stateful_tabs_are_keep_alive_tabs() {
        assert_eq!(TabKind::Terminal.lifecycle(), TabLifecycle::KeepAlive);
        assert_eq!(TabKind::Editor.lifecycle(), TabLifecycle::KeepAlive);
        assert_eq!(TabKind::Diff.lifecycle(), TabLifecycle::KeepAlive);
    }
}
