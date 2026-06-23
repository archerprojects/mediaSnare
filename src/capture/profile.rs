// capture/profile.rs — output format profile loader
//
// Loads profiles.toml from GResource at startup. Each profile's availability
// is checked against the running GStreamer plugin registry. Unavailable
// profiles are silently dropped — the UI only sees what can actually run.
//
// vaapi = true profiles are gated on vah264enc element presence rather than
// full pipeline parse, since a missing VAAPI stack produces an ambiguous error.

use anyhow::{Context, Result};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use std::num::NonZeroUsize;

use relm4::gtk::gio;
// ── TOML schema ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ProfilesFile {
    #[serde(default)]
    video: Vec<RawVideoProfile>,
    #[serde(default)]
    audio: Vec<RawAudioProfile>,
}

#[derive(Debug, Deserialize)]
struct RawVideoProfile {
    id:        String,
    name:      String,
    extension: String,
    videoenc:  String,
    audioenc:  String,
    muxer:     String,
    #[serde(default)]
    vaapi:     bool,
}

#[derive(Debug, Deserialize)]
struct RawAudioProfile {
    id:        String,
    name:      String,
    extension: String,
    audioenc:  String,
    muxer:     String,
}

// ── public types ──────────────────────────────────────────────────────────────

/// A video output profile confirmed available on this system.
#[derive(Debug, Clone)]
pub struct VideoProfile {
    pub id:        String,
    pub name:      String,
    pub extension: String,
    /// Encoder pipeline string with ${N_THREADS} substituted.
    pub videoenc:  String,
    /// Encoder pipeline string with ${N_THREADS} substituted.
    pub audioenc:  String,
    pub muxer:     String,
    #[allow(dead_code)]
    pub vaapi:     bool,
}

/// An audio output profile confirmed available on this system.
#[derive(Debug, Clone)]
pub struct AudioProfile {
    pub id:        String,
    pub name:      String,
    pub extension: String,
    pub audioenc:  String,
    /// Empty string for raw formats (e.g. mp3).
    pub muxer:     String,
}

#[derive(Debug, Clone)]
pub struct Profiles {
    pub video: Vec<VideoProfile>,
    pub audio: Vec<AudioProfile>,
}

// ── thread count ──────────────────────────────────────────────────────────────

fn thread_count() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(4)
        .min(16) // cap — diminishing returns above this for encode workloads
}

fn substitute_threads(s: &str, n: usize) -> String {
    s.replace("${N_THREADS}", &n.to_string())
}

// ── availability checks ───────────────────────────────────────────────────────

/// Check a pipeline string by attempting a dry-run parse.
/// Returns true if GStreamer can construct the bin.
fn pipeline_available(description: &str) -> bool {
    // Wrap in a bin — parse::bin_from_description expects a bin description.
    match gst::parse::bin_from_description(description, true) {
        Ok(bin) => {
            // Drop immediately — we only needed the parse result.
            drop(bin);
            true
        }
        Err(_) => false,
    }
}

/// Check VAAPI availability by looking for vah264enc in the registry.
/// Faster and more reliable than parsing the full pipeline when VAAPI is absent.
fn vaapi_available() -> bool {
    gst::Registry::get()
        .find_plugin("va")
        .is_some_and(|_| {
            gst::ElementFactory::find("vah264enc").is_some()
        })
}

// ── loader ────────────────────────────────────────────────────────────────────

static PROFILES: OnceCell<Profiles> = OnceCell::new();

/// Load and validate profiles. Called once at application startup after
/// GStreamer has been initialised. Subsequent calls return the cached result.
pub fn load() -> Result<&'static Profiles> {
    PROFILES.get_or_try_init(|| {
        let bytes = gio::resources_lookup_data(
            "/org/archerprojects/mediaSnare/profiles.toml",
            gio::ResourceLookupFlags::NONE,
        )
        .context("profiles.toml not found in GResource")?;

        let text = std::str::from_utf8(&bytes)
            .context("profiles.toml is not valid UTF-8")?;

        let raw: ProfilesFile = toml::from_str(text)
            .context("failed to parse profiles.toml")?;

        let threads = thread_count();
        let vaapi   = vaapi_available();

        let video: Vec<VideoProfile> = raw.video
            .into_iter()
            .filter_map(|p| {
                // VAAPI profiles: gate on element presence, not pipeline parse.
                if p.vaapi {
                    if !vaapi {
                        tracing::debug!(id = %p.id, "VAAPI profile unavailable — vah264enc not found");
                        return None;
                    }
                } else {
                    // Software profiles: confirm the encoder pipeline parses cleanly.
                    let enc = substitute_threads(&p.videoenc, threads);
                    if !pipeline_available(&enc) {
                        tracing::warn!(id = %p.id, "video profile unavailable — encoder pipeline failed");
                        return None;
                    }
                }

                Some(VideoProfile {
                    videoenc:  substitute_threads(&p.videoenc, threads),
                    audioenc:  substitute_threads(&p.audioenc, threads),
                    muxer:     p.muxer,
                    id:        p.id,
                    name:      p.name,
                    extension: p.extension,
                    vaapi:     p.vaapi,
                })
            })
            .collect();

        let audio: Vec<AudioProfile> = raw.audio
            .into_iter()
            .filter_map(|p| {
                let enc = substitute_threads(&p.audioenc, threads);
                if !pipeline_available(&enc) {
                    tracing::warn!(id = %p.id, "audio profile unavailable — encoder pipeline failed");
                    return None;
                }
                Some(AudioProfile {
                    audioenc:  substitute_threads(&p.audioenc, threads),
                    id:        p.id,
                    name:      p.name,
                    extension: p.extension,
                    muxer:     p.muxer,
                })
            })
            .collect();

        tracing::info!(
            video_count = video.len(),
            audio_count = audio.len(),
            "profiles loaded"
        );

        Ok(Profiles { video, audio })
    })
}

/// Convenience — get a video profile by id. Returns None if unavailable.
pub fn video(id: &str) -> Option<&'static VideoProfile> {
    load().ok()?.video.iter().find(|p| p.id == id)
}

/// Convenience — get an audio profile by id. Returns None if unavailable.
pub fn audio(id: &str) -> Option<&'static AudioProfile> {
    load().ok()?.audio.iter().find(|p| p.id == id)
}
