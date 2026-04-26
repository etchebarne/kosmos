use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

use crate::IconName;

const KOSMOS_ICON_SVG: &[u8] =
    include_bytes!("../../../assets/brand/kosmos-icon.svg");
const KOSMOS_TEXT_SVG: &[u8] =
    include_bytes!("../../../assets/brand/kosmos-text.svg");
const KOSMOS_LOGO_SVG: &[u8] =
    include_bytes!("../../../assets/brand/kosmos-logo.svg");

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(bytes) = brand_asset(path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }

        let Some(icon) = IconName::from_path(path) else {
            return Ok(None);
        };

        Ok(Some(Cow::Owned(icon.to_svg().into_bytes())))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        if path == "icons" {
            return Ok(IconName::ALL
                .iter()
                .map(|icon| icon.file_name().into())
                .collect());
        }

        Ok(Vec::new())
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
