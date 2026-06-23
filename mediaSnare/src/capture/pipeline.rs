// capture/pipeline.rs — GStreamer video capture pipeline
//
// Two source paths:
//   PipeWire  — preferred on both X11 and Wayland via xdg-desktop-portal
//   ximagesrc — X11 fallback when portal is unavailable
//
// Pipeline shape (single monitor):
//   src → videorate/caps → videoenc_bin ──→ muxer → filesink
//   audio_bin (if audio ≠ none) ──────────→ muxer
//
// The muxer is constructed as a real gst::Element, NOT wrapped in a bin.
// bin_from_description(.., true) only ghost-pads existing static pads —
// muxer sink pads are request pads, so a bin-wrapped muxer exposes no sink
// pads and every link to it fails. gst::Element::link() against the bare
// muxer element requests compatible pads correctly for both branches.
//
// Multi-monitor: compositor element with xpos offsets per stream.
// Stop: caller fires stop_rx → EOS is sent → pipeline drains → bus posts
// EOS which this module awaits before returning.

use anyhow::{anyhow, bail, Context, Result};
use gst::prelude::*;
use std::os::unix::io::{IntoRawFd, RawFd};
use std::sync::{Arc, Mutex};
use std::path::{Path, PathBuf};

use crate::capture::profile;
use crate::portal::screencast::ScreencastSession;
use crate::portal::types::{CursorMode, PersistMode, SourceType};
use crate::settings::Settings;
use crate::window::Scope;

// ── Shared pipeline handle for pause/resume from the main thread ──────────

pub type PipelineHandle = Arc<Mutex<Option<gst::Pipeline>>>;

pub fn pause_pipeline(handle: &PipelineHandle) {
    if let Ok(guard) = handle.lock() {
        if let Some(ref pipeline) = *guard {
            let _ = pipeline.set_state(gst::State::Paused);
        }
    }
}

pub fn resume_pipeline(handle: &PipelineHandle) {
    if let Ok(guard) = handle.lock() {
        if let Some(ref pipeline) = *guard {
            let _ = pipeline.set_state(gst::State::Playing);
        }
    }
}

// ── Output path helpers ───────────────────────────────────────────────────────

/// Derive a timestamped output path in the correct XDG directory.
pub fn output_path(extension: &str) -> PathBuf {
    let dir = Settings::get()
        .video_directory()
        .unwrap_or_else(|| dirs_next("videos"));
    let ts = chrono_stamp();
    dir.join(format!("mediasnare-{ts}.{extension}"))
}

pub fn dirs_next(kind: &str) -> PathBuf {
    // Fallback XDG dirs without the dirs crate dependency
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    let xdg = match kind {
        "videos"  => std::env::var("XDG_VIDEOS_DIR").unwrap_or_else(|_| format!("{home}/Videos")),
        "music"   => std::env::var("XDG_MUSIC_DIR").unwrap_or_else(|_| format!("{home}/Music")),
        _         => std::env::var("XDG_PICTURES_DIR").unwrap_or_else(|_| format!("{home}/Pictures")),
    };
    PathBuf::from(xdg)
}

pub fn chrono_stamp() -> String {
    // Simple timestamp without chrono dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

// ── Round to even (x264enc requirement) ──────────────────────────────────────

fn round_to_even(n: i32) -> i32 {
    if n % 2 == 0 { n } else { n + 1 }
}

// ── Element construction ──────────────────────────────────────────────────────

/// Build a single gst::Element from a "factory prop=val prop=val" description.
/// Used for muxers, which must be real elements (not bins) so that
/// Element::link() can request sink pads — see module header.
fn make_element_from_desc(desc: &str) -> Result<gst::Element> {
    let mut parts = desc.split_whitespace();
    let factory = parts.next()
        .ok_or_else(|| anyhow!("empty element description"))?;

    let element = gst::ElementFactory::make(factory)
        .build()
        .with_context(|| format!("element '{factory}' unavailable"))?;

    for prop in parts {
        let (key, value) = prop.split_once('=')
            .ok_or_else(|| anyhow!("malformed property '{prop}' in '{desc}'"))?;
        element.set_property_from_str(key, value);
    }

    Ok(element)
}

// ── Public entry point ────────────────────────────────────────────────────────

/// Run a video capture until stop_rx fires (or the pipeline errors).
/// Returns the output path on success.
pub async fn run_video_with_stop(
    profile_id: &str,
    audio_source: &str,
    scope: Scope,
    region: Option<(i32, i32, i32, i32)>,
    pipeline_handle: PipelineHandle,
    stop_rx: tokio::sync::oneshot::Receiver<()>,
) -> Result<PathBuf> {
    let prof = profile::video(profile_id)
        .ok_or_else(|| anyhow!("video profile '{profile_id}' not available"))?;

    let output = output_path(&prof.extension);
    std::fs::create_dir_all(output.parent().unwrap_or(Path::new("/tmp")))?;

    // Read settings before any await — gio::Settings is not Send and must
    // not be held across an await point inside a Send future.
    let (framerate, cursor) = {
        let settings = Settings::get();
        (settings.framerate(), settings.capture_cursor())
    };

    let session_type = std::env::var("XDG_SESSION_TYPE")
        .unwrap_or_else(|_| "x11".into())
        .to_lowercase();

    let portal_available = crate::portal::screencast::is_available().await;

    // The portal frontend always answers is_available() even when no backend
    // implements ScreenCast (e.g. Cinnamon X11: xapp does Screenshot only).
    // So a present-but-failing portal falls through to the direct X11
    // pipeline — only a user cancel propagates untouched.
    let pipeline = if portal_available {
        match build_pipewire_pipeline_for_stop(
            prof, audio_source, scope, &output, framerate, cursor,
        ).await {
            Ok(p) => p,
            Err(e) if e.downcast_ref::<crate::capture::Cancelled>().is_some() => {
                return Err(e);
            }
            Err(e) if session_type != "wayland" => {
                tracing::warn!("portal screencast failed — falling back to X11 pipeline: {e:#}");
                build_x11_pipeline(prof, audio_source, &output, framerate, region)?
            }
            Err(e) => return Err(e),
        }
    } else if session_type == "wayland" {
        bail!("Wayland session requires xdg-desktop-portal — portal not available")
    } else {
        build_x11_pipeline(prof, audio_source, &output, framerate, region)?
    };

    // Expose the pipeline so the main thread can pause/resume
    *pipeline_handle.lock().unwrap() = Some(pipeline.clone());

    pipeline.set_state(gst::State::Playing)
        .context("Failed to start pipeline")?;

    // Wait for stop signal or EOS/error
    tokio::select! {
        _ = stop_rx => {
            // User pressed Stop — send EOS and drain
            pipeline.send_event(gst::event::Eos::new());
            wait_eos(&pipeline).await?;
        }
        result = wait_eos(&pipeline) => {
            result?;
        }
    }

    pipeline.set_state(gst::State::Null)?;
    *pipeline_handle.lock().unwrap() = None;
    Ok(output)
}

// ── PipeWire path ─────────────────────────────────────────────────────────────

async fn build_pipewire_pipeline_for_stop(
    prof: &'static profile::VideoProfile,
    audio_source: &str,
    scope: Scope,
    output: &Path,
    framerate: u32,
    cursor: bool,
) -> Result<gst::Pipeline> {
    let cursor_mode = if cursor { CursorMode::Embedded } else { CursorMode::Hidden };

    let cached_token = Settings::get().portal_token();
    let session = ScreencastSession::new().await
        .context("Failed to create screencast portal session")?;

    let source_type = match scope {
        Scope::Window => SourceType::Window,
        _             => SourceType::Monitor,
    };

    // A stale restore token (monitor layout changed, compositor restarted)
    // makes the portal fail — clear it on any portal-phase error so the next
    // attempt gets a fresh permission dialog instead of failing forever.
    if let Err(e) = session.select_sources(
        source_type,
        cursor_mode,
        PersistMode::Persistent,
        cached_token.as_deref(),
    ).await {
        Settings::get().clear_portal_token();
        return Err(e).context("Portal source selection failed");
    }

    // start() persists the returned restore token via Settings internally.
    let (streams, _token) = match session.start().await {
        Ok(v) => v,
        Err(e) => {
            Settings::get().clear_portal_token();
            return Err(e).context("Portal session start failed");
        }
    };

    if streams.is_empty() {
        bail!("Portal returned no streams");
    }

    let owned_fd = session.open_pipewire_remote().await
        .context("Failed to open PipeWire remote fd")?;
    let fd: RawFd = owned_fd.into_raw_fd();

    build_pipewire_pipeline(prof, audio_source, &streams, fd, output, framerate)
}

fn build_pipewire_pipeline(
    prof: &profile::VideoProfile,
    audio_source: &str,
    streams: &[crate::portal::types::Stream],
    fd: RawFd,
    output: &Path,
    framerate: u32,
) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new();

    // ── Video branch ──
    let video_branch = if streams.len() == 1 {
        build_single_stream_src(fd, streams[0].node_id, framerate)?
    } else {
        build_multi_stream_compositor(fd, streams, framerate)?
    };

    // videoenc bin from profile — static pads, safe to wrap as a bin
    let videoenc = gst::parse::bin_from_description(&prof.videoenc, true)
        .context("Failed to parse videoenc pipeline")?;

    // ── Audio branch ──
    let audio_branch = build_audio_branch(audio_source, &prof.audioenc)?;

    // ── Muxer + sink — muxer must be a bare element (request pads) ──
    let muxer = make_element_from_desc(&prof.muxer)
        .context("Failed to construct muxer")?;

    let filesink = gst::ElementFactory::make("filesink")
        .property("location", output.to_str().unwrap_or("/tmp/mediasnare.mp4"))
        .build()
        .context("filesink element unavailable")?;

    pipeline.add_many([&video_branch, videoenc.upcast_ref(), &muxer, &filesink])?;

    // Link video branch → videoenc → muxer (muxer requests a video pad)
    video_branch.link(&videoenc)?;
    videoenc.link(&muxer)?;

    if let Some(audio_el) = audio_branch {
        pipeline.add(&audio_el)?;
        audio_el.link(&muxer)?;
    }

    muxer.link(&filesink)?;

    Ok(pipeline)
}

fn build_single_stream_src(
    fd: RawFd,
    node_id: u32,
    framerate: u32,
) -> Result<gst::Element> {
    let src = gst::ElementFactory::make("pipewiresrc")
        .property("fd", fd)
        .property("path", node_id.to_string().as_str())
        .build()
        .context("pipewiresrc element unavailable — gstreamer1.0-pipewire required")?;

    let capsfilter = gst::ElementFactory::make("capsfilter")
        .property(
            "caps",
            gst::Caps::builder("video/x-raw")
                .field("framerate", gst::Fraction::new(framerate as i32, 1))
                .build(),
        )
        .build()?;

    let bin = gst::Bin::new();
    bin.add_many([&src, &capsfilter])?;
    src.link(&capsfilter)?;

    // Ghost pad so the bin looks like a single element downstream
    let pad = capsfilter.static_pad("src").unwrap();
    let ghost = gst::GhostPad::with_target(&pad)?;
    bin.add_pad(&ghost)?;

    Ok(bin.upcast())
}

fn build_multi_stream_compositor(
    fd: RawFd,
    streams: &[crate::portal::types::Stream],
    framerate: u32,
) -> Result<gst::Element> {
    let bin = gst::Bin::new();
    let compositor = gst::ElementFactory::make("compositor")
        .build()
        .context("compositor element unavailable")?;
    bin.add(&compositor)?;

    for stream in streams {
        let src = gst::ElementFactory::make("pipewiresrc")
            .property("fd", fd)
            .property("path", stream.node_id.to_string().as_str())
            .build()
            .context("pipewiresrc unavailable")?;

        let capsfilter = gst::ElementFactory::make("capsfilter")
            .property(
                "caps",
                gst::Caps::builder("video/x-raw")
                    .field("framerate", gst::Fraction::new(framerate as i32, 1))
                    .build(),
            )
            .build()?;

        bin.add_many([&src, &capsfilter])?;
        src.link(&capsfilter)?;

        let comp_sink = compositor.request_pad_simple("sink_%u")
            .ok_or_else(|| anyhow!("compositor refused sink pad"))?;
        comp_sink.set_property("xpos", round_to_even(stream.position.0));
        comp_sink.set_property("ypos", round_to_even(stream.position.1));

        let src_pad = capsfilter.static_pad("src").unwrap();
        src_pad.link(&comp_sink)?;
    }

    let pad = compositor.static_pad("src").unwrap();
    let ghost = gst::GhostPad::with_target(&pad)?;
    bin.add_pad(&ghost)?;

    Ok(bin.upcast())
}

// ── Audio branch ──────────────────────────────────────────────────────────────

fn build_audio_branch(
    source: &str,
    audioenc_desc: &str,
) -> Result<Option<gst::Element>> {
    if source == "none" {
        return Ok(None);
    }

    let bin = gst::Bin::new();

    match source {
        "both" => {
            let desktop_src = make_pulsesrc(true)?;
            let mic_src     = make_pulsesrc(false)?;
            let mixer       = gst::ElementFactory::make("audiomixer").build()
                .context("audiomixer element unavailable")?;
            let audioenc    = gst::parse::bin_from_description(audioenc_desc, true)?;

            bin.add_many([&desktop_src, &mic_src, &mixer, audioenc.upcast_ref()])?;
            desktop_src.link(&mixer)?;
            mic_src.link(&mixer)?;
            mixer.link(&audioenc)?;

            let pad = audioenc.static_pad("src").unwrap();
            let ghost = gst::GhostPad::with_target(&pad)?;
            bin.add_pad(&ghost)?;
        }
        source => {
            let monitor = source == "desktop";
            let src = make_pulsesrc(monitor)?;
            let audioenc = gst::parse::bin_from_description(audioenc_desc, true)?;

            bin.add_many([&src, audioenc.upcast_ref()])?;
            src.link(&audioenc)?;

            let pad = audioenc.static_pad("src").unwrap();
            let ghost = gst::GhostPad::with_target(&pad)?;
            bin.add_pad(&ghost)?;
        }
    }

    Ok(Some(bin.upcast()))
}

fn make_pulsesrc(monitor: bool) -> Result<gst::Element> {
    let src = gst::ElementFactory::make("pulsesrc")
        .build()
        .context("pulsesrc element unavailable — gstreamer1.0-plugins-good required")?;

    if monitor {
        // @DEFAULT_MONITOR@ is the PulseAudio well-known name for the default
        // sink's monitor source. pipewire-pulse honours it too. An empty/unset
        // device selects the default *input* (microphone), not the monitor.
        src.set_property("device", "@DEFAULT_MONITOR@");
    }
    // Mic: default input — no device property needed

    Ok(src)
}

// ── X11 fallback path ─────────────────────────────────────────────────────────

fn build_x11_pipeline(
    prof: &'static profile::VideoProfile,
    audio_source: &str,
    output: &Path,
    framerate: u32,
    region: Option<(i32, i32, i32, i32)>,
) -> Result<gst::Pipeline> {
    // Built programmatically — a linear parse::launch string cannot wire a
    // second source branch into the muxer. Same structure as the PipeWire
    // path: video bin → videoenc bin → muxer element ← audio bin.
    let pipeline = gst::Pipeline::new();

    // Region capture: ximagesrc takes startx/starty/endx/endy to grab a
    // sub-rectangle of the screen. endx/endy are inclusive pixel coords.
    let region_props = match region {
        Some((x, y, w, h)) => format!(
            " startx={x} starty={y} endx={} endy={}",
            x + w - 1,
            y + h - 1,
        ),
        None => String::new(),
    };

    // NOTE: bin_from_description rejects bare caps notation ("! video/x-raw,
    // ... !") — unlike gst-launch — and tries to find an element named
    // "video". The explicit capsfilter form parses correctly (the profile
    // encoder strings already use it for the same reason).
    let video_src = gst::parse::bin_from_description(
        &format!(
            "ximagesrc use-damage=false{region_props} ! videoconvert ! videorate ! \
             capsfilter caps=video/x-raw,framerate={framerate}/1"
        ),
        true,
    ).context("Failed to build ximagesrc source bin")?;

    let videoenc = gst::parse::bin_from_description(&prof.videoenc, true)
        .context("Failed to parse videoenc pipeline")?;

    let audio_branch = build_audio_branch(audio_source, &prof.audioenc)?;

    let muxer = make_element_from_desc(&prof.muxer)
        .context("Failed to construct muxer")?;

    let filesink = gst::ElementFactory::make("filesink")
        .property("location", output.to_str().unwrap_or("/tmp/mediasnare.mp4"))
        .build()
        .context("filesink element unavailable")?;

    pipeline.add_many([video_src.upcast_ref(), videoenc.upcast_ref(), &muxer, &filesink])?;

    video_src.link(&videoenc)?;
    videoenc.link(&muxer)?;

    if let Some(audio_el) = audio_branch {
        pipeline.add(&audio_el)?;
        audio_el.link(&muxer)?;
    }

    muxer.link(&filesink)?;

    Ok(pipeline)
}

// ── EOS waiter ────────────────────────────────────────────────────────────────

/// Poll the pipeline bus until EOS or Error.
pub async fn wait_eos(pipeline: &gst::Pipeline) -> Result<()> {
    let bus = pipeline.bus().ok_or_else(|| anyhow!("pipeline has no bus"))?;

    loop {
        // Non-blocking poll — yield back to tokio between checks
        if let Some(msg) = bus.timed_pop(gst::ClockTime::from_mseconds(100)) {
            match msg.view() {
                gst::MessageView::Eos(_) => return Ok(()),
                gst::MessageView::Error(e) => {
                    bail!("GStreamer pipeline error: {}", e.error());
                }
                _ => {}
            }
        } else {
            tokio::task::yield_now().await;
        }
    }
}
