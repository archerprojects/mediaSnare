# mediaSnare_production_notes_v1_6.md
## Lean Linux — Screen, Audio, and Image Capture Application
## Developer: archerprojects (archer.projects@proton.me)
## Repository: https://github.com/archerprojects/mediaSnare
## 2026/06/19

---

## BUILD COMMANDS

### Build (deb only — no install, no sudo)
```bash
meson setup --wipe _build && ninja -C _build && bash build-aux/build-deb.sh _build dist
```

### Clean to delivery state (strip all build artifacts before posting source)
```bash
rm -rf _build dist target && rm -f mediasnare *.tar.gz *.deb
```

### Clean and build (fresh tree → deb)
```bash
rm -rf _build dist target && rm -f mediasnare *.tar.gz *.deb && meson setup _build && ninja -C _build && bash build-aux/build-deb.sh _build dist
```

### Install (manual — only when commanded)
```bash
sudo dpkg -i dist/mediasnare_1.0.0_amd64.deb
```

### Source archive for handoff
```bash
git archive --format=tar.gz HEAD -o mediasnare_source_$(date +%Y%m%d).tar.gz
```
Or without git (exclude artifacts and nested archives):
```bash
tar --exclude='./_build' --exclude='./dist' --exclude='./target' \
    --exclude='./mediasnare' --exclude='./*.tar.gz' \
    -czf mediasnare_source_$(date +%Y%m%d).tar.gz .
```

---

## 1. OVERVIEW

mediaSnare is a native capture application replacing gnome-screenshot and Kazam.
Handles still image capture, video capture with audio, and standalone audio
recording from a single minimal UI. Wayland-ready via PipeWire/xdg-desktop-portal
with X11 as the primary session target.

App ID: org.archerprojects.mediaSnare. Version 1.0.0.
mediaSnare ships with Lean Linux but is not exclusive to it.

mediaSnare is a capture tool. Post-capture editing (annotation, markup) is out
of scope — that work is left to dedicated editors. The annotation scaffold that
existed through v1.5 was removed in v1.6 (see change log).

---

## 2. PACKAGE IDENTITY

| Field | Value |
|---|---|
| App name | mediaSnare |
| Binary | `mediasnare` |
| Package | `mediasnare_1.0.0_amd64.deb` |
| Icon name | `mediasnare` |
| Desktop file | `mediasnare.desktop` |
| App ID | `org.archerprojects.mediaSnare` |
| Install path | `/usr/bin/mediasnare` |
| Config path | `/etc/mediasnare/` |
| User config | `~/.config/mediasnare/` |
| Developer | archerprojects |
| Contact | archer.projects@proton.me |
| Repository | https://github.com/archerprojects/mediaSnare |
| License | GPL-3.0-or-later |
| Version | 1.0.0 |

The release deb carries a clean Debian version (`1.0.0`) with no build
timestamp — the artifact name is deterministic for a public release.

---

## 3. TECHNOLOGY STACK

| Component | Library | Version |
|---|---|---|
| Language | Rust | 1.95.0 (host) / 1.80 MSRV |
| UI framework | relm4 | 0.10.1 |
| UI toolkit | GTK4 + libadwaita | gtk4 0.10.x / adw 0.8.x (via relm4) |
| GStreamer | gstreamer-rs | 0.22.x |
| D-Bus / portals | zbus | 5.x |
| Image encoding | image-rs | 0.25 |
| Build system | Cargo + Meson | 1.3.2 |
| Settings | GSettings (gio) | — |
| Logging | tracing + tracing-subscriber | — |
| Error handling | anyhow | — |

### Critical dependency constraint
relm4 0.10.1 pulls gtk4 0.10.x which uses glib 0.20.x.
gstreamer 0.22.x uses glib 0.19.x — compatible via semver unification.
gstreamer 0.23+ uses glib 0.21.x — INCOMPATIBLE. Do not upgrade gst past 0.22.

`Cargo.lock` is committed. mediaSnare is a binary application, so the locked
dependency graph travels with the source and the public tree builds against the
exact versions that were tested.

---

## 4. CURRENT IMPLEMENTATION STATUS — V1.0.0

### Working — Image capture
- Three scopes: Full, Region, Window
- Region uses the in-app draggable selector (see §6) — NOT Cinnamon SelectArea
- Capture chain (Full/Window): Cinnamon shell D-Bus → xdg-desktop-portal →
  ximagesrc fallback
- Capture chain (Region): in-app selector coordinates → Cinnamon ScreenshotArea
  (shell `capture_region`) → ximagesrc crop fallback
- Cinnamon shell (org.gnome.Shell.Screenshot on org.Cinnamon) is primary path
  on Cinnamon desktops — direct file write, no FUSE mount, no URI decoding
- Window scope uses ScreenshotWindow — captures focused window with decorations
- Portal path is fallback for non-Cinnamon desktops (GNOME, KDE, etc.)
- ximagesrc is last resort when no shell/portal backend exists
- Window hides before capture via main-loop-flushed hide
  (glib::timeout_add_local_once ensures the compositor processes the unmap
  before capture fires). Window re-presents after save.
- Esc / X in the region selector cancels quietly — no error dialog, Snare re-arms
- Output formats: PNG, JPG, WebP (selectable in preferences)
- Image copied to clipboard on capture (gated on copy-to-clipboard preference)
- Desktop notification on capture (org.freedesktop.Notifications via zbus,
  fires before window re-present so the banner appears while app is unfocused)
- Optional Save As dialog after capture (gated on ask-save-name preference)
- Save path: per image-directory setting or ~/Pictures default

### Working — Video capture
- Full and Region scope recording via ximagesrc (X11)
- Region: in-app selector draws a live box → recording bar appears Ready →
  the box stays adjustable → Record locks the final rectangle and starts;
  ximagesrc crops via startx/starty/endx/endy
- Camcorder workflow: Snare → bar appears Ready → Record → recording →
  Pause/Record toggle → Stop saves
- Video+audio confirmed (desktop monitor audio via @DEFAULT_MONITOR@, mic,
  both, or none)
- All four video profiles produce playable files: MP4, MKV, WebM, MP4 (GPU)
- Pause/Resume wired to the GStreamer pipeline via a shared
  Arc<Mutex<Pipeline>> handle
- Portal screencast path exists but no backend on Cinnamon — graceful fallback
  to the X11 pipeline on non-cancel failure (logged, not surfaced as error)
- Settings-driven: framerate, capture-cursor, video-profile from GSettings
- Output path: per video-directory setting or ~/Videos default
- Optional Save As dialog after recording

### Working — Audio capture
- Standalone recording from desktop audio, microphone, or both
- Same camcorder workflow as video (floating bar Record/Pause/Stop)
- Both audio profiles: OGG/Opus, MP3
- Audio muxer constructed as a bare element (request-pad pattern, as video)
- Monitor device uses @DEFAULT_MONITOR@ for desktop audio capture
- Pause/Resume via the shared pipeline handle
- Output path: per audio-directory setting or ~/Music default
- Optional Save As dialog after recording

### Working — Region selector overlay (new in v1.6)
- Fullscreen, semi-transparent, always-on-top overlay owned by mediaSnare —
  replaces Cinnamon's one-shot SelectArea for both image and video region
- Drag out a rectangle; grab any of 8 handles to resize, or the body to move;
  drag on empty space to start a fresh box
- Image kind: a small action bar (camera = confirm, X = cancel) tracks the
  bottom edge of the box and flips inside when near the screen bottom; Enter
  also confirms, Esc also cancels
- Video kind: no action bar — the recording bar's Record is the confirm; the
  box stays live and adjustable until Record is pressed; Esc cancels
- Single-monitor target: widget coordinates map 1:1 to screen pixels

### Working — Floating recording bar
- Four buttons: app icon (minimize bar), Record (red dot), Pause (two bars),
  Stop (light square)
- Lean palette: background rgba(46,46,46,0.92), record #e35d4f, stop #4b8bd4,
  icons #f0f0f0; no tooltips
- State-driven sensitivity (Ready/Recording/Paused) managed by main_window.rs
- Draggable via gtk::WindowHandle; always-on-top via wmctrl; positioning via
  xdotool with retry; both degrade gracefully if absent
- Default position: bottom-left (full/audio) or below-left of the drawn region
- Position saved to GSettings (bar-x, bar-y) on stop, restored next time

### Working — UI and preferences
- Side tabs (Image/Video/Audio), controls, Snare button
- Tab switching via controls_stack; window size fixed (480×360)
- Snare disables during capture, re-enables on completion
- Preferences dialog (480×680, all settings visible without scrolling):
  Save Locations (Images/Videos/Audio pickers + reset); Image (format, copy
  to clipboard, ask for filename); Video (format, framerate 1–60, capture
  cursor); Audio (format); About (Developer, Contact, Repository link)
- Gear icon renders from app-private mediasnare-settings-symbolic SVG in
  GResource (theme-independent, blue accent via CSS)
- Headerbar minimize/close styled with blue accent
- Last-used mode, scope, audio source persisted and restored on launch
- GSettings schema installed; icon cache updates on install
- postinst clean — no apt-get calls, no lock conflicts on install

### Working — Build system
- Meson + Cargo + deb packaging
- GResource compiles bundled icon and profiles.toml
- Build command produces deb only — no automatic install
- Zero compiler warnings
- Clean deterministic deb name: `mediasnare_1.0.0_amd64.deb`
- GPL-3.0-or-later; README.md, LICENSE, .gitignore in the source bundle

---

## 5. ROADMAP — POST V1

### Removed from scope
- **Annotation.** mediaSnare is a capture tool; post-capture markup is left to
  dedicated editors. The dormant annotation scaffold (sketch_board, tools,
  style) was deleted in v1.6 along with its stray references.

### Deferred features
- **Global hotkey** — start/stop recording without switching windows
  (e.g. Super+Shift+R). Needs an X11 key grab or the GlobalShortcuts portal
  (no Cinnamon backend yet).
- **Wayland video recording** — blocked on Mint implementing
  org.gnome.Mutter.ScreenCast inside Muffin. The entire /org/gnome/Mutter
  D-Bus tree is hollow (paths registered, no interfaces). Mint 23
  (Christmas 2026) is the likely timeline. The portal-first architecture
  activates automatically when a backend appears — no code changes needed.
- **Video Window scope** — image Window works via Cinnamon ScreenshotWindow;
  video Window would need X11 window enumeration (avoided) or the ScreenCast
  portal. Deferred — Full + Region cover the practical cases.

---

## 6. ARCHITECTURE NOTES

### Image capture path
```
Snare pressed (Image mode)
→ root.set_visible(false) [main thread, synchronous]
→ glib::timeout_add_local_once(300ms) [main loop flushes hide to compositor]
→ HideFlushed → match (mode, scope)
  Region → present_region_selector(Image): overlay shown, box drawn/adjusted
    → camera button or Enter → RegionConfirmed((x,y,w,h))
        → drop selector → 300ms flush → DispatchImageRegion
        → screenshot::run_area(x,y,w,h):
            shell capture_region (Cinnamon ScreenshotArea, explicit coords)
            → on failure: ximagesrc crop (startx/starty/endx/endy, inclusive)
            → if format ≠ PNG: re-encode via image-rs
    → X button or Esc → CaptureCancelled (quiet re-arm)
  Full/Window → dispatch_image_capture:
    Full:   shell Screenshot(include_frame=false, flash=true, filename)
    Window: shell ScreenshotWindow(include_frame=true, include_cursor, flash)
    → on shell failure: portal → ximagesrc fallback
→ CommandMsg::CaptureDone(path) or CaptureCancelled
→ if ask-save-name: Save As dialog
→ notify via org.freedesktop.Notifications (before re-present)
→ PresentWindow → root.set_visible(true) + present()
→ clipboard copy if enabled
```

### Video / Audio capture path (camcorder workflow)
```
Snare pressed (Video or Audio mode)
→ root.set_visible(false) → 300ms flush → HideFlushed → match (mode, scope)
  Video + Region → present_region_selector(Video): overlay shown
    → first valid box draw → RegionReady((x,y,w,h)) → ReadyToRecord(Some(region))
    → box stays live and adjustable; recording bar shown Ready
  Video Full / Audio (any) → ReadyToRecord(None)
→ ReadyToRecord(region): RecordingBar created, shown Ready
    (region: positioned outside the initial box; else saved pos or bottom-left)
→ Bar Record pressed:
    Ready + live selector (video region) → read selector.current_rect() →
      recording_region = final box → close overlay → 250ms flush →
      StartRecordingNow → start_recording
    Ready, no selector (video full / audio) → start_recording
→ start_recording spawns the async pipeline:
    oneshot stop channel + Arc<Mutex<Pipeline>> handle
    Video: run_video_with_stop(profile, audio_source, scope, region, handle, stop_rx)
      → portal screencast attempted → failure + X11 → ximagesrc fallback
      → region: ximagesrc startx/starty/endx/endy crop
      → muxer as bare element (request pads), audio branch programmatic
    Audio: run_with_stop(audio_source, profile, handle, stop_rx)
→ CaptureStarted → bar switches to Recording
→ Bar Pause → set_state(Paused); Record → set_state(Playing)
→ Bar Stop → stop_tx fires → EOS → pipeline drains → CaptureDone
→ bar saves position and closes → if ask-save-name: Save As → main window re-presents
```

### Region selector overlay
`window/region_selector.rs`. A fullscreen undecorated gtk::Window with a
transparent background and a DrawingArea that, via `set_draw_func`, paints a
dim layer (rgba 0,0,0,0.45), punches a cleared hole at the selection
(Operator::Clear), strokes the accent border (#4b8bd4), and fills eight handle
squares. A GestureDrag decides at drag-begin whether the press hits a handle
(resize), the box body (move), or empty space (new box); MIN size is 10px. An
EventControllerKey maps Enter → confirm (Image only) and Esc → cancel (both).

The selector talks to the main window through `relm4::Sender<MainWindowMsg>`
(the same pattern as the recording bar) — it is a plain GTK window, not a relm4
component, because GTK objects are not Send and must stay on the main thread.
It exposes `present()`, `current_rect()`, and `close()`. Always-on-top is
requested via wmctrl after a 120ms map delay.

Two kinds:
- **Image** — an action bar (camera/X buttons, theme symbolic icons,
  accent-blue confirm) is overlaid and repositioned under the box on every
  drag, flipping inside when near the screen bottom. Confirm sends
  `RegionConfirmed`; cancel sends `CaptureCancelled`; the overlay closes itself.
- **Video** — no action bar. The first valid box sends `RegionReady` once (a
  `bar_shown` latch guards re-fire) so the main window raises the Ready bar.
  The overlay stays live; the main window reads `current_rect()` when Record is
  pressed, then tears the overlay down before the pipeline starts.

### SelectArea retired
Cinnamon's `SelectArea()` is no longer used anywhere. `shell_screenshot.rs`
dropped `select_area()`; `capture()` now serves Full/Window only (the Region
arm is a defensive internal bail). Region capture goes through the new
`capture_region(x,y,w,h,output)`, which calls `ScreenshotArea` with explicit
coordinates from the in-app selector. `screenshot.rs` gained `run_area()` with
an ximagesrc-crop fallback for non-Cinnamon desktops.

### GStreamer capsfilter syntax
`bin_from_description` rejects bare caps notation (`! video/x-raw,... !`) —
unlike gst-launch, it tries to find an element literally named "video". The
explicit form `capsfilter caps=video/x-raw,...` parses correctly. All pipeline
string construction uses the explicit form.

### Notification delivery on Cinnamon
GLib's GNotification selects a backend at startup. On Cinnamon/MATE it picks
org.gtk.Notifications, which nothing implements — notifications vanish silently
with no runtime warning. mediaSnare bypasses this: notifications go direct to
org.freedesktop.Notifications via zbus, which every notification daemon
(Cinnamon, MATE, GNOME, KDE, dunst) implements. Cinnamon's daemon suppresses
banners from the focused app, so the notification is sent before the window
re-presents (500ms safety timeout on the ack).

### Cancelled marker vs overlay cancel
`capture::Cancelled` is a typed error marker (implements std::error::Error).
The portal and screencast paths return it on user cancel; the dispatch layer
downcasts it → quiet re-arm. The region selector takes a more direct route: on
Esc/X it sends `MainWindowMsg::CaptureCancelled` itself, so its cancel never
needs the error channel.

### GTK objects not Send
`adw::ApplicationWindow` (and the overlay/bar windows) are not Send. They cannot
be passed into `sender.command()` closures that run on a thread pool. All
window show/hide/overlay operations happen in `update()` on the main thread.

### relm4 0.10.1 update_view constraint
relm4's macro generates `update_view` internally; it cannot be defined manually
(E0201). Stack switches are driven by storing widget refs (`main_stack`,
`controls_stack`) at init and calling `set_visible_child_name()` in `update()`.

### Pipeline pause/resume
The pipeline handle is `Arc<Mutex<Option<gst::Pipeline>>>` (PipelineHandle),
created in `start_recording()`, stored in the model, passed into the async
command. The main thread pauses/resumes via helpers in pipeline.rs that lock
the mutex and call `set_state(Paused)` / `set_state(Playing)`. Cleared to None
on CaptureDone/Error/Cancelled and Stop-as-cancel. GStreamer Pipeline objects
are Send + Sync.

### Recording bar positioning
Undecorated gtk::Window made draggable via gtk::WindowHandle. GTK4 removed
window positioning APIs, so position is set via xdotool after the window maps
(350ms delay); always-on-top via wmctrl with a 300ms retry. Both degrade
gracefully if not installed. Position saved to GSettings (bar-x, bar-y) on stop.
For region captures the bar auto-positions outside the drawn rectangle:
below-left, then above, left, right, fallback bottom-left.

### Wayland ScreenCast — platform status (verified 2026/06/13)
Muffin (Cinnamon's compositor, forked from Mutter) registers the D-Bus path
`/org/gnome/Mutter/ScreenCast` but implements no interfaces — the entire
`/org/gnome/Mutter` tree is hollow. xdg-desktop-portal-xapp has no ScreenCast
implementation (GitHub issue #13, March 2024, no response from Mint). Video
recording on Cinnamon Wayland is blocked until Mint implements
org.gnome.Mutter.ScreenCast inside Muffin (Mint 23, Christmas 2026 likely). On
X11, video recording works via ximagesrc regardless of portal status.

---

## 7. FILE STRUCTURE

```
mediaSnare/
├── Cargo.toml              — version source of truth (1.0.0)
├── Cargo.lock              — committed (application: locked dependency graph)
├── build.rs                — generates config.rs into OUT_DIR
├── meson.build             — outer build, GResource compile, schema install
├── meson_options.txt
├── README.md               — project readme, prerequisites, build instructions
├── LICENSE                 — GPL-3.0-or-later
├── .gitignore              — Rust + Meson + deb artifacts (Cargo.lock tracked)
├── build-aux/
│   ├── cargo-build.sh
│   ├── build-deb.sh        — clean 1.0.0 deb, reads version from Cargo.toml
│   ├── compile-schemas.sh
│   └── update-icon-cache.sh
├── data/
│   ├── resources/
│   │   ├── resources.gresource.xml
│   │   ├── profiles.toml    — video + audio GStreamer profiles
│   │   ├── style.css        — sidebar + headerbar accent styling
│   │   └── icons/scalable/actions/mediasnare-settings-symbolic.svg
│   ├── icons/
│   │   ├── mediasnare.svg
│   │   └── hicolor/         — PNG sizes (16–256)
│   ├── mediasnare.desktop
│   └── org.archerprojects.mediaSnare.gschema.xml
├── debian/
│   └── postinst            — schema compile + icon cache only
└── src/
    ├── main.rs
    ├── application.rs       — GResource + CSS + icon theme registration
    ├── config.rs.in
    ├── settings.rs          — GSettings wrapper, all read/write
    ├── window/
    │   ├── mod.rs           — exports
    │   ├── main_window.rs   — Component, UI, dispatch, preferences, cancel
    │   ├── recording_bar.rs — floating control bar (Record/Pause/Stop)
    │   └── region_selector.rs — in-app draggable region overlay (NEW)
    ├── capture/
    │   ├── mod.rs           — exports + Cancelled marker type
    │   ├── pipeline.rs      — GStreamer video pipeline, PipelineHandle, pause/resume
    │   ├── screenshot.rs    — shell-first image capture + run_area() region path
    │   ├── audio.rs         — standalone audio pipeline with shared handle
    │   ├── profile.rs       — profile loader from GResource profiles.toml
    │   └── recording.rs     — RecordingState enum
    └── portal/
        ├── mod.rs
        ├── screencast.rs    — xdg-desktop-portal ScreenCast session
        ├── screenshot.rs    — xdg-desktop-portal Screenshot (is_available + request)
        ├── shell_screenshot.rs — Cinnamon shell D-Bus: capture() + capture_region()
        ├── notify.rs        — org.freedesktop.Notifications D-Bus
        └── types.rs         — SourceType, CursorMode, PersistMode, Stream
```

---

## 8. GSETTINGS SCHEMA KEYS

Schema: `org.archerprojects.mediaSnare`

| Key | Type | Default | Usage |
|---|---|---|---|
| `image-directory` | string | `""` | Image save path. Empty = ~/Pictures |
| `video-directory` | string | `""` | Video save path. Empty = ~/Videos |
| `audio-directory` | string | `""` | Audio save path. Empty = ~/Music |
| `image-format` | string | `"png"` | png / jpg / webp |
| `video-profile` | string | `"mp4"` | Video profile ID (mp4, mkv, webm-vp8, va-h264) |
| `audio-profile` | string | `"opus"` | Audio profile ID (opus, mp3) |
| `framerate` | uint32 | `30` | Recording framerate (1–60) |
| `capture-cursor` | bool | `true` | Include cursor in recordings |
| `timer-delay` | uint32 | `0` | Countdown seconds before capture (0–10) |
| `notify-on-complete` | bool | `true` | Desktop notification after capture |
| `ask-save-name` | bool | `false` | Show Save As dialog after capture |
| `copy-to-clipboard` | bool | `true` | Copy captured images to clipboard |
| `capture-mode` | string | `"image"` | Last used mode (restored on launch) |
| `capture-scope` | string | `"fullscreen"` | Last used scope (restored on launch) |
| `audio-source` | string | `"desktop"` | Audio source for recording |
| `bar-x` | int32 | `-1` | Recording bar X position (-1 = auto) |
| `bar-y` | int32 | `-1` | Recording bar Y position (-1 = auto) |
| `portal-token` | string | `""` | Cached screencast restore token |

---

## 9. VIDEO PROFILES (profiles.toml)

| ID | Name | Extension | Muxer | Video Encoder | Audio Encoder | Requires |
|---|---|---|---|---|---|---|
| mp4 | MP4 | .mp4 | mp4mux (fragmented) | x264enc (ultrafast, baseline) | lamemp3enc | — |
| mkv | MKV | .mkv | matroskamux | x264enc (ultrafast, baseline) | opusenc | — |
| webm-vp8 | WebM | .webm | webmmux | vp8enc | opusenc | — |
| va-h264 | MP4 (GPU) | .mp4 | mp4mux (fragmented) | vah264enc | lamemp3enc | VA-API |

## 10. AUDIO PROFILES (profiles.toml)

| ID | Name | Extension | Muxer | Audio Encoder |
|---|---|---|---|---|
| opus | OGG / Opus | .ogg | oggmux | opusenc |
| mp3 | MP3 | .mp3 | (none — self-framing) | lamemp3enc |

---

## 11. DEBIAN PACKAGE

Built with `dpkg-deb --build` from build-aux/build-deb.sh. The control file is
generated inline from Cargo.toml; the deb name is `mediasnare_1.0.0_amd64.deb`.

### Runtime dependencies (Depends)
- libgtk-4-1, libadwaita-1-0
- libgstreamer1.0-0, gstreamer1.0-plugins-base, gstreamer1.0-plugins-good,
  gstreamer1.0-plugins-ugly
- gstreamer1.0-pipewire, libpipewire-0.3-0
- xdg-desktop-portal

### Recommended
- gstreamer1.0-vaapi (GPU-accelerated H.264)
- wmctrl (recording bar + region selector always-on-top)
- xdotool (recording bar positioning)

### Desktop file categories
```
Categories=X-LEAN-Tools;AudioVideo;Video;Audio;Recorder;
```
X-LEAN for the Lean Linux menu; AudioVideo/Video/Audio/Recorder for Sound &
Video on Mint and standard desktops.

---

## CHANGE LOG

| Version | Date | Change |
|---|---|---|
| v1.0 | 2026/06/04 | Initial specification |
| v1.1 | 2026/06/07 | Developer identity, zbus 5, UI shell complete, profile.rs complete |
| v1.2 | 2026/06/11 | App ID org.archerprojects. Build system stabilised. postinst fixed. Timestamped deb. Image capture pipeline implemented. |
| v1.3 | 2026/06/12 | Portal-first capture. Fixed UI resize. Stack switch wired. Preferences dialog (save dir, image format). |
| v1.4 | 2026/06/13 | Cinnamon shell D-Bus capture (all image scopes). Region + Window image capture. FUSE save error fixed. Window hide deterministic. Preferences icon bundled. Video/audio format selectors. Video capture working (Full, X11 fallback, all muxers, audio). Clipboard copy. freedesktop notification. Cancelled marker. MKV profile. Last-used persistence. Wayland status documented. |
| v1.5 | 2026/06/15 | V1 feature complete. Camcorder workflow. Floating recording bar (Lean palette, Record/Pause/Stop). Pause/Resume via Arc<Mutex>. Video Region via SelectArea + ximagesrc crop. Per-type save directories. Audio muxer + monitor fix. Save As dialog. About section. 0 warnings. README, LICENSE (GPL v3), .gitignore. Sound & Video categories. wmctrl/xdotool in Recommends. |
| v1.6 | 2026/06/19 | V1.0.0 release. In-app draggable region selector (`window/region_selector.rs`) replaces Cinnamon SelectArea for both image and video region — dim/box/handle overlay, camera+Enter / X+Esc confirm (image), live box + Record (video). `shell_screenshot::capture_region()`; `screenshot::run_area()` with ximagesrc-crop fallback; SelectArea retired app-wide. Annotation removed from scope (scaffold + all references deleted). Dead files removed (unused `.ui`, orphan root `config.rs.in`). Clean deterministic deb `mediasnare_1.0.0_amd64.deb` (build timestamp dropped). `Cargo.lock` committed. README region description, desktop Keywords, and deb control Description corrected. |

---

*mediaSnare_production_notes_v1_6.md — 2026/06/19*
*Developed for Lean Linux by archerprojects (archer.projects@proton.me)*
