// application.rs

use relm4::{gtk, RelmApp};
use crate::window::MainWindow;

pub fn run() {
    if let Ok(res) = gtk::gio::Resource::load(crate::config::RESOURCES_FILE) {
        gtk::gio::resources_register(&res);

        if let Some(display) = gtk::gdk::Display::default() {
            let provider = gtk::CssProvider::new();
            provider.load_from_resource("/org/archerprojects/mediaSnare/style.css");
            gtk::style_context_add_provider_for_display(
                &display,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );

            // Bundled icons (e.g. preferences-system-symbolic) — resolve from
            // the GResource regardless of what the system icon theme ships.
            gtk::IconTheme::for_display(&display)
                .add_resource_path("/org/archerprojects/mediaSnare/icons");
        } else {
            tracing::warn!("No default display — CSS and bundled icons not registered");
        }
    } else {
        tracing::warn!("GResource bundle not found — running without theme CSS");
    }

    RelmApp::new(crate::config::APP_ID).run::<MainWindow>(());
}
