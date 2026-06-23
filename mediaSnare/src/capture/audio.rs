// capture/audio.rs — standalone audio recording
//
// Pipeline: pulsesrc → audiorate → audioenc → filesink
//
// On PipeWire systems pulsesrc routes correctly to PipeWire sources.
// No portal session required — PipeWire audio is accessible without
// the screencast portal.

use anyhow::{anyhow, Context, Result};
use gst::prelude::*;
use std::path::{Path, PathBuf};

use crate::capture::profile;
use crate::capture::pipeline::{chrono_stamp, dirs_next, PipelineHandle};
use crate::settings::Settings;

fn build_pipeline(
    audio_source: &str,
    prof: &profile::AudioProfile,
    output: &Path,
) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new();

    let source_el: gst::Element = match audio_source {
        "both" => {
            // Mix desktop monitor + mic via audiomixer
            let bin       = gst::Bin::new();
            let desktop   = make_pulsesrc(true)?;
            let mic       = make_pulsesrc(false)?;
            let rate_d    = gst::ElementFactory::make("audiorate").build()?;
            let rate_m    = gst::ElementFactory::make("audiorate").build()?;
            let mixer     = gst::ElementFactory::make("audiomixer").build()
                .context("audiomixer unavailable")?;

            bin.add_many([&desktop, &mic, &rate_d, &rate_m, &mixer])?;
            desktop.link(&rate_d)?;
            mic.link(&rate_m)?;
            rate_d.link(&mixer)?;
            rate_m.link(&mixer)?;

            let src_pad = mixer.static_pad("src")
                .ok_or_else(|| anyhow!("audiomixer has no src pad"))?;
            let ghost = gst::GhostPad::with_target(&src_pad)?;
            bin.add_pad(&ghost)?;

            bin.upcast()
        }
        source => {
            let monitor = source == "desktop";
            let src  = make_pulsesrc(monitor)?;
            let rate = gst::ElementFactory::make("audiorate").build()?;

            let bin = gst::Bin::new();
            bin.add_many([&src, &rate])?;
            src.link(&rate)?;

            let src_pad = rate.static_pad("src")
                .ok_or_else(|| anyhow!("audiorate has no src pad"))?;
            let ghost = gst::GhostPad::with_target(&src_pad)?;
            bin.add_pad(&ghost)?;

            bin.upcast()
        }
    };

    let audioenc = gst::parse::bin_from_description(&prof.audioenc, true)
        .context("Failed to parse audioenc")?;

    // Muxer must be a bare element — not bin-wrapped — because muxer sink
    // pads are request pads. bin_from_description ghost-pads only static pads,
    // so a bin-wrapped muxer exposes no sink pad and every link to it fails.
    // Same pattern as pipeline.rs video muxer.
    let muxer: Option<gst::Element> = if prof.muxer.is_empty() {
        None
    } else {
        let mut parts = prof.muxer.split_whitespace();
        let factory = parts.next()
            .ok_or_else(|| anyhow!("empty muxer description"))?;
        let el = gst::ElementFactory::make(factory)
            .build()
            .with_context(|| format!("muxer element '{factory}' unavailable"))?;
        for prop in parts {
            if let Some((k, v)) = prop.split_once('=') {
                el.set_property_from_str(k, v);
            }
        }
        Some(el)
    };

    let filesink = gst::ElementFactory::make("filesink")
        .property("location", output.to_str().unwrap_or("/tmp/audio.ogg"))
        .build()
        .context("filesink unavailable")?;

    pipeline.add(&source_el)?;
    pipeline.add(audioenc.upcast_ref::<gst::Element>())?;
    if let Some(ref mux) = muxer {
        pipeline.add(mux)?;
    }
    pipeline.add(&filesink)?;

    source_el.link(&audioenc)?;

    if let Some(ref mux) = muxer {
        audioenc.link(mux)?;
        mux.link(&filesink)?;
    } else {
        audioenc.link(&filesink)?;
    }

    Ok(pipeline)
}

fn make_pulsesrc(monitor: bool) -> Result<gst::Element> {
    let src = gst::ElementFactory::make("pulsesrc")
        .build()
        .context("pulsesrc unavailable — gstreamer1.0-plugins-good required")?;

    if monitor {
        // @DEFAULT_MONITOR@ is the PulseAudio well-known name for the default
        // sink's monitor source. pipewire-pulse honours it too.
        src.set_property("device", "@DEFAULT_MONITOR@");
    }
    // Mic: default input — no device property needed

    Ok(src)
}

fn build_output_path(extension: &str) -> PathBuf {
    let dir = Settings::get()
        .audio_directory()
        .unwrap_or_else(|| dirs_next("music"));
    let ts  = chrono_stamp();
    dir.join(format!("mediasnare-{ts}.{extension}"))
}

/// Public entry point with stop signal support.
pub async fn run_with_stop(
    audio_source: &str,
    profile_id: &str,
    pipeline_handle: PipelineHandle,
    stop_rx: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<std::path::PathBuf> {
    if audio_source == "none" {
        anyhow::bail!("Audio mode requires an audio source — select Desktop, Mic, or Both");
    }

    let prof = crate::capture::profile::audio(profile_id)
        .ok_or_else(|| anyhow::anyhow!("audio profile '{profile_id}' not available"))?;

    let output = build_output_path(&prof.extension);
    std::fs::create_dir_all(output.parent().unwrap_or(std::path::Path::new("/tmp")))?;

    let pipeline = build_pipeline(audio_source, prof, &output)
        .context("Failed to build audio pipeline")?;

    *pipeline_handle.lock().unwrap() = Some(pipeline.clone());

    pipeline.set_state(gst::State::Playing)
        .context("Failed to start audio pipeline")?;

    tokio::select! {
        _ = stop_rx => {
            pipeline.send_event(gst::event::Eos::new());
            crate::capture::pipeline::wait_eos(&pipeline).await?;
        }
        result = crate::capture::pipeline::wait_eos(&pipeline) => {
            result?;
        }
    }

    pipeline.set_state(gst::State::Null)?;
    *pipeline_handle.lock().unwrap() = None;
    Ok(output)
}
