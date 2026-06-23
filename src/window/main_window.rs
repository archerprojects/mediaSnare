// window/main_window.rs

use relm4::adw;
use relm4::gtk;
use relm4::adw::prelude::*;
use relm4::gtk::{gio, glib};
use relm4::prelude::*;
use std::sync::{Arc, Mutex};
use tokio::sync::oneshot;

use crate::capture::recording::RecordingState;
use crate::capture::pipeline::PipelineHandle;
use crate::settings::Settings;
use crate::window::recording_bar::{BarState, RecordingBar};
use crate::window::region_selector::{RegionSelector, SelectorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Image,
    Video,
    Audio,
}

impl Mode {
    fn from_setting(s: &str) -> Self {
        match s {
            "video" => Mode::Video,
            "audio" => Mode::Audio,
            _       => Mode::Image,
        }
    }

    fn as_setting(&self) -> &'static str {
        match self {
            Mode::Image => "image",
            Mode::Video => "video",
            Mode::Audio => "audio",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Full,
    Region,
    Window,
}

impl Scope {
    // Schema choice values are "fullscreen"/"region"/"window" — note
    // "fullscreen", not "full". GSettings silently rejects non-choice writes.
    fn from_setting(s: &str) -> Self {
        match s {
            "region" => Scope::Region,
            "window" => Scope::Window,
            _        => Scope::Full,
        }
    }

    fn as_setting(&self) -> &'static str {
        match self {
            Scope::Full   => "fullscreen",
            Scope::Region => "region",
            Scope::Window => "window",
        }
    }
}

// Schema choice values are "desktop"/"microphone"/"both"/"none" — note
// "microphone", not "mic". The UI uses "mic" internally.
fn audio_source_from_setting(s: &str) -> String {
    match s {
        "microphone" => "mic".into(),
        other        => other.to_owned(),
    }
}

fn audio_source_as_setting(s: &str) -> &'static str {
    match s {
        "desktop" => "desktop",
        "mic"     => "microphone",
        "both"    => "both",
        _         => "none",
    }
}

#[derive(Debug)]
pub enum MainWindowMsg {
    SetMode(Mode),
    SetScope(Scope),
    SetTimer(u32),
    SetAudioSource(String),
    Snare,
    HideFlushed,
    RegionConfirmed((i32, i32, i32, i32)),
    RegionReady((i32, i32, i32, i32)),
    DispatchImageRegion((i32, i32, i32, i32)),
    ReadyToRecord(Option<(i32, i32, i32, i32)>),
    Record,
    StartRecordingNow,
    Pause,
    Stop,
    CaptureStarted,
    CaptureDone(std::path::PathBuf),
    CaptureCancelled,
    PresentWindow,
    CaptureError(String),
    Tick,
    OpenPreferences,
}

#[derive(Debug)]
pub enum CommandMsg {
    CaptureStarted,
    CaptureDone(std::path::PathBuf),
    CaptureCancelled,
    CaptureError(String),
    Tick,
}

pub struct MainWindow {
    mode:            Mode,
    scope:           Scope,
    timer_secs:      u32,
    audio_source:    String,
    recording_state: RecordingState,
    elapsed_secs:    u64,
    snare_sensitive: bool,
    stop_tx:         Option<oneshot::Sender<()>>,
    pipeline_handle: Option<PipelineHandle>,
    recording_region: Option<(i32, i32, i32, i32)>,
    recording_bar:   Option<RecordingBar>,
    region_selector: Option<RegionSelector>,
    // Widget refs captured at init — not reachable from update() otherwise
    main_stack:      gtk::Stack,
    controls_stack:  gtk::Stack,
}

impl MainWindow {
    fn elapsed_formatted(&self) -> String {
        let m = self.elapsed_secs / 60;
        let s = self.elapsed_secs % 60;
        format!("{m}:{s:02}")
    }

    fn is_recording(&self) -> bool {
        matches!(self.recording_state,
            RecordingState::Ready     | RecordingState::Recording |
            RecordingState::Paused    | RecordingState::Delayed |
            RecordingState::Flushing)
    }
}

#[relm4::component(pub)]
impl Component for MainWindow {
    type Init          = ();
    type Input         = MainWindowMsg;
    type Output        = ();
    type CommandOutput = CommandMsg;

    view! {
        #[name = "root_window"]
        adw::ApplicationWindow {
            set_title: Some("mediaSnare"),
            set_default_width:  480,
            set_default_height: 360,
            set_resizable: false,

            gtk::Box {
                set_orientation: gtk::Orientation::Horizontal,

                // ── Side tab bar ──────────────────────────────────────────────
                gtk::Box {
                    set_orientation: gtk::Orientation::Vertical,
                    set_width_request: 72,
                    add_css_class: "sidebar",
                    #[watch]
                    set_sensitive: !model.is_recording(),

                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_vexpand: true,
                        set_valign: gtk::Align::Center,
                        set_spacing: 4,
                        set_margin_top: 12,
                        set_margin_bottom: 12,
                        set_margin_start: 8,
                        set_margin_end: 8,

                        #[name = "tab_image"]
                        gtk::ToggleButton {
                            set_active: true,
                            add_css_class: "tab-button",
                            set_tooltip_text: Some("Image"),
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,
                                set_halign: gtk::Align::Center,
                                gtk::Image {
                                    set_icon_name: Some("camera-photo-symbolic"),
                                    set_pixel_size: 22,
                                },
                                gtk::Label {
                                    set_label: "Image",
                                    add_css_class: "tab-label",
                                },
                            },
                            connect_toggled[sender] => move |btn| {
                                if btn.is_active() {
                                    sender.input(MainWindowMsg::SetMode(Mode::Image));
                                }
                            },
                        },

                        #[name = "tab_video"]
                        gtk::ToggleButton {
                            set_group: Some(&tab_image),
                            add_css_class: "tab-button",
                            set_tooltip_text: Some("Video"),
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,
                                set_halign: gtk::Align::Center,
                                gtk::Image {
                                    set_icon_name: Some("camera-video-symbolic"),
                                    set_pixel_size: 22,
                                },
                                gtk::Label {
                                    set_label: "Video",
                                    add_css_class: "tab-label",
                                },
                            },
                            connect_toggled[sender] => move |btn| {
                                if btn.is_active() {
                                    sender.input(MainWindowMsg::SetMode(Mode::Video));
                                }
                            },
                        },

                        #[name = "tab_audio"]
                        gtk::ToggleButton {
                            set_group: Some(&tab_image),
                            add_css_class: "tab-button",
                            set_tooltip_text: Some("Audio"),
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,
                                set_halign: gtk::Align::Center,
                                gtk::Image {
                                    set_icon_name: Some("audio-input-microphone-symbolic"),
                                    set_pixel_size: 22,
                                },
                                gtk::Label {
                                    set_label: "Audio",
                                    add_css_class: "tab-label",
                                },
                            },
                            connect_toggled[sender] => move |btn| {
                                if btn.is_active() {
                                    sender.input(MainWindowMsg::SetMode(Mode::Audio));
                                }
                            },
                        },
                    },

                    gtk::Button {
                        set_icon_name: "mediasnare-settings-symbolic",
                        set_tooltip_text: Some("Preferences"),
                        set_margin_bottom: 12,
                        set_margin_start: 8,
                        set_margin_end: 8,
                        add_css_class: "flat",
                        connect_clicked[sender] => move |_| {
                            sender.input(MainWindowMsg::OpenPreferences);
                        },
                    },
                },

                gtk::Separator {
                    set_orientation: gtk::Orientation::Vertical,
                },

                // ── Main stack ────────────────────────────────────────────────
                #[name = "main_stack"]
                gtk::Stack {
                    set_hexpand: true,
                    set_vexpand: true,
                    set_transition_type: gtk::StackTransitionType::Crossfade,
                    set_transition_duration: 150,

                    // ── Capture view ──────────────────────────────────────────
                    add_named[Some("capture")] = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,

                        adw::HeaderBar {
                            set_show_end_title_buttons: true,
                            #[wrap(Some)]
                            set_title_widget = &gtk::Label {
                                set_label: "mediaSnare",
                                add_css_class: "heading",
                            },
                        },

                        // Fixed-height controls stack — all pages same size,
                        // window never resizes on mode switch
                        #[name = "controls_stack"]
                        gtk::Stack {
                            set_vexpand: false,
                            set_valign: gtk::Align::Center,
                            set_height_request: 160,
                            set_transition_type: gtk::StackTransitionType::None,

                            // Image controls
                            add_named[Some("image_controls")] = &gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 16,
                                set_margin_start: 24,
                                set_margin_end: 24,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    gtk::Label {
                                        set_label: "Scope",
                                        set_hexpand: true,
                                        set_halign: gtk::Align::Start,
                                        add_css_class: "dim-label",
                                    },
                                    gtk::Box {
                                        add_css_class: "linked",
                                        #[name = "scope_full"]
                                        gtk::ToggleButton {
                                            set_label: "Full",
                                            set_active: true,
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetScope(Scope::Full));
                                                }
                                            },
                                        },
                                        #[name = "scope_region"]
                                        gtk::ToggleButton {
                                            set_label: "Region",
                                            set_group: Some(&scope_full),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetScope(Scope::Region));
                                                }
                                            },
                                        },
                                        #[name = "scope_window"]
                                        gtk::ToggleButton {
                                            set_label: "Window",
                                            set_group: Some(&scope_full),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetScope(Scope::Window));
                                                }
                                            },
                                        },
                                    },
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    gtk::Label {
                                        set_label: "Timer",
                                        set_hexpand: true,
                                        set_halign: gtk::Align::Start,
                                        add_css_class: "dim-label",
                                    },
                                    gtk::Box {
                                        add_css_class: "linked",
                                        #[name = "timer_0"]
                                        gtk::ToggleButton {
                                            set_label: "0s",
                                            set_active: true,
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetTimer(0));
                                                }
                                            },
                                        },
                                        gtk::ToggleButton {
                                            set_label: "3s",
                                            set_group: Some(&timer_0),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetTimer(3));
                                                }
                                            },
                                        },
                                        gtk::ToggleButton {
                                            set_label: "5s",
                                            set_group: Some(&timer_0),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetTimer(5));
                                                }
                                            },
                                        },
                                        gtk::ToggleButton {
                                            set_label: "10s",
                                            set_group: Some(&timer_0),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetTimer(10));
                                                }
                                            },
                                        },
                                    },
                                },
                            },

                            // Video controls
                            add_named[Some("video_controls")] = &gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 16,
                                set_margin_start: 24,
                                set_margin_end: 24,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    gtk::Label {
                                        set_label: "Scope",
                                        set_hexpand: true,
                                        set_halign: gtk::Align::Start,
                                        add_css_class: "dim-label",
                                    },
                                    gtk::Box {
                                        add_css_class: "linked",
                                        #[name = "vscope_full"]
                                        gtk::ToggleButton {
                                            set_label: "Full",
                                            set_active: true,
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetScope(Scope::Full));
                                                }
                                            },
                                        },
                                        #[name = "vscope_region"]
                                        gtk::ToggleButton {
                                            set_label: "Region",
                                            set_group: Some(&vscope_full),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetScope(Scope::Region));
                                                }
                                            },
                                        },
                                    },
                                },

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    gtk::Label {
                                        set_label: "Audio",
                                        set_hexpand: true,
                                        set_halign: gtk::Align::Start,
                                        add_css_class: "dim-label",
                                    },
                                    gtk::Box {
                                        add_css_class: "linked",
                                        #[name = "vaudio_none"]
                                        gtk::ToggleButton {
                                            set_label: "None",
                                            set_active: true,
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("none".into()));
                                                }
                                            },
                                        },
                                        #[name = "vaudio_desktop"]
                                        gtk::ToggleButton {
                                            set_label: "Desktop",
                                            set_group: Some(&vaudio_none),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("desktop".into()));
                                                }
                                            },
                                        },
                                        #[name = "vaudio_mic"]
                                        gtk::ToggleButton {
                                            set_label: "Mic",
                                            set_group: Some(&vaudio_none),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("mic".into()));
                                                }
                                            },
                                        },
                                        #[name = "vaudio_both"]
                                        gtk::ToggleButton {
                                            set_label: "Both",
                                            set_group: Some(&vaudio_none),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("both".into()));
                                                }
                                            },
                                        },
                                    },
                                },
                            },

                            // Audio controls
                            add_named[Some("audio_controls")] = &gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 16,
                                set_margin_start: 24,
                                set_margin_end: 24,

                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    gtk::Label {
                                        set_label: "Audio",
                                        set_hexpand: true,
                                        set_halign: gtk::Align::Start,
                                        add_css_class: "dim-label",
                                    },
                                    gtk::Box {
                                        add_css_class: "linked",
                                        #[name = "audio_none"]
                                        gtk::ToggleButton {
                                            set_label: "None",
                                            set_active: true,
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("none".into()));
                                                }
                                            },
                                        },
                                        #[name = "audio_desktop"]
                                        gtk::ToggleButton {
                                            set_label: "Desktop",
                                            set_group: Some(&audio_none),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("desktop".into()));
                                                }
                                            },
                                        },
                                        #[name = "audio_mic"]
                                        gtk::ToggleButton {
                                            set_label: "Mic",
                                            set_group: Some(&audio_none),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("mic".into()));
                                                }
                                            },
                                        },
                                        #[name = "audio_both"]
                                        gtk::ToggleButton {
                                            set_label: "Both",
                                            set_group: Some(&audio_none),
                                            connect_toggled[sender] => move |btn| {
                                                if btn.is_active() {
                                                    sender.input(MainWindowMsg::SetAudioSource("both".into()));
                                                }
                                            },
                                        },
                                    },
                                },
                            },
                        },

                        gtk::Box {
                            set_margin_start: 24,
                            set_margin_end: 24,
                            set_margin_bottom: 24,
                            set_margin_top: 16,
                            gtk::Button {
                                set_label: "Snare",
                                set_hexpand: true,
                                add_css_class: "suggested-action",
                                add_css_class: "pill",
                                #[watch]
                                set_sensitive: model.snare_sensitive,
                                connect_clicked[sender] => move |_| {
                                    sender.input(MainWindowMsg::Snare);
                                },
                            },
                        },
                    },

                    // ── Recording bar ─────────────────────────────────────────
                    add_named[Some("recording")] = &gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,

                        adw::HeaderBar {
                            set_show_end_title_buttons: false,
                            #[wrap(Some)]
                            set_title_widget = &gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 8,
                                set_halign: gtk::Align::Center,
                                gtk::Image {
                                    set_icon_name: Some("media-record-symbolic"),
                                    add_css_class: "recording-dot",
                                },
                                gtk::Label {
                                    #[watch]
                                    set_label: &model.elapsed_formatted(),
                                    add_css_class: "numeric",
                                },
                            },
                        },

                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 12,
                            set_halign: gtk::Align::Center,
                            set_vexpand: true,
                            set_valign: gtk::Align::Center,
                            gtk::Button {
                                set_icon_name: "media-playback-pause-symbolic",
                                set_tooltip_text: Some("Pause"),
                                add_css_class: "circular",
                                connect_clicked[sender] => move |_| {
                                    sender.input(MainWindowMsg::Pause);
                                },
                            },
                            gtk::Button {
                                set_icon_name: "media-playback-stop-symbolic",
                                set_tooltip_text: Some("Stop"),
                                add_css_class: "circular",
                                add_css_class: "destructive-action",
                                connect_clicked[sender] => move |_| {
                                    sender.input(MainWindowMsg::Stop);
                                },
                            },
                        },
                    },
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        _root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        // Restore last-used state from GSettings
        let settings     = Settings::get();
        let mode         = Mode::from_setting(&settings.capture_mode());
        let scope        = Scope::from_setting(&settings.capture_scope());
        let audio_source = audio_source_from_setting(&settings.audio_source());

        let model = MainWindow {
            mode,
            scope,
            timer_secs:      0,
            audio_source:    audio_source.clone(),
            recording_state: RecordingState::Idle,
            elapsed_secs:    0,
            snare_sensitive: true,
            stop_tx:         None,
            pipeline_handle: None,
            recording_region: None,
            recording_bar:   None,
            region_selector: None,
            main_stack:      gtk::Stack::new(),
            controls_stack:  gtk::Stack::new(),
        };
        let widgets = view_output!();
        let _ = sender;

        let mut model = model;
        model.main_stack     = widgets.main_stack.clone();
        model.controls_stack = widgets.controls_stack.clone();

        // Apply restored state to the toggle groups. set_active(true) on a
        // grouped toggle untoggles the rest and re-fires SetMode/SetScope/
        // SetAudioSource inputs — harmless, they write the same value back.
        match mode {
            Mode::Image => widgets.tab_image.set_active(true),
            Mode::Video => widgets.tab_video.set_active(true),
            Mode::Audio => widgets.tab_audio.set_active(true),
        }
        model.controls_stack.set_visible_child_name(match mode {
            Mode::Image => "image_controls",
            Mode::Video => "video_controls",
            Mode::Audio => "audio_controls",
        });

        match scope {
            Scope::Full   => widgets.scope_full.set_active(true),
            Scope::Region => widgets.scope_region.set_active(true),
            Scope::Window => widgets.scope_window.set_active(true),
        }
        if scope == Scope::Region {
            widgets.vscope_region.set_active(true);
        } else {
            widgets.vscope_full.set_active(true);
        }

        let (v_btn, a_btn) = match audio_source.as_str() {
            "desktop" => (&widgets.vaudio_desktop, &widgets.audio_desktop),
            "mic"     => (&widgets.vaudio_mic,     &widgets.audio_mic),
            "both"    => (&widgets.vaudio_both,    &widgets.audio_both),
            _         => (&widgets.vaudio_none,    &widgets.audio_none),
        };
        v_btn.set_active(true);
        a_btn.set_active(true);

        // Single tick task for the component lifetime. The Tick handler
        // only increments when RecordingState::Recording, so this is
        // harmless during idle. Spawning per-CaptureStarted caused N
        // concurrent tick tasks after N recordings → timer ran at Nx speed.
        sender.command(|out, shutdown| {
            shutdown.register(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    if out.send(CommandMsg::Tick).is_err() { break; }
                }
            }).drop_on_shutdown()
        });

        ComponentParts { model, widgets }
    }

    fn update(
        &mut self,
        msg: MainWindowMsg,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match msg {
            MainWindowMsg::SetMode(m) => {
                self.mode = m;
                let page = match m {
                    Mode::Image => "image_controls",
                    Mode::Video => "video_controls",
                    Mode::Audio => "audio_controls",
                };
                self.controls_stack.set_visible_child_name(page);
                Settings::get().set_capture_mode(m.as_setting());
            }

            MainWindowMsg::SetScope(s) => {
                self.scope = s;
                Settings::get().set_capture_scope(s.as_setting());
            }

            MainWindowMsg::SetTimer(t) => self.timer_secs = t,

            MainWindowMsg::SetAudioSource(a) => {
                Settings::get().set_audio_source(audio_source_as_setting(&a));
                self.audio_source = a;
            }

            MainWindowMsg::Snare => {
                self.snare_sensitive = false;
                // Hide for all modes: images need it so the app doesn't appear
                // in the shot, video/audio need it so the main window doesn't
                // appear in the recording. The floating bar provides controls
                // during video/audio recording.
                root.set_visible(false);
                let sender = sender.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(300),
                    move || sender.input(MainWindowMsg::HideFlushed),
                );
            }

            MainWindowMsg::HideFlushed => {
                match (self.mode, self.scope) {
                    (Mode::Image, Scope::Region) => {
                        self.present_region_selector(&sender, SelectorKind::Image);
                    }
                    (Mode::Image, _) => self.dispatch_image_capture(sender),
                    (Mode::Video, Scope::Region) => {
                        self.present_region_selector(&sender, SelectorKind::Video);
                    }
                    // Video Full/Window and all Audio: no region to select.
                    _ => { sender.input(MainWindowMsg::ReadyToRecord(None)); }
                }
            }

            MainWindowMsg::RegionConfirmed(coords) => {
                // Image region confirmed. Drop the overlay handle (it closed
                // itself), then capture after a short flush so the dimmed
                // overlay is fully gone before the grab fires.
                self.region_selector = None;
                let sender = sender.clone();
                glib::timeout_add_local_once(
                    std::time::Duration::from_millis(300),
                    move || sender.input(MainWindowMsg::DispatchImageRegion(coords)),
                );
            }

            MainWindowMsg::DispatchImageRegion(coords) => {
                self.dispatch_image_capture_region(sender, coords);
            }

            MainWindowMsg::RegionReady(coords) => {
                // Video region: the box is drawn. Raise the Ready bar while the
                // overlay stays live for further adjustment. The final geometry
                // is read from the overlay when Record is pressed.
                sender.input(MainWindowMsg::ReadyToRecord(Some(coords)));
            }

            MainWindowMsg::Pause => {
                if self.recording_state == RecordingState::Recording {
                    self.recording_state = RecordingState::Paused;
                    if let Some(handle) = &self.pipeline_handle {
                        crate::capture::pipeline::pause_pipeline(handle);
                    }
                    if let Some(bar) = &self.recording_bar {
                        bar.set_state(BarState::Paused);
                    }
                }
            }

            MainWindowMsg::Stop => {
                if self.recording_state == RecordingState::Ready {
                    self.recording_state = RecordingState::Idle;
                    self.snare_sensitive = true;
                    self.recording_region = None;
                    if let Some(sel) = self.region_selector.take() {
                        sel.close();
                    }
                    if let Some(bar) = self.recording_bar.take() {
                        bar.save_and_close();
                    }
                    self.main_stack.set_visible_child_name("capture");
                    root.set_visible(true);
                    root.present();
                } else {
                    if let Some(tx) = self.stop_tx.take() {
                        let _ = tx.send(());
                    }
                    self.recording_state = RecordingState::Flushing;
                }
            }

            MainWindowMsg::ReadyToRecord(region) => {
                self.recording_state = RecordingState::Ready;
                self.recording_region = region;
                let bar = RecordingBar::new(sender.input_sender().clone());
                bar.show(region);
                self.recording_bar = Some(bar);
            }

            MainWindowMsg::Record => {
                match self.recording_state {
                    RecordingState::Ready => {
                        if let Some(sel) = self.region_selector.take() {
                            // Video region: lock in the overlay's final
                            // rectangle, tear the overlay down, then start once
                            // it has unmapped so the dim/box never reach the
                            // first frames.
                            if let Some(rect) = sel.current_rect() {
                                self.recording_region = Some(rect);
                            }
                            sel.close();
                            let sender = sender.clone();
                            glib::timeout_add_local_once(
                                std::time::Duration::from_millis(250),
                                move || sender.input(MainWindowMsg::StartRecordingNow),
                            );
                        } else {
                            self.start_recording(sender);
                        }
                    }
                    RecordingState::Paused => {
                        self.recording_state = RecordingState::Recording;
                        if let Some(handle) = &self.pipeline_handle {
                            crate::capture::pipeline::resume_pipeline(handle);
                        }
                        if let Some(bar) = &self.recording_bar {
                            bar.set_state(BarState::Recording);
                        }
                    }
                    _ => {}
                }
            }

            MainWindowMsg::StartRecordingNow => {
                if self.recording_state == RecordingState::Ready {
                    self.start_recording(sender);
                }
            }

            MainWindowMsg::CaptureStarted => {
                self.recording_state = RecordingState::Recording;
                self.main_stack.set_visible_child_name("recording");
                if let Some(bar) = &self.recording_bar {
                    bar.set_state(BarState::Recording);
                }
            }

            MainWindowMsg::CaptureDone(path) => {
                self.recording_state = RecordingState::Idle;
                self.elapsed_secs    = 0;
                self.snare_sensitive = true;
                self.stop_tx         = None;
                self.pipeline_handle = None;
                self.recording_region = None;
                if let Some(bar) = self.recording_bar.take() {
                    bar.save_and_close();
                }
                self.main_stack.set_visible_child_name("capture");

                if self.mode == Mode::Image && Settings::get().copy_to_clipboard() {
                    copy_image_to_clipboard(root, &path);
                }

                if Settings::get().ask_save_name() {
                    show_save_as_dialog(root, &path);
                }

                if Settings::get().notify_on_complete() {
                    // Notify BEFORE re-presenting: the daemon suppresses
                    // banners from the focused app, so the window comes back
                    // only after the daemon has acked (500ms safety cap —
                    // the window must never stay hidden on a stuck bus).
                    let body = path.display().to_string();
                    let sender = sender.clone();
                    relm4::spawn(async move {
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_millis(500),
                            crate::portal::notify::send("Capture saved", &body),
                        ).await;
                        sender.input(MainWindowMsg::PresentWindow);
                    });
                } else {
                    root.set_visible(true);
                    root.present();
                }
                tracing::info!("capture saved: {}", path.display());
            }

            MainWindowMsg::CaptureCancelled => {
                if let Some(sel) = self.region_selector.take() {
                    sel.close();
                }
                self.recording_state = RecordingState::Idle;
                self.elapsed_secs    = 0;
                self.snare_sensitive = true;
                self.stop_tx         = None;
                self.pipeline_handle = None;
                if let Some(bar) = self.recording_bar.take() {
                    bar.save_and_close();
                }
                self.main_stack.set_visible_child_name("capture");
                root.set_visible(true);
                root.present();
            }

            MainWindowMsg::PresentWindow => {
                root.set_visible(true);
                root.present();
            }

            MainWindowMsg::CaptureError(msg) => {
                if let Some(sel) = self.region_selector.take() {
                    sel.close();
                }
                self.recording_state = RecordingState::Idle;
                self.elapsed_secs    = 0;
                self.snare_sensitive = true;
                self.stop_tx         = None;
                self.pipeline_handle = None;
                if let Some(bar) = self.recording_bar.take() {
                    bar.save_and_close();
                }
                self.main_stack.set_visible_child_name("capture");
                root.set_visible(true);
                root.present();
                show_error(root, &msg);
            }

            MainWindowMsg::Tick => {
                if self.recording_state == RecordingState::Recording {
                    self.elapsed_secs += 1;
                }
            }

            MainWindowMsg::OpenPreferences => {
                show_preferences(root);
            }
        }
    }

    fn update_cmd(
        &mut self,
        msg: CommandMsg,
        sender: ComponentSender<Self>,
        _root: &Self::Root,
    ) {
        match msg {
            CommandMsg::CaptureStarted   => sender.input(MainWindowMsg::CaptureStarted),
            CommandMsg::CaptureDone(p)  => sender.input(MainWindowMsg::CaptureDone(p)),
            CommandMsg::CaptureCancelled => sender.input(MainWindowMsg::CaptureCancelled),
            CommandMsg::CaptureError(e) => sender.input(MainWindowMsg::CaptureError(e)),
            CommandMsg::Tick            => sender.input(MainWindowMsg::Tick),
        }
    }
}

impl MainWindow {
    fn dispatch_image_capture(&mut self, sender: ComponentSender<Self>) {
        let scope      = self.scope;
        let timer_secs = self.timer_secs;

        sender.command(move |out, shutdown| {
            shutdown.register(async move {
                if timer_secs > 0 {
                    tokio::time::sleep(
                        std::time::Duration::from_secs(timer_secs as u64)
                    ).await;
                }
                let result = crate::capture::screenshot::run(scope).await;
                report(&out, result);
            }).drop_on_shutdown()
        });
    }

    fn present_region_selector(&mut self, sender: &ComponentSender<Self>, kind: SelectorKind) {
        let selector = RegionSelector::new(sender.input_sender().clone(), kind);
        selector.present();
        self.region_selector = Some(selector);
    }

    fn dispatch_image_capture_region(
        &mut self,
        sender: ComponentSender<Self>,
        region: (i32, i32, i32, i32),
    ) {
        let timer_secs = self.timer_secs;

        sender.command(move |out, shutdown| {
            shutdown.register(async move {
                if timer_secs > 0 {
                    tokio::time::sleep(
                        std::time::Duration::from_secs(timer_secs as u64)
                    ).await;
                }
                let (x, y, w, h) = region;
                let result = crate::capture::screenshot::run_area(x, y, w, h).await;
                report(&out, result);
            }).drop_on_shutdown()
        });
    }

    fn start_recording(&mut self, sender: ComponentSender<Self>) {
        let mode          = self.mode;
        let scope         = self.scope;
        let region        = self.recording_region;
        let audio_source  = self.audio_source.clone();
        let settings      = Settings::get();
        let video_profile = settings.video_profile();
        let audio_profile = settings.audio_profile();

        self.elapsed_secs = 0;
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        self.stop_tx = Some(stop_tx);

        let handle: PipelineHandle = Arc::new(Mutex::new(None));
        self.pipeline_handle = Some(handle.clone());

        sender.command(move |out, shutdown| {
            shutdown.register(async move {
                let _ = out.send(CommandMsg::CaptureStarted);
                let result = match mode {
                    Mode::Video => {
                        crate::capture::pipeline::run_video_with_stop(
                            &video_profile, &audio_source, scope, region,
                            handle.clone(), stop_rx,
                        ).await
                    }
                    Mode::Audio => {
                        crate::capture::audio::run_with_stop(
                            &audio_source, &audio_profile,
                            handle.clone(), stop_rx,
                        ).await
                    }
                    _ => return,
                };
                report(&out, result);
            }).drop_on_shutdown()
        });
    }
}

fn report(out: &relm4::Sender<CommandMsg>, result: anyhow::Result<std::path::PathBuf>) {
    match result {
        Ok(path) => { let _ = out.send(CommandMsg::CaptureDone(path)); }
        Err(e) if e.downcast_ref::<crate::capture::Cancelled>().is_some() => {
            let _ = out.send(CommandMsg::CaptureCancelled);
        }
        Err(e) => { let _ = out.send(CommandMsg::CaptureError(e.to_string())); }
    }
}


// ── Post-capture helpers ──────────────────────────────────────────────────────

/// Copy the captured image to the display clipboard (gnome-screenshot parity).
/// Best-effort — webp requires a pixbuf loader that may not be installed.
fn copy_image_to_clipboard(root: &adw::ApplicationWindow, path: &std::path::Path) {
    match gtk::gdk::Texture::from_file(&gio::File::for_path(path)) {
        Ok(texture) => {
            // RootExt::display — disambiguate from WidgetExt::display
            gtk::prelude::RootExt::display(root).clipboard().set_texture(&texture);
        }
        Err(e) => tracing::warn!("clipboard copy failed for {}: {e}", path.display()),
    }
}

// ── Dialogs ───────────────────────────────────────────────────────────────────

fn show_preferences(root: &adw::ApplicationWindow) {
    let settings = Settings::get();

    // adw::PreferencesWindow is the 1.5-compatible equivalent of PreferencesDialog
    let win = adw::PreferencesWindow::new();
    win.set_title(Some("Preferences"));
    win.set_transient_for(Some(root));
    win.set_modal(true);
    win.set_default_size(480, 680);

    let page = adw::PreferencesPage::new();
    page.set_title("General");
    page.set_icon_name(Some("mediasnare-settings-symbolic"));

    // ── Save locations ────────────────────────────────────────────────────────
    let save_group = adw::PreferencesGroup::new();
    save_group.set_title("Save Locations");

    add_directory_row(
        &save_group, &win, "Images",
        settings.image_directory(),
        "Pictures",
        |path| Settings::get().set_image_directory(path),
    );
    add_directory_row(
        &save_group, &win, "Videos",
        settings.video_directory(),
        "Videos",
        |path| Settings::get().set_video_directory(path),
    );
    add_directory_row(
        &save_group, &win, "Audio",
        settings.audio_directory(),
        "Music",
        |path| Settings::get().set_audio_directory(path),
    );

    // ── Image format ──────────────────────────────────────────────────────────
    let image_group = adw::PreferencesGroup::new();
    image_group.set_title("Image");

    let formats = gtk::StringList::new(&["PNG", "JPG", "WebP"]);
    let format_row = adw::ComboRow::new();
    format_row.set_title("Format");
    format_row.set_model(Some(&formats));
    let current_format = settings.image_format();
    format_row.set_selected(match current_format.as_str() {
        "jpg" | "jpeg" => 1,
        "webp"         => 2,
        _              => 0,
    });
    format_row.connect_selected_notify(|row| {
        let fmt = match row.selected() {
            1 => "jpg",
            2 => "webp",
            _ => "png",
        };
        Settings::get().set_image_format(fmt);
    });
    image_group.add(&format_row);

    let clip_row = adw::ActionRow::new();
    clip_row.set_title("Copy to Clipboard");
    clip_row.set_subtitle("Also copy captured images to the clipboard");
    let clip_switch = gtk::Switch::new();
    clip_switch.set_active(settings.copy_to_clipboard());
    clip_switch.set_valign(gtk::Align::Center);
    clip_switch.connect_active_notify(|sw| {
        Settings::get().set_copy_to_clipboard(sw.is_active());
    });
    clip_row.add_suffix(&clip_switch);
    clip_row.set_activatable_widget(Some(&clip_switch));
    image_group.add(&clip_row);

    let savename_row = adw::ActionRow::new();
    savename_row.set_title("Ask for Filename");
    savename_row.set_subtitle("Show a Save As dialog after each capture");
    let savename_switch = gtk::Switch::new();
    savename_switch.set_active(settings.ask_save_name());
    savename_switch.set_valign(gtk::Align::Center);
    savename_switch.connect_active_notify(|sw| {
        Settings::get().set_ask_save_name(sw.is_active());
    });
    savename_row.add_suffix(&savename_switch);
    savename_row.set_activatable_widget(Some(&savename_switch));
    image_group.add(&savename_row);

    // ── Video ─────────────────────────────────────────────────────────────────
    let video_group = adw::PreferencesGroup::new();
    video_group.set_title("Video");

    // Format row — populated from the profile loader, which has already
    // dropped anything this machine can't encode (e.g. GPU profile without
    // VA-API). New profiles in profiles.toml appear here with no code change.
    if let Ok(profiles) = crate::capture::profile::load() {
        if !profiles.video.is_empty() {
            let ids:   Vec<String> = profiles.video.iter().map(|p| p.id.clone()).collect();
            let names: Vec<&str>   = profiles.video.iter().map(|p| p.name.as_str()).collect();

            let vformat_row = adw::ComboRow::new();
            vformat_row.set_title("Format");
            vformat_row.set_model(Some(&gtk::StringList::new(&names)));
            if let Some(idx) = ids.iter().position(|i| *i == settings.video_profile()) {
                vformat_row.set_selected(idx as u32);
            }
            vformat_row.connect_selected_notify(move |row| {
                if let Some(id) = ids.get(row.selected() as usize) {
                    Settings::get().set_video_profile(id);
                }
            });
            video_group.add(&vformat_row);
        }
    }

    // Framerate row — ActionRow + SpinButton suffix (adw 1.5 compatible)
    let fps_row = adw::ActionRow::new();
    fps_row.set_title("Framerate");
    fps_row.set_subtitle("Frames per second");
    let fps_spin = gtk::SpinButton::with_range(1.0, 60.0, 1.0);
    fps_spin.set_value(settings.framerate() as f64);
    fps_spin.set_valign(gtk::Align::Center);
    fps_spin.connect_value_changed(|spin| {
        Settings::get().set_framerate(spin.value() as u32);
    });
    fps_row.add_suffix(&fps_spin);
    fps_row.set_activatable_widget(Some(&fps_spin));

    // Cursor row — ActionRow + Switch suffix (adw 1.5 compatible)
    let cursor_row = adw::ActionRow::new();
    cursor_row.set_title("Capture Cursor");
    cursor_row.set_subtitle("Include cursor in recordings");
    let cursor_switch = gtk::Switch::new();
    cursor_switch.set_active(settings.capture_cursor());
    cursor_switch.set_valign(gtk::Align::Center);
    cursor_switch.connect_active_notify(|sw| {
        Settings::get().set_capture_cursor(sw.is_active());
    });
    cursor_row.add_suffix(&cursor_switch);
    cursor_row.set_activatable_widget(Some(&cursor_switch));

    video_group.add(&fps_row);
    video_group.add(&cursor_row);

    // ── Audio ─────────────────────────────────────────────────────────────────
    let audio_group = adw::PreferencesGroup::new();
    audio_group.set_title("Audio");

    if let Ok(profiles) = crate::capture::profile::load() {
        if !profiles.audio.is_empty() {
            let ids:   Vec<String> = profiles.audio.iter().map(|p| p.id.clone()).collect();
            let names: Vec<&str>   = profiles.audio.iter().map(|p| p.name.as_str()).collect();

            let aformat_row = adw::ComboRow::new();
            aformat_row.set_title("Format");
            aformat_row.set_model(Some(&gtk::StringList::new(&names)));
            if let Some(idx) = ids.iter().position(|i| *i == settings.audio_profile()) {
                aformat_row.set_selected(idx as u32);
            }
            aformat_row.connect_selected_notify(move |row| {
                if let Some(id) = ids.get(row.selected() as usize) {
                    Settings::get().set_audio_profile(id);
                }
            });
            audio_group.add(&aformat_row);
        }
    }

    // ── About ──────────────────────────────────────────────────────────────────
    let about_group = adw::PreferencesGroup::new();
    about_group.set_title("About");

    let dev_row = adw::ActionRow::new();
    dev_row.set_title("Developer");
    dev_row.set_subtitle("archerprojects");
    about_group.add(&dev_row);

    let contact_row = adw::ActionRow::new();
    contact_row.set_title("Contact");
    contact_row.set_subtitle("archer.projects@proton.me");
    about_group.add(&contact_row);

    let repo_row = adw::ActionRow::new();
    repo_row.set_title("Repository");
    repo_row.set_subtitle("github.com/archerprojects/mediaSnare");
    repo_row.set_activatable(true);
    repo_row.connect_activated(|_| {
        let _ = gtk::gio::AppInfo::launch_default_for_uri(
            "https://github.com/archerprojects/mediaSnare",
            gtk::gio::AppLaunchContext::NONE,
        );
    });
    about_group.add(&repo_row);

    page.add(&save_group);
    page.add(&image_group);
    page.add(&video_group);
    page.add(&audio_group);
    page.add(&about_group);
    win.add(&page);
    win.present();
}

/// Build a save-directory chooser row and wire Choose / Reset buttons.
/// `setter` writes the chosen path (or empty for reset) to GSettings.
fn add_directory_row(
    group: &adw::PreferencesGroup,
    parent: &adw::PreferencesWindow,
    title: &str,
    current: Option<std::path::PathBuf>,
    default_subdir: &str,
    setter: fn(&str),
) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let display_path = current
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{home}/{default_subdir}"));

    let row = adw::ActionRow::new();
    row.set_title(title);
    row.set_subtitle(&display_path);

    let choose_btn = gtk::Button::with_label("Choose…");
    choose_btn.set_valign(gtk::Align::Center);
    choose_btn.add_css_class("flat");

    let reset_btn = gtk::Button::new();
    reset_btn.set_icon_name("edit-clear-symbolic");
    reset_btn.set_tooltip_text(Some("Reset to default"));
    reset_btn.set_valign(gtk::Align::Center);
    reset_btn.add_css_class("flat");

    row.add_suffix(&choose_btn);
    row.add_suffix(&reset_btn);
    group.add(&row);

    let row_clone = row.clone();
    let parent_clone = parent.clone();
    choose_btn.connect_clicked(move |_| {
        let dialog = gtk::FileChooserDialog::new(
            Some("Select Save Directory"),
            Some(&parent_clone),
            gtk::FileChooserAction::SelectFolder,
            &[
                ("Cancel", gtk::ResponseType::Cancel),
                ("Select", gtk::ResponseType::Accept),
            ],
        );
        let row_inner = row_clone.clone();
        dialog.connect_response(move |d, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(file) = d.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().into_owned();
                        setter(&path_str);
                        row_inner.set_subtitle(&path_str);
                    }
                }
            }
            d.close();
        });
        dialog.present();
    });

    let row_clone2 = row.clone();
    let default_display = format!("{home}/{default_subdir}");
    reset_btn.connect_clicked(move |_| {
        setter("");
        row_clone2.set_subtitle(&default_display);
    });
}

/// Save As dialog — pre-filled with the default path. User can rename or
/// relocate. The file is already saved, so Cancel keeps the default name.
fn show_save_as_dialog(root: &adw::ApplicationWindow, path: &std::path::Path) {
    let dialog = gtk::FileChooserDialog::new(
        Some("Save Capture As"),
        Some(root),
        gtk::FileChooserAction::Save,
        &[
            ("Cancel", gtk::ResponseType::Cancel),
            ("Save", gtk::ResponseType::Accept),
        ],
    );

    // Pre-fill with current directory and filename
    if let Some(parent) = path.parent() {
        let _ = dialog.set_current_folder(Some(&gio::File::for_path(parent)));
    }
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        dialog.set_current_name(name);
    }

    let original = path.to_path_buf();
    dialog.connect_response(move |d, response| {
        if response == gtk::ResponseType::Accept {
            if let Some(file) = d.file() {
                if let Some(new_path) = file.path() {
                    if new_path != original {
                        if let Err(e) = std::fs::rename(&original, &new_path) {
                            tracing::warn!(
                                "rename failed ({} → {}): {e} — trying copy",
                                original.display(), new_path.display()
                            );
                            // Cross-device: read + write + remove
                            if let Ok(bytes) = std::fs::read(&original) {
                                if std::fs::write(&new_path, bytes).is_ok() {
                                    let _ = std::fs::remove_file(&original);
                                }
                            }
                        }
                    }
                }
            }
        }
        d.close();
    });
    dialog.present();
}

fn show_error(root: &adw::ApplicationWindow, message: &str) {
    let dialog = gtk::MessageDialog::new(
        Some(root),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Ok,
        message,
    );
    dialog.set_title(Some("Capture failed"));
    dialog.connect_response(|d, _| d.close());
    dialog.present();
}
