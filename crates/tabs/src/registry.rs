use icons::IconName;

pub struct TabKind {
    pub id: &'static str,
    pub name: &'static str,
    pub icon: IconName,
    pub is_hidden: bool,
}

pub const BLANK: TabKind = TabKind {
    id: "blank",
    name: "Blank",
    icon: IconName::EmptyWindow,
    is_hidden: true,
};

pub const FILE_TREE: TabKind = TabKind {
    id: "file_tree",
    name: "File Tree",
    icon: IconName::ListTree,
    is_hidden: false,
};

pub const FILE_SEARCH: TabKind = TabKind {
    id: "file_search",
    name: "Search",
    icon: IconName::Search,
    is_hidden: false,
};

pub const GIT: TabKind = TabKind {
    id: "git",
    name: "Git",
    icon: IconName::SourceControl,
    is_hidden: false,
};

pub const TERMINAL: TabKind = TabKind {
    id: "terminal",
    name: "Terminal",
    icon: IconName::Terminal,
    is_hidden: false,
};

pub const SETTINGS: TabKind = TabKind {
    id: "settings",
    name: "Settings",
    icon: IconName::SettingsGear,
    is_hidden: false,
};

pub const FILE_EDITOR: TabKind = TabKind {
    id: "file_editor",
    name: "Editor",
    icon: IconName::File,
    is_hidden: true,
};

pub const ALL: &[&TabKind] = &[
    &BLANK,
    &FILE_TREE,
    &FILE_SEARCH,
    &GIT,
    &TERMINAL,
    &SETTINGS,
    &FILE_EDITOR,
];

pub fn get(id: &str) -> Option<&'static TabKind> {
    ALL.iter().copied().find(|kind| kind.id == id)
}
