use super::*;
use tabs::registry;

fn leaf(node: &PaneNode) -> &Pane {
    match node {
        PaneNode::Leaf(p) => p,
        PaneNode::Split { .. } => panic!("expected leaf"),
    }
}

fn split_children(node: &PaneNode) -> (&PaneNode, &PaneNode) {
    match node {
        PaneNode::Split { first, second, .. } => (first, second),
        PaneNode::Leaf(_) => panic!("expected split"),
    }
}

fn tab_ids(pane: &Pane) -> Vec<usize> {
    pane.tabs().iter().map(|t| t.id).collect()
}

#[test]
fn new_tree_has_single_pane_with_one_tab() {
    let tree = PaneTree::new();
    assert_eq!(tree.total_tabs(), 1);
    let pane = leaf(tree.root());
    assert_eq!(pane.id(), 0);
    assert_eq!(pane.active_tab(), 0);
    assert_eq!(tab_ids(pane), vec![0]);
}

#[test]
fn add_tab_to_known_pane_appends_and_activates() {
    let mut tree = PaneTree::new();
    assert!(tree.add_tab(0, &registry::BLANK));
    let pane = leaf(tree.root());
    assert_eq!(tab_ids(pane), vec![0, 1]);
    assert_eq!(pane.active_tab(), 1);
}

#[test]
fn add_tab_unknown_pane_does_not_consume_id() {
    let mut tree = PaneTree::new();
    assert!(!tree.add_tab(99, &registry::BLANK));
    assert!(tree.add_tab(0, &registry::BLANK));
    let pane = leaf(tree.root());
    assert_eq!(pane.tabs().last().unwrap().id, 1);
}

#[test]
fn replace_tab_kind_swaps_in_place_keeping_id_and_position() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.replace_tab_kind(0, 1, &registry::TERMINAL));
    let pane = leaf(tree.root());
    assert_eq!(tab_ids(pane), vec![0, 1, 2]);
    assert_eq!(pane.tabs()[1].kind.as_ref(), "terminal");
    assert_eq!(pane.tabs()[0].kind.as_ref(), "blank");
}

#[test]
fn replace_tab_kind_unknown_pane_or_tab_returns_false() {
    let mut tree = PaneTree::new();
    assert!(!tree.replace_tab_kind(99, 0, &registry::TERMINAL));
    assert!(!tree.replace_tab_kind(0, 99, &registry::TERMINAL));
}

#[test]
fn select_tab_validates_membership() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.select_tab(0, 0));
    assert_eq!(leaf(tree.root()).active_tab(), 0);
    assert!(!tree.select_tab(0, 999));
    assert_eq!(leaf(tree.root()).active_tab(), 0);
    assert!(!tree.select_tab(99, 0));
}

#[test]
fn close_last_tab_is_rejected() {
    let mut tree = PaneTree::new();
    assert!(!tree.close_tab(0, 0));
    assert_eq!(tree.total_tabs(), 1);
}

#[test]
fn close_tab_keeps_remaining_tabs() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.close_tab(0, 1));
    assert_eq!(tab_ids(leaf(tree.root())), vec![0]);
}

#[test]
fn close_tab_collapses_empty_split_pane() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.split_pane(0, 1, 0, DropZone::Right));
    assert!(matches!(tree.root(), PaneNode::Split { .. }));
    assert!(tree.close_tab(1, 1));
    assert!(matches!(tree.root(), PaneNode::Leaf(_)));
    assert_eq!(tree.total_tabs(), 1);
}

#[test]
fn move_tab_before_reorders_within_pane() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.move_tab_before(0, 2, 0, 0));
    assert_eq!(tab_ids(leaf(tree.root())), vec![2, 0, 1]);
}

#[test]
fn move_tab_before_self_is_rejected() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(!tree.move_tab_before(0, 0, 0, 0));
}

#[test]
fn move_tab_before_unknown_target_leaves_state_unchanged() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    let before = tab_ids(leaf(tree.root()));
    let active_before = leaf(tree.root()).active_tab();
    assert!(!tree.move_tab_before(0, 1, 0, 999));
    assert_eq!(tab_ids(leaf(tree.root())), before);
    assert_eq!(leaf(tree.root()).active_tab(), active_before);
}

#[test]
fn move_tab_to_pane_moves_across_split() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.split_pane(0, 1, 0, DropZone::Right));
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.move_tab_to_pane(0, 2, 1));

    let (first, second) = split_children(tree.root());
    assert_eq!(tab_ids(leaf(first)), vec![0]);
    assert_eq!(tab_ids(leaf(second)), vec![1, 2]);
}

#[test]
fn move_tab_to_pane_same_pane_is_rejected() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(!tree.move_tab_to_pane(0, 1, 0));
}

#[test]
fn split_pane_with_only_tab_is_rejected() {
    let mut tree = PaneTree::new();
    assert!(!tree.split_pane(0, 0, 0, DropZone::Right));
    assert!(matches!(tree.root(), PaneNode::Leaf(_)));
}

#[test]
fn split_pane_to_self_with_one_tab_is_rejected() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.split_pane(0, 1, 0, DropZone::Right));
    assert!(!tree.split_pane(1, 1, 1, DropZone::Right));
}

#[test]
fn split_pane_assigns_axis_from_drop_zone() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    assert!(tree.split_pane(0, 1, 0, DropZone::Bottom));
    assert!(matches!(
        tree.root(),
        PaneNode::Split {
            axis: SplitAxis::Column,
            ..
        }
    ));
}

#[test]
fn split_pane_unknown_target_preserves_ids_and_tabs() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    let next_pane = tree.next_pane_id;
    let next_split = tree.next_split_id;
    assert!(!tree.split_pane(0, 1, 99, DropZone::Right));
    assert_eq!(tree.next_pane_id, next_pane);
    assert_eq!(tree.next_split_id, next_split);
    assert_eq!(tree.total_tabs(), 2);
    assert_eq!(tab_ids(leaf(tree.root())), vec![0, 1]);
}

#[test]
fn resize_split_clamps_to_range() {
    let mut tree = PaneTree::new();
    tree.add_tab(0, &registry::BLANK);
    tree.split_pane(0, 1, 0, DropZone::Right);
    let split_id = match tree.root() {
        PaneNode::Split { id, .. } => *id,
        PaneNode::Leaf(_) => panic!("expected split"),
    };
    assert!(tree.resize_split(split_id, 0.99));
    if let PaneNode::Split { ratio, .. } = tree.root() {
        assert!((*ratio - 0.85).abs() < f32::EPSILON);
    }
    assert!(tree.resize_split(split_id, 0.01));
    if let PaneNode::Split { ratio, .. } = tree.root() {
        assert!((*ratio - 0.15).abs() < f32::EPSILON);
    }
    assert!(!tree.resize_split(999, 0.5));
}
