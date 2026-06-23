// settings.rs — GSettings wrapper for mediaSnare preferences

use relm4::gtk::gio;
use relm4::gtk::prelude::*;

use crate::config;

pub struct Settings(gio::Settings);

impl Settings {
    pub fn get() -> Self {
        Self(gio::Settings::new(config::APP_ID))
    }

    // ── Read ──────────────────────────────────────────────────────────────────

    pub fn image_directory(&self) -> Option<std::path::PathBuf> {
        let s = self.0.string("image-directory");
        if s.is_empty() { None } else { Some(s.into()) }
    }

    pub fn video_directory(&self) -> Option<std::path::PathBuf> {
        let s = self.0.string("video-directory");
        if s.is_empty() { None } else { Some(s.into()) }
    }

    pub fn audio_directory(&self) -> Option<std::path::PathBuf> {
        let s = self.0.string("audio-directory");
        if s.is_empty() { None } else { Some(s.into()) }
    }

    pub fn image_format(&self) -> String {
        self.0.string("image-format").into()
    }

    pub fn video_profile(&self) -> String {
        self.0.string("video-profile").into()
    }

    pub fn audio_profile(&self) -> String {
        self.0.string("audio-profile").into()
    }

    pub fn framerate(&self) -> u32 {
        self.0.uint("framerate")
    }

    pub fn capture_cursor(&self) -> bool {
        self.0.boolean("capture-cursor")
    }

    pub fn notify_on_complete(&self) -> bool {
        self.0.boolean("notify-on-complete")
    }

    pub fn ask_save_name(&self) -> bool {
        self.0.boolean("ask-save-name")
    }

    pub fn copy_to_clipboard(&self) -> bool {
        self.0.boolean("copy-to-clipboard")
    }

    pub fn capture_mode(&self) -> String {
        self.0.string("capture-mode").into()
    }

    pub fn capture_scope(&self) -> String {
        self.0.string("capture-scope").into()
    }

    pub fn audio_source(&self) -> String {
        self.0.string("audio-source").into()
    }

    pub fn bar_x(&self) -> i32 {
        self.0.int("bar-x")
    }

    pub fn bar_y(&self) -> i32 {
        self.0.int("bar-y")
    }

    pub fn portal_token(&self) -> Option<String> {
        let s = self.0.string("portal-token");
        if s.is_empty() { None } else { Some(s.into()) }
    }

    // ── Write ─────────────────────────────────────────────────────────────────

    pub fn set_image_directory(&self, path: &str) {
        let _ = self.0.set_string("image-directory", path);
    }

    pub fn set_video_directory(&self, path: &str) {
        let _ = self.0.set_string("video-directory", path);
    }

    pub fn set_audio_directory(&self, path: &str) {
        let _ = self.0.set_string("audio-directory", path);
    }

    pub fn set_image_format(&self, format: &str) {
        let _ = self.0.set_string("image-format", format);
    }

    pub fn set_framerate(&self, fps: u32) {
        let _ = self.0.set_uint("framerate", fps);
    }

    pub fn set_capture_cursor(&self, enabled: bool) {
        let _ = self.0.set_boolean("capture-cursor", enabled);
    }

    pub fn set_video_profile(&self, id: &str) {
        let _ = self.0.set_string("video-profile", id);
    }

    pub fn set_audio_profile(&self, id: &str) {
        let _ = self.0.set_string("audio-profile", id);
    }

    pub fn set_ask_save_name(&self, enabled: bool) {
        let _ = self.0.set_boolean("ask-save-name", enabled);
    }

    pub fn set_copy_to_clipboard(&self, enabled: bool) {
        let _ = self.0.set_boolean("copy-to-clipboard", enabled);
    }

    pub fn set_capture_mode(&self, mode: &str) {
        let _ = self.0.set_string("capture-mode", mode);
    }

    pub fn set_capture_scope(&self, scope: &str) {
        let _ = self.0.set_string("capture-scope", scope);
    }

    pub fn set_audio_source(&self, source: &str) {
        let _ = self.0.set_string("audio-source", source);
    }

    pub fn set_bar_x(&self, x: i32) {
        let _ = self.0.set_int("bar-x", x);
    }

    pub fn set_bar_y(&self, y: i32) {
        let _ = self.0.set_int("bar-y", y);
    }

    pub fn set_portal_token(&self, token: &str) {
        let _ = self.0.set_string("portal-token", token);
    }

    pub fn clear_portal_token(&self) {
        let _ = self.0.set_string("portal-token", "");
    }
}
