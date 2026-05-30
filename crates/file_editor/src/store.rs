use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{App, AppContext, BorrowAppContext, Entity, Global};

use crate::{Buffer, BufferId};

/// Global cache for open file buffers. Paths map to stable [`BufferId`]s, and
/// IDs map to GPUI entities so subsystems can either open by path or later
/// look buffers up by their stable id.
#[derive(Default)]
pub struct BufferStore {
    next_id: u64,
    by_path: HashMap<PathBuf, BufferId>,
    by_id: HashMap<BufferId, Entity<Buffer>>,
}

impl BufferStore {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    /// Return the existing buffer for `path`, opening (and caching) one if
    /// none exists yet.
    pub fn open(path: PathBuf, cx: &mut App) -> Entity<Buffer> {
        if let Some(existing) = cx
            .try_global::<Self>()
            .and_then(|s| s.by_path.get(&path).and_then(|id| s.by_id.get(id)).cloned())
        {
            return existing;
        }
        let id = cx.update_global::<Self, _>(|store, _| {
            let id = BufferId(store.next_id);
            store.next_id += 1;
            id
        });
        let path_for_buffer = path.clone();
        let entity = cx.new(move |cx| Buffer::new(id, path_for_buffer, cx));
        cx.update_global::<Self, _>(|store, _| {
            store.by_path.insert(path, id);
            store.by_id.insert(id, entity.clone());
        });
        entity
    }

    pub fn get(id: BufferId, cx: &App) -> Option<Entity<Buffer>> {
        cx.try_global::<Self>()
            .and_then(|s| s.by_id.get(&id).cloned())
    }

    pub fn is_path_dirty(path: &Path, cx: &App) -> bool {
        cx.try_global::<Self>()
            .and_then(|store| {
                store
                    .by_path
                    .get(path)
                    .and_then(|id| store.by_id.get(id))
                    .cloned()
            })
            .is_some_and(|buffer| buffer.read(cx).is_dirty())
    }

    pub fn content_for_path(path: &Path, cx: &App) -> Option<String> {
        cx.try_global::<Self>()
            .and_then(|store| {
                store
                    .by_path
                    .get(path)
                    .and_then(|id| store.by_id.get(id))
                    .cloned()
            })
            .map(|buffer| buffer.read(cx).content().to_string())
    }

    pub fn write_path_content(
        path: &Path,
        content: impl Into<String>,
        cx: &mut App,
    ) -> std::io::Result<()> {
        let content = content.into();
        let buffer = cx.try_global::<Self>().and_then(|store| {
            store
                .by_path
                .get(path)
                .and_then(|id| store.by_id.get(id))
                .cloned()
        });

        if let Some(buffer) = buffer {
            buffer.update(cx, |buffer, cx| {
                let len = buffer.content().len();
                buffer.replace_range(0..len, &content, cx);
                buffer.save(cx)
            })?;
            return Ok(());
        }

        std::fs::write(path, content)
    }

    pub fn has_dirty_buffers(cx: &App) -> bool {
        cx.try_global::<Self>().is_some_and(|store| {
            store
                .by_id
                .values()
                .any(|buffer| buffer.read(cx).is_dirty())
        })
    }

    pub fn save_path(path: &Path, cx: &mut App) -> std::io::Result<bool> {
        let buffer = cx.try_global::<Self>().and_then(|store| {
            store
                .by_path
                .get(path)
                .and_then(|id| store.by_id.get(id))
                .cloned()
        });
        let Some(buffer) = buffer else {
            return Ok(false);
        };
        buffer.update(cx, |buffer, cx| buffer.save(cx))?;
        Ok(true)
    }

    pub fn save_all(cx: &mut App) -> std::io::Result<usize> {
        let buffers = cx
            .try_global::<Self>()
            .map(|store| store.by_id.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let mut saved = 0;
        for buffer in buffers {
            if !buffer.read(cx).is_dirty() {
                continue;
            }
            buffer.update(cx, |buffer, cx| buffer.save(cx))?;
            saved += 1;
        }
        Ok(saved)
    }

    pub fn reload_paths(paths: impl IntoIterator<Item = PathBuf>, cx: &mut App) {
        let buffers = cx
            .try_global::<Self>()
            .map(|store| {
                paths
                    .into_iter()
                    .filter_map(|path| {
                        store
                            .by_path
                            .get(&path)
                            .and_then(|id| store.by_id.get(id))
                            .cloned()
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        for buffer in buffers {
            buffer.update(cx, |buffer, cx| buffer.reload_from_disk(cx));
        }
    }
}

impl Global for BufferStore {}
