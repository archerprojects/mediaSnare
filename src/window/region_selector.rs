// window/region_selector.rs — Flameshot-style region selector overlay
//
// A fullscreen, semi-transparent, always-on-top window that mediaSnare owns
// directly (replacing Cinnamon's one-shot SelectArea). The user drags out a
// rectangle, then grabs corners/edges to resize or the body to move. The
// selection coordinates are screen coordinates: on a single monitor the
// widget origin is the screen origin, so widget pixels == screen pixels.
//
// Two kinds:
//   Image — a small action bar (camera / X) tracks the bottom of the box.
//           Enter or the camera button confirms; Esc or X cancels. On confirm
//           the final rectangle is sent as RegionConfirmed and the overlay
//           closes.
//   Video — no action bar. As soon as the first valid box is drawn, RegionReady
//           is sent once so the main window raises its Ready recording bar. The
//           box stays live and adjustable; the main window reads the final
//           rectangle via current_rect() when Record is pressed. Esc cancels.
//
// Both kinds send CaptureCancelled (the existing quiet-cancel marker path) on
// Esc/X, so the dispatch layer re-arms without an error dialog.

use relm4::gtk;
use relm4::gtk::prelude::*;
use relm4::gtk::{cairo, gdk, glib};
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use crate::window::MainWindowMsg;

const TITLE:  &str = "mediaSnare Region";
const ACCENT: (f64, f64, f64) = (0.294, 0.545, 0.831); // #4b8bd4
const DIM_ALPHA: f64 = 0.45;
const HANDLE: f64 = 8.0;  // drawn handle square (px)
const HIT:    f64 = 14.0; // grab tolerance around a handle (px)
const MIN:    f64 = 10.0; // minimum confirmable selection size (px)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorKind {
    Image,
    Video,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Handle { Nw, N, Ne, E, Se, S, Sw, W }

#[derive(Clone, Copy)]
enum DragMode { New, Move, Resize(Handle) }

#[derive(Clone, Copy)]
struct DragState {
    mode:  DragMode,
    orig:  (f64, f64, f64, f64), // rectangle at drag-begin
    start: (f64, f64),           // drag-begin point
}

type RectCell = Rc<Cell<Option<(f64, f64, f64, f64)>>>;

pub struct RegionSelector {
    window: gtk::Window,
    rect:   RectCell,
}

impl RegionSelector {
    pub fn new(sender: relm4::Sender<MainWindowMsg>, kind: SelectorKind) -> Self {
        install_css();

        let window = gtk::Window::new();
        window.set_title(Some(TITLE));
        window.set_decorated(false);
        window.add_css_class("region-overlay");

        let area = gtk::DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_cursor(gdk::Cursor::from_name("crosshair", None).as_ref());

        let rect: RectCell = Rc::new(Cell::new(None));
        let drag_state: Rc<RefCell<Option<DragState>>> = Rc::new(RefCell::new(None));
        let bar_shown = Rc::new(Cell::new(false));

        // ── Draw: dim everything, punch a clear hole at the selection, then
        //    stroke the accent border and fill the eight handles. ───────────
        {
            let rect = rect.clone();
            area.set_draw_func(move |_area, cr, _w, _h| {
                cr.set_operator(cairo::Operator::Over);
                cr.set_source_rgba(0.0, 0.0, 0.0, DIM_ALPHA);
                let _ = cr.paint();

                if let Some((rx, ry, rw, rh)) = rect.get() {
                    if rw > 0.0 && rh > 0.0 {
                        cr.set_operator(cairo::Operator::Clear);
                        cr.rectangle(rx, ry, rw, rh);
                        let _ = cr.fill();

                        cr.set_operator(cairo::Operator::Over);
                        cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 1.0);
                        cr.set_line_width(2.0);
                        cr.rectangle(rx + 1.0, ry + 1.0, (rw - 2.0).max(0.0), (rh - 2.0).max(0.0));
                        let _ = cr.stroke();

                        for (hx, hy) in handle_points((rx, ry, rw, rh)) {
                            cr.rectangle(hx - HANDLE / 2.0, hy - HANDLE / 2.0, HANDLE, HANDLE);
                        }
                        let _ = cr.fill();
                    }
                }
            });
        }

        // Confirm / cancel actions shared by keyboard and the action buttons.
        let do_confirm: Rc<dyn Fn()> = {
            let window = window.clone();
            let rect = rect.clone();
            let sender = sender.clone();
            Rc::new(move || {
                if let Some(coords) = read_valid(&rect) {
                    let _ = sender.send(MainWindowMsg::RegionConfirmed(coords));
                    window.close();
                }
            })
        };
        let do_cancel: Rc<dyn Fn()> = {
            let window = window.clone();
            let sender = sender.clone();
            Rc::new(move || {
                let _ = sender.send(MainWindowMsg::CaptureCancelled);
                window.close();
            })
        };

        // ── Action bar (Image kind only) ─────────────────────────────────────
        let action_box: Option<gtk::Box> = if kind == SelectorKind::Image {
            let bx = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            bx.add_css_class("region-actions");
            bx.set_halign(gtk::Align::Start);
            bx.set_valign(gtk::Align::Start);
            bx.set_visible(false);

            let cam = gtk::Button::from_icon_name("camera-photo-symbolic");
            cam.add_css_class("region-confirm");
            let f = do_confirm.clone();
            cam.connect_clicked(move |_| f());

            let cancel = gtk::Button::from_icon_name("window-close-symbolic");
            cancel.add_css_class("region-cancel");
            let f = do_cancel.clone();
            cancel.connect_clicked(move |_| f());

            bx.append(&cam);
            bx.append(&cancel);
            Some(bx)
        } else {
            None
        };

        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&area));
        if let Some(ab) = &action_box {
            overlay.add_overlay(ab);
        }
        window.set_child(Some(&overlay));

        // ── Drag gesture: create / move / resize ─────────────────────────────
        let drag = gtk::GestureDrag::new();
        {
            let rect = rect.clone();
            let drag_state = drag_state.clone();
            let area_ref = area.clone();
            drag.connect_drag_begin(move |_g, x, y| {
                let current = rect.get();
                let mode = match current {
                    Some(rc) => {
                        if let Some(hd) = hit_handle((x, y), rc) {
                            DragMode::Resize(hd)
                        } else if inside(rc, x, y) {
                            DragMode::Move
                        } else {
                            DragMode::New
                        }
                    }
                    None => DragMode::New,
                };
                if matches!(mode, DragMode::New) {
                    rect.set(Some((x, y, 0.0, 0.0)));
                }
                let orig = current.unwrap_or((x, y, 0.0, 0.0));
                drag_state.replace(Some(DragState { mode, orig, start: (x, y) }));
                area_ref.queue_draw();
            });
        }
        {
            let rect = rect.clone();
            let drag_state = drag_state.clone();
            let area_ref = area.clone();
            let action_box = action_box.clone();
            drag.connect_drag_update(move |_g, ox, oy| {
                let Some(ds) = *drag_state.borrow() else { return };
                let new_rect = apply_drag(ds, ox, oy, &area_ref);
                rect.set(Some(new_rect));
                if let Some(ab) = &action_box {
                    place_actions(ab, new_rect, &area_ref);
                }
                area_ref.queue_draw();
            });
        }
        {
            let rect = rect.clone();
            let drag_state = drag_state.clone();
            let area_ref = area.clone();
            let action_box = action_box.clone();
            let bar_shown = bar_shown.clone();
            let sender = sender.clone();
            drag.connect_drag_end(move |_g, ox, oy| {
                let ds_opt = *drag_state.borrow();
                drag_state.replace(None);
                if let Some(ds) = ds_opt {
                    let new_rect = apply_drag(ds, ox, oy, &area_ref);
                    rect.set(Some(new_rect));
                    let valid = new_rect.2 >= MIN && new_rect.3 >= MIN;

                    if let Some(ab) = &action_box {
                        ab.set_visible(valid);
                        if valid {
                            place_actions(ab, new_rect, &area_ref);
                        }
                    }
                    if kind == SelectorKind::Video && valid && !bar_shown.get() {
                        bar_shown.set(true);
                        let _ = sender.send(MainWindowMsg::RegionReady(round_rect(new_rect)));
                    }
                }
                area_ref.queue_draw();
            });
        }
        area.add_controller(drag);

        // ── Keyboard: Enter confirms (Image), Esc cancels (both) ─────────────
        let key = gtk::EventControllerKey::new();
        {
            let do_confirm = do_confirm.clone();
            let do_cancel = do_cancel.clone();
            key.connect_key_pressed(move |_, keyval, _, _| match keyval {
                gdk::Key::Escape => {
                    do_cancel();
                    glib::Propagation::Stop
                }
                gdk::Key::Return | gdk::Key::KP_Enter => {
                    if kind == SelectorKind::Image {
                        do_confirm();
                    }
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            });
        }
        window.add_controller(key);

        RegionSelector { window, rect }
    }

    /// Show the overlay fullscreen and force it above other windows.
    pub fn present(&self) {
        self.window.fullscreen();
        self.window.present();
        let title = TITLE.to_string();
        glib::timeout_add_local_once(Duration::from_millis(120), move || {
            let _ = Command::new("wmctrl")
                .args(["-r", &title, "-b", "add,above"])
                .output();
        });
    }

    /// The current selection in screen coordinates, or None if nothing valid
    /// has been drawn yet.
    pub fn current_rect(&self) -> Option<(i32, i32, i32, i32)> {
        read_valid(&self.rect)
    }

    pub fn close(&self) {
        self.window.close();
    }
}

// ── Geometry helpers ──────────────────────────────────────────────────────────

fn read_valid(rect: &RectCell) -> Option<(i32, i32, i32, i32)> {
    rect.get().and_then(|(x, y, w, h)| {
        if w >= MIN && h >= MIN {
            Some((x.round() as i32, y.round() as i32, w.round() as i32, h.round() as i32))
        } else {
            None
        }
    })
}

fn round_rect(r: (f64, f64, f64, f64)) -> (i32, i32, i32, i32) {
    (r.0.round() as i32, r.1.round() as i32, r.2.round() as i32, r.3.round() as i32)
}

fn handle_points(r: (f64, f64, f64, f64)) -> [(f64, f64); 8] {
    let (x, y, w, h) = r;
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    [
        (x, y),         // Nw
        (cx, y),        // N
        (x + w, y),     // Ne
        (x + w, cy),    // E
        (x + w, y + h), // Se
        (cx, y + h),    // S
        (x, y + h),     // Sw
        (x, cy),        // W
    ]
}

fn hit_handle(p: (f64, f64), r: (f64, f64, f64, f64)) -> Option<Handle> {
    const ORDER: [Handle; 8] = [
        Handle::Nw, Handle::N, Handle::Ne, Handle::E,
        Handle::Se, Handle::S, Handle::Sw, Handle::W,
    ];
    for (i, (hx, hy)) in handle_points(r).iter().enumerate() {
        if (p.0 - hx).abs() <= HIT && (p.1 - hy).abs() <= HIT {
            return Some(ORDER[i]);
        }
    }
    None
}

fn inside(r: (f64, f64, f64, f64), px: f64, py: f64) -> bool {
    px > r.0 && px < r.0 + r.2 && py > r.1 && py < r.1 + r.3
}

fn normalize(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64, f64, f64) {
    (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
}

fn apply_drag(ds: DragState, ox: f64, oy: f64, area: &gtk::DrawingArea) -> (f64, f64, f64, f64) {
    let aw = area.width() as f64;
    let ah = area.height() as f64;
    match ds.mode {
        DragMode::New => normalize(ds.start.0, ds.start.1, ds.start.0 + ox, ds.start.1 + oy),
        DragMode::Move => {
            let (x, y, w, h) = ds.orig;
            let nx = (x + ox).clamp(0.0, (aw - w).max(0.0));
            let ny = (y + oy).clamp(0.0, (ah - h).max(0.0));
            (nx, ny, w, h)
        }
        DragMode::Resize(hd) => {
            let (x, y, w, h) = ds.orig;
            let (mut l, mut t, mut r, mut b) = (x, y, x + w, y + h);
            match hd {
                Handle::Nw => { l += ox; t += oy; }
                Handle::N  => { t += oy; }
                Handle::Ne => { r += ox; t += oy; }
                Handle::E  => { r += ox; }
                Handle::Se => { r += ox; b += oy; }
                Handle::S  => { b += oy; }
                Handle::Sw => { l += ox; b += oy; }
                Handle::W  => { l += ox; }
            }
            normalize(l, t, r, b)
        }
    }
}

fn place_actions(ab: &gtk::Box, rect: (f64, f64, f64, f64), area: &gtk::DrawingArea) {
    let aw = area.width() as f64;
    let ah = area.height() as f64;
    let bar_w = 84.0;
    let bar_h = 40.0;
    let gap = 8.0;
    let (x, y, w, h) = rect;

    let bx = (x + w / 2.0 - bar_w / 2.0).clamp(4.0, (aw - bar_w - 4.0).max(4.0));
    let by = if y + h + gap + bar_h <= ah {
        y + h + gap
    } else {
        (y + h - bar_h - gap).max(4.0)
    };
    ab.set_margin_start(bx as i32);
    ab.set_margin_top(by as i32);
}

// ── CSS ───────────────────────────────────────────────────────────────────────

fn install_css() {
    let css = gtk::CssProvider::new();
    css.load_from_data(
        "window.region-overlay { background-color: transparent; }
         .region-actions {
            background-color: rgba(46, 46, 46, 0.92);
            border-radius: 8px;
            padding: 2px;
         }
         .region-actions button {
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
         .region-actions button image { color: rgba(220, 220, 220, 0.9); }
         .region-actions button:hover { background-color: rgba(255, 255, 255, 0.12); }
         .region-actions button.region-confirm image { color: #4b8bd4; }
         .region-actions button.region-confirm:hover { background-color: rgba(75, 139, 212, 0.2); }",
    );
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display, &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
        );
    }
}
