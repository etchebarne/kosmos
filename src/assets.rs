use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

use crate::icon::IconName;

pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
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
