use std::collections::HashMap;

pub const DEFAULT_PANEL_WIDTH_REM: f32 = 36.0;
pub const DEFAULT_PANEL_HEIGHT_REM: f32 = 26.0;
pub const MIN_PANEL_WIDTH_REM: f32 = 16.0;
pub const MIN_PANEL_HEIGHT_REM: f32 = 12.0;
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 3.0;
pub const SNAP_GRID_REM: f32 = 2.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasPoint {
    pub x: f32,
    pub y: f32,
}

impl CanvasPoint {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CanvasSize {
    pub width: f32,
    pub height: f32,
}

impl CanvasSize {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CanvasPanel {
    pub id: usize,
    pub kind: String,
    pub position: CanvasPoint,
    pub size: CanvasSize,
    pub z_index: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub pan: CanvasPoint,
    pub zoom: f32,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: CanvasPoint::new(8.0, 6.0),
            zoom: 1.0,
        }
    }
}

impl Viewport {
    pub fn canvas_to_screen(self, point: CanvasPoint) -> CanvasPoint {
        CanvasPoint::new(
            point.x * self.zoom + self.pan.x,
            point.y * self.zoom + self.pan.y,
        )
    }

    pub fn screen_to_canvas(self, point: CanvasPoint) -> CanvasPoint {
        CanvasPoint::new(
            (point.x - self.pan.x) / self.zoom,
            (point.y - self.pan.y) / self.zoom,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasInteraction {
    Panning {
        pointer_start: CanvasPoint,
        pan_start: CanvasPoint,
    },
    MovingPanel {
        panel_id: usize,
        pointer_start: CanvasPoint,
        position_start: CanvasPoint,
    },
    ResizingPanel {
        panel_id: usize,
        pointer_start: CanvasPoint,
        size_start: CanvasSize,
    },
}

#[derive(Clone, Debug, Default)]
pub struct InfinityCanvas {
    viewport: Viewport,
    panels: Vec<CanvasPanel>,
    next_panel_id: usize,
    next_z_index: usize,
    active_interaction: Option<CanvasInteraction>,
}

impl InfinityCanvas {
    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    pub fn panels(&self) -> &[CanvasPanel] {
        &self.panels
    }

    pub fn active_interaction(&self) -> Option<CanvasInteraction> {
        self.active_interaction
    }

    pub fn add_panel(&mut self, kind: impl Into<String>, center: CanvasPoint) -> usize {
        let id = self.next_panel_id;
        self.next_panel_id += 1;

        let size = CanvasSize::new(DEFAULT_PANEL_WIDTH_REM, DEFAULT_PANEL_HEIGHT_REM);
        let panel = CanvasPanel {
            id,
            kind: kind.into(),
            position: snap_point(CanvasPoint::new(
                center.x - size.width / 2.0,
                center.y - size.height / 2.0,
            )),
            size,
            z_index: self.next_panel_z_index(),
        };
        self.panels.push(panel);
        id
    }

    pub fn remove_panel(&mut self, panel_id: usize) -> bool {
        let Some(index) = self.panels.iter().position(|panel| panel.id == panel_id) else {
            return false;
        };
        self.panels.remove(index);
        true
    }

    pub fn bring_panel_to_front(&mut self, panel_id: usize) -> bool {
        let z_index = self.next_panel_z_index();
        let Some(panel) = self.panel_mut(panel_id) else {
            return false;
        };
        panel.z_index = z_index;
        true
    }

    pub fn begin_pan(&mut self, pointer_screen: CanvasPoint) {
        self.active_interaction = Some(CanvasInteraction::Panning {
            pointer_start: pointer_screen,
            pan_start: self.viewport.pan,
        });
    }

    pub fn begin_move_panel(&mut self, panel_id: usize, pointer_screen: CanvasPoint) -> bool {
        let Some(position_start) = self.panel(panel_id).map(|panel| panel.position) else {
            return false;
        };
        self.bring_panel_to_front(panel_id);
        self.active_interaction = Some(CanvasInteraction::MovingPanel {
            panel_id,
            pointer_start: pointer_screen,
            position_start,
        });
        true
    }

    pub fn begin_resize_panel(&mut self, panel_id: usize, pointer_screen: CanvasPoint) -> bool {
        let Some(size_start) = self.panel(panel_id).map(|panel| panel.size) else {
            return false;
        };
        self.bring_panel_to_front(panel_id);
        self.active_interaction = Some(CanvasInteraction::ResizingPanel {
            panel_id,
            pointer_start: pointer_screen,
            size_start,
        });
        true
    }

    pub fn drag_to(&mut self, pointer_screen: CanvasPoint) -> bool {
        let Some(interaction) = self.active_interaction else {
            return false;
        };
        match interaction {
            CanvasInteraction::Panning {
                pointer_start,
                pan_start,
            } => {
                self.viewport.pan = CanvasPoint::new(
                    pan_start.x + pointer_screen.x - pointer_start.x,
                    pan_start.y + pointer_screen.y - pointer_start.y,
                );
                true
            }
            CanvasInteraction::MovingPanel {
                panel_id,
                pointer_start,
                position_start,
            } => {
                let zoom = self.viewport.zoom;
                let Some(panel) = self.panel_mut(panel_id) else {
                    return false;
                };
                panel.position = snap_point(CanvasPoint::new(
                    position_start.x + (pointer_screen.x - pointer_start.x) / zoom,
                    position_start.y + (pointer_screen.y - pointer_start.y) / zoom,
                ));
                true
            }
            CanvasInteraction::ResizingPanel {
                panel_id,
                pointer_start,
                size_start,
            } => {
                let zoom = self.viewport.zoom;
                let Some(panel) = self.panel_mut(panel_id) else {
                    return false;
                };
                panel.size = snap_size(CanvasSize::new(
                    (size_start.width + (pointer_screen.x - pointer_start.x) / zoom)
                        .max(MIN_PANEL_WIDTH_REM),
                    (size_start.height + (pointer_screen.y - pointer_start.y) / zoom)
                        .max(MIN_PANEL_HEIGHT_REM),
                ));
                true
            }
        }
    }

    pub fn finish_interaction(&mut self) -> bool {
        self.active_interaction.take().is_some()
    }

    pub fn zoom_at(&mut self, anchor_screen: CanvasPoint, factor: f32) -> bool {
        if factor <= 0.0 {
            return false;
        }
        let previous_zoom = self.viewport.zoom;
        let next_zoom = (previous_zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (next_zoom - previous_zoom).abs() <= f32::EPSILON {
            return false;
        }

        let anchor_canvas = self.viewport.screen_to_canvas(anchor_screen);
        self.viewport.zoom = next_zoom;
        self.viewport.pan = CanvasPoint::new(
            anchor_screen.x - anchor_canvas.x * next_zoom,
            anchor_screen.y - anchor_canvas.y * next_zoom,
        );
        true
    }

    pub fn pan_by(&mut self, delta_screen: CanvasPoint) -> bool {
        if delta_screen.x == 0.0 && delta_screen.y == 0.0 {
            return false;
        }
        self.viewport.pan = CanvasPoint::new(
            self.viewport.pan.x + delta_screen.x,
            self.viewport.pan.y + delta_screen.y,
        );
        true
    }

    pub fn center_canvas_point(&self, viewport_size: CanvasSize) -> CanvasPoint {
        self.viewport.screen_to_canvas(CanvasPoint::new(
            viewport_size.width / 2.0,
            viewport_size.height / 2.0,
        ))
    }

    pub fn ordered_panels(&self) -> Vec<CanvasPanel> {
        let mut panels = self.panels.clone();
        panels.sort_by_key(|panel| panel.z_index);
        panels
    }

    fn panel(&self, panel_id: usize) -> Option<&CanvasPanel> {
        self.panels.iter().find(|panel| panel.id == panel_id)
    }

    fn panel_mut(&mut self, panel_id: usize) -> Option<&mut CanvasPanel> {
        self.panels.iter_mut().find(|panel| panel.id == panel_id)
    }

    fn next_panel_z_index(&mut self) -> usize {
        let z_index = self.next_z_index;
        self.next_z_index += 1;
        z_index
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InfinityCanvasKey {
    pub workspace_id: usize,
    pub tab_id: usize,
}

impl InfinityCanvasKey {
    pub const fn new(workspace_id: usize, tab_id: usize) -> Self {
        Self {
            workspace_id,
            tab_id,
        }
    }
}

#[derive(Default)]
pub struct InfinityCanvasStore {
    canvases: HashMap<InfinityCanvasKey, InfinityCanvas>,
}

impl InfinityCanvasStore {
    pub fn canvas(&mut self, key: InfinityCanvasKey) -> &mut InfinityCanvas {
        self.canvases.entry(key).or_default()
    }

    pub fn snapshot(&self, key: InfinityCanvasKey) -> InfinityCanvas {
        self.canvases.get(&key).cloned().unwrap_or_default()
    }

    pub fn remove_canvas(&mut self, key: InfinityCanvasKey) -> bool {
        self.canvases.remove(&key).is_some()
    }
}

pub fn virtual_panel_tab_id(owner_tab_id: usize, panel_id: usize) -> usize {
    let mixed = owner_tab_id.wrapping_mul(1_000_003).wrapping_add(panel_id) % (usize::MAX / 2);
    usize::MAX / 2 + mixed
}

fn snap_point(point: CanvasPoint) -> CanvasPoint {
    CanvasPoint::new(snap_value(point.x), snap_value(point.y))
}

fn snap_size(size: CanvasSize) -> CanvasSize {
    CanvasSize::new(
        snap_value(size.width).max(MIN_PANEL_WIDTH_REM),
        snap_value(size.height).max(MIN_PANEL_HEIGHT_REM),
    )
}

fn snap_value(value: f32) -> f32 {
    (value / SNAP_GRID_REM).round() * SNAP_GRID_REM
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_anchor_fixed() {
        let mut canvas = InfinityCanvas::default();
        let anchor = CanvasPoint::new(10.0, 5.0);
        let before = canvas.viewport().screen_to_canvas(anchor);

        assert!(canvas.zoom_at(anchor, 1.5));

        let after = canvas.viewport().screen_to_canvas(anchor);
        assert!((before.x - after.x).abs() < 0.001);
        assert!((before.y - after.y).abs() < 0.001);
    }

    #[test]
    fn moving_panel_accounts_for_zoom() {
        let mut canvas = InfinityCanvas::default();
        let panel_id = canvas.add_panel("terminal", CanvasPoint::new(0.0, 0.0));
        canvas.zoom_at(CanvasPoint::new(0.0, 0.0), 2.0);
        let initial = canvas.panels()[0].position;

        assert!(canvas.begin_move_panel(panel_id, CanvasPoint::new(10.0, 10.0)));
        assert!(canvas.drag_to(CanvasPoint::new(14.0, 16.0)));

        let moved = canvas.panels()[0].position;
        assert_eq!(moved, CanvasPoint::new(initial.x + 2.0, initial.y + 2.0));
    }

    #[test]
    fn moving_panel_snaps_to_grid() {
        let mut canvas = InfinityCanvas::default();
        let panel_id = canvas.add_panel("terminal", CanvasPoint::new(0.0, 0.0));

        assert!(canvas.begin_move_panel(panel_id, CanvasPoint::new(0.0, 0.0)));
        assert!(canvas.drag_to(CanvasPoint::new(1.1, 2.9)));

        let position = canvas.panels()[0].position;
        assert_eq!(position.x % SNAP_GRID_REM, 0.0);
        assert_eq!(position.y % SNAP_GRID_REM, 0.0);
    }

    #[test]
    fn resizing_panel_enforces_minimum_size() {
        let mut canvas = InfinityCanvas::default();
        let panel_id = canvas.add_panel("terminal", CanvasPoint::new(0.0, 0.0));

        assert!(canvas.begin_resize_panel(panel_id, CanvasPoint::new(10.0, 10.0)));
        assert!(canvas.drag_to(CanvasPoint::new(-100.0, -100.0)));

        let size = canvas.panels()[0].size;
        assert_eq!(
            size,
            CanvasSize::new(MIN_PANEL_WIDTH_REM, MIN_PANEL_HEIGHT_REM)
        );
    }
}
