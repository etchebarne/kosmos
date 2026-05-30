pub struct TabKind {
    pub id: &'static str,
    pub name: &'static str,
    pub is_hidden: bool,
}

pub const BLANK: TabKind = TabKind {
    id: "blank",
    name: "Blank",
    is_hidden: true,
};

pub const FILE_TREE: TabKind = TabKind {
    id: "file_tree",
    name: "File Tree",
    is_hidden: false,
};

pub const FILE_SEARCH: TabKind = TabKind {
    id: "file_search",
    name: "Search",
    is_hidden: false,
};

pub const GIT: TabKind = TabKind {
    id: "git",
    name: "Git",
    is_hidden: false,
};

pub const DIFF: TabKind = TabKind {
    id: "diff",
    name: "Diff",
    is_hidden: true,
};

pub const TERMINAL: TabKind = TabKind {
    id: "terminal",
    name: "Terminal",
    is_hidden: false,
};

pub const SETTINGS: TabKind = TabKind {
    id: "settings",
    name: "Settings",
    is_hidden: false,
};

pub const FILE_EDITOR: TabKind = TabKind {
    id: "file_editor",
    name: "Editor",
    is_hidden: true,
};

pub const ALL: &[&TabKind] = &[
    &BLANK,
    &FILE_TREE,
    &FILE_SEARCH,
    &GIT,
    &DIFF,
    &TERMINAL,
    &SETTINGS,
    &FILE_EDITOR,
];

pub fn get(id: &str) -> Option<&'static TabKind> {
    ALL.iter().copied().find(|kind| kind.id == id)
}
