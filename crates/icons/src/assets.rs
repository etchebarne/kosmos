use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

use crate::IconName;

const KOSMOS_ICON_SVG: &[u8] =
    include_bytes!("../../../assets/brand/kosmos-icon.svg");
const KOSMOS_TEXT_SVG: &[u8] =
    include_bytes!("../../../assets/brand/kosmos-text.svg");
const KOSMOS_LOGO_SVG: &[u8] =
    include_bytes!("../../../assets/brand/kosmos-logo.svg");

const LANG_ASSETS: &[(&str, &[u8])] = &[
    ("astro.svg", include_bytes!("../../../assets/langs/astro.svg")),
    ("astro_light.svg", include_bytes!("../../../assets/langs/astro_light.svg")),
    ("bash.svg", include_bytes!("../../../assets/langs/bash.svg")),
    ("bash_light.svg", include_bytes!("../../../assets/langs/bash_light.svg")),
    ("bun.svg", include_bytes!("../../../assets/langs/bun.svg")),
    ("c.svg", include_bytes!("../../../assets/langs/c.svg")),
    ("cpp.svg", include_bytes!("../../../assets/langs/cpp.svg")),
    ("csharp.svg", include_bytes!("../../../assets/langs/csharp.svg")),
    ("css.svg", include_bytes!("../../../assets/langs/css.svg")),
    ("dart.svg", include_bytes!("../../../assets/langs/dart.svg")),
    ("docker.svg", include_bytes!("../../../assets/langs/docker.svg")),
    ("dotenv.svg", include_bytes!("../../../assets/langs/dotenv.svg")),
    ("git.svg", include_bytes!("../../../assets/langs/git.svg")),
    ("go.svg", include_bytes!("../../../assets/langs/go.svg")),
    ("go_light.svg", include_bytes!("../../../assets/langs/go_light.svg")),
    ("graphql.svg", include_bytes!("../../../assets/langs/graphql.svg")),
    ("haskell.svg", include_bytes!("../../../assets/langs/haskell.svg")),
    ("html.svg", include_bytes!("../../../assets/langs/html.svg")),
    ("java.svg", include_bytes!("../../../assets/langs/java.svg")),
    ("javascript.svg", include_bytes!("../../../assets/langs/javascript.svg")),
    ("json.svg", include_bytes!("../../../assets/langs/json.svg")),
    ("julia.svg", include_bytes!("../../../assets/langs/julia.svg")),
    ("kotlin.svg", include_bytes!("../../../assets/langs/kotlin.svg")),
    ("lua.svg", include_bytes!("../../../assets/langs/lua.svg")),
    ("markdown.svg", include_bytes!("../../../assets/langs/markdown.svg")),
    ("markdown_light.svg", include_bytes!("../../../assets/langs/markdown_light.svg")),
    ("php.svg", include_bytes!("../../../assets/langs/php.svg")),
    ("php_light.svg", include_bytes!("../../../assets/langs/php_light.svg")),
    ("powershell.svg", include_bytes!("../../../assets/langs/powershell.svg")),
    ("python.svg", include_bytes!("../../../assets/langs/python.svg")),
    ("r.svg", include_bytes!("../../../assets/langs/r.svg")),
    ("r_light.svg", include_bytes!("../../../assets/langs/r_light.svg")),
    ("react.svg", include_bytes!("../../../assets/langs/react.svg")),
    ("react_light.svg", include_bytes!("../../../assets/langs/react_light.svg")),
    ("ruby.svg", include_bytes!("../../../assets/langs/ruby.svg")),
    ("rust.svg", include_bytes!("../../../assets/langs/rust.svg")),
    ("rust_light.svg", include_bytes!("../../../assets/langs/rust_light.svg")),
    ("sass.svg", include_bytes!("../../../assets/langs/sass.svg")),
    ("scala.svg", include_bytes!("../../../assets/langs/scala.svg")),
    ("solidity.svg", include_bytes!("../../../assets/langs/solidity.svg")),
    ("sql.svg", include_bytes!("../../../assets/langs/sql.svg")),
    ("sql_light.svg", include_bytes!("../../../assets/langs/sql_light.svg")),
    ("svelte.svg", include_bytes!("../../../assets/langs/svelte.svg")),
    ("swift.svg", include_bytes!("../../../assets/langs/swift.svg")),
    ("terraform.svg", include_bytes!("../../../assets/langs/terraform.svg")),
    ("typescript.svg", include_bytes!("../../../assets/langs/typescript.svg")),
    ("vue.svg", include_bytes!("../../../assets/langs/vue.svg")),
    ("zig.svg", include_bytes!("../../../assets/langs/zig.svg")),
];

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = brand_asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }

        if let Some(bytes) = lang_asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }

        let Some(icon) = IconName::from_path(path) else {
            return Ok(None);
        };

        let Some(svg) = icon.to_svg() else {
            return Ok(None);
        };

        Ok(Some(Cow::Owned(svg.into_bytes())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let prefix = match path {
            "icons" => "icons/",
            "langs" => "langs/",
            _ => return Ok(Vec::new()),
        };
        Ok(IconName::ALL
            .iter()
            .filter_map(|icon| icon.path().strip_prefix(prefix))
            .map(SharedString::from)
            .collect())
    }
}

fn brand_asset(path: &str) -> Option<&'static [u8]> {
    match path {
        "brand/kosmos-icon.svg" => Some(KOSMOS_ICON_SVG),
        "brand/kosmos-text.svg" => Some(KOSMOS_TEXT_SVG),
        "brand/kosmos-logo.svg" => Some(KOSMOS_LOGO_SVG),
        _ => None,
    }
}

fn lang_asset(path: &str) -> Option<&'static [u8]> {
    let name = path.strip_prefix("langs/")?;
    LANG_ASSETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, bytes)| *bytes)
}

/// True if a `<stem>_light.svg` variant is bundled — designed for use on light
/// backgrounds (the default lang SVG is the dark-bg, light-content version).
pub(crate) fn has_light_variant(path: &str) -> bool {
    let Some(name) = path.strip_prefix("langs/") else {
        return false;
    };
    let Some(stem) = name.strip_suffix(".svg") else {
        return false;
    };
    let light = format!("{stem}_light.svg");
    LANG_ASSETS.iter().any(|(n, _)| *n == light)
}
