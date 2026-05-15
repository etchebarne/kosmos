use panes::Pane;
use tabs::{Tab, TabKind, registry};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    Row,
    Column,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone)]
pub enum PaneNode {
    Leaf(Pane),
    Split {
        id: usize,
        axis: SplitAxis,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

pub struct PaneTree {
    root: PaneNode,
    next_tab_id: usize,
    next_pane_id: usize,
    next_split_id: usize,
    active_pane_id: usize,
}

impl Default for PaneTree {
    fn default() -> Self {
        Self::new()
    }
}
