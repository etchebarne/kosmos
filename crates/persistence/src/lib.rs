mod db;

use std::path::PathBuf;

use gpui::{Bounds, Point, Size, WindowBounds, px};
use rusqlite::types::Type;
use rusqlite::{Connection, OptionalExtension, Result, Transaction, params, params_from_iter};

use pane_tree::{PaneNode, PaneTree, SplitAxis};
use panes::Pane;
use tabs::Tab;
use workspace::{Workspace, WorkspaceManager};

pub fn load() -> WorkspaceManager {
    db::with(load_inner)
        .flatten()
        .unwrap_or_else(WorkspaceManager::new)
}

pub fn save_session(manager: &WorkspaceManager) {
    db::with(|conn| save_session_inner(conn, manager));
}

pub fn save_workspace(workspace: &Workspace) {
    db::with(|conn| save_workspace_inner(conn, workspace));
}

pub fn load_window_bounds() -> Option<WindowBounds> {
    db::with(load_window_inner).flatten()
}

pub fn save_window_bounds(bounds: WindowBounds) {
    db::with(|conn| save_window_inner(conn, bounds));
}

fn load_inner(conn: &mut Connection) -> Result<Option<WorkspaceManager>> {
    let session: Option<(Option<i64>, i64)> = conn
        .query_row(
            "SELECT active_workspace_id, next_workspace_id FROM session WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((active_raw, next_raw)) = session else {
        return Ok(None);
    };

    let workspace_rows = read_workspace_rows(conn)?;
    let mut workspaces = Vec::with_capacity(workspace_rows.len());
    for row in workspace_rows {
        let pane_tree = load_pane_tree(
            conn,
            row.id,
            row.next_tab_id,
            row.next_pane_id,
            row.next_split_id,
        )?;
        workspaces.push(Workspace {
            id: row.id,
            path: PathBuf::from(row.path),
            name: row.name,
            pane_tree,
        });
    }

    let active = active_raw
        .map(|x| x as usize)
        .filter(|id| workspaces.iter().any(|w| w.id == *id))
        .or_else(|| workspaces.first().map(|w| w.id));

    Ok(Some(WorkspaceManager::from_parts(
        workspaces,
        active,
        next_raw as usize,
    )))
}

struct WorkspaceRow {
    id: usize,
    path: String,
    name: String,
    next_tab_id: usize,
    next_pane_id: usize,
    next_split_id: usize,
}

fn read_workspace_rows(conn: &Connection) -> Result<Vec<WorkspaceRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, path, name, next_tab_id, next_pane_id, next_split_id
         FROM workspaces ORDER BY position ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(WorkspaceRow {
            id: row.get::<_, i64>(0)? as usize,
            path: row.get(1)?,
            name: row.get(2)?,
            next_tab_id: row.get::<_, i64>(3)? as usize,
            next_pane_id: row.get::<_, i64>(4)? as usize,
            next_split_id: row.get::<_, i64>(5)? as usize,
        })
    })?;
    rows.collect()
}

struct NodeRow {
    id: i64,
    parent_id: Option<i64>,
    position: Option<i64>,
    kind: String,
    split_id: Option<i64>,
    axis: Option<String>,
    ratio: Option<f64>,
    pane_id: Option<i64>,
    active_tab_id: Option<i64>,
}

fn load_pane_tree(
    conn: &Connection,
    workspace_id: usize,
    next_tab_id: usize,
    next_pane_id: usize,
    next_split_id: usize,
) -> Result<PaneTree> {
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, position, kind, split_id, axis, ratio, pane_id, active_tab_id
         FROM pane_nodes WHERE workspace_id = ?",
    )?;
    let nodes: Vec<NodeRow> = stmt
        .query_map([workspace_id as i64], |row| {
            Ok(NodeRow {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                position: row.get(2)?,
                kind: row.get(3)?,
                split_id: row.get(4)?,
                axis: row.get(5)?,
                ratio: row.get(6)?,
                pane_id: row.get(7)?,
                active_tab_id: row.get(8)?,
            })
        })?
        .collect::<Result<_>>()?;
    drop(stmt);

    let Some(root) = nodes.iter().find(|n| n.parent_id.is_none()) else {
        return Ok(PaneTree::new());
    };
    let root_node = build_node(conn, &nodes, root)?;
    Ok(PaneTree::from_parts(
        root_node,
        next_tab_id,
        next_pane_id,
        next_split_id,
    ))
}

fn build_node(conn: &Connection, nodes: &[NodeRow], node: &NodeRow) -> Result<PaneNode> {
    match node.kind.as_str() {
        "leaf" => {
            let pane_id = node.pane_id.ok_or_else(|| bad("leaf missing pane_id"))? as usize;
            let active_tab =
                node.active_tab_id
                    .ok_or_else(|| bad("leaf missing active_tab_id"))? as usize;
            let tabs = load_tabs(conn, node.id)?;
            Ok(PaneNode::Leaf(Pane::from_parts(pane_id, tabs, active_tab)))
        }
        "split" => {
            let split_id = node.split_id.ok_or_else(|| bad("split missing split_id"))? as usize;
            let axis = match node.axis.as_deref() {
                Some("row") => SplitAxis::Row,
                Some("column") => SplitAxis::Column,
                _ => return Err(bad("split has invalid axis")),
            };
            let ratio = node.ratio.ok_or_else(|| bad("split missing ratio"))? as f32;
            let mut children: Vec<&NodeRow> = nodes
                .iter()
                .filter(|n| n.parent_id == Some(node.id))
                .collect();
            children.sort_by_key(|n| n.position.unwrap_or(0));
            if children.len() != 2 {
                return Err(bad("split must have two children"));
            }
            let first = build_node(conn, nodes, children[0])?;
            let second = build_node(conn, nodes, children[1])?;
            Ok(PaneNode::Split {
                id: split_id,
                axis,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            })
        }
        _ => Err(bad("unknown pane_node kind")),
    }
}

fn load_tabs(conn: &Connection, pane_node_id: i64) -> Result<Vec<Tab>> {
    let mut stmt = conn.prepare(
        "SELECT tab_id, kind, title, path FROM tabs WHERE pane_node_id = ? ORDER BY position ASC",
    )?;
    let rows = stmt.query_map([pane_node_id], |row| {
        let id: i64 = row.get(0)?;
        let kind: String = row.get(1)?;
        let title: Option<String> = row.get(2)?;
        let path: Option<String> = row.get(3)?;
        Ok(Tab {
            id: id as usize,
            kind,
            title,
            path: path.map(PathBuf::from),
        })
    })?;
    rows.collect()
}

fn save_session_inner(conn: &mut Connection, manager: &WorkspaceManager) -> Result<()> {
    let tx = conn.transaction()?;

    let kept: Vec<i64> = manager.workspaces().iter().map(|w| w.id as i64).collect();
    if kept.is_empty() {
        tx.execute("DELETE FROM workspaces", [])?;
    } else {
        let placeholders = vec!["?"; kept.len()].join(",");
        let sql = format!("DELETE FROM workspaces WHERE id NOT IN ({placeholders})");
        tx.execute(&sql, params_from_iter(kept.iter()))?;
    }

    for (pos, ws) in manager.workspaces().iter().enumerate() {
        tx.execute(
            "UPDATE workspaces SET position = ?1 WHERE id = ?2",
            params![pos as i64, ws.id as i64],
        )?;
    }

    tx.execute(
        "INSERT INTO session (id, active_workspace_id, next_workspace_id) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET
           active_workspace_id = excluded.active_workspace_id,
           next_workspace_id = excluded.next_workspace_id",
        params![
            manager.active_id().map(|x| x as i64),
            manager.next_id() as i64
        ],
    )?;

    tx.commit()
}

fn save_workspace_inner(conn: &mut Connection, workspace: &Workspace) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO workspaces (id, position, path, name, next_tab_id, next_pane_id, next_split_id)
         VALUES (?1, COALESCE((SELECT MAX(position) FROM workspaces), -1) + 1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
           path = excluded.path,
           name = excluded.name,
           next_tab_id = excluded.next_tab_id,
           next_pane_id = excluded.next_pane_id,
           next_split_id = excluded.next_split_id",
        params![
            workspace.id as i64,
            workspace.path.to_string_lossy(),
            workspace.name.as_str(),
            workspace.pane_tree.next_tab_id() as i64,
            workspace.pane_tree.next_pane_id() as i64,
            workspace.pane_tree.next_split_id() as i64,
        ],
    )?;
    tx.execute(
        "DELETE FROM pane_nodes WHERE workspace_id = ?1",
        params![workspace.id as i64],
    )?;
    write_node(&tx, workspace.id, None, None, workspace.pane_tree.root())?;
    tx.commit()
}

fn write_node(
    tx: &Transaction,
    workspace_id: usize,
    parent_id: Option<i64>,
    position: Option<i64>,
    node: &PaneNode,
) -> Result<()> {
    match node {
        PaneNode::Leaf(pane) => {
            tx.execute(
                "INSERT INTO pane_nodes
                   (workspace_id, parent_id, position, kind, pane_id, active_tab_id)
                 VALUES (?1, ?2, ?3, 'leaf', ?4, ?5)",
                params![
                    workspace_id as i64,
                    parent_id,
                    position,
                    pane.id() as i64,
                    pane.active_tab() as i64,
                ],
            )?;
            let pane_node_id = tx.last_insert_rowid();
            for (i, tab) in pane.tabs().iter().enumerate() {
                tx.execute(
                    "INSERT INTO tabs (pane_node_id, position, tab_id, kind, title, path)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        pane_node_id,
                        i as i64,
                        tab.id as i64,
                        tab.kind.as_str(),
                        tab.title.as_deref(),
                        tab.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                    ],
                )?;
            }
        }
        PaneNode::Split {
            id,
            axis,
            ratio,
            first,
            second,
        } => {
            let axis_str = match axis {
                SplitAxis::Row => "row",
                SplitAxis::Column => "column",
            };
            tx.execute(
                "INSERT INTO pane_nodes
                   (workspace_id, parent_id, position, kind, split_id, axis, ratio)
                 VALUES (?1, ?2, ?3, 'split', ?4, ?5, ?6)",
                params![
                    workspace_id as i64,
                    parent_id,
                    position,
                    *id as i64,
                    axis_str,
                    *ratio as f64,
                ],
            )?;
            let pane_node_id = tx.last_insert_rowid();
            write_node(tx, workspace_id, Some(pane_node_id), Some(0), first)?;
            write_node(tx, workspace_id, Some(pane_node_id), Some(1), second)?;
        }
    }
    Ok(())
}

fn load_window_inner(conn: &mut Connection) -> Result<Option<WindowBounds>> {
    let row: Option<(String, f64, f64, f64, f64)> = conn
        .query_row(
            "SELECT state, origin_x, origin_y, width, height FROM window WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((state, ox, oy, w, h)) = row else {
        return Ok(None);
    };
    let bounds = Bounds {
        origin: Point {
            x: px(ox as f32),
            y: px(oy as f32),
        },
        size: Size {
            width: px(w as f32),
            height: px(h as f32),
        },
    };
    let bounds = match state.as_str() {
        "windowed" => WindowBounds::Windowed(bounds),
        "maximized" => WindowBounds::Maximized(bounds),
        "fullscreen" => WindowBounds::Fullscreen(bounds),
        _ => return Ok(None),
    };
    Ok(Some(bounds))
}

fn save_window_inner(conn: &mut Connection, bounds: WindowBounds) -> Result<()> {
    let (state, b) = match bounds {
        WindowBounds::Windowed(b) => ("windowed", b),
        WindowBounds::Maximized(b) => ("maximized", b),
        WindowBounds::Fullscreen(b) => ("fullscreen", b),
    };
    conn.execute(
        "INSERT INTO window (id, state, origin_x, origin_y, width, height)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET
           state = excluded.state,
           origin_x = excluded.origin_x,
           origin_y = excluded.origin_y,
           width = excluded.width,
           height = excluded.height",
        params![
            state,
            f32::from(b.origin.x) as f64,
            f32::from(b.origin.y) as f64,
            f32::from(b.size.width) as f64,
            f32::from(b.size.height) as f64,
        ],
    )?;
    Ok(())
}

fn bad(msg: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, Type::Null, msg.into())
}
