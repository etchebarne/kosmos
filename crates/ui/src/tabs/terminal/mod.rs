use std::cell::Cell as StdCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use gpui::{
    Anchor, AnyElement, App, Bounds, ClipboardItem, Context, Element, ElementId,
    ElementInputHandler, Entity, FocusHandle, FontFeatures, Global, GlobalElementId,
    InteractiveElement, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, SharedString, StrikethroughStyle,
    Style, TextRun, UnderlineStyle, Window, canvas, div, fill, outline, point, prelude::*, px,
    relative, rems, rgb,
};
use gpui_component::{
    Icon as ComponentIcon, Sizable,
    button::{Button, ButtonVariants},
    menu::{DropdownMenu, PopupMenuItem},
    separator::Separator,
};
use icons::IconName;
use terminal::{
    ShellProfile, TerminalCell, TerminalCellRun, TerminalColor, TerminalCursorShape, TerminalKey,
    TerminalKeyInput, TerminalMouseButton, TerminalMouseModifiers, TerminalPalette,
    TerminalSelectionRange, TerminalSession, TerminalSnapshot, TerminalStatus, TerminalStore,
    TerminalStyle, TerminalTheme,
};
use theme::{ActiveTheme, Theme};

const KEY_CONTEXT: &str = "Terminal";
const BASE_FONT_SIZE_REM: f32 = 0.875;
const BASE_ROW_HEIGHT_REM: f32 = 1.15625;
const BASE_CELL_WIDTH_REM: f32 = 0.54;
const CELL_WIDTH_SAMPLE_COLUMNS: usize = 64;
const MONOSPACE_SAMPLE_REPETITIONS: usize = 8;
const MONOSPACE_WIDTH_TOLERANCE_REM: f32 = 0.01;
const TERMINAL_ASCII_WIDTH_SAMPLE: [char; 5] = ['i', 'm', 'W', '0', 'A'];
// Font selection probes these terminal drawing glyphs; rendering remains font-driven.
const TERMINAL_DRAWING_WIDTH_SAMPLE: [char; 4] = ['\u{2502}', '\u{2580}', '\u{2584}', '\u{2588}'];
const BOTTOM_BAR_HEIGHT_REM: f32 = 1.75;
const BOTTOM_BAR_BUTTON_SIZE_REM: f32 = 1.375;
const SHELL_PICKER_WIDTH_REM: f32 = 8.0;

#[derive(Default)]
pub struct TerminalUi;

impl TerminalUi {
    pub fn install(cx: &mut App) {
        cx.set_global(Self::default());
    }

    pub fn close_shell_picker(&mut self) -> bool {
        false
    }
}

impl Global for TerminalUi {}

pub fn render<T: 'static>(
    workspace_id: usize,
    cwd: &Path,
    tab_id: usize,
    window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let key = TerminalKey::new(workspace_id, tab_id);
    let session = TerminalStore::for_tab(key, cwd.to_path_buf(), cx);
    observe_session(&session, cx);
    let theme = *cx.theme();
    let terminal_theme = terminal_theme(theme);
    session.update(cx, |session, _| session.set_theme(terminal_theme));

    let snapshot = session.update(cx, |session, _| session.snapshot());
    let screen_background = dominant_background_color(&snapshot, terminal_theme.background);
    let selection_color =
        gpui::Hsla::from(theme.accent).opacity(if theme.is_dark { 0.35 } else { 0.25 });
    let metrics = terminal_metrics(
        snapshot.zoom_percent,
        screen_background,
        snapshot.cursor_color,
        selection_color,
        window,
    );
    let focus_handle = session.read(cx).focus_handle();
    let is_focused = focus_handle.is_focused(window);

    let bottom_bar = render_bottom_bar(&session, &snapshot, cx);
    let screen = render_screen(
        &session,
        snapshot,
        metrics,
        focus_handle,
        is_focused,
        window,
        cx,
    );

    div()
        .size_full()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgba_for_terminal_color(screen_background))
        .child(screen)
        .child(bottom_bar)
        .into_any_element()
}

fn terminal_theme(theme: Theme) -> TerminalTheme {
    TerminalTheme::new(
        TerminalPalette::for_dark_theme(theme.is_dark),
        terminal_color_from_rgba(theme.text),
        terminal_color_from_rgba(theme.bg_surface),
    )
}

fn terminal_color_from_rgba(color: gpui::Rgba) -> TerminalColor {
    TerminalColor {
        r: rgba_component_to_u8(color.r),
        g: rgba_component_to_u8(color.g),
        b: rgba_component_to_u8(color.b),
    }
}

fn rgba_component_to_u8(component: f32) -> u8 {
    (component.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn observe_session<T: 'static>(session: &Entity<TerminalSession>, cx: &mut Context<T>) {
    if session.read(cx).observed_by_ui() {
        return;
    }
    session.update(cx, |session, _| session.mark_observed_by_ui());
    cx.observe(session, |_, _, cx| cx.notify()).detach();
}

#[derive(Clone, Copy)]
struct TerminalMetrics {
    font_family: &'static str,
    font_size_rem: f32,
    row_height_rem: f32,
    cell_width_rem: f32,
    screen_background: TerminalColor,
    cursor_color: TerminalColor,
    selection_color: gpui::Hsla,
}

#[derive(Clone, Copy)]
struct TerminalBackgroundRect {
    row: usize,
    rows: usize,
    column: usize,
    width: usize,
    color: TerminalColor,
}

struct TerminalPaintRect {
    bounds: Bounds<Pixels>,
    color: TerminalColor,
}

#[derive(Clone, Copy)]
struct TerminalGridPixels {
    origin: Point<Pixels>,
    cell_width: Pixels,
    row_height: Pixels,
    font_size: Pixels,
    underline_thickness: Pixels,
    cursor_thickness: Pixels,
}

struct TerminalPaintTextRun {
    origin: Point<Pixels>,
    line: ShapedLine,
}

struct TerminalPaintBuffers<'a> {
    text_runs: &'a mut Vec<TerminalPaintTextRun>,
    custom_glyph_rects: &'a mut Vec<TerminalPaintCustomGlyphRect>,
}

struct TerminalTextRunBuilder {
    row: usize,
    column: usize,
    width: usize,
    text: String,
    style: TerminalStyle,
}

struct TerminalPaintCustomGlyphRect {
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
}

#[derive(Clone, Copy)]
enum TerminalLineWeight {
    Light,
    Heavy,
}

#[derive(Clone, Copy)]
struct TerminalBoxDrawingGlyph {
    up: Option<TerminalLineWeight>,
    right: Option<TerminalLineWeight>,
    down: Option<TerminalLineWeight>,
    left: Option<TerminalLineWeight>,
}

struct TerminalPaintCursor {
    bounds: Bounds<Pixels>,
    shape: TerminalCursorShape,
}

struct TerminalSurfacePaintState {
    background_rects: Vec<TerminalPaintRect>,
    selection_rects: Vec<TerminalPaintCustomGlyphRect>,
    custom_glyph_rects: Vec<TerminalPaintCustomGlyphRect>,
    text_runs: Vec<TerminalPaintTextRun>,
}

fn terminal_metrics(
    zoom_percent: i64,
    screen_background: TerminalColor,
    cursor_color: TerminalColor,
    selection_color: gpui::Hsla,
    window: &mut Window,
) -> TerminalMetrics {
    let font_family = terminal_font_family(window);
    let scale = zoom_percent as f32 / 100.0;
    let font_size_rem = BASE_FONT_SIZE_REM * scale;
    let cell_width_rem = terminal_base_cell_width_rem(font_family, window) * scale;
    TerminalMetrics {
        font_family,
        font_size_rem,
        row_height_rem: BASE_ROW_HEIGHT_REM * scale,
        cell_width_rem,
        screen_background,
        cursor_color,
        selection_color,
    }
}

fn terminal_base_cell_width_rem(font_family: &str, window: &mut Window) -> f32 {
    static TERMINAL_BASE_CELL_WIDTH_REM: OnceLock<f32> = OnceLock::new();
    *TERMINAL_BASE_CELL_WIDTH_REM.get_or_init(|| {
        measure_terminal_cell_width_rem(font_family, BASE_FONT_SIZE_REM, window)
            .unwrap_or(BASE_CELL_WIDTH_REM)
    })
}

fn terminal_font_family(window: &mut Window) -> &'static str {
    static TERMINAL_FONT_FAMILY: OnceLock<String> = OnceLock::new();
    TERMINAL_FONT_FAMILY.get_or_init(|| choose_terminal_font_family(window))
}

fn choose_terminal_font_family(window: &mut Window) -> String {
    let mut best_font: Option<(String, f32)> = None;
    for font_family in window.text_system().all_font_names() {
        if font_family.starts_with('.') {
            continue;
        }
        let Some(score) = terminal_font_score(&font_family, window) else {
            continue;
        };
        let is_better = best_font
            .as_ref()
            .is_none_or(|(_, best_score)| score < *best_score);
        if is_better {
            best_font = Some((font_family, score));
        }
    }

    best_font
        .map(|(font_family, _)| font_family)
        .unwrap_or_else(|| ".SystemUIFont".to_string())
}

fn terminal_font_score(font_family: &str, window: &mut Window) -> Option<f32> {
    let space_width = repeated_character_width_rem(font_family, ' ', window)?;

    let mut score = 0.0;
    for character in TERMINAL_ASCII_WIDTH_SAMPLE {
        let width = repeated_character_width_rem(font_family, character, window)?;
        let deviation = (width - space_width).abs();
        if deviation > MONOSPACE_WIDTH_TOLERANCE_REM {
            return None;
        }
        score += deviation;
    }

    for character in TERMINAL_DRAWING_WIDTH_SAMPLE {
        let width = repeated_character_width_rem(font_family, character, window)?;
        score += (width - space_width).abs();
    }

    Some(score)
}

fn repeated_character_width_rem(font_family: &str, ch: char, window: &mut Window) -> Option<f32> {
    let sample = ch.to_string().repeat(MONOSPACE_SAMPLE_REPETITIONS);
    let total_width =
        measure_terminal_text_width_rem(font_family, BASE_FONT_SIZE_REM, sample, window)?;
    Some(total_width / MONOSPACE_SAMPLE_REPETITIONS as f32)
}

fn terminal_font(font_family: &str) -> gpui::Font {
    let mut font = gpui::font(font_family.to_string());
    font.features = FontFeatures::disable_ligatures();
    font
}

fn measure_terminal_cell_width_rem(
    font_family: &str,
    font_size_rem: f32,
    window: &mut Window,
) -> Option<f32> {
    let text = " ".repeat(CELL_WIDTH_SAMPLE_COLUMNS);
    let total_width = measure_terminal_text_width_rem(font_family, font_size_rem, text, window)?;
    Some(total_width / CELL_WIDTH_SAMPLE_COLUMNS as f32)
}

fn measure_terminal_text_width_rem(
    font_family: &str,
    font_size_rem: f32,
    text: String,
    window: &mut Window,
) -> Option<f32> {
    let mut text_style = window.text_style();
    text_style.font_family = SharedString::from(font_family.to_string());
    text_style.font_features = FontFeatures::disable_ligatures();
    text_style.font_size = rems(font_size_rem).into();

    let text = SharedString::from(text);
    let text_len = text.len();
    let run = text_style.to_run(text.len());
    let rem_size = window.rem_size();
    let font_size = rems(font_size_rem).to_pixels(rem_size);
    let line_height = rems(BASE_ROW_HEIGHT_REM).to_pixels(rem_size);
    let lines = window
        .text_system()
        .shape_text(text, font_size, &[run], None, None)
        .ok()?;
    let width = lines.first()?.position_for_index(text_len, line_height)?.x;
    let width_rem = f32::from(width) / f32::from(rem_size);
    (width_rem > 0.0).then_some(width_rem)
}

fn dominant_background_color(
    snapshot: &TerminalSnapshot,
    fallback_background: TerminalColor,
) -> TerminalColor {
    let mut weights: Vec<(TerminalColor, usize)> = Vec::new();
    for row in &snapshot.rows {
        for run in &row.cell_runs {
            if let Some((_, weight)) = weights
                .iter_mut()
                .find(|(color, _)| *color == run.style.background)
            {
                *weight += run.width;
            } else {
                weights.push((run.style.background, run.width));
            }
        }
    }

    weights
        .into_iter()
        .max_by_key(|(_, weight)| *weight)
        .map(|(color, _)| color)
        .unwrap_or(fallback_background)
}

fn render_screen<T: 'static>(
    session: &Entity<TerminalSession>,
    snapshot: Arc<TerminalSnapshot>,
    metrics: TerminalMetrics,
    focus_handle: FocusHandle,
    is_focused: bool,
    _window: &mut Window,
    cx: &mut Context<T>,
) -> AnyElement {
    let session_for_resize = session.clone();
    let session_for_key = session.clone();
    let session_for_copy = session.clone();
    let session_for_paste = session.clone();
    let session_for_scroll = session.clone();
    let session_for_mouse_down = session.clone();
    let session_for_mouse_move = session.clone();
    let session_for_mouse_up_left = session.clone();
    let session_for_mouse_up_middle = session.clone();
    let session_for_mouse_up_right = session.clone();
    let session_for_mouse_up_out = session.clone();
    let focus_for_click = focus_handle.clone();
    let terminal_bounds = Rc::new(StdCell::new(None));
    let bounds_for_mouse_down = terminal_bounds.clone();
    let bounds_for_mouse_move = terminal_bounds.clone();
    let bounds_for_mouse_up_left = terminal_bounds.clone();
    let bounds_for_mouse_up_middle = terminal_bounds.clone();
    let bounds_for_mouse_up_right = terminal_bounds.clone();
    let bounds_for_mouse_up_out = terminal_bounds.clone();
    let bounds_for_scroll = terminal_bounds.clone();
    let bounds_for_resize = terminal_bounds.clone();
    let columns = snapshot.columns;
    let rows = snapshot.screen_rows;
    let cursor = snapshot.cursor;
    let surface = render_terminal_surface(snapshot, metrics);

    div()
        .relative()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .overflow_hidden()
        .track_focus(&focus_handle)
        .key_context(KEY_CONTEXT)
        .font(terminal_font(metrics.font_family))
        .text_size(rems(metrics.font_size_rem))
        .line_height(rems(metrics.row_height_rem))
        .bg(rgba_for_terminal_color(metrics.screen_background))
        .text_color(rgba_for_terminal_color(metrics.cursor_color))
        .on_any_mouse_down(cx.listener(move |_, event: &MouseDownEvent, window, cx| {
            window.focus(&focus_for_click, cx);
            let Some(button) = terminal_mouse_button(event.button) else {
                return;
            };
            let Some((row, column)) = terminal_grid_position_for_point(
                event.position,
                bounds_for_mouse_down.get(),
                metrics,
                window,
                columns,
                rows,
            ) else {
                return;
            };
            let handled = session_for_mouse_down.update(cx, |session, cx| {
                session.mouse_down(
                    row,
                    column,
                    button,
                    terminal_mouse_modifiers(event.modifiers),
                    cx,
                )
            });
            if handled {
                cx.stop_propagation();
            }
        }))
        .on_mouse_move(cx.listener(move |_, event: &MouseMoveEvent, window, cx| {
            let Some((row, column)) = terminal_grid_position_for_point(
                event.position,
                bounds_for_mouse_move.get(),
                metrics,
                window,
                columns,
                rows,
            ) else {
                return;
            };
            let handled = session_for_mouse_move.update(cx, |session, cx| {
                session.mouse_move(row, column, terminal_mouse_modifiers(event.modifiers), cx)
            });
            if handled {
                cx.stop_propagation();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |_, event: &MouseUpEvent, window, cx| {
                let Some((row, column)) = terminal_grid_position_for_point(
                    event.position,
                    bounds_for_mouse_up_left.get(),
                    metrics,
                    window,
                    columns,
                    rows,
                ) else {
                    return;
                };
                let handled = session_for_mouse_up_left.update(cx, |session, cx| {
                    session.mouse_up(
                        row,
                        column,
                        TerminalMouseButton::Left,
                        terminal_mouse_modifiers(event.modifiers),
                        cx,
                    )
                });
                if handled {
                    cx.stop_propagation();
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(move |_, event: &MouseUpEvent, window, cx| {
                let Some((row, column)) = terminal_grid_position_for_point(
                    event.position,
                    bounds_for_mouse_up_middle.get(),
                    metrics,
                    window,
                    columns,
                    rows,
                ) else {
                    return;
                };
                let handled = session_for_mouse_up_middle.update(cx, |session, cx| {
                    session.mouse_up(
                        row,
                        column,
                        TerminalMouseButton::Middle,
                        terminal_mouse_modifiers(event.modifiers),
                        cx,
                    )
                });
                if handled {
                    cx.stop_propagation();
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Right,
            cx.listener(move |_, event: &MouseUpEvent, window, cx| {
                let Some((row, column)) = terminal_grid_position_for_point(
                    event.position,
                    bounds_for_mouse_up_right.get(),
                    metrics,
                    window,
                    columns,
                    rows,
                ) else {
                    return;
                };
                let handled = session_for_mouse_up_right.update(cx, |session, cx| {
                    session.mouse_up(
                        row,
                        column,
                        TerminalMouseButton::Right,
                        terminal_mouse_modifiers(event.modifiers),
                        cx,
                    )
                });
                if handled {
                    cx.stop_propagation();
                }
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            move |event: &MouseUpEvent, window: &mut Window, cx: &mut App| {
                let Some((row, column)) = terminal_grid_position_for_point(
                    event.position,
                    bounds_for_mouse_up_out.get(),
                    metrics,
                    window,
                    columns,
                    rows,
                ) else {
                    return;
                };
                let handled = session_for_mouse_up_out.update(cx, |session, cx| {
                    session.mouse_up(
                        row,
                        column,
                        TerminalMouseButton::Left,
                        terminal_mouse_modifiers(event.modifiers),
                        cx,
                    )
                });
                if handled {
                    cx.stop_propagation();
                }
            },
        )
        .capture_key_down(cx.listener(move |_, event: &KeyDownEvent, _window, cx| {
            if is_terminal_copy(event) {
                if let Some(text) =
                    session_for_copy.update(cx, |session, _| session.selected_text())
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
                cx.stop_propagation();
                return;
            }

            if is_terminal_paste(event) {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    session_for_paste.update(cx, |session, cx| {
                        session.paste(&text);
                        cx.notify();
                    });
                }
                cx.stop_propagation();
                return;
            }

            let input = terminal_key_input(event);
            let handled = session_for_key.update(cx, |session, cx| {
                let handled = session.write_key(input);
                if handled {
                    cx.notify();
                }
                handled
            });
            if handled {
                cx.stop_propagation();
            }
        }))
        .on_scroll_wheel(move |event, window, cx| {
            let row_height = terminal_row_height_pixels(metrics, window);
            let delta = event.delta.pixel_delta(row_height);
            let lines = if row_height == Pixels::ZERO {
                0
            } else {
                (delta.y / row_height).round() as i32
            };
            if lines != 0 {
                let handled = terminal_grid_position_for_point(
                    event.position,
                    bounds_for_scroll.get(),
                    metrics,
                    window,
                    columns,
                    rows,
                )
                .is_some_and(|(row, column)| {
                    session_for_scroll.update(cx, |session, cx| {
                        session.scroll_lines_at(
                            row,
                            column,
                            lines,
                            terminal_mouse_modifiers(event.modifiers),
                            cx,
                        )
                    })
                });
                if !handled {
                    session_for_scroll.update(cx, |session, cx| session.scroll_lines(lines, cx));
                }
                cx.stop_propagation();
            }
        })
        .child(
            div()
                .size_full()
                .min_w_0()
                .min_h_0()
                .relative()
                .child(surface)
                .when_some(cursor, |this, cursor| {
                    this.child(render_cursor(cursor, metrics, is_focused))
                })
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .left_0()
                        .child(TerminalInputElement {
                            session: session.clone(),
                        }),
                ),
        )
        .on_children_prepainted(move |bounds, window, cx| {
            let Some(bounds) = bounds.first().copied() else {
                return;
            };
            bounds_for_resize.set(Some(bounds));
            resize_terminal_to_bounds(&session_for_resize, bounds, metrics, window, cx);
        })
        .into_any_element()
}

fn render_terminal_surface(
    snapshot: Arc<TerminalSnapshot>,
    metrics: TerminalMetrics,
) -> AnyElement {
    canvas(
        move |bounds, window, _cx| {
            let grid = terminal_grid_pixels(bounds, metrics, window);
            let background_rects = terminal_background_rects(&snapshot, metrics.screen_background)
                .iter()
                .map(|rect| TerminalPaintRect {
                    bounds: snapped_terminal_rect(grid, rect),
                    color: rect.color,
                })
                .collect::<Vec<_>>();
            let selection_rects =
                terminal_selection_rects(&snapshot.selection_ranges, grid, metrics);
            let mut text_runs = Vec::new();
            let mut custom_glyph_rects = Vec::new();
            {
                let mut paint_buffers = TerminalPaintBuffers {
                    text_runs: &mut text_runs,
                    custom_glyph_rects: &mut custom_glyph_rects,
                };
                for (row_index, row) in snapshot.rows.iter().enumerate() {
                    for run in &row.cell_runs {
                        push_terminal_cell_run(
                            row_index,
                            run,
                            &row.cells,
                            grid,
                            metrics,
                            window,
                            &mut paint_buffers,
                        );
                    }
                }
            }

            TerminalSurfacePaintState {
                background_rects,
                selection_rects,
                custom_glyph_rects,
                text_runs,
            }
        },
        move |bounds, paint_state, window, cx| {
            window.paint_quad(fill(
                bounds,
                rgba_for_terminal_color(metrics.screen_background),
            ));
            for rect in paint_state.background_rects {
                window.paint_quad(fill(rect.bounds, rgba_for_terminal_color(rect.color)));
            }
            for rect in paint_state.selection_rects {
                window.paint_quad(fill(rect.bounds, rect.color));
            }
            for rect in paint_state.custom_glyph_rects {
                window.paint_quad(fill(rect.bounds, rect.color));
            }
            let row_height = terminal_grid_pixels(bounds, metrics, window).row_height;
            for run in paint_state.text_runs {
                let _ = run.line.paint(
                    run.origin,
                    row_height,
                    gpui::TextAlign::default(),
                    None,
                    window,
                    cx,
                );
            }
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .bottom_0()
    .left_0()
    .into_any_element()
}

fn terminal_grid_pixels(
    bounds: Bounds<Pixels>,
    metrics: TerminalMetrics,
    window: &mut Window,
) -> TerminalGridPixels {
    let rem_size = window.rem_size();
    TerminalGridPixels {
        origin: point(
            floor_to_device_pixels(bounds.left(), window),
            floor_to_device_pixels(bounds.top(), window),
        ),
        cell_width: terminal_cell_width_pixels(metrics, window),
        row_height: terminal_row_height_pixels(metrics, window),
        font_size: rems(metrics.font_size_rem).to_pixels(rem_size),
        underline_thickness: rems(0.0625).to_pixels(rem_size),
        cursor_thickness: round_to_device_pixels(rems(0.125).to_pixels(rem_size), window)
            .max(px(1.0)),
    }
}

fn floor_to_device_pixels(value: Pixels, window: &Window) -> Pixels {
    let scale_factor = window.scale_factor().max(1.0);
    px((f32::from(value) * scale_factor).floor() / scale_factor)
}

fn round_to_device_pixels(value: Pixels, window: &Window) -> Pixels {
    let scale_factor = window.scale_factor().max(1.0);
    px((f32::from(value) * scale_factor).round() / scale_factor)
}

fn terminal_cell_width_pixels(metrics: TerminalMetrics, window: &mut Window) -> Pixels {
    round_to_device_pixels(
        rems(metrics.cell_width_rem).to_pixels(window.rem_size()),
        window,
    )
    .max(px(1.0))
}

fn terminal_row_height_pixels(metrics: TerminalMetrics, window: &mut Window) -> Pixels {
    round_to_device_pixels(
        rems(metrics.row_height_rem).to_pixels(window.rem_size()),
        window,
    )
    .max(px(1.0))
}

fn snapped_terminal_rect(
    grid: TerminalGridPixels,
    rect: &TerminalBackgroundRect,
) -> Bounds<Pixels> {
    terminal_cell_bounds(rect.row, rect.column, rect.rows, rect.width, grid)
}

fn shape_terminal_text_run(
    text: String,
    style: TerminalStyle,
    origin: Point<Pixels>,
    grid: TerminalGridPixels,
    metrics: TerminalMetrics,
    window: &mut Window,
) -> Option<TerminalPaintTextRun> {
    if !terminal_text_has_visible_content(&text, style) {
        return None;
    }

    let text = SharedString::from(text);
    let text_run = terminal_text_run(text.len(), style, metrics, grid.underline_thickness);
    let line =
        window
            .text_system()
            .shape_line(text, grid.font_size, &[text_run], Some(grid.cell_width));

    Some(TerminalPaintTextRun { origin, line })
}

fn push_terminal_cell_run(
    row: usize,
    run: &TerminalCellRun,
    cells: &[TerminalCell],
    grid: TerminalGridPixels,
    metrics: TerminalMetrics,
    window: &mut Window,
    paint_buffers: &mut TerminalPaintBuffers<'_>,
) {
    if !terminal_text_has_visible_content(&run.text, run.style) {
        return;
    }

    if terminal_text_contains_custom_glyph_candidate(&run.text) {
        push_terminal_mixed_cell_run(row, run, cells, grid, metrics, window, paint_buffers);
        return;
    }

    let origin = point(
        grid.origin.x + grid.cell_width * run.column,
        grid.origin.y + grid.row_height * row,
    );
    if let Some(text_run) =
        shape_terminal_text_run(run.text.clone(), run.style, origin, grid, metrics, window)
    {
        paint_buffers.text_runs.push(text_run);
    }
}

fn push_terminal_mixed_cell_run(
    row: usize,
    run: &TerminalCellRun,
    cells: &[TerminalCell],
    grid: TerminalGridPixels,
    metrics: TerminalMetrics,
    window: &mut Window,
    paint_buffers: &mut TerminalPaintBuffers<'_>,
) {
    let mut text_builder: Option<TerminalTextRunBuilder> = None;
    let run_end = run.column + run.width;

    for cell in cells {
        if cell.column >= run_end {
            break;
        }
        if cell.column + cell.width <= run.column {
            continue;
        }

        let cell_bounds = terminal_cell_bounds(row, cell.column, 1, cell.width, grid);
        if let Some(glyph_rects) = terminal_custom_glyph_rects(cell, cell_bounds) {
            flush_terminal_text_run(
                &mut text_builder,
                grid,
                metrics,
                window,
                paint_buffers.text_runs,
            );
            paint_buffers.custom_glyph_rects.extend(glyph_rects);
        } else {
            push_terminal_text_cell(
                &mut text_builder,
                row,
                cell,
                grid,
                metrics,
                window,
                paint_buffers.text_runs,
            );
        }
    }

    flush_terminal_text_run(
        &mut text_builder,
        grid,
        metrics,
        window,
        paint_buffers.text_runs,
    );
}

fn push_terminal_text_cell(
    builder: &mut Option<TerminalTextRunBuilder>,
    row: usize,
    cell: &TerminalCell,
    grid: TerminalGridPixels,
    metrics: TerminalMetrics,
    window: &mut Window,
    text_runs: &mut Vec<TerminalPaintTextRun>,
) {
    if !terminal_cell_has_visible_text(cell) && builder.is_none() {
        return;
    }

    if let Some(builder) = builder.as_mut()
        && builder.style == cell.style
        && builder.row == row
        && builder.column + builder.width == cell.column
    {
        builder.text.push_str(&cell.text);
        builder.width += cell.width;
        return;
    }

    flush_terminal_text_run(builder, grid, metrics, window, text_runs);

    if terminal_cell_has_visible_text(cell) {
        *builder = Some(TerminalTextRunBuilder {
            row,
            column: cell.column,
            width: cell.width,
            text: cell.text.clone(),
            style: cell.style,
        });
    }
}

fn flush_terminal_text_run(
    builder: &mut Option<TerminalTextRunBuilder>,
    grid: TerminalGridPixels,
    metrics: TerminalMetrics,
    window: &mut Window,
    text_runs: &mut Vec<TerminalPaintTextRun>,
) {
    let Some(builder) = builder.take() else {
        return;
    };
    let origin = point(
        grid.origin.x + grid.cell_width * builder.column,
        grid.origin.y + grid.row_height * builder.row,
    );
    if let Some(text_run) =
        shape_terminal_text_run(builder.text, builder.style, origin, grid, metrics, window)
    {
        text_runs.push(text_run);
    }
}

fn terminal_cell_has_visible_text(cell: &TerminalCell) -> bool {
    terminal_text_has_visible_content(&cell.text, cell.style)
}

fn terminal_text_has_visible_content(text: &str, style: TerminalStyle) -> bool {
    style.underline || style.strikeout || text.chars().any(|character| !character.is_whitespace())
}

fn terminal_text_contains_custom_glyph_candidate(text: &str) -> bool {
    text.chars().any(is_terminal_custom_glyph_candidate)
}

fn is_terminal_custom_glyph_candidate(character: char) -> bool {
    matches!(
        character as u32,
        0x2500..=0x259f | 0x2800..=0x28ff | 0x1fb00..=0x1fbff
    )
}

fn terminal_custom_glyph_rects(
    cell: &TerminalCell,
    bounds: Bounds<Pixels>,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let character = single_terminal_character(&cell.text)?;
    let color = terminal_style_foreground(cell.style);
    terminal_block_element_rects(character, bounds, color)
        .or_else(|| terminal_legacy_computing_rects(character, bounds, color))
        .or_else(|| terminal_braille_rects(character, bounds, color))
        .or_else(|| terminal_box_drawing_rects(character, bounds, color))
}

fn single_terminal_character(text: &str) -> Option<char> {
    let mut characters = text.chars();
    let character = characters.next()?;
    characters.next().is_none().then_some(character)
}

fn terminal_block_element_rects(
    character: char,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let codepoint = character as u32;
    let mut rects = Vec::new();

    match codepoint {
        0x2580 => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.0, 0.0, 1.0, 0.5)
        }
        0x2581..=0x2588 => {
            let height = (codepoint - 0x2580) as f32 / 8.0;
            push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                color,
                0.0,
                1.0 - height,
                1.0,
                height,
            );
        }
        0x2589..=0x258f => {
            let width = (0x2590 - codepoint) as f32 / 8.0;
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.0, 0.0, width, 1.0);
        }
        0x2590 => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.5, 0.0, 0.5, 1.0)
        }
        0x2591 => push_terminal_custom_glyph_rect(&mut rects, bounds, color.opacity(0.25)),
        0x2592 => push_terminal_custom_glyph_rect(&mut rects, bounds, color.opacity(0.5)),
        0x2593 => push_terminal_custom_glyph_rect(&mut rects, bounds, color.opacity(0.75)),
        0x2594 => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.0, 0.0, 1.0, 0.125)
        }
        0x2595 => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.875, 0.0, 0.125, 1.0)
        }
        0x2596..=0x259f => push_terminal_quadrant_glyph(&mut rects, codepoint, bounds, color),
        _ => return None,
    }

    Some(rects)
}

fn terminal_legacy_computing_rects(
    character: char,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let codepoint = character as u32;

    terminal_sextant_mask(codepoint)
        .map(|mask| terminal_sextant_rects(mask, bounds, color))
        .or_else(|| terminal_legacy_octant_rects(codepoint, bounds, color))
        .or_else(|| terminal_legacy_shade_rects(codepoint, bounds, color))
}

fn terminal_sextant_mask(codepoint: u32) -> Option<u8> {
    const SEXTANT_MASKS: [u8; 60] = [
        0b000001, 0b000010, 0b000011, 0b000100, 0b000101, 0b000110, 0b000111, 0b001000, 0b001001,
        0b001010, 0b001011, 0b001100, 0b001101, 0b001110, 0b001111, 0b010000, 0b010001, 0b010010,
        0b010011, 0b010100, 0b010110, 0b010111, 0b011000, 0b011001, 0b011010, 0b011011, 0b011100,
        0b011101, 0b011110, 0b011111, 0b100000, 0b100001, 0b100010, 0b100011, 0b100100, 0b100101,
        0b100110, 0b100111, 0b101000, 0b101001, 0b101011, 0b101100, 0b101101, 0b101110, 0b101111,
        0b110000, 0b110001, 0b110010, 0b110011, 0b110100, 0b110101, 0b110110, 0b110111, 0b111000,
        0b111001, 0b111010, 0b111011, 0b111100, 0b111101, 0b111110,
    ];

    let index = codepoint.checked_sub(0x1fb00)? as usize;
    SEXTANT_MASKS.get(index).copied()
}

fn terminal_sextant_rects(
    mask: u8,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Vec<TerminalPaintCustomGlyphRect> {
    let mut rects = Vec::new();
    let row_tops = [0.0, 3.0 / 8.0, 5.0 / 8.0];
    let row_heights = [3.0 / 8.0, 2.0 / 8.0, 3.0 / 8.0];

    for row in 0..3 {
        let left_bit = mask & (1 << (row * 2)) != 0;
        let right_bit = mask & (1 << (row * 2 + 1)) != 0;
        match (left_bit, right_bit) {
            (true, true) => push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                color,
                0.0,
                row_tops[row],
                1.0,
                row_heights[row],
            ),
            (true, false) => push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                color,
                0.0,
                row_tops[row],
                0.5,
                row_heights[row],
            ),
            (false, true) => push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                color,
                0.5,
                row_tops[row],
                0.5,
                row_heights[row],
            ),
            (false, false) => {}
        }
    }

    rects
}

fn terminal_legacy_octant_rects(
    codepoint: u32,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let mut rects = Vec::new();

    match codepoint {
        0x1fb70..=0x1fb75 => {
            let column = (codepoint - 0x1fb70 + 1) as f32;
            push_terminal_octant_glyph(&mut rects, bounds, color, column, 0.0, 1.0, 8.0);
        }
        0x1fb76..=0x1fb7b => {
            let row = (codepoint - 0x1fb76 + 1) as f32;
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, row, 8.0, 1.0);
        }
        0x1fb7c => {
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 0.0, 1.0, 8.0);
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 7.0, 8.0, 1.0);
        }
        0x1fb7d => {
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 0.0, 1.0, 8.0);
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 0.0, 8.0, 1.0);
        }
        0x1fb7e => {
            push_terminal_octant_glyph(&mut rects, bounds, color, 7.0, 0.0, 1.0, 8.0);
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 0.0, 8.0, 1.0);
        }
        0x1fb7f => {
            push_terminal_octant_glyph(&mut rects, bounds, color, 7.0, 0.0, 1.0, 8.0);
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 7.0, 8.0, 1.0);
        }
        0x1fb80 => {
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 0.0, 8.0, 1.0);
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 7.0, 8.0, 1.0);
        }
        0x1fb81 => {
            for row in [0.0, 2.0, 4.0, 7.0] {
                push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, row, 8.0, 1.0);
            }
        }
        0x1fb82..=0x1fb86 => {
            let height = match codepoint {
                0x1fb82 => 2.0,
                0x1fb83 => 3.0,
                0x1fb84 => 5.0,
                0x1fb85 => 6.0,
                _ => 7.0,
            };
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 0.0, 8.0, height);
        }
        0x1fb87..=0x1fb8b => {
            let (left, width) = match codepoint {
                0x1fb87 => (6.0, 2.0),
                0x1fb88 => (5.0, 3.0),
                0x1fb89 => (3.0, 5.0),
                0x1fb8a => (2.0, 6.0),
                _ => (1.0, 7.0),
            };
            push_terminal_octant_glyph(&mut rects, bounds, color, left, 0.0, width, 8.0);
        }
        0x1fb95 => push_terminal_checker_glyph(&mut rects, bounds, color, false),
        0x1fb96 => push_terminal_checker_glyph(&mut rects, bounds, color, true),
        0x1fb97 => {
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 2.0, 8.0, 2.0);
            push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 6.0, 8.0, 2.0);
        }
        0x1fbce => {
            push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                color,
                0.0,
                0.0,
                2.0 / 3.0,
                1.0,
            );
        }
        0x1fbcf => {
            push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                color,
                0.0,
                0.0,
                1.0 / 3.0,
                1.0,
            );
        }
        0x1fbe4 => push_terminal_octant_glyph(&mut rects, bounds, color, 2.0, 0.0, 4.0, 4.0),
        0x1fbe5 => push_terminal_octant_glyph(&mut rects, bounds, color, 2.0, 4.0, 4.0, 4.0),
        0x1fbe6 => push_terminal_octant_glyph(&mut rects, bounds, color, 0.0, 2.0, 4.0, 4.0),
        0x1fbe7 => push_terminal_octant_glyph(&mut rects, bounds, color, 4.0, 2.0, 4.0, 4.0),
        _ => return None,
    }

    Some(rects)
}

fn terminal_legacy_shade_rects(
    codepoint: u32,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let mut rects = Vec::new();
    let shade_color = color.opacity(0.5);

    match codepoint {
        0x1fb8c => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, shade_color, 0.0, 0.0, 0.5, 1.0)
        }
        0x1fb8d => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, shade_color, 0.5, 0.0, 0.5, 1.0)
        }
        0x1fb8e => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, shade_color, 0.0, 0.0, 1.0, 0.5)
        }
        0x1fb8f => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, shade_color, 0.0, 0.5, 1.0, 0.5)
        }
        0x1fb90 => push_terminal_custom_glyph_rect(&mut rects, bounds, shade_color),
        0x1fb91 => {
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.0, 0.0, 1.0, 0.5);
            push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                shade_color,
                0.0,
                0.5,
                1.0,
                0.5,
            );
        }
        0x1fb92 => {
            push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                shade_color,
                0.0,
                0.0,
                1.0,
                0.5,
            );
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.0, 0.5, 1.0, 0.5);
        }
        0x1fb94 => {
            push_terminal_custom_glyph_fraction(
                &mut rects,
                bounds,
                shade_color,
                0.0,
                0.0,
                0.5,
                1.0,
            );
            push_terminal_custom_glyph_fraction(&mut rects, bounds, color, 0.5, 0.0, 0.5, 1.0);
        }
        _ => return None,
    }

    Some(rects)
}

fn terminal_braille_rects(
    character: char,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let pattern = (character as u32).checked_sub(0x2800)?;
    if pattern > 0xff {
        return None;
    }

    let mut rects = Vec::new();
    let dot_positions = [
        (1.0, 0.0),
        (1.0, 2.0),
        (1.0, 4.0),
        (5.0, 0.0),
        (5.0, 2.0),
        (5.0, 4.0),
        (1.0, 6.0),
        (5.0, 6.0),
    ];
    let x_eighth = bounds.size.width / 8.0;
    let vertical_padding = bounds.size.height * 0.1;
    let y_eighth = (bounds.size.height - vertical_padding * 2.0) / 8.0;
    let radius = x_eighth.min(y_eighth);

    for (bit, (x, y)) in dot_positions.into_iter().enumerate() {
        if pattern & (1 << bit) == 0 {
            continue;
        }
        let center_x = bounds.left() + x_eighth * (x + 1.0);
        let center_y = bounds.top() + vertical_padding + y_eighth * (y + 1.0);
        push_terminal_custom_glyph_rect(
            &mut rects,
            snapped_pixel_bounds(
                center_x - radius,
                center_y - radius,
                center_x + radius,
                center_y + radius,
            ),
            color,
        );
    }

    Some(rects)
}

fn terminal_box_drawing_rects(
    character: char,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) -> Option<Vec<TerminalPaintCustomGlyphRect>> {
    let glyph = terminal_box_drawing_glyph(character)?;
    let mut rects = Vec::new();
    if let Some(weight) = glyph.up {
        push_terminal_box_vertical_segment(&mut rects, bounds, color, weight, 0.0, 0.5);
    }
    if let Some(weight) = glyph.right {
        push_terminal_box_horizontal_segment(&mut rects, bounds, color, weight, 0.5, 1.0);
    }
    if let Some(weight) = glyph.down {
        push_terminal_box_vertical_segment(&mut rects, bounds, color, weight, 0.5, 1.0);
    }
    if let Some(weight) = glyph.left {
        push_terminal_box_horizontal_segment(&mut rects, bounds, color, weight, 0.0, 0.5);
    }

    Some(rects)
}

fn terminal_box_drawing_glyph(character: char) -> Option<TerminalBoxDrawingGlyph> {
    let light = TerminalLineWeight::Light;
    let heavy = TerminalLineWeight::Heavy;
    let glyph = match character {
        '─' => TerminalBoxDrawingGlyph::horizontal(light),
        '━' => TerminalBoxDrawingGlyph::horizontal(heavy),
        '│' => TerminalBoxDrawingGlyph::vertical(light),
        '┃' => TerminalBoxDrawingGlyph::vertical(heavy),
        '┌' => TerminalBoxDrawingGlyph::new(None, Some(light), Some(light), None),
        '┍' => TerminalBoxDrawingGlyph::new(None, Some(heavy), Some(light), None),
        '┎' => TerminalBoxDrawingGlyph::new(None, Some(light), Some(heavy), None),
        '┏' => TerminalBoxDrawingGlyph::new(None, Some(heavy), Some(heavy), None),
        '┐' => TerminalBoxDrawingGlyph::new(None, None, Some(light), Some(light)),
        '┑' => TerminalBoxDrawingGlyph::new(None, None, Some(light), Some(heavy)),
        '┒' => TerminalBoxDrawingGlyph::new(None, None, Some(heavy), Some(light)),
        '┓' => TerminalBoxDrawingGlyph::new(None, None, Some(heavy), Some(heavy)),
        '└' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), None, None),
        '┕' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), None, None),
        '┖' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), None, None),
        '┗' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), None, None),
        '┘' => TerminalBoxDrawingGlyph::new(Some(light), None, None, Some(light)),
        '┙' => TerminalBoxDrawingGlyph::new(Some(light), None, None, Some(heavy)),
        '┚' => TerminalBoxDrawingGlyph::new(Some(heavy), None, None, Some(light)),
        '┛' => TerminalBoxDrawingGlyph::new(Some(heavy), None, None, Some(heavy)),
        '├' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), Some(light), None),
        '┝' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), Some(light), None),
        '┞' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), Some(light), None),
        '┟' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), Some(heavy), None),
        '┠' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), Some(heavy), None),
        '┡' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), Some(light), None),
        '┢' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), Some(heavy), None),
        '┣' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), Some(heavy), None),
        '┤' => TerminalBoxDrawingGlyph::new(Some(light), None, Some(light), Some(light)),
        '┥' => TerminalBoxDrawingGlyph::new(Some(light), None, Some(light), Some(heavy)),
        '┦' => TerminalBoxDrawingGlyph::new(Some(heavy), None, Some(light), Some(light)),
        '┧' => TerminalBoxDrawingGlyph::new(Some(light), None, Some(heavy), Some(light)),
        '┨' => TerminalBoxDrawingGlyph::new(Some(heavy), None, Some(heavy), Some(light)),
        '┩' => TerminalBoxDrawingGlyph::new(Some(heavy), None, Some(light), Some(heavy)),
        '┪' => TerminalBoxDrawingGlyph::new(Some(light), None, Some(heavy), Some(heavy)),
        '┫' => TerminalBoxDrawingGlyph::new(Some(heavy), None, Some(heavy), Some(heavy)),
        '┬' => TerminalBoxDrawingGlyph::new(None, Some(light), Some(light), Some(light)),
        '┭' => TerminalBoxDrawingGlyph::new(None, Some(light), Some(light), Some(heavy)),
        '┮' => TerminalBoxDrawingGlyph::new(None, Some(heavy), Some(light), Some(light)),
        '┯' => TerminalBoxDrawingGlyph::new(None, Some(heavy), Some(light), Some(heavy)),
        '┰' => TerminalBoxDrawingGlyph::new(None, Some(light), Some(heavy), Some(light)),
        '┱' => TerminalBoxDrawingGlyph::new(None, Some(light), Some(heavy), Some(heavy)),
        '┲' => TerminalBoxDrawingGlyph::new(None, Some(heavy), Some(heavy), Some(light)),
        '┳' => TerminalBoxDrawingGlyph::new(None, Some(heavy), Some(heavy), Some(heavy)),
        '┴' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), None, Some(light)),
        '┵' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), None, Some(heavy)),
        '┶' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), None, Some(light)),
        '┷' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), None, Some(heavy)),
        '┸' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), None, Some(light)),
        '┹' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), None, Some(heavy)),
        '┺' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), None, Some(light)),
        '┻' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), None, Some(heavy)),
        '┼' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), Some(light), Some(light)),
        '┽' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), Some(light), Some(heavy)),
        '┾' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), Some(light), Some(light)),
        '┿' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), Some(light), Some(heavy)),
        '╀' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), Some(light), Some(light)),
        '╁' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), Some(heavy), Some(light)),
        '╂' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), Some(heavy), Some(light)),
        '╃' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), Some(light), Some(heavy)),
        '╄' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), Some(light), Some(light)),
        '╅' => TerminalBoxDrawingGlyph::new(Some(light), Some(light), Some(heavy), Some(heavy)),
        '╆' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), Some(heavy), Some(light)),
        '╇' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), Some(light), Some(heavy)),
        '╈' => TerminalBoxDrawingGlyph::new(Some(light), Some(heavy), Some(heavy), Some(heavy)),
        '╉' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(light), Some(heavy), Some(heavy)),
        '╊' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), Some(heavy), Some(light)),
        '╋' => TerminalBoxDrawingGlyph::new(Some(heavy), Some(heavy), Some(heavy), Some(heavy)),
        '╴' => TerminalBoxDrawingGlyph::new(None, None, None, Some(light)),
        '╵' => TerminalBoxDrawingGlyph::new(Some(light), None, None, None),
        '╶' => TerminalBoxDrawingGlyph::new(None, Some(light), None, None),
        '╷' => TerminalBoxDrawingGlyph::new(None, None, Some(light), None),
        '╸' => TerminalBoxDrawingGlyph::new(None, None, None, Some(heavy)),
        '╹' => TerminalBoxDrawingGlyph::new(Some(heavy), None, None, None),
        '╺' => TerminalBoxDrawingGlyph::new(None, Some(heavy), None, None),
        '╻' => TerminalBoxDrawingGlyph::new(None, None, Some(heavy), None),
        '╼' => TerminalBoxDrawingGlyph::new(None, Some(heavy), None, Some(light)),
        '╽' => TerminalBoxDrawingGlyph::new(Some(light), None, Some(heavy), None),
        '╾' => TerminalBoxDrawingGlyph::new(None, Some(light), None, Some(heavy)),
        '╿' => TerminalBoxDrawingGlyph::new(Some(heavy), None, Some(light), None),
        _ => return None,
    };

    Some(glyph)
}

impl TerminalBoxDrawingGlyph {
    fn new(
        up: Option<TerminalLineWeight>,
        right: Option<TerminalLineWeight>,
        down: Option<TerminalLineWeight>,
        left: Option<TerminalLineWeight>,
    ) -> Self {
        Self {
            up,
            right,
            down,
            left,
        }
    }

    fn horizontal(weight: TerminalLineWeight) -> Self {
        Self::new(None, Some(weight), None, Some(weight))
    }

    fn vertical(weight: TerminalLineWeight) -> Self {
        Self::new(Some(weight), None, Some(weight), None)
    }
}

fn push_terminal_box_horizontal_segment(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
    weight: TerminalLineWeight,
    left: f32,
    right: f32,
) {
    let thickness = terminal_box_line_thickness(bounds, weight);
    let y_center = bounds.top() + bounds.size.height / 2.0;
    let segment_left = bounds.left() + bounds.size.width * left;
    let segment_right = bounds.left() + bounds.size.width * right;
    push_terminal_custom_glyph_rect(
        rects,
        snapped_pixel_bounds(
            segment_left,
            y_center - thickness / 2.0,
            segment_right,
            y_center + thickness / 2.0,
        ),
        color,
    );
}

fn push_terminal_box_vertical_segment(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
    weight: TerminalLineWeight,
    top: f32,
    bottom: f32,
) {
    let thickness = terminal_box_line_thickness(bounds, weight);
    let x_center = bounds.left() + bounds.size.width / 2.0;
    let segment_top = bounds.top() + bounds.size.height * top;
    let segment_bottom = bounds.top() + bounds.size.height * bottom;
    push_terminal_custom_glyph_rect(
        rects,
        snapped_pixel_bounds(
            x_center - thickness / 2.0,
            segment_top,
            x_center + thickness / 2.0,
            segment_bottom,
        ),
        color,
    );
}

fn terminal_box_line_thickness(bounds: Bounds<Pixels>, weight: TerminalLineWeight) -> Pixels {
    let light = px((f32::from(bounds.size.width) / 5.0).floor().max(1.0));
    match weight {
        TerminalLineWeight::Light => light,
        TerminalLineWeight::Heavy => (light + px(2.0)).min(bounds.size.width),
    }
}

fn push_terminal_quadrant_glyph(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    codepoint: u32,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) {
    let quadrant_mask = match codepoint {
        0x2596 => 0b0100,
        0x2597 => 0b1000,
        0x2598 => 0b0001,
        0x2599 => 0b1101,
        0x259a => 0b1001,
        0x259b => 0b0111,
        0x259c => 0b1011,
        0x259d => 0b0010,
        0x259e => 0b0110,
        0x259f => 0b1110,
        _ => 0,
    };

    if quadrant_mask & 0b0001 != 0 {
        push_terminal_custom_glyph_fraction(rects, bounds, color, 0.0, 0.0, 0.5, 0.5);
    }
    if quadrant_mask & 0b0010 != 0 {
        push_terminal_custom_glyph_fraction(rects, bounds, color, 0.5, 0.0, 0.5, 0.5);
    }
    if quadrant_mask & 0b0100 != 0 {
        push_terminal_custom_glyph_fraction(rects, bounds, color, 0.0, 0.5, 0.5, 0.5);
    }
    if quadrant_mask & 0b1000 != 0 {
        push_terminal_custom_glyph_fraction(rects, bounds, color, 0.5, 0.5, 0.5, 0.5);
    }
}

fn push_terminal_octant_glyph(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
) {
    push_terminal_custom_glyph_fraction(
        rects,
        bounds,
        color,
        left / 8.0,
        top / 8.0,
        width / 8.0,
        height / 8.0,
    );
}

fn push_terminal_checker_glyph(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
    inverse: bool,
) {
    for row in 0..4 {
        for column in 0..4 {
            if (row + column) % 2 == usize::from(inverse) {
                push_terminal_octant_glyph(
                    rects,
                    bounds,
                    color,
                    column as f32 * 2.0,
                    row as f32 * 2.0,
                    2.0,
                    2.0,
                );
            }
        }
    }
}

fn push_terminal_custom_glyph_rect(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
) {
    if bounds.size.width > Pixels::ZERO && bounds.size.height > Pixels::ZERO {
        rects.push(TerminalPaintCustomGlyphRect { bounds, color });
    }
}

fn push_terminal_custom_glyph_fraction(
    rects: &mut Vec<TerminalPaintCustomGlyphRect>,
    bounds: Bounds<Pixels>,
    color: gpui::Hsla,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
) {
    let bounds = terminal_fraction_bounds(bounds, left, top, width, height);
    push_terminal_custom_glyph_rect(rects, bounds, color);
}

fn terminal_fraction_bounds(
    bounds: Bounds<Pixels>,
    left: f32,
    top: f32,
    width: f32,
    height: f32,
) -> Bounds<Pixels> {
    let bounds_left = f32::from(bounds.left());
    let bounds_top = f32::from(bounds.top());
    let bounds_width = f32::from(bounds.size.width);
    let bounds_height = f32::from(bounds.size.height);
    let rect_left = bounds_left + bounds_width * left;
    let rect_top = bounds_top + bounds_height * top;
    let rect_right = rect_left + bounds_width * width;
    let rect_bottom = rect_top + bounds_height * height;

    snapped_pixel_bounds(px(rect_left), px(rect_top), px(rect_right), px(rect_bottom))
}

fn snapped_pixel_bounds(
    left: Pixels,
    top: Pixels,
    right: Pixels,
    bottom: Pixels,
) -> Bounds<Pixels> {
    let left = f32::from(left).floor();
    let top = f32::from(top).floor();
    let right = f32::from(right).ceil();
    let bottom = f32::from(bottom).ceil();

    Bounds::new(
        point(px(left), px(top)),
        gpui::size(px((right - left).max(0.0)), px((bottom - top).max(0.0))),
    )
}

fn terminal_style_foreground(style: TerminalStyle) -> gpui::Hsla {
    let mut color = gpui::Hsla::from(rgba_for_terminal_color(style.foreground));
    if style.dim {
        color = color.opacity(0.65);
    }
    color
}

fn terminal_text_run(
    len: usize,
    style: TerminalStyle,
    metrics: TerminalMetrics,
    underline_thickness: Pixels,
) -> TextRun {
    let foreground = rgba_for_terminal_color(style.foreground);
    let color = terminal_style_foreground(style);
    TextRun {
        len,
        font: terminal_font_for_style(metrics.font_family, style),
        color,
        background_color: None,
        underline: style.underline.then_some(UnderlineStyle {
            thickness: underline_thickness,
            color: Some(gpui::Hsla::from(foreground)),
            wavy: false,
        }),
        strikethrough: style.strikeout.then_some(StrikethroughStyle {
            thickness: underline_thickness,
            color: Some(gpui::Hsla::from(foreground)),
        }),
    }
}

fn terminal_font_for_style(font_family: &str, style: TerminalStyle) -> gpui::Font {
    let mut font = terminal_font(font_family);
    if style.bold {
        font = font.bold();
    }
    if style.italic {
        font = font.italic();
    }
    font
}

fn terminal_background_rects(
    snapshot: &TerminalSnapshot,
    screen_background: TerminalColor,
) -> Vec<TerminalBackgroundRect> {
    let mut rects: Vec<TerminalBackgroundRect> = Vec::new();
    for (row_index, row) in snapshot.rows.iter().enumerate() {
        let mut row_rects: Vec<TerminalBackgroundRect> = Vec::new();
        for run in &row.cell_runs {
            if run.style.background == screen_background {
                continue;
            }
            if let Some(last) = row_rects.last_mut()
                && last.color == run.style.background
                && last.column + last.width == run.column
            {
                last.width += run.width;
                continue;
            }

            row_rects.push(TerminalBackgroundRect {
                row: row_index,
                rows: 1,
                column: run.column,
                width: run.width,
                color: run.style.background,
            });
        }
        for rect in row_rects {
            push_terminal_background_rect(&mut rects, rect);
        }
    }
    rects
}

fn push_terminal_background_rect(
    rects: &mut Vec<TerminalBackgroundRect>,
    rect: TerminalBackgroundRect,
) {
    if let Some(last) = rects.last_mut()
        && last.color == rect.color
        && last.column == rect.column
        && last.width == rect.width
        && last.row + last.rows == rect.row
    {
        last.rows += rect.rows;
        return;
    }

    rects.push(rect);
}

fn terminal_selection_rects(
    ranges: &[TerminalSelectionRange],
    grid: TerminalGridPixels,
    metrics: TerminalMetrics,
) -> Vec<TerminalPaintCustomGlyphRect> {
    ranges
        .iter()
        .filter(|range| range.width > 0)
        .map(|range| TerminalPaintCustomGlyphRect {
            bounds: terminal_cell_bounds(range.row, range.column, 1, range.width, grid),
            color: metrics.selection_color,
        })
        .collect()
}

fn terminal_grid_position_for_point(
    point: Point<Pixels>,
    bounds: Option<Bounds<Pixels>>,
    metrics: TerminalMetrics,
    window: &mut Window,
    columns: usize,
    rows: usize,
) -> Option<(usize, usize)> {
    if columns == 0 || rows == 0 {
        return None;
    }
    let grid = terminal_grid_pixels(bounds?, metrics, window);
    if grid.cell_width <= Pixels::ZERO || grid.row_height <= Pixels::ZERO {
        return None;
    }

    let relative_x = f32::from(point.x - grid.origin.x);
    let relative_y = f32::from(point.y - grid.origin.y);
    let column = terminal_grid_axis_index(relative_x, f32::from(grid.cell_width), columns);
    let row = terminal_grid_axis_index(relative_y, f32::from(grid.row_height), rows);
    Some((row, column))
}

fn terminal_grid_axis_index(offset: f32, cell_size: f32, limit: usize) -> usize {
    if offset <= 0.0 {
        return 0;
    }
    ((offset / cell_size).floor() as usize).min(limit.saturating_sub(1))
}

fn terminal_mouse_button(button: MouseButton) -> Option<TerminalMouseButton> {
    match button {
        MouseButton::Left => Some(TerminalMouseButton::Left),
        MouseButton::Middle => Some(TerminalMouseButton::Middle),
        MouseButton::Right => Some(TerminalMouseButton::Right),
        MouseButton::Navigate(_) => None,
    }
}

fn terminal_mouse_modifiers(modifiers: gpui::Modifiers) -> TerminalMouseModifiers {
    TerminalMouseModifiers {
        shift: modifiers.shift,
        alt: modifiers.alt,
        control: modifiers.control,
    }
}

fn is_terminal_copy(event: &KeyDownEvent) -> bool {
    event.keystroke.key == "c"
        && event.keystroke.modifiers.control
        && event.keystroke.modifiers.shift
}

fn is_terminal_paste(event: &KeyDownEvent) -> bool {
    event.keystroke.key == "v"
        && event.keystroke.modifiers.control
        && event.keystroke.modifiers.shift
}

fn terminal_key_input(event: &KeyDownEvent) -> TerminalKeyInput {
    TerminalKeyInput {
        key: event.keystroke.key.clone(),
        text: event.keystroke.key_char.clone(),
        control: event.keystroke.modifiers.control,
        alt: event.keystroke.modifiers.alt,
        shift: event.keystroke.modifiers.shift,
        platform: event.keystroke.modifiers.platform,
    }
}

fn resize_terminal_to_bounds(
    session: &Entity<TerminalSession>,
    bounds: Bounds<Pixels>,
    metrics: TerminalMetrics,
    window: &mut Window,
    cx: &mut App,
) {
    let cell_width = terminal_cell_width_pixels(metrics, window);
    let row_height = terminal_row_height_pixels(metrics, window);
    if cell_width <= Pixels::ZERO || row_height <= Pixels::ZERO {
        return;
    }
    let columns = ((bounds.size.width / cell_width).floor() as usize).max(2);
    let rows = ((bounds.size.height / row_height).floor() as usize).max(1);
    let cell_width_px = f32::from(cell_width).round().max(1.0).min(u16::MAX as f32) as u16;
    let cell_height_px = f32::from(row_height).round().max(1.0).min(u16::MAX as f32) as u16;
    session.update(cx, |session, _| {
        session.resize(columns, rows, cell_width_px, cell_height_px)
    });
}

fn rgba_for_terminal_color(color: TerminalColor) -> gpui::Rgba {
    rgb(((color.r as u32) << 16) | ((color.g as u32) << 8) | color.b as u32)
}

fn render_cursor(
    cursor: terminal::TerminalCursor,
    metrics: TerminalMetrics,
    is_focused: bool,
) -> AnyElement {
    canvas(
        move |bounds, window, _cx| {
            if !is_focused || cursor.shape == TerminalCursorShape::Hidden {
                return None;
            }

            let grid = terminal_grid_pixels(bounds, metrics, window);
            Some(terminal_paint_cursor(cursor, grid))
        },
        move |_bounds, paint_cursor, window, _cx| {
            let Some(paint_cursor) = paint_cursor else {
                return;
            };

            let color =
                gpui::Hsla::from(rgba_for_terminal_color(metrics.cursor_color)).opacity(0.85);
            match paint_cursor.shape {
                TerminalCursorShape::Block => {
                    window.paint_quad(fill(paint_cursor.bounds, color));
                }
                TerminalCursorShape::HollowBlock => {
                    window.paint_quad(outline(
                        paint_cursor.bounds,
                        color,
                        gpui::BorderStyle::default(),
                    ));
                }
                TerminalCursorShape::Underline | TerminalCursorShape::Beam => {
                    window.paint_quad(fill(paint_cursor.bounds, color));
                }
                TerminalCursorShape::Hidden => {}
            }
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .bottom_0()
    .left_0()
    .into_any_element()
}

fn terminal_paint_cursor(
    cursor: terminal::TerminalCursor,
    grid: TerminalGridPixels,
) -> TerminalPaintCursor {
    let cell_bounds = terminal_cell_bounds(cursor.row, cursor.column, 1, 1, grid);
    let bounds = match cursor.shape {
        TerminalCursorShape::Underline => Bounds::new(
            point(
                cell_bounds.left(),
                cell_bounds.bottom() - grid.cursor_thickness,
            ),
            gpui::size(cell_bounds.size.width, grid.cursor_thickness),
        ),
        TerminalCursorShape::Beam => Bounds::new(
            cell_bounds.origin,
            gpui::size(grid.cursor_thickness, cell_bounds.size.height),
        ),
        TerminalCursorShape::Block
        | TerminalCursorShape::HollowBlock
        | TerminalCursorShape::Hidden => cell_bounds,
    };

    TerminalPaintCursor {
        bounds,
        shape: cursor.shape,
    }
}

fn terminal_cell_bounds(
    row: usize,
    column: usize,
    rows: usize,
    columns: usize,
    grid: TerminalGridPixels,
) -> Bounds<Pixels> {
    let left = f32::from(grid.origin.x + grid.cell_width * column).floor();
    let top = f32::from(grid.origin.y + grid.row_height * row).floor();
    let right = f32::from(grid.origin.x + grid.cell_width * (column + columns)).ceil();
    let bottom = f32::from(grid.origin.y + grid.row_height * (row + rows)).ceil();

    Bounds::new(
        point(px(left), px(top)),
        gpui::size(px((right - left).max(0.0)), px((bottom - top).max(0.0))),
    )
}

fn render_bottom_bar<T: 'static>(
    session: &Entity<TerminalSession>,
    snapshot: &TerminalSnapshot,
    cx: &mut Context<T>,
) -> AnyElement {
    let theme = *cx.theme();
    let key = session.read(cx).key();
    let id_prefix = format!("terminal-{}-{}", key.workspace_id, key.tab_id);
    let zoom_out = session.clone();
    let zoom_in = session.clone();
    let reset_zoom = session.clone();
    let reload = session.clone();

    div()
        .h(rems(BOTTOM_BAR_HEIGHT_REM))
        .flex_none()
        .w_full()
        .flex()
        .items_center()
        .justify_between()
        .px_2()
        .bg(theme.bg_surface)
        .border_t_1()
        .border_color(theme.border_subtle)
        .text_xs()
        .text_color(theme.text_muted)
        .child(render_status(snapshot))
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(compact_text_button(
                    format!("{id_prefix}-zoom-out"),
                    "-",
                    move |_, _, cx| {
                        zoom_out.update(cx, |session, cx| session.zoom_out(cx));
                    },
                ))
                .child(
                    Button::new(SharedString::from(format!("{id_prefix}-zoom-reset")))
                        .ghost()
                        .tab_stop(false)
                        .min_w(rems(2.5))
                        .h(rems(BOTTOM_BAR_BUTTON_SIZE_REM))
                        .label(format!("{}", snapshot.zoom_percent))
                        .on_click(move |_, _, cx| {
                            reset_zoom.update(cx, |session, cx| session.reset_zoom(cx));
                        }),
                )
                .child(compact_text_button(
                    format!("{id_prefix}-zoom-in"),
                    "+",
                    move |_, _, cx| {
                        zoom_in.update(cx, |session, cx| session.zoom_in(cx));
                    },
                ))
                .child(render_separator())
                .child(render_shell_picker(
                    key,
                    &snapshot.selected_shell_label,
                    &snapshot.shells,
                    session,
                ))
                .child(render_separator())
                .child(icon_button(
                    format!("{id_prefix}-reload"),
                    IconName::Refresh,
                    move |_, _, cx| {
                        reload.update(cx, |session, cx| session.reload(cx));
                    },
                )),
        )
        .into_any_element()
}

fn render_status(snapshot: &TerminalSnapshot) -> AnyElement {
    let text = match &snapshot.status {
        TerminalStatus::Running => snapshot.title.as_deref().unwrap_or("Terminal").to_string(),
        TerminalStatus::Restarting => "Restarting".to_string(),
        TerminalStatus::Failed(err) => err.clone(),
        TerminalStatus::Exited => "Exited".to_string(),
    };
    div()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(text)
        .into_any_element()
}

fn render_shell_picker(
    key: TerminalKey,
    selected_label: &str,
    shells: &[ShellProfile],
    session: &Entity<TerminalSession>,
) -> AnyElement {
    let selected_label = selected_label.to_string();
    let options = shells
        .iter()
        .map(|shell| (shell.id.clone(), shell.label.clone()))
        .collect::<Vec<_>>();
    let session = session.clone();

    Button::new(SharedString::from(format!(
        "terminal-shell-picker-{}-{}",
        key.workspace_id, key.tab_id
    )))
    .ghost()
    .tab_stop(false)
    .w(rems(SHELL_PICKER_WIDTH_REM))
    .h(rems(BOTTOM_BAR_BUTTON_SIZE_REM))
    .label(selected_label.clone())
    .dropdown_caret(true)
    .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, window, _| {
        let menu_width = rems(SHELL_PICKER_WIDTH_REM).to_pixels(window.rem_size());
        options
            .iter()
            .fold(menu.min_w(menu_width), |menu, (shell_id, label)| {
                let shell_id = shell_id.clone();
                let session = session.clone();
                menu.item(
                    PopupMenuItem::new(label.clone())
                        .checked(label == &selected_label)
                        .on_click(move |_, _, cx| {
                            session.update(cx, |session, cx| session.select_shell(&shell_id, cx));
                        }),
                )
            })
    })
    .into_any_element()
}

fn compact_text_button(
    id: impl Into<SharedString>,
    label: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id.into())
        .ghost()
        .tab_stop(false)
        .size(rems(BOTTOM_BAR_BUTTON_SIZE_REM))
        .label(label)
        .on_click(on_click)
}

fn icon_button(
    id: impl Into<SharedString>,
    icon_name: IconName,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    Button::new(id.into())
        .ghost()
        .tab_stop(false)
        .size(rems(BOTTOM_BAR_BUTTON_SIZE_REM))
        .child(component_icon(icon_name).small())
        .on_click(on_click)
}

fn component_icon(icon: IconName) -> ComponentIcon {
    ComponentIcon::empty().path(icon.path())
}

fn render_separator() -> AnyElement {
    Separator::vertical()
        .h(rems(1.0))
        .color(gpui::Hsla::from(rgb(0x363636)))
        .into_any_element()
}

struct TerminalInputElement {
    session: Entity<TerminalSession>,
}

impl IntoElement for TerminalInputElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalInputElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.session.read(cx).focus_handle();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.session.clone()),
            cx,
        );
    }
}
