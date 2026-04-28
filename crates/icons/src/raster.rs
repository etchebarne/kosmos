//! Per-paint rasterization for multi-color SVGs.
//!
//! gpui's `img()` rasterizes an SVG once and caches a single bitmap, which the
//! GPU then stretches at any display size. For small file-tree icons that means
//! either heavy downsampling artifacts or blurry upscaling on zoom.
//!
//! We rasterize each lang SVG at the element's actual device-pixel bounds during
//! paint (matching what a browser does), keyed by `(path, w, h)`. We use resvg
//! / usvg directly because gpui's own `SvgRenderer::render_pixmap` takes an
//! `SvgSize` enum that isn't publicly re-exported.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use gpui::{App, Bounds, Pixels, RenderImage, SharedString, Window};
use image::{Frame, ImageBuffer};
use resvg::{tiny_skia, usvg};
use smallvec::SmallVec;
use theme::ActiveTheme;

use crate::assets::has_light_variant;

/// Soft cap. Each entry is roughly w*h*4 bytes plus overhead; with ~60 icons ×
/// a handful of zoom levels we should stay well under this.
const CACHE_LIMIT: usize = 512;

type CacheKey = (SharedString, u32, u32);

static CACHE: LazyLock<Mutex<HashMap<CacheKey, Arc<RenderImage>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn paint(
    path: SharedString,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let scale = window.scale_factor();
    let device = bounds.size.scale(scale);
    let w = u32::from(device.width).max(1);
    let h = u32::from(device.height).max(1);

    let resolved = resolve_path_for_theme(path, cx);
    let Some(image) = render_or_cache(&resolved, w, h, cx) else {
        return;
    };
    let _ = window.paint_image(bounds, Default::default(), image, 0, false);
}

/// On light themes, swap to the `<stem>_light.svg` variant when one exists. The
/// default lang SVGs are designed for dark backgrounds (white-on-transparent),
/// which would be invisible on a light theme.
fn resolve_path_for_theme(path: SharedString, cx: &App) -> SharedString {
    if cx.theme().is_dark {
        return path;
    }
    if !has_light_variant(&path) {
        return path;
    }
    let Some(stem) = path.strip_suffix(".svg") else {
        return path;
    };
    SharedString::from(format!("{stem}_light.svg"))
}

fn render_or_cache(path: &SharedString, w: u32, h: u32, cx: &mut App) -> Option<Arc<RenderImage>> {
    let key = (path.clone(), w, h);
    if let Some(image) = CACHE.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Some(image);
    }

    let bytes = cx.asset_source().load(path).ok().flatten()?;
    let tree = usvg::Tree::from_data(&bytes, &usvg::Options::default()).ok()?;
    let svg_size = tree.size();
    let scale_x = w as f32 / svg_size.width();
    let scale_y = h as f32 / svg_size.height();

    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // resvg returns RGBA with premultiplied alpha; gpui's paint_image expects
    // straight BGRA. Convert in place.
    let mut data = pixmap.take();
    for px in data.chunks_exact_mut(4) {
        px.swap(0, 2);
        if px[3] > 0 {
            let a = px[3] as f32 / 255.0;
            px[0] = ((px[0] as f32) / a).min(255.0) as u8;
            px[1] = ((px[1] as f32) / a).min(255.0) as u8;
            px[2] = ((px[2] as f32) / a).min(255.0) as u8;
        }
    }

    let buffer = ImageBuffer::from_raw(w, h, data)?;
    let image = Arc::new(RenderImage::new(SmallVec::from_elem(Frame::new(buffer), 1)));

    if let Ok(mut cache) = CACHE.lock() {
        if cache.len() > CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, image.clone());
    }

    Some(image)
}
