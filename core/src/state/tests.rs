use std::path::PathBuf;

use super::*;
use crate::formatters::{DocumentFormattingRequest, FormatterError, FormattingError};
use crate::language_servers::{LanguageServerRequestCancellation, StagedWorkspaceEditOperation};
use crate::tabs::editor::EditorError;
use crate::tabs::git::{GitChangeKind, GitRepository};
use crate::tabs::search::SearchMode;
use crate::tree::SplitAxis;

#[test]
fn format_document_rejects_zero_tab_size() {
    let state = State::new();
    let cancellation = LanguageServerRequestCancellation::new();

    let result = state.format_document(
        DocumentFormattingRequest {
            workspace_id: WorkspaceId::new(1),
            path: "missing.rs",
            language_id: "rust",
            generation: 1,
            version: 1,
            text: "",
            options: crate::language_servers::LanguageServerFormattingOptions {
                tab_size: 0,
                insert_spaces: true,
            },
        },
        &cancellation,
    );

    assert!(matches!(
        result,
        Err(FormattingError::Formatter(FormatterError::InvalidOptions(
            _
        )))
    ));
}

#[test]
fn opening_workspace_creates_active_workspace() {
    let mut state = State::new();

    let workspace_id = state.open_workspace("/workspaces/main");

    assert_eq!(workspace_id, WorkspaceId::new(1));
    assert_eq!(state.workspaces().active_workspace_id(), Some(workspace_id));
    assert_eq!(state.workspaces().workspaces().len(), 1);
}

#[test]
fn persistent_candidates_are_isolated_until_commit() {
    let mut state = State::new();
    state.open_workspace("/workspaces/first");
    let mut candidate = state.persistent_candidate();

    candidate.state_mut().open_workspace("/workspaces/second");

    assert_eq!(state.workspaces().workspaces().len(), 1);

    assert!(state.commit_persistent_candidate(candidate));

    assert_eq!(state.workspaces().workspaces().len(), 2);
    assert_eq!(
        state
            .workspaces()
            .active_workspace()
            .expect("workspace should be active")
            .directory(),
        Path::new("/workspaces/second")
    );
}

#[test]
fn committing_candidates_preserves_valid_terminal_sessions() {
    let root = test_directory("persistent-terminal");
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::Terminal,
    ));
    state
        .open_terminal(Some(workspace_id), TabId::new(1), 80, 24)
        .expect("terminal should open");
    let mut candidate = state.persistent_candidate();
    assert!(candidate.state_mut().open_tab(
        Some(workspace_id),
        Some(PaneId::new(1)),
        None,
        TabKind::Search,
    ));

    assert!(state.commit_persistent_candidate(candidate));

    assert_eq!(state.terminal_sessions.len(), 1);
    assert!(
        state
            .read_terminal_output(Some(workspace_id), TabId::new(1))
            .is_ok()
    );

    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn committing_candidates_removes_invalid_terminal_sessions() {
    let root = test_directory("closed-persistent-terminal");
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::Terminal,
    ));
    state
        .open_terminal(Some(workspace_id), TabId::new(1), 80, 24)
        .expect("terminal should open");
    let mut candidate = state.persistent_candidate();
    assert!(candidate.state_mut().set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::Search,
    ));

    assert!(state.commit_persistent_candidate(candidate));

    assert_eq!(state.terminal_sessions.len(), 0);

    drop(state);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn stale_and_cross_state_candidates_are_rejected() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");
    let first_candidate = state.persistent_candidate();
    let stale_candidate = state.persistent_candidate();
    let other_candidate = State::new().persistent_candidate();

    assert!(state.commit_persistent_candidate(first_candidate));
    assert!(!state.commit_persistent_candidate(stale_candidate));
    assert!(!state.commit_persistent_candidate(other_candidate));

    let candidate_before_direct_mutation = state.persistent_candidate();
    state.open_workspace("/workspaces/direct");

    assert!(!state.commit_persistent_candidate(candidate_before_direct_mutation));
    assert_eq!(state.workspaces().workspaces().len(), 2);
}

#[test]
fn opening_existing_workspace_path_activates_existing_workspace() {
    let mut state = State::new();

    let first_workspace_id = state.open_workspace("/workspaces/first");
    let second_workspace_id = state.open_workspace("/workspaces/second");
    let reopened_workspace_id = state.open_workspace("/workspaces/first");

    assert_eq!(second_workspace_id, WorkspaceId::new(2));
    assert_eq!(reopened_workspace_id, first_workspace_id);
    assert_eq!(
        state.workspaces().active_workspace_id(),
        Some(first_workspace_id)
    );
    assert_eq!(state.workspaces().workspaces().len(), 2);
    assert_eq!(
        state.open_workspace("/workspaces/third"),
        WorkspaceId::new(3)
    );
}

#[test]
fn opening_tab_adds_it_to_active_pane() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");

    assert!(state.open_tab(None, None, None, TabKind::Search));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");
    let pane = workspace
        .active_pane()
        .expect("workspace should have an active pane");

    assert_eq!(pane.tabs().len(), 2);
    assert_eq!(pane.active_tab().title(), "Search");
}

#[test]
fn generic_tab_operations_cannot_create_specialized_tabs() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");

    assert!(!state.open_tab(None, None, None, TabKind::Diff));
    assert!(!state.open_tab(None, None, None, TabKind::Editor));
    assert!(!state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Diff,));
    assert!(!state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Editor,));

    let active_tab = state
        .workspaces()
        .active_workspace()
        .expect("workspace should exist")
        .active_pane()
        .expect("pane should exist")
        .active_tab();

    assert_eq!(active_tab.kind(), &TabKind::Blank);
    assert!(state.git_diff_view_states().is_empty());
    assert!(state.editor_view_states().is_empty());
}

#[test]
fn restoring_diff_tabs_requires_exactly_one_view_state() {
    let workspace_id = WorkspaceId::new(1);
    let tab_id = TabId::new(1);
    let workspace = Workspace::new(
        workspace_id,
        "/workspaces/main",
        Pane::new(PaneId::new(1), Tab::new(tab_id, "Diff", TabKind::Diff)),
    );

    assert!(State::from_workspaces(vec![workspace.clone()], Some(workspace_id)).is_none());
    assert!(
        State::from_workspaces_with_view_states(
            vec![workspace],
            Some(workspace_id),
            Vec::new(),
            vec![
                GitDiffViewState::new(workspace_id, tab_id, "README.md"),
                GitDiffViewState::new(workspace_id, tab_id, "README.md"),
            ],
        )
        .is_none()
    );
}

#[test]
fn restoring_editor_tabs_requires_unique_normalized_view_state() {
    let workspace_id = WorkspaceId::new(1);
    let first_tab_id = TabId::new(1);
    let second_tab_id = TabId::new(2);
    let mut pane = Pane::new(
        PaneId::new(1),
        Tab::new(first_tab_id, "main.rs", TabKind::Editor),
    );
    pane.add_tab(Tab::new(second_tab_id, "lib.rs", TabKind::Editor));
    let workspace = Workspace::new(workspace_id, "/workspaces/main", pane);

    assert!(State::from_workspaces(vec![workspace.clone()], Some(workspace_id)).is_none());
    assert!(
        State::from_workspaces_with_all_view_states(
            vec![workspace.clone()],
            Some(workspace_id),
            Vec::new(),
            Vec::new(),
            vec![
                EditorViewState::new(workspace_id, first_tab_id, "src/main.rs"),
                EditorViewState::new(workspace_id, second_tab_id, "src/main.rs"),
            ],
        )
        .is_none()
    );
    assert!(
        State::from_workspaces_with_all_view_states(
            vec![workspace],
            Some(workspace_id),
            Vec::new(),
            Vec::new(),
            vec![
                EditorViewState::new(workspace_id, first_tab_id, "src/../main.rs"),
                EditorViewState::new(workspace_id, second_tab_id, "src/lib.rs"),
            ],
        )
        .is_none()
    );
}

#[test]
fn setting_tab_kind_updates_kind_and_default_title() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");

    assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Git));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");
    let pane = workspace
        .active_pane()
        .expect("workspace should have an active pane");

    assert_eq!(pane.active_tab().title(), "Git");
    assert_eq!(pane.active_tab().kind(), &TabKind::Git);
}

#[test]
fn opening_git_diff_tab_places_it_in_largest_pane() {
    let mut state = State::new();
    let workspace_id = state.open_workspace("/workspaces/main");
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::Git,
    ));
    assert!(state.split_pane(
        Some(workspace_id),
        Some(PaneId::new(1)),
        SplitAxis::Horizontal,
        false,
    ));
    assert!(state.resize_split(Some(workspace_id), SplitPaneId::new(1), 0.7));

    state
        .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
        .expect("diff tab should open");

    let workspace = state
        .workspaces()
        .workspace(workspace_id)
        .expect("workspace should exist");
    let largest_pane = workspace
        .root()
        .find_pane(PaneId::new(1))
        .expect("largest pane should exist");

    assert_eq!(workspace.active_pane_id(), PaneId::new(1));
    assert_eq!(largest_pane.active_tab_id(), TabId::new(3));
    assert_eq!(largest_pane.active_tab().kind(), &TabKind::Diff);
    assert_eq!(state.git_diff_view_states()[0].path(), "src/main.rs");
}

#[test]
fn opening_existing_git_diff_tab_reuses_it_and_updates_focus_path() {
    let mut state = State::new();
    let workspace_id = state.open_workspace("/workspaces/main");
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::Git,
    ));

    state
        .open_git_diff_tab(Some(workspace_id), TabId::new(1), "README.md")
        .expect("diff tab should open");
    state
        .open_git_diff_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
        .expect("existing diff tab should activate");

    let workspace = state
        .workspaces()
        .workspace(workspace_id)
        .expect("workspace should exist");
    let pane = workspace
        .root()
        .find_pane(PaneId::new(1))
        .expect("pane should exist");

    assert_eq!(pane.tabs().len(), 2);
    assert_eq!(state.git_diff_view_states().len(), 1);
    assert_eq!(state.git_diff_view_states()[0].path(), "src/main.rs");
    assert_eq!(pane.active_tab_id(), TabId::new(2));
    assert_eq!(pane.active_tab().title(), "Diff");
}

#[test]
fn editor_tabs_use_the_largest_pane_and_reuse_only_the_same_path() {
    let root = test_directory("editor-tabs");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn library() {}").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    assert!(state.split_pane(
        Some(workspace_id),
        Some(PaneId::new(1)),
        SplitAxis::Horizontal,
        false,
    ));
    assert!(state.resize_split(Some(workspace_id), SplitPaneId::new(1), 0.7));

    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
        .unwrap();
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/lib.rs")
        .unwrap();
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
        .unwrap();

    let workspace = state.workspaces().workspace(workspace_id).unwrap();
    let largest_pane = workspace.root().find_pane(PaneId::new(1)).unwrap();
    let smaller_pane = workspace.root().find_pane(PaneId::new(2)).unwrap();

    assert_eq!(workspace.active_pane_id(), PaneId::new(1));
    assert_eq!(largest_pane.active_tab_id(), TabId::new(3));
    assert_eq!(largest_pane.active_tab().title(), "main.rs");
    assert_eq!(largest_pane.tabs().len(), 3);
    assert_eq!(smaller_pane.tabs().len(), 1);
    assert_eq!(state.editor_view_states().len(), 2);
    assert_eq!(state.editor_view_states()[0].path(), "src/main.rs");
    assert_eq!(state.editor_view_states()[1].path(), "src/lib.rs");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn workspace_resource_rename_updates_and_rolls_back_editor_paths_and_titles() {
    let root = test_directory("editor-resource-rename");
    std::fs::create_dir(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
        .unwrap();
    let operations = vec![StagedWorkspaceEditOperation::RenameFile {
        workspace_id,
        old_path: "src/main.rs".to_owned(),
        new_path: "src/renamed.rs".to_owned(),
    }];

    state.reconcile_workspace_edit_resources(1, &operations, false);
    assert_eq!(state.editor_view_states()[0].path(), "src/renamed.rs");
    assert_eq!(
        state
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .root()
            .find_pane(PaneId::new(1))
            .unwrap()
            .active_tab()
            .title(),
        "renamed.rs"
    );
    state.reconcile_workspace_edit_resources(1, &operations, true);
    assert_eq!(state.editor_view_states()[0].path(), "src/main.rs");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn overwrite_rename_removes_destination_tab_and_restores_both_tabs_on_rollback() {
    let root = test_directory("editor-overwrite-rename");
    std::fs::write(root.join("source.rs"), "source").unwrap();
    std::fs::write(root.join("destination.rs"), "destination").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "source.rs")
        .unwrap();
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "destination.rs")
        .unwrap();
    assert_eq!(state.editor_view_states().len(), 2);
    let operations = vec![StagedWorkspaceEditOperation::RenameFile {
        workspace_id,
        old_path: "source.rs".to_owned(),
        new_path: "destination.rs".to_owned(),
    }];

    state.reconcile_workspace_edit_resources(11, &operations, false);
    assert_eq!(state.editor_view_states().len(), 1);
    assert_eq!(state.editor_view_states()[0].path(), "destination.rs");

    state.reconcile_workspace_edit_resources(11, &operations, true);
    let mut paths = state
        .editor_view_states()
        .iter()
        .map(|state| state.path())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    assert_eq!(paths, ["destination.rs", "source.rs"]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn overwrite_create_removes_old_tab_lineage_and_persists_replacement_semantics() {
    let root = test_directory("editor-overwrite-create-lineage");
    let database_root = test_directory("editor-overwrite-create-lineage-store");
    std::fs::write(root.join("a.rs"), "old content").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "a.rs")
        .unwrap();
    let operations = vec![
        StagedWorkspaceEditOperation::CreateFile {
            workspace_id,
            path: "a.rs".to_owned(),
        },
        StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "a.rs".to_owned(),
            new_path: "b.rs".to_owned(),
        },
        StagedWorkspaceEditOperation::TextDocument { document: 0 },
    ];

    state.reconcile_workspace_edit_resources(15, &operations, false);
    assert!(state.editor_view_states().is_empty());
    assert_eq!(
        state
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .root()
            .find_pane(PaneId::new(1))
            .unwrap()
            .tabs()
            .iter()
            .find(|tab| tab.id() == TabId::new(2))
            .unwrap()
            .kind(),
        &TabKind::Blank
    );

    let store = crate::persistence::StateStore::open(database_root.join("state.sqlite3")).unwrap();
    store.save(&state).unwrap();
    assert!(store.load().unwrap().editor_view_states().is_empty());

    state.reconcile_workspace_edit_resources(15, &operations, true);
    assert_eq!(state.editor_view_states().len(), 1);
    assert_eq!(state.editor_view_states()[0].path(), "a.rs");
    assert_eq!(
        state
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .root()
            .find_pane(PaneId::new(1))
            .unwrap()
            .tabs()
            .iter()
            .find(|tab| tab.id() == TabId::new(2))
            .unwrap()
            .kind(),
        &TabKind::Editor
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(database_root);
}

#[test]
fn persisted_overwrite_rename_restores_tabs_independent_of_tab_id_order() {
    let root = test_directory("editor-overwrite-rename-persistence");
    let database_root = test_directory("editor-overwrite-rename-persistence-store");
    let database = database_root.join("state.sqlite3");
    std::fs::write(root.join("source.rs"), "source").unwrap();
    std::fs::write(root.join("destination.rs"), "destination").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "destination.rs")
        .unwrap();
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "source.rs")
        .unwrap();
    let operations = vec![StagedWorkspaceEditOperation::RenameFile {
        workspace_id,
        old_path: "source.rs".to_owned(),
        new_path: "destination.rs".to_owned(),
    }];
    state.reconcile_workspace_edit_resources(14, &operations, false);
    let store = crate::persistence::StateStore::open(&database).unwrap();
    store.save(&state).unwrap();

    store.restore_workspace_edit_editor_recovery(14).unwrap();
    let restored = store.load().unwrap();

    let paths = restored
        .editor_view_states()
        .iter()
        .map(|state| (state.tab_id(), state.path()))
        .collect::<HashMap<_, _>>();
    assert_eq!(paths[&TabId::new(2)], "destination.rs");
    assert_eq!(paths[&TabId::new(3)], "source.rs");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(database_root);
}

#[test]
fn file_resource_chain_restores_original_tab_identity_view_and_title() {
    let root = test_directory("editor-file-resource-chain");
    std::fs::write(root.join("first.rs"), "first").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "first.rs")
        .unwrap();
    state
        .workspace_mut(workspace_id)
        .unwrap()
        .root_mut()
        .find_pane_mut(PaneId::new(1))
        .unwrap()
        .rename_tab(TabId::new(2), "Pinned source");
    let operations = vec![
        StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "first.rs".to_owned(),
            new_path: "second.rs".to_owned(),
        },
        StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "second.rs".to_owned(),
            new_path: "third.rs".to_owned(),
        },
        StagedWorkspaceEditOperation::DeleteFile {
            workspace_id,
            path: "third.rs".to_owned(),
            recursive: false,
        },
    ];

    state.reconcile_workspace_edit_resources(12, &operations, false);
    assert!(state.editor_view_states().is_empty());
    state.reconcile_workspace_edit_resources(12, &operations, true);

    assert_eq!(state.editor_view_states().len(), 1);
    assert_eq!(state.editor_view_states()[0].tab_id(), TabId::new(2));
    assert_eq!(state.editor_view_states()[0].path(), "first.rs");
    let tab = state
        .workspaces()
        .workspace(workspace_id)
        .unwrap()
        .root()
        .find_pane(PaneId::new(1))
        .unwrap()
        .tabs()
        .iter()
        .find(|tab| tab.id() == TabId::new(2))
        .unwrap();
    assert_eq!(tab.kind(), &TabKind::Editor);
    assert_eq!(tab.title(), "Pinned source");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn directory_resource_chain_persists_exact_editor_recovery() {
    let root = test_directory("editor-directory-resource-chain");
    let database_root = test_directory("editor-directory-resource-chain-store");
    let database = database_root.join("state.sqlite3");
    std::fs::create_dir_all(root.join("src/nested")).unwrap();
    std::fs::write(root.join("src/nested/main.rs"), "main").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/nested/main.rs")
        .unwrap();
    state
        .workspace_mut(workspace_id)
        .unwrap()
        .root_mut()
        .find_pane_mut(PaneId::new(1))
        .unwrap()
        .rename_tab(TabId::new(2), "Pinned nested file");
    let operations = vec![
        StagedWorkspaceEditOperation::RenameFile {
            workspace_id,
            old_path: "src".to_owned(),
            new_path: "moved".to_owned(),
        },
        StagedWorkspaceEditOperation::DeleteFile {
            workspace_id,
            path: "moved".to_owned(),
            recursive: true,
        },
    ];
    state.reconcile_workspace_edit_resources(13, &operations, false);
    let store = crate::persistence::StateStore::open(&database).unwrap();
    store.save(&state).unwrap();

    store.restore_workspace_edit_editor_recovery(13).unwrap();
    let restored = store.load().unwrap();

    assert_eq!(restored.editor_view_states().len(), 1);
    assert_eq!(restored.editor_view_states()[0].tab_id(), TabId::new(2));
    assert_eq!(
        restored.editor_view_states()[0].path(),
        "src/nested/main.rs"
    );
    let tab = restored
        .workspaces()
        .workspace(workspace_id)
        .unwrap()
        .root()
        .find_pane(PaneId::new(1))
        .unwrap()
        .tabs()
        .iter()
        .find(|tab| tab.id() == TabId::new(2))
        .unwrap();
    assert_eq!(tab.kind(), &TabKind::Editor);
    assert_eq!(tab.title(), "Pinned nested file");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(database_root);
}

#[test]
fn startup_recovers_persisted_editor_directory_rename_before_state_load() {
    let root = test_directory("editor-resource-rename-startup");
    let database_root = test_directory("editor-resource-rename-database");
    let database = database_root.join("state.sqlite3");
    std::fs::create_dir_all(root.join("src/nested")).unwrap();
    std::fs::write(root.join("src/nested/main.rs"), "fn main() {}").unwrap();
    let store = crate::persistence::StateStore::open(&database).unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/nested/main.rs")
        .unwrap();
    store.save(&state).unwrap();

    let paths = crate::language_servers::LanguageServerPaths::new(
        database_root.join("language-servers"),
        database_root.join("language-server-cache"),
    );
    let manager =
        crate::language_servers::LanguageServerManager::open(paths, store.clone()).unwrap();
    let staged = manager
        .stage_workspace_edit(
            &serde_json::json!({ "documentChanges": [{
                "kind": "rename",
                "oldUri": format!("file://{}", root.join("src").display()),
                "newUri": format!("file://{}", root.join("renamed").display())
            }]}),
            &state.workspace_edit_roots().unwrap(),
        )
        .unwrap();
    state.attach_language_server_manager(manager);
    state
        .commit_workspace_edit(staged.transaction_id, &staged.authorization)
        .unwrap();
    store.save(&state).unwrap();
    assert_eq!(
        state.editor_view_states()[0].path(),
        "renamed/nested/main.rs"
    );
    drop(state);

    let reopened_store = crate::persistence::StateStore::open(&database).unwrap();
    let reopened_paths = crate::language_servers::LanguageServerPaths::new(
        database_root.join("language-servers"),
        database_root.join("language-server-cache"),
    );
    let restarted_manager = crate::language_servers::LanguageServerManager::open(
        reopened_paths,
        reopened_store.clone(),
    )
    .unwrap();
    assert!(matches!(
        restarted_manager.workspace_edit_status(staged.transaction_id, &staged.authorization),
        Err(crate::language_servers::WorkspaceEditError::Invalid(_))
    ));
    let recovery = restarted_manager
        .workspace_edit_recoveries()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        recovery.status.phase,
        crate::language_servers::WorkspaceEditTransactionPhase::FinishedRolledBack
    );
    let recovered = reopened_store.load().unwrap();
    assert_eq!(
        recovered.editor_view_states()[0].path(),
        "src/nested/main.rs"
    );
    assert_eq!(
        recovered
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .root()
            .find_pane(PaneId::new(1))
            .unwrap()
            .active_tab()
            .title(),
        "main.rs"
    );
    assert!(root.join("src/nested/main.rs").is_file());
    assert!(!root.join("renamed").exists());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(database_root);
}

#[test]
fn finished_workspace_edit_survives_unrelated_full_save_without_stale_editor_recovery() {
    let root = test_directory("finished-editor-recovery-workspace");
    let database_root = test_directory("finished-editor-recovery-database");
    let database = database_root.join("state.sqlite3");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
    let store = crate::persistence::StateStore::open(&database).unwrap();
    let paths = crate::language_servers::LanguageServerPaths::new(
        database_root.join("language-servers"),
        database_root.join("language-server-cache"),
    );
    let manager =
        crate::language_servers::LanguageServerManager::open(paths.clone(), store.clone()).unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "src/main.rs")
        .unwrap();
    let staged = manager
        .stage_workspace_edit(
            &serde_json::json!({ "documentChanges": [{
                "kind": "rename",
                "oldUri": format!("file://{}", root.join("src").display()),
                "newUri": format!("file://{}", root.join("renamed").display())
            }]}),
            &state.workspace_edit_roots().unwrap(),
        )
        .unwrap();
    state.attach_language_server_manager(manager);
    state
        .commit_workspace_edit(staged.transaction_id, &staged.authorization)
        .unwrap();
    store.save(&state).unwrap();
    state
        .finish_workspace_edit(staged.transaction_id, &staged.authorization)
        .unwrap();
    assert_eq!(state.editor_view_states()[0].path(), "renamed/main.rs");

    assert!(state.activate_workspace(workspace_id));
    store.save(&state).unwrap();
    drop(state);

    let restarted_manager =
        crate::language_servers::LanguageServerManager::open(paths, store.clone()).unwrap();
    let restarted = store.load().unwrap();
    assert_eq!(restarted.editor_view_states()[0].path(), "renamed/main.rs");
    assert_eq!(restarted.workspace_edit_editor_recoveries().count(), 0);
    assert!(root.join("renamed/main.rs").is_file());
    assert!(!root.join("src").exists());
    let recovery = restarted_manager
        .workspace_edit_recoveries()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        recovery.status.phase,
        crate::language_servers::WorkspaceEditTransactionPhase::FinishedCommitted
    );
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(database_root);
}

#[test]
fn workspace_resource_delete_persists_and_restores_editor_tabs() {
    let root = test_directory("editor-resource-delete");
    std::fs::write(root.join("open.rs"), "fn main() {}").unwrap();
    let database = root.join("state.sqlite3");
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "open.rs")
        .unwrap();
    let operations = vec![StagedWorkspaceEditOperation::DeleteFile {
        workspace_id,
        path: "open.rs".to_owned(),
        recursive: false,
    }];
    state.reconcile_workspace_edit_resources(9, &operations, false);
    assert!(state.editor_view_states().is_empty());
    let store = crate::persistence::StateStore::open(&database).unwrap();
    store.save(&state).unwrap();
    store.restore_workspace_edit_editor_recovery(9).unwrap();
    let restored = store.load().unwrap();
    assert_eq!(restored.editor_view_states()[0].path(), "open.rs");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn opening_editor_tabs_requires_a_supported_source_and_existing_file() {
    let root = test_directory("editor-open-validation");
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);

    assert!(matches!(
        state.open_editor_tab(Some(workspace_id), TabId::new(1), "missing.txt"),
        Err(EditorError::SourceTabNotFound)
    ));
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    assert!(matches!(
        state.open_editor_tab(Some(workspace_id), TabId::new(1), "missing.txt"),
        Err(EditorError::FileNotFound(_))
    ));
    assert!(state.editor_view_states().is_empty());

    let workspace = state.workspaces().workspace(workspace_id).unwrap();
    assert_eq!(workspace.active_pane().unwrap().tabs().len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_editor_location_uses_depth_first_source_tab_selection() {
    let root = test_directory("editor-location-source-order");
    let workspace_id = WorkspaceId::new(1);
    let first = pane_with_tabs(
        1,
        vec![
            Tab::new(TabId::new(1), "Blank", TabKind::Blank),
            Tab::new(TabId::new(2), "Search", TabKind::Search),
            Tab::new(TabId::new(3), "Files", TabKind::FileTree),
        ],
    );
    let second = pane_with_tabs(2, vec![Tab::new(TabId::new(4), "Files", TabKind::FileTree)]);
    let workspace = Workspace::from_root(
        workspace_id,
        &root,
        PaneNode::split(
            SplitPaneId::new(1),
            SplitAxis::Horizontal,
            0.5,
            PaneNode::leaf(first),
            PaneNode::leaf(second),
        ),
        PaneId::new(1),
    )
    .unwrap();
    let state = State::from_workspaces(vec![workspace], Some(workspace_id)).unwrap();

    assert_eq!(
        state.editor_source_tab_id(workspace_id),
        Some(TabId::new(2))
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_editor_location_activates_an_inactive_workspace_and_opens_a_new_tab() {
    let active_root = test_directory("editor-location-active");
    let target_root = test_directory("editor-location-target");
    std::fs::write(target_root.join("document.txt"), "target").unwrap();
    let active_workspace_id = WorkspaceId::new(1);
    let target_workspace_id = WorkspaceId::new(2);
    let active_workspace = Workspace::new(
        active_workspace_id,
        &active_root,
        pane_with_tabs(1, vec![Tab::new(TabId::new(1), "Blank", TabKind::Blank)]),
    );
    let target_workspace = Workspace::new(
        target_workspace_id,
        &target_root,
        pane_with_tabs(2, vec![Tab::new(TabId::new(2), "Files", TabKind::FileTree)]),
    );
    let mut state = State::from_workspaces(
        vec![active_workspace, target_workspace],
        Some(active_workspace_id),
    )
    .unwrap();

    let result = state
        .open_editor_location(target_workspace_id, "document.txt")
        .unwrap();

    assert_eq!(result.source_tab_id(), TabId::new(2));
    assert_eq!(result.workspace_id(), target_workspace_id);
    assert_eq!(result.tab_id(), TabId::new(3));
    assert_eq!(result.path(), "document.txt");
    assert_eq!(
        result.workspaces().active_workspace_id(),
        Some(target_workspace_id)
    );
    assert_eq!(
        state.workspaces().active_workspace_id(),
        Some(target_workspace_id)
    );
    assert_eq!(
        state
            .workspaces()
            .workspace(target_workspace_id)
            .unwrap()
            .active_pane()
            .unwrap()
            .active_tab_id(),
        TabId::new(3)
    );

    let _ = std::fs::remove_dir_all(active_root);
    let _ = std::fs::remove_dir_all(target_root);
}

#[test]
fn open_editor_location_reuses_an_existing_editor_tab() {
    let root = test_directory("editor-location-existing");
    std::fs::write(root.join("document.txt"), "target").unwrap();
    let workspace_id = WorkspaceId::new(1);
    let workspace = Workspace::new(
        workspace_id,
        &root,
        pane_with_tabs(
            1,
            vec![
                Tab::new(TabId::new(1), "Files", TabKind::FileTree),
                Tab::new(TabId::new(2), "document.txt", TabKind::Editor),
            ],
        ),
    );
    let mut state = State::from_workspaces_with_all_view_states(
        vec![workspace],
        Some(workspace_id),
        Vec::new(),
        Vec::new(),
        vec![EditorViewState::new(
            workspace_id,
            TabId::new(2),
            "document.txt",
        )],
    )
    .unwrap();

    let result = state
        .open_editor_location(workspace_id, "document.txt")
        .unwrap();

    assert_eq!(result.tab_id(), TabId::new(2));
    assert_eq!(
        state
            .workspaces()
            .workspace(workspace_id)
            .unwrap()
            .active_pane()
            .unwrap()
            .tabs()
            .len(),
        2
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn open_editor_location_missing_file_is_atomic() {
    let (mut state, active_workspace_id, target_workspace_id, active_root, target_root) =
        location_state("editor-location-missing-file", true);
    let before = location_state_snapshot(&state);

    assert!(matches!(
        state.open_editor_location(target_workspace_id, "missing.txt"),
        Err(EditorError::FileNotFound(_))
    ));
    assert_location_state_unchanged(&state, &before, active_workspace_id);

    let _ = std::fs::remove_dir_all(active_root);
    let _ = std::fs::remove_dir_all(target_root);
}

#[test]
fn open_editor_location_missing_workspace_is_atomic() {
    let (mut state, active_workspace_id, _, active_root, target_root) =
        location_state("editor-location-missing-workspace", true);
    let before = location_state_snapshot(&state);

    assert!(matches!(
        state.open_editor_location(WorkspaceId::new(99), "missing.txt"),
        Err(EditorError::WorkspaceNotFound)
    ));
    assert_location_state_unchanged(&state, &before, active_workspace_id);

    let _ = std::fs::remove_dir_all(active_root);
    let _ = std::fs::remove_dir_all(target_root);
}

#[test]
fn open_editor_location_without_source_tab_is_atomic() {
    let (mut state, active_workspace_id, target_workspace_id, active_root, target_root) =
        location_state("editor-location-missing-source", false);
    let before = location_state_snapshot(&state);

    assert!(matches!(
        state.open_editor_location(target_workspace_id, "missing.txt"),
        Err(EditorError::SourceTabNotFound)
    ));
    assert_location_state_unchanged(&state, &before, active_workspace_id);

    let _ = std::fs::remove_dir_all(active_root);
    let _ = std::fs::remove_dir_all(target_root);
}

#[test]
fn search_tabs_query_preview_and_open_editor_results() {
    let root = test_directory("search-tab");
    std::fs::write(root.join("notes.txt"), "first\nSearch target\n").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);

    assert!(matches!(
        state.search_workspace(
            Some(workspace_id),
            TabId::new(1),
            "target",
            SearchMode::Content,
        ),
        Err(SearchError::TabNotFound)
    ));
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::Search,
    ));

    let results = state
        .search_workspace(
            Some(workspace_id),
            TabId::new(1),
            "target",
            SearchMode::Content,
        )
        .unwrap();
    assert_eq!(results.matches().len(), 1);
    assert_eq!(results.matches()[0].line_number(), Some(2));
    let document = state
        .search_document(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();
    assert_eq!(document.content(), "first\nSearch target\n");

    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();
    let workspace = state.workspaces().workspace(workspace_id).unwrap();
    assert_eq!(
        workspace.active_pane().unwrap().active_tab().kind(),
        &TabKind::Editor
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editor_document_uses_tab_state_and_saves_existing_file() {
    let root = test_directory("editor-document");
    std::fs::write(root.join("notes.txt"), "before").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();

    let document = state
        .editor_document(Some(workspace_id), TabId::new(2))
        .unwrap();
    assert_eq!(document.content(), "before");

    state
        .save_editor_document(Some(workspace_id), TabId::new(2), "after")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("notes.txt")).unwrap(),
        "after"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_preserve_staged_status() {
    let root = test_directory("file-tree-git-staged");
    GitRepository::init(&root).expect("repository should initialize");
    std::fs::write(root.join("staged.txt"), "staged\n").expect("file should be written");
    GitRepository::stage_paths(&root, &["staged.txt".to_owned()]).expect("file should be staged");
    let (state, workspace_id) = file_tree_git_state(&root);

    let decorations = state
        .file_tree_git_decorations(Some(workspace_id), TabId::new(1))
        .expect("decorations should load");

    assert_eq!(decorations.entries().len(), 1);
    assert_eq!(decorations.entries()[0].path(), "staged.txt");
    assert_eq!(
        decorations.entries()[0].staged(),
        Some(GitChangeKind::Added)
    );
    assert_eq!(decorations.entries()[0].unstaged(), None);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_preserve_unstaged_status() {
    let root = test_directory("file-tree-git-unstaged");
    let path = root.join("tracked.txt");
    GitRepository::init(&root).expect("repository should initialize");
    std::fs::write(&path, "before\n").expect("file should be written");
    GitRepository::stage_paths(&root, &["tracked.txt".to_owned()]).expect("file should be staged");
    commit_git(&root, "Initial");
    std::fs::write(&path, "after\n").expect("file should be changed");
    let (state, workspace_id) = file_tree_git_state(&root);

    let decorations = state
        .file_tree_git_decorations(Some(workspace_id), TabId::new(1))
        .expect("decorations should load");

    assert_eq!(decorations.entries().len(), 1);
    assert_eq!(decorations.entries()[0].path(), "tracked.txt");
    assert_eq!(decorations.entries()[0].staged(), None);
    assert_eq!(
        decorations.entries()[0].unstaged(),
        Some(GitChangeKind::Modified)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_preserve_staged_and_unstaged_statuses() {
    let root = test_directory("file-tree-git-both");
    let path = root.join("tracked.txt");
    GitRepository::init(&root).expect("repository should initialize");
    std::fs::write(&path, "before\n").expect("file should be written");
    GitRepository::stage_paths(&root, &["tracked.txt".to_owned()]).expect("file should be staged");
    commit_git(&root, "Initial");
    std::fs::write(&path, "staged\n").expect("file should be changed");
    GitRepository::stage_paths(&root, &["tracked.txt".to_owned()]).expect("file should be staged");
    std::fs::write(&path, "unstaged\n").expect("file should be changed");
    let (state, workspace_id) = file_tree_git_state(&root);

    let decorations = state
        .file_tree_git_decorations(Some(workspace_id), TabId::new(1))
        .expect("decorations should load");

    assert_eq!(decorations.entries().len(), 1);
    assert_eq!(decorations.entries()[0].path(), "tracked.txt");
    assert_eq!(
        decorations.entries()[0].staged(),
        Some(GitChangeKind::Modified)
    );
    assert_eq!(
        decorations.entries()[0].unstaged(),
        Some(GitChangeKind::Modified)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_are_empty_for_a_clean_repository() {
    let root = test_directory("file-tree-git-clean");
    GitRepository::init(&root).expect("repository should initialize");
    let (state, workspace_id) = file_tree_git_state(&root);

    let decorations = state
        .file_tree_git_decorations(Some(workspace_id), TabId::new(1))
        .expect("decorations should load");

    assert!(decorations.entries().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_scope_paths_to_a_nested_workspace() {
    let root = test_directory("file-tree-git-nested");
    let workspace = root.join("packages/app");
    std::fs::create_dir_all(&workspace).expect("workspace should be created");
    GitRepository::init(&root).expect("repository should initialize");
    std::fs::write(root.join("outside.txt"), "outside\n").expect("file should be written");
    std::fs::write(workspace.join("inside.txt"), "inside\n").expect("file should be written");
    let (state, workspace_id) = file_tree_git_state(&workspace);

    let decorations = state
        .file_tree_git_decorations(Some(workspace_id), TabId::new(1))
        .expect("decorations should load");

    assert_eq!(decorations.entries().len(), 1);
    assert_eq!(decorations.entries()[0].path(), "inside.txt");
    assert_eq!(
        decorations.entries()[0].unstaged(),
        Some(GitChangeKind::Untracked)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_are_empty_without_a_repository() {
    let root = test_directory("file-tree-git-no-repository");
    std::fs::write(root.join("notes.txt"), "notes\n").expect("file should be written");
    let (state, workspace_id) = file_tree_git_state(&root);

    let decorations = state
        .file_tree_git_decorations(Some(workspace_id), TabId::new(1))
        .expect("a non-repository should have no decorations");

    assert!(decorations.entries().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_git_decorations_report_invalid_workspace_and_tab() {
    let root = test_directory("file-tree-git-invalid-target");
    let (state, workspace_id) = file_tree_git_state(&root);

    assert!(matches!(
        state.file_tree_git_decorations(Some(WorkspaceId::new(99)), TabId::new(1)),
        Err(FileTreeGitDecorationsError::FileTree(
            FileTreeError::WorkspaceNotFound
        ))
    ));
    assert!(matches!(
        state.file_tree_git_decorations(Some(workspace_id), TabId::new(99)),
        Err(FileTreeGitDecorationsError::FileTree(
            FileTreeError::TabNotFound
        ))
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editor_git_line_hunks_use_the_editor_view_path() {
    let root = test_directory("editor-git-line-hunks");
    std::fs::write(root.join("notes.txt"), "first\nsecond\n").unwrap();
    GitRepository::init(&root).expect("repository should initialize");
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();

    let hunks = state
        .editor_git_line_hunks(Some(workspace_id), TabId::new(2))
        .expect("editor line hunks should load");

    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].new_start(), 1);
    assert_eq!(hunks[0].new_lines(), 2);
    assert!(matches!(
        state.editor_git_line_hunks(Some(workspace_id), TabId::new(1)),
        Err(GitError::TabNotFound)
    ));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn editor_git_line_hunks_are_empty_without_a_repository() {
    let root = test_directory("editor-git-line-hunks-no-repository");
    std::fs::write(root.join("notes.txt"), "first\nsecond\n").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();

    let hunks = state
        .editor_git_line_hunks(Some(workspace_id), TabId::new(2))
        .expect("a non-repository should have no line hunks");

    assert!(hunks.is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn closing_editor_tabs_and_workspaces_removes_view_state() {
    let root = test_directory("editor-cleanup");
    std::fs::write(root.join("notes.txt"), "notes").unwrap();
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));
    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();

    assert!(
        state
            .close_tab(Some(workspace_id), PaneId::new(1), TabId::new(2))
            .is_some()
    );
    assert!(state.editor_view_states().is_empty());

    state
        .open_editor_tab(Some(workspace_id), TabId::new(1), "notes.txt")
        .unwrap();
    assert!(state.close_workspace(Some(workspace_id)).is_some());
    assert!(state.editor_view_states().is_empty());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn splitting_tab_moves_it_to_a_new_pane() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");
    state.open_tab(None, None, None, TabKind::Search);

    assert!(state.split_tab(
        None,
        PaneId::new(1),
        PaneId::new(1),
        TabId::new(2),
        SplitAxis::Horizontal,
        false,
    ));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");

    assert_eq!(workspace.root().pane_count(), 2);
    assert_eq!(workspace.active_pane_id(), PaneId::new(2));

    let source_pane = workspace
        .root()
        .find_pane(PaneId::new(1))
        .expect("source pane should remain");
    let new_pane = workspace
        .root()
        .find_pane(PaneId::new(2))
        .expect("new pane should exist");

    assert_eq!(source_pane.tabs().len(), 1);
    assert_eq!(new_pane.active_tab().id(), TabId::new(2));
}

#[test]
fn splitting_only_tab_keeps_source_pane_valid() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");

    assert!(state.split_tab(
        None,
        PaneId::new(1),
        PaneId::new(1),
        TabId::new(1),
        SplitAxis::Vertical,
        false,
    ));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");
    let source_pane = workspace
        .root()
        .find_pane(PaneId::new(1))
        .expect("source pane should remain");
    let new_pane = workspace
        .root()
        .find_pane(PaneId::new(2))
        .expect("new pane should exist");

    assert_eq!(source_pane.tabs().len(), 1);
    assert_eq!(new_pane.active_tab().id(), TabId::new(1));
}

#[test]
fn moving_pane_reuses_existing_pane() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");
    assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

    assert!(state.move_pane(
        None,
        PaneId::new(1),
        PaneId::new(2),
        SplitAxis::Vertical,
        false,
    ));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");

    assert_eq!(workspace.root().pane_count(), 2);
    assert!(workspace.root().contains_pane(PaneId::new(1)));
    assert!(workspace.root().contains_pane(PaneId::new(2)));
    assert_eq!(workspace.active_pane_id(), PaneId::new(1));
}

#[test]
fn moving_tab_to_another_pane_adds_it_to_target_pane() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");
    state.open_tab(None, None, None, TabKind::Search);
    assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

    assert!(state.move_tab(None, PaneId::new(1), PaneId::new(2), TabId::new(2), 1,));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");
    let source_pane = workspace
        .root()
        .find_pane(PaneId::new(1))
        .expect("source pane should remain");
    let target_pane = workspace
        .root()
        .find_pane(PaneId::new(2))
        .expect("target pane should exist");

    assert_eq!(source_pane.tabs().len(), 1);
    assert_eq!(
        target_pane.tabs().iter().map(Tab::id).collect::<Vec<_>>(),
        vec![TabId::new(3), TabId::new(2)]
    );
    assert_eq!(target_pane.active_tab_id(), TabId::new(2));
    assert_eq!(workspace.active_pane_id(), PaneId::new(2));
}

#[test]
fn moving_last_tab_to_another_pane_removes_source_pane() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");
    assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

    assert!(state.move_tab(
        None,
        PaneId::new(1),
        PaneId::new(2),
        TabId::new(1),
        usize::MAX,
    ));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");
    let target_pane = workspace
        .root()
        .find_pane(PaneId::new(2))
        .expect("target pane should exist");

    assert_eq!(workspace.root().pane_count(), 1);
    assert!(!workspace.root().contains_pane(PaneId::new(1)));
    assert_eq!(
        target_pane.tabs().iter().map(Tab::id).collect::<Vec<_>>(),
        vec![TabId::new(2), TabId::new(1)]
    );
    assert_eq!(target_pane.active_tab_id(), TabId::new(1));
    assert_eq!(workspace.active_pane_id(), PaneId::new(2));
}

#[test]
fn resizing_split_updates_server_owned_ratio() {
    let mut state = State::new();
    state.open_workspace("/workspaces/main");
    assert!(state.split_pane(None, None, SplitAxis::Horizontal, false));

    assert!(state.resize_split(None, SplitPaneId::new(1), 0.7));
    assert!(!state.resize_split(None, SplitPaneId::new(1), 1.0));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("workspace should be active");
    let crate::tree::PaneNode::Split(split) = workspace.root() else {
        panic!("workspace root should be split");
    };

    assert_eq!(split.ratio(), 0.7);
}

#[test]
fn resized_split_survives_workspace_switches() {
    let mut state = State::new();
    let first_workspace_id = state.open_workspace("/workspaces/first");
    assert!(state.split_pane(Some(first_workspace_id), None, SplitAxis::Horizontal, false,));
    assert!(state.resize_split(Some(first_workspace_id), SplitPaneId::new(1), 0.7));

    state.open_workspace("/workspaces/second");
    assert!(state.activate_workspace(first_workspace_id));

    let workspace = state
        .workspaces()
        .active_workspace()
        .expect("first workspace should be active again");
    let crate::tree::PaneNode::Split(split) = workspace.root() else {
        panic!("workspace root should be split");
    };

    assert_eq!(split.ratio(), 0.7);
}

#[test]
fn file_tree_requires_a_workspace() {
    let state = State::new();

    let error = state
        .file_tree(None, None)
        .expect_err("missing workspace should fail");

    assert!(matches!(error, FileTreeError::WorkspaceNotFound));
}

#[test]
fn file_tree_expanded_paths_are_stored_for_file_tree_tabs() {
    let root = test_directory("file-tree-state");
    std::fs::create_dir(root.join("src")).expect("test directory should be created");
    let mut state = State::new();
    let workspace_id = state.open_workspace(&root);
    assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));

    assert!(state.set_file_tree_expanded_paths(
        Some(workspace_id),
        TabId::new(1),
        vec!["src".to_owned(), "missing".to_owned()],
    ));

    let file_tree = state
        .file_tree(Some(workspace_id), Some(TabId::new(1)))
        .expect("file tree should load");

    assert_eq!(file_tree.expanded_paths(), &["src/"]);
    assert_eq!(state.file_tree_view_states().len(), 1);

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_tree_expanded_paths_require_a_file_tree_tab() {
    let mut state = State::new();
    let workspace_id = state.open_workspace("/workspaces/main");

    assert!(!state.set_file_tree_expanded_paths(
        Some(workspace_id),
        TabId::new(1),
        vec!["src".to_owned()],
    ));

    let error = state
        .file_tree(Some(workspace_id), Some(TabId::new(1)))
        .expect_err("blank tabs should not expose file tree state");

    assert!(matches!(error, FileTreeError::TabNotFound));
}

#[test]
fn terminal_sessions_require_a_terminal_tab() {
    let mut state = State::new();
    let workspace_id = state.open_workspace("/workspaces/main");

    let error = state
        .read_terminal_output(Some(workspace_id), TabId::new(1))
        .expect_err("blank tabs should not expose terminal sessions");

    assert!(matches!(error, TerminalError::TabNotFound));
    assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::Terminal));

    let error = state
        .read_terminal_output(Some(workspace_id), TabId::new(1))
        .expect_err("terminal tabs should require a started session");

    assert!(matches!(error, TerminalError::SessionNotFound));
}

#[test]
fn closing_tab_removes_file_tree_view_state() {
    let mut state = State::new();
    let workspace_id = state.open_workspace("/workspaces/main");
    assert!(state.set_tab_kind(None, PaneId::new(1), TabId::new(1), TabKind::FileTree));
    assert!(state.set_file_tree_expanded_paths(
        Some(workspace_id),
        TabId::new(1),
        vec!["src".to_owned()],
    ));

    assert!(
        state
            .close_tab(Some(workspace_id), PaneId::new(1), TabId::new(1))
            .is_some()
    );

    assert!(state.file_tree_view_states().is_empty());
}

type LocationStateSnapshot = (
    WorkspaceList,
    Vec<FileTreeViewState>,
    Vec<GitDiffViewState>,
    Vec<EditorViewState>,
    u64,
    PersistenceScope,
);

fn pane_with_tabs(id: u64, tabs: Vec<Tab>) -> Pane {
    let mut tabs = tabs.into_iter();
    let mut pane = Pane::new(PaneId::new(id), tabs.next().expect("pane requires a tab"));
    for tab in tabs {
        pane.add_tab(tab);
    }
    pane
}

fn file_tree_git_state(root: &Path) -> (State, WorkspaceId) {
    let mut state = State::new();
    let workspace_id = state.open_workspace(root);
    assert!(state.set_tab_kind(
        Some(workspace_id),
        PaneId::new(1),
        TabId::new(1),
        TabKind::FileTree,
    ));

    (state, workspace_id)
}

fn commit_git(root: &Path, message: &str) {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "user.name=Kosmos Test",
            "-c",
            "user.email=kosmos@example.com",
            "commit",
            "--message",
            message,
        ])
        .output()
        .expect("git commit should run");

    assert!(
        output.status.success(),
        "commit should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn location_state(
    name: &str,
    target_has_source_tab: bool,
) -> (State, WorkspaceId, WorkspaceId, PathBuf, PathBuf) {
    let active_root = test_directory(&format!("{name}-active"));
    let target_root = test_directory(&format!("{name}-target"));
    let active_workspace_id = WorkspaceId::new(1);
    let target_workspace_id = WorkspaceId::new(2);
    let active_workspace = Workspace::new(
        active_workspace_id,
        &active_root,
        pane_with_tabs(1, vec![Tab::new(TabId::new(1), "Blank", TabKind::Blank)]),
    );
    let target_kind = if target_has_source_tab {
        TabKind::FileTree
    } else {
        TabKind::Blank
    };
    let target_workspace = Workspace::new(
        target_workspace_id,
        &target_root,
        pane_with_tabs(2, vec![Tab::new(TabId::new(2), "Target", target_kind)]),
    );
    let state = State::from_workspaces(
        vec![active_workspace, target_workspace],
        Some(active_workspace_id),
    )
    .unwrap();

    (
        state,
        active_workspace_id,
        target_workspace_id,
        active_root,
        target_root,
    )
}

fn location_state_snapshot(state: &State) -> LocationStateSnapshot {
    (
        state.workspaces.clone(),
        state.file_tree_view_states.clone(),
        state.git_diff_view_states.clone(),
        state.editor_view_states.clone(),
        state.next_tab_id,
        state.persistence_scope,
    )
}

fn assert_location_state_unchanged(
    state: &State,
    before: &LocationStateSnapshot,
    active_workspace_id: WorkspaceId,
) {
    assert_eq!(state.workspaces, before.0);
    assert_eq!(state.file_tree_view_states, before.1);
    assert_eq!(state.git_diff_view_states, before.2);
    assert_eq!(state.editor_view_states, before.3);
    assert_eq!(state.next_tab_id, before.4);
    assert_eq!(state.persistence_scope, before.5);
    assert_eq!(state.persistence_scope, PersistenceScope::Clean);
    assert_eq!(
        state.workspaces.active_workspace_id(),
        Some(active_workspace_id)
    );
}

fn test_directory(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kosmos-core-state-{}-{name}-{nanos}",
        std::process::id()
    ));

    std::fs::create_dir_all(&root).expect("test root should be created");

    root
}
