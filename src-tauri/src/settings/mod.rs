mod ai;
mod theme;
pub mod types;

use types::SettingsSchema;

fn build_schema() -> SettingsSchema {
    SettingsSchema {
        sections: vec![theme::section(), ai::section()],
    }
}

#[tauri::command]
pub fn get_settings_schema() -> Result<SettingsSchema, String> {
    Ok(build_schema())
}
