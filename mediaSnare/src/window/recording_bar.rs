// window/recording_bar.rs — floating recording controls (camcorder workflow)
//
// Buttons: App icon (minimize), Record (red dot), Pause (two bars), Stop (square).
// No tooltips. State driven externally via set_state().

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::glib;
use std::process::Command;
use std::time::Duration;

use crate::settings::Settings;
use crate::window::MainWindowMsg;

const BAR_TITLE: &str = "mediaSnare Controls";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarState {
    Ready,
    Recording,
    Paused,
}

pub struct RecordingBar {
    window: gtk::Window,
    record_btn: gtk::Button,
    pause_btn: gtk::Button,
    stop_btn: gtk::Button,
}

impl RecordingBar {
    pub fn new(sender: relm4::Sender<MainWindowMsg>) -> Self {
        let window = gtk::Window::new();
        window.set_title(Some(BAR_TITLE));
        window.set_decorated(false);
        window.set_resizable(false);
        window.set_deletable(false);

        let css = gtk::CssProvider::new();
        css.load_from_data(
            "window.recording-bar {
                background-color: rgba(46, 46, 46, 0.92);
                border-radius: 8px;
                padding: 2px;
            }
            window.recording-bar button {
                min-width: 32px;
                min-height: 32px;
                padding: 4px;
                margin: 2px;
                border-radius: 6px;
                background: transparent;
                border: none;
                box-shadow: none;
                outline: none;
            }
            window.recording-bar button image {
                color: rgba(220, 220, 220, 0.9);
            }
            window.recording-bar button:hover {
                background-color: rgba(255, 255, 255, 0.12);
            }
            window.recording-bar button.bar-record image {
                color: #e35d4f;
            }
            window.recording-bar button.bar-record:hover {
                background-color: rgba(227, 93, 79, 0.2);
            }
            window.recording-bar button.bar-stop image {
                color: #4b8bd4;
            }
            window.recording-bar button:disabled {
                opacity: 0.3;
            }",
        );
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display, &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }

        window.add_css_class("recording-bar");

        let app_btn = gtk::Button::new();
        app_btn.set_icon_name("mediasnare");

        let record_btn = gtk::Button::new();
        record_btn.set_icon_name("media-record-symbolic");
        record_btn.add_css_class("bar-record");

        let pause_btn = gtk::Button::new();
        pause_btn.set_icon_name("media-playback-pause-symbolic");

        let stop_btn = gtk::Button::new();
        stop_btn.set_icon_name("media-playback-stop-symbolic");
        stop_btn.add_css_class("bar-stop");

        let button_box = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        button_box.append(&app_btn);
        button_box.append(&record_btn);
        button_box.append(&pause_btn);
        button_box.append(&stop_btn);

        let handle = gtk::WindowHandle::new();
        handle.set_child(Some(&button_box));
        window.set_child(Some(&handle));

        let w = window.clone();
        app_btn.connect_clicked(move |_| { w.minimize(); });

        let s = sender.clone();
        record_btn.connect_clicked(move |_| { let _ = s.send(MainWindowMsg::Record); });

        let s = sender.clone();
        pause_btn.connect_clicked(move |_| { let _ = s.send(MainWindowMsg::Pause); });

        let s = sender.clone();
        stop_btn.connect_clicked(move |_| { let _ = s.send(MainWindowMsg::Stop); });

        RecordingBar { window, record_btn, pause_btn, stop_btn }
    }

    pub fn set_state(&self, state: BarState) {
        match state {
            BarState::Ready => {
                self.record_btn.set_sensitive(true);
                self.pause_btn.set_sensitive(false);
                self.stop_btn.set_sensitive(true);
            }
            BarState::Recording => {
                self.record_btn.set_sensitive(false);
                self.pause_btn.set_sensitive(true);
                self.stop_btn.set_sensitive(true);
            }
            BarState::Paused => {
                self.record_btn.set_sensitive(true);
                self.pause_btn.set_sensitive(false);
                self.stop_btn.set_sensitive(true);
            }
        }
    }

    pub fn show(&self, region: Option<(i32, i32, i32, i32)>) {
        self.set_state(BarState::Ready);
        self.window.present();

        let title = BAR_TITLE.to_owned();
        glib::timeout_add_local_once(Duration::from_millis(350), move || {
            let screen = screen_size();
            let (x, y) = if let Some(r) = region {
                position_outside_region(r, screen)
            } else {
                let settings = Settings::get();
                let (sx, sy) = (settings.bar_x(), settings.bar_y());
                if sx >= 0 && sy >= 0 { (sx, sy) }
                else { (20, screen.1 - 60) }
            };
            xdo_move(&title, x, y);
            xdo_above(&title);
            // Retry above after a short delay — window manager may not
            // have processed the first request if the window was still
            // mapping when wmctrl ran.
            let title2 = title.clone();
            glib::timeout_add_local_once(Duration::from_millis(300), move || {
                xdo_above(&title2);
            });
        });
    }

    pub fn save_and_close(&self) {
        if let Some((x, y)) = xdo_get_position(BAR_TITLE) {
            let settings = Settings::get();
            settings.set_bar_x(x);
            settings.set_bar_y(y);
        }
        self.window.close();
    }
}

fn screen_size() -> (i32, i32) {
    if let Some(display) = gtk::gdk::Display::default() {
        let monitors = display.monitors();
        for i in 0..monitors.n_items() {
            if let Some(obj) = monitors.item(i) {
                if let Ok(monitor) = obj.downcast::<gtk::gdk::Monitor>() {
                    let geom = monitor.geometry();
                    return (geom.width(), geom.height());
                }
            }
        }
    }
    (1920, 1080)
}

fn position_outside_region(region: (i32, i32, i32, i32), screen: (i32, i32)) -> (i32, i32) {
    let (rx, ry, rw, rh) = region;
    let (sw, sh) = screen;
    let bar_w = 180;
    let bar_h = 44;
    let gap = 10;
    // Align left edge of bar with left edge of region (bottom-left bias)
    let align_x = rx.clamp(0, sw - bar_w);

    // Try below the region first (close to video player controls)
    if ry + rh + bar_h + gap <= sh { return (align_x, ry + rh + gap); }
    // Try above
    if ry >= bar_h + gap { return (align_x, ry - bar_h - gap); }
    // Try left of the region
    if rx >= bar_w + gap { return (rx - bar_w - gap, (ry + rh - bar_h).clamp(0, sh - bar_h)); }
    // Try right
    if rx + rw + bar_w + gap <= sw { return (rx + rw + gap, (ry + rh - bar_h).clamp(0, sh - bar_h)); }
    // Fallback: bottom-left corner
    (20, sh - bar_h - 20)
}

fn xdo_move(title: &str, x: i32, y: i32) {
    let _ = Command::new("xdotool")
        .args(["search", "--name", title, "windowmove", "--", &x.to_string(), &y.to_string()])
        .output();
}

fn xdo_above(title: &str) {
    let _ = Command::new("wmctrl").args(["-r", title, "-b", "add,above"]).output();
}

fn xdo_get_position(title: &str) -> Option<(i32, i32)> {
    let output = Command::new("xdotool")
        .args(["search", "--name", title, "getwindowgeometry", "--shell"])
        .output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let mut x = None;
    let mut y = None;
    for line in text.lines() {
        if let Some(val) = line.strip_prefix("X=") { x = val.parse().ok(); }
        if let Some(val) = line.strip_prefix("Y=") { y = val.parse().ok(); }
    }
    Some((x?, y?))
}
