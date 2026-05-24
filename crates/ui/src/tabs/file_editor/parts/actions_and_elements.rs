use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, EntityInputHandler, Global,
    IntoElement, KeyBinding, SharedString, Task, Window, div, prelude::*, rems,
};
use gpui_component::input::{
    HoverProvider, Input, InputEvent, InputState, Rope, RopeExt, TabSize,
};

use file_editor::{Buffer, BufferId, BufferStore, soft_wrap_enabled};
use file_tree::ActiveFileTree;
use icons::{Icon, IconName};
use tabs::{Tab, registry};
use theme::{ActiveTheme, Theme};

const COMPONENT_INPUT_KEY_CONTEXT: &str = "Input";
const FONT_FAMILY: &str = "DejaVu Sans Mono";
const TAB_SIZE_COLUMNS: usize = 4;

#[derive(Default)]
struct ComponentEditorStore {
    inputs: HashMap<usize, ComponentEditorInput>,
}

struct ComponentEditorInput {
    input: Entity<InputState>,
    buffer_id: BufferId,
    soft_wrap: bool,
    root: Option<PathBuf>,
}

#[derive(Clone)]
struct ComponentHoverProvider {
    buffer: Entity<Buffer>,
    root: Option<PathBuf>,
}

impl ComponentEditorStore {
    fn input_for_tab(
        tab_id: usize,
        buffer: &Entity<Buffer>,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<InputState> {
        if cx.try_global::<Self>().is_none() {
            cx.set_global(Self::default());
        }

        let buffer_id = buffer.read(cx).id();
        let soft_wrap = soft_wrap_enabled(cx);

        if let Some((input, previous_soft_wrap, previous_root)) = cx
            .global::<Self>()
            .inputs
            .get(&tab_id)
            .filter(|existing| existing.buffer_id == buffer_id)
            .map(|existing| {
                (
                    existing.input.clone(),
                    existing.soft_wrap,
                    existing.root.clone(),
                )
            })
        {
            Self::sync_soft_wrap(tab_id, &input, previous_soft_wrap, soft_wrap, window, cx);
            Self::sync_hover_provider(tab_id, &input, buffer, previous_root, root, cx);
            Self::sync_from_buffer(&input, buffer, window, cx);
            return input;
        }

        let (initial, language) = {
            let buffer = buffer.read(cx);
            (
                buffer.content().to_string(),
                buffer
                    .language()
                    .map(|language| component_editor_language(language.as_str()).to_string())
                    .unwrap_or_else(|| "plaintext".to_string()),
            )
        };

        let hover_provider: Rc<dyn HoverProvider> = Rc::new(ComponentHoverProvider {
            buffer: buffer.clone(),
            root: root.clone(),
        });
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx)
                .code_editor(language)
                .multi_line(true)
                .soft_wrap(soft_wrap)
                .tab_size(TabSize {
                    tab_size: TAB_SIZE_COLUMNS,
                    ..Default::default()
                })
                .default_value(initial);
            input.lsp.hover_provider = Some(hover_provider);
            input
        });

        let input_for_sub = input.clone();
        let buffer_for_sub = buffer.clone();
        cx.subscribe(&input, move |_, event: &InputEvent, cx| {
            if !matches!(event, InputEvent::Change) {
                return;
            }
            let value = input_for_sub.read(cx).value().to_string();
            buffer_for_sub.update(cx, |buffer, cx| {
                let current_len = buffer.content().len();
                if buffer.content() != value {
                    buffer.replace_range(0..current_len, &value, cx);
                }
            });
        })
        .detach();

        cx.update_global::<Self, _>(|store, _| {
            store.inputs.insert(
                tab_id,
                ComponentEditorInput {
                    input: input.clone(),
                    buffer_id,
                    soft_wrap,
                    root,
                },
            );
        });

        input
    }

    fn drop_tab(tab_id: usize, cx: &mut App) {
        if cx.try_global::<Self>().is_none() {
            return;
        }
        cx.update_global::<Self, _>(|store, _| {
            store.inputs.remove(&tab_id);
        });
    }

    fn sync_soft_wrap(
        tab_id: usize,
        input: &Entity<InputState>,
        previous: bool,
        current: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if previous == current {
            return;
        }

        input.update(cx, |input, cx| input.set_soft_wrap(current, window, cx));
        cx.update_global::<Self, _>(|store, _| {
            if let Some(existing) = store.inputs.get_mut(&tab_id) {
                existing.soft_wrap = current;
            }
        });
    }

    fn sync_hover_provider(
        tab_id: usize,
        input: &Entity<InputState>,
        buffer: &Entity<Buffer>,
        previous_root: Option<PathBuf>,
        current_root: Option<PathBuf>,
        cx: &mut App,
    ) {
        if previous_root == current_root {
            return;
        }

        let hover_provider: Rc<dyn HoverProvider> = Rc::new(ComponentHoverProvider {
            buffer: buffer.clone(),
            root: current_root.clone(),
        });
        input.update(cx, |input, _| {
            input.lsp.hover_provider = Some(hover_provider);
        });
        cx.update_global::<Self, _>(|store, _| {
            if let Some(existing) = store.inputs.get_mut(&tab_id) {
                existing.root = current_root;
            }
        });
    }

    fn sync_from_buffer(
        input: &Entity<InputState>,
        buffer: &Entity<Buffer>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let buffer_text = buffer.read(cx).content().to_string();
        if input.read(cx).value().as_ref() != buffer_text.as_str() {
            input.update(cx, |input, cx| input.set_value(buffer_text, window, cx));
        }
    }
}

impl Global for ComponentEditorStore {}

impl HoverProvider for ComponentHoverProvider {
    fn hover(
        &self,
        text: &Rope,
        offset: usize,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Option<lsp_types::Hover>>> {
        let content = text.to_string();
        let position = text.offset_to_position(offset);
        let request = {
            let buffer = self.buffer.read(cx);
            let Some(language) = buffer.language() else {
                return Task::ready(Ok(None));
            };
            let language_id = language.as_str().to_string();
            if !lsp::has_installed_server(&language_id) {
                return Task::ready(Ok(None));
            }
            let path = buffer.path().to_path_buf();
            let Some(root) = self
                .root
                .clone()
                .or_else(|| path.parent().map(Path::to_path_buf))
            else {
                return Task::ready(Ok(None));
            };

            lsp::HoverRequest {
                root,
                path,
                language_id,
                content,
                position: lsp::Position {
                    line: position.line,
                    character: position.character,
                },
            }
        };

        cx.background_executor().spawn(async move {
            let hover = lsp::hover(request).map_err(anyhow::Error::from)?;
            Ok(hover.map(|hover| lsp_types::Hover {
                contents: lsp_types::HoverContents::Scalar(lsp_types::MarkedString::String(
                    hover.contents,
                )),
                range: None,
            }))
        })
    }
}

fn component_editor_language(language: &str) -> &str {
    match language {
        "shellscript" => "bash",
        "typescriptreact" => "tsx",
        // gpui-component does not currently ship a separate JSX language.
        "javascriptreact" => "javascript",
        other => other,
    }
}

fn copy_current_component_line<T: 'static>(input: &Entity<InputState>, cx: &mut Context<T>) {
    let Some((content, range)) = current_component_line_range(input, cx) else {
        return;
    };
    if range.is_empty() {
        return;
    }

    cx.write_to_clipboard(ClipboardItem::new_string(content[range].to_string()));
    cx.stop_propagation();
}

fn cut_current_component_line<T: 'static>(
    input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<T>,
) {
    let Some((content, range)) = current_component_line_range(input, cx) else {
        return;
    };
    if range.is_empty() {
        return;
    }

    cx.write_to_clipboard(ClipboardItem::new_string(content[range.clone()].to_string()));
    let utf16_range = component_range_to_utf16(&content, &range);
    input.update(cx, |input, cx| {
        EntityInputHandler::replace_text_in_range(input, Some(utf16_range), "", window, cx);
    });
    cx.stop_propagation();
}

fn current_component_line_range<T: 'static>(
    input: &Entity<InputState>,
    cx: &Context<T>,
) -> Option<(String, Range<usize>)> {
    let input = input.read(cx);
    let selection = input.selected_range();
    if !selection.is_empty() {
        return None;
    }

    let content = input.value().to_string();
    let range = component_line_range_for_offset(&content, selection.start);
    Some((content, range))
}

fn component_line_range_for_offset(content: &str, offset: usize) -> Range<usize> {
    let offset = offset.min(content.len());
    let start = content[..offset].rfind('\n').map_or(0, |index| index + 1);
    let end = content[offset..]
        .find('\n')
        .map_or(content.len(), |index| offset + index + 1);
    start..end
}

fn component_range_to_utf16(content: &str, range: &Range<usize>) -> Range<usize> {
    component_offset_to_utf16(content, range.start)..component_offset_to_utf16(content, range.end)
}

fn component_offset_to_utf16(content: &str, offset: usize) -> usize {
    let mut utf16_offset = 0usize;
    let mut utf8_offset = 0usize;
    for ch in content.chars() {
        if utf8_offset >= offset {
            break;
        }
        utf8_offset += ch.len_utf8();
        utf16_offset += ch.len_utf16();
    }
    utf16_offset
}

pub fn install_default_keybindings(cx: &mut App) {
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new(
            "alt-left",
            gpui_component::input::MoveToPreviousWord,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "alt-right",
            gpui_component::input::MoveToNextWord,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "alt-shift-left",
            gpui_component::input::SelectToPreviousWordStart,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
        KeyBinding::new(
            "alt-shift-right",
            gpui_component::input::SelectToNextWordEnd,
            Some(COMPONENT_INPUT_KEY_CONTEXT),
        ),
    ]);
}
