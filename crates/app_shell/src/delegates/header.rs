use gpui::Context;
use ui::delegate::{HeaderDelegate, HeaderMenu};

use crate::app::KosmosApp;

impl HeaderDelegate for KosmosApp {
    fn toggle_header_menu(&mut self, menu: HeaderMenu, cx: &mut Context<Self>) {
        self.active_menu = if self.active_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
        cx.notify();
    }
}
