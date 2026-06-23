// capture/screenshot.rs — single-frame image capture
//
// Capture order:
//   1. Cinnamon shell D-Bus (org.Cinnamon) — native selector, direct file
//      write, all scopes. Non-cancel failures fall through to the portal so
//      a future Cinnamon API change degrades instead of breaking.
//   2. xdg-desktop-portal Screenshot (other desktops, Wayland)
//   3. ximagesrc — only when nothing else is available
//
// Scope:
//   Full   → non-interactive portal grab
//   Region → interactive portal picker
//   Window → interactive portal picker
//
// Window hide/show is handled by the caller (main_window.rs) on the main
// thread before and after dispatch — GTK objects are not Send.

use anyhow::{anyhow, bail, Context, Result};
use gst::prelude::*;
use gst_app;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::portal;
use crate::settings::Settings;
use crate::window::Scope;

pub async fn run(scope: Scope) -> Result<PathBuf> {
    // Read settings before any await — gio::Settings is not Send.
    let (format, include_cursor) = {
        let s = Settings::get();
        (s.image_format(), s.capture_cursor())
    };

    if portal::shell_screenshot::is_available().await {
        match run_shell(scope, &format, include_cursor).await {
            Ok(path) => return Ok(path),
            Err(e) if e.downcast_ref::<crate::capture::Cancelled>().is_some() => {
                return Err(e);
            }
            Err(e) => {
                tracing::warn!("shell screenshot failed — falling back to portal: {e:#}");
            }
        }
    }

    if portal::screenshot::is_available().await {
        run_portal(scope, &format).await
    } else {
        tracing::warn!("xdg-desktop-portal unavailable — falling back to ximagesrc");
        run_x11_fallback(&format).await
    }
}

// ── Region capture (explicit coordinates) ─────────────────────────────────────

/// Capture an explicit screen rectangle (Region scope). Coordinates come from
/// the in-app region selector and are absolute screen pixels. Shell path first
/// (Cinnamon ScreenshotArea writes the file directly), ximagesrc crop as the
/// fallback on other desktops. No interactive portal step — the selection has
/// already been made in-app.
pub async fn run_area(x: i32, y: i32, w: i32, h: i32) -> Result<PathBuf> {
    let format = Settings::get().image_format();

    if portal::shell_screenshot::is_available().await {
        match run_shell_area(x, y, w, h, &format).await {
            Ok(path) => return Ok(path),
            Err(e) => {
                tracing::warn!("shell area capture failed — falling back to ximagesrc: {e:#}");
            }
        }
    }

    tracing::warn!("capturing region via ximagesrc fallback");
    run_x11_area_fallback(x, y, w, h, &format).await
}

async fn run_shell_area(x: i32, y: i32, w: i32, h: i32, format: &str) -> Result<PathBuf> {
    let output = build_output_path(format)?;

    if format == "png" {
        let used = portal::shell_screenshot::capture_region(x, y, w, h, &output).await?;
        finalize(&used, &output)?;
    } else {
        let tmp = std::env::temp_dir()
            .join(format!("mediasnare-shell-area-{}.png", std::process::id()));
        let used = portal::shell_screenshot::capture_region(x, y, w, h, &tmp).await?;
        re_encode(&used, &output, format)?;
        let _ = std::fs::remove_file(&used);
    }

    if output.exists() {
        Ok(output)
    } else {
        bail!("Capture file not found after save: {}", output.display())
    }
}

async fn run_x11_area_fallback(x: i32, y: i32, w: i32, h: i32, format: &str) -> Result<PathBuf> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    let format = format.to_owned();
    tokio::task::spawn_blocking(move || capture_x11_area_sync(x, y, w, h, &format))
        .await
        .map_err(|e| anyhow!("X11 area capture task panicked: {e}"))?
}

fn capture_x11_area_sync(x: i32, y: i32, w: i32, h: i32, format: &str) -> Result<PathBuf> {
    // ximagesrc startx/starty/endx/endy grab a sub-rectangle; endx/endy are
    // inclusive pixel coordinates.
    let endx = x + w - 1;
    let endy = y + h - 1;
    let desc = format!(
        "ximagesrc num-buffers=1 use-damage=false \
         startx={x} starty={y} endx={endx} endy={endy} ! \
         videoconvert ! video/x-raw,format=RGBA ! \
         appsink name=sink emit-signals=false max-buffers=1 drop=false sync=false"
    );

    let pipeline = gst::parse::launch(&desc)
        .context("Failed to build ximagesrc area pipeline")?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("pipeline parse did not return Pipeline"))?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| anyhow!("appsink not found"))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| anyhow!("sink is not AppSink"))?;

    pipeline.set_state(gst::State::Playing)
        .context("Failed to start ximagesrc area pipeline")?;

    let sample = appsink.pull_sample()
        .map_err(|_| anyhow!("Failed to pull frame from ximagesrc"))?;

    pipeline.set_state(gst::State::Null)?;

    let buffer = sample.buffer()
        .ok_or_else(|| anyhow!("Sample has no buffer"))?;
    let caps = sample.caps()
        .ok_or_else(|| anyhow!("Sample has no caps"))?;
    let structure = caps.structure(0)
        .ok_or_else(|| anyhow!("Caps have no structure"))?;

    let width:  i32 = structure.get("width")?;
    let height: i32 = structure.get("height")?;
    let map = buffer.map_readable()
        .map_err(|_| anyhow!("Failed to map buffer"))?;

    let output = build_output_path(format)?;
    encode_rgba(map.as_slice(), width as u32, height as u32, &output, format)?;

    Ok(output)
}

// ── Cinnamon shell path ───────────────────────────────────────────────────────

async fn run_shell(scope: Scope, format: &str, include_cursor: bool) -> Result<PathBuf> {
    let output = build_output_path(format)?;

    if format == "png" {
        // The shell writes the file itself — hand it the final path.
        let used = portal::shell_screenshot::capture(scope, include_cursor, &output).await?;
        finalize(&used, &output)?;
    } else {
        // Shell always writes PNG — capture to a temp path, then re-encode.
        let tmp = std::env::temp_dir()
            .join(format!("mediasnare-shell-{}.png", std::process::id()));
        let used = portal::shell_screenshot::capture(scope, include_cursor, &tmp).await?;
        re_encode(&used, &output, format)?;
        let _ = std::fs::remove_file(&used);
    }

    if output.exists() {
        Ok(output)
    } else {
        bail!("Capture file not found after save: {}", output.display())
    }
}

/// Move src to dst if they differ (shell may adjust the filename it used).
/// rename first; read+write across filesystem boundaries.
fn finalize(src: &Path, dst: &Path) -> Result<()> {
    if src == dst {
        return Ok(());
    }
    if std::fs::rename(src, dst).is_err() {
        let bytes = std::fs::read(src)
            .with_context(|| format!("Failed to read capture at {}", src.display()))?;
        std::fs::write(dst, bytes)
            .with_context(|| format!("Failed to save capture to {}", dst.display()))?;
        let _ = std::fs::remove_file(src);
    }
    Ok(())
}

// ── Portal path ───────────────────────────────────────────────────────────────

async fn run_portal(scope: Scope, format: &str) -> Result<PathBuf> {
    let interactive = scope != Scope::Full;

    let png_path = portal::screenshot::request(interactive).await
        .context("Screenshot portal request failed")?;

    let output = build_output_path(format)?;

    if format == "png" {
        // rename() works on the same filesystem. The portal writes to the
        // document-portal FUSE mount (/run/user/N/doc/) — rename across that
        // boundary fails with EXDEV. fs::copy is deliberately NOT used as the
        // fallback: its trailing permissions-copy step fails on the FUSE mount
        // after the bytes have already landed, surfacing a phantom error for a
        // capture that succeeded. read + write avoids metadata operations.
        if std::fs::rename(&png_path, &output).is_err() {
            let bytes = std::fs::read(&png_path)
                .context("Failed to read portal capture")?;
            std::fs::write(&output, bytes)
                .with_context(|| format!("Failed to save capture to {}", output.display()))?;
        }
    } else {
        re_encode(&png_path, &output, format)?;
    }

    // Cleanup of the portal temp file is best-effort — never let it surface
    // as a capture error.
    let _ = std::fs::remove_file(&png_path);

    // Final guard: the file on disk is the only truth that matters.
    if output.exists() {
        Ok(output)
    } else {
        bail!("Capture file not found after save: {}", output.display())
    }
}

// ── ximagesrc fallback ────────────────────────────────────────────────────────

async fn run_x11_fallback(format: &str) -> Result<PathBuf> {
    tokio::time::sleep(Duration::from_millis(300)).await;
    let format = format.to_owned();
    tokio::task::spawn_blocking(move || capture_x11_sync(&format))
        .await
        .map_err(|e| anyhow!("X11 capture task panicked: {e}"))?
}

fn capture_x11_sync(format: &str) -> Result<PathBuf> {
    let pipeline = gst::parse::launch(
        "ximagesrc num-buffers=1 use-damage=false ! \
         videoconvert ! \
         video/x-raw,format=RGBA ! \
         appsink name=sink emit-signals=false max-buffers=1 drop=false sync=false"
    )
    .context("Failed to build ximagesrc pipeline")?
    .downcast::<gst::Pipeline>()
    .map_err(|_| anyhow!("pipeline parse did not return Pipeline"))?;

    let appsink = pipeline
        .by_name("sink")
        .ok_or_else(|| anyhow!("appsink not found"))?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| anyhow!("sink is not AppSink"))?;

    pipeline.set_state(gst::State::Playing)
        .context("Failed to start ximagesrc pipeline")?;

    let sample = appsink.pull_sample()
        .map_err(|_| anyhow!("Failed to pull frame from ximagesrc"))?;

    pipeline.set_state(gst::State::Null)?;

    let buffer = sample.buffer()
        .ok_or_else(|| anyhow!("Sample has no buffer"))?;
    let caps = sample.caps()
        .ok_or_else(|| anyhow!("Sample has no caps"))?;
    let structure = caps.structure(0)
        .ok_or_else(|| anyhow!("Caps have no structure"))?;

    let width:  i32 = structure.get("width")?;
    let height: i32 = structure.get("height")?;
    let map = buffer.map_readable()
        .map_err(|_| anyhow!("Failed to map buffer"))?;

    let output = build_output_path(format)?;
    encode_rgba(map.as_slice(), width as u32, height as u32, &output, format)?;

    Ok(output)
}

// ── Encode helpers ────────────────────────────────────────────────────────────

fn re_encode(src: &Path, dst: &Path, format: &str) -> Result<()> {
    let img  = image::open(src).context("Failed to open portal PNG")?;
    let rgba = img.to_rgba8();
    encode_rgba(rgba.as_raw(), rgba.width(), rgba.height(), dst, format)
}

fn encode_rgba(data: &[u8], width: u32, height: u32, dst: &Path, format: &str) -> Result<()> {
    use image::ImageBuffer;

    let buf: image::RgbaImage = ImageBuffer::from_raw(width, height, data.to_vec())
        .ok_or_else(|| anyhow!("Failed to create image buffer"))?;

    match format {
        "jpg" | "jpeg" => {
            image::DynamicImage::ImageRgba8(buf)
                .to_rgb8()
                .save_with_format(dst, image::ImageFormat::Jpeg)
                .context("Failed to save JPEG")?;
        }
        "webp" => {
            buf.save_with_format(dst, image::ImageFormat::WebP)
                .context("Failed to save WebP")?;
        }
        _ => {
            buf.save_with_format(dst, image::ImageFormat::Png)
                .context("Failed to save PNG")?;
        }
    }
    Ok(())
}

fn build_output_path(format: &str) -> Result<PathBuf> {
    let dir = Settings::get().image_directory().unwrap_or_else(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join("Pictures")
    });

    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create save directory: {}", dir.display()))?;

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let ext = if format == "jpg" { "jpg" } else { format };
    Ok(dir.join(format!("mediasnare-{ts}.{ext}")))
}
