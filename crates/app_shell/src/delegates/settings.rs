use gpui::{BorrowAppContext, Context};
use settings::{SettingValue, Settings};
use ui::delegate::{SettingsDelegate, SettingsUiState};

use crate::app::KosmosApp;

impl SettingsDelegate for KosmosApp {
    fn select_settings_category(&mut self, category_id: &'static str, cx: &mut Context<Self>) {
        let mut changed = false;
        cx.update_global::<SettingsUiState, _>(|state, _| {
            if state.active_category != category_id {
                state.active_category = category_id;
                state.open_dropdown = None;
                changed = true;
            }
        });
        if changed {
            cx.notify();
        }
    }

    fn toggle_settings_dropdown(&mut self, setting_id: &'static str, cx: &mut Context<Self>) {
        cx.update_global::<SettingsUiState, _>(|state, _| {
            state.open_dropdown = if state.open_dropdown == Some(setting_id) {
                None
            } else {
                Some(setting_id)
            };
        });
        cx.notify();
    }

    fn set_setting_value(
        &mut self,
        key: &'static str,
        value: SettingValue,
        cx: &mut Context<Self>,
    ) {
        cx.update_global::<Settings, _>(|settings, _| {
            settings.set(key, value);
        });
        cx.notify();
    }

    fn install_tool(&mut self, entry: &'static registry::RegistryEntry, cx: &mut Context<Self>) {
        let tool_id = entry.id;
        let already = cx.global::<SettingsUiState>().installing.contains(tool_id);
        if already {
            return;
        }
        cx.update_global::<SettingsUiState, _>(|state, _| {
            state.installing.insert(tool_id);
            state.install_errors.remove(tool_id);
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { installer::ensure(entry) })
                .await;
            let _ = this.update(cx, |_, cx| {
                cx.update_global::<SettingsUiState, _>(|state, _| {
                    state.installing.remove(tool_id);
                    if let Err(err) = &result {
                        state
                            .install_errors
                            .insert(tool_id, format!("{err}").into());
                    }
                });
                cx.notify();
            });
        })
        .detach();
    }

    fn uninstall_tool(&mut self, entry: &'static registry::RegistryEntry, cx: &mut Context<Self>) {
        let dir = installer::tool_dir(entry);
        let tool_id = entry.id;
        let result = std::fs::remove_dir_all(&dir);
        cx.update_global::<SettingsUiState, _>(|state, _| {
            state.install_errors.remove(tool_id);
            if let Err(err) = result
                && err.kind() != std::io::ErrorKind::NotFound
            {
                state
                    .install_errors
                    .insert(tool_id, format!("uninstall failed: {err}").into());
            }
        });
        cx.notify();
    }
}
