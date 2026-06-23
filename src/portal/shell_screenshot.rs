// portal/shell_screenshot.rs — direct D-Bus capture via Cinnamon
//
// Cinnamon exposes org.gnome.Shell.Screenshot on the bus name org.Cinnamon.
// Calling it directly gives us window capture and direct-to-path area capture
// that xdg-desktop-portal-gtk lacks, and the shell writes the capture straight
// to the path we pass — no document-portal FUSE mount, no URI decoding.
//
// Region selection is handled in-app by window::region_selector (a Flameshot-
// style adjustable overlay); the coordinates it produces are passed to
// ScreenshotArea via capture_region(). Cinnamon's own SelectArea is no longer
// used.
//
// SIGNATURES ARE VERSION-SPECIFIC. This module targets the legacy (flash-era)
// API verified by introspection on the deployment target:
//   Screenshot(include_frame b, flash b, filename s) → (success b, used s)
//   ScreenshotArea(x i, y i, w i, h i, flash b, filename s) → (b, s)
//   ScreenshotWindow(include_frame b, include_cursor b, flash b, filename s) → (b, s)
// Cinnamon 6.6+ changed these arities (flash removed). The caller
// (capture::screenshot) treats any non-cancel failure here as "fall back to
// the portal path", so an API change degrades gracefully instead of breaking
// capture.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use zbus::Connection;

use crate::window::Scope;

const BUS_NAME: &str = "org.Cinnamon";
const OBJ_PATH: &str = "/org/gnome/Shell/Screenshot";
const INTERFACE: &str = "org.gnome.Shell.Screenshot";

/// True when a Cinnamon shell owns org.Cinnamon on the session bus.
pub async fn is_available() -> bool {
    let Ok(conn) = Connection::session().await else { return false };
    let Ok(dbus) = zbus::fdo::DBusProxy::new(&conn).await else { return false };
    let Ok(name) = zbus::names::BusName::try_from(BUS_NAME) else { return false };
    dbus.name_has_owner(name).await.unwrap_or(false)
}

/// Capture via the shell. Writes to `output` (the shell may adjust the
/// name — the actually-used path is returned). Esc in the region selector
/// returns the Cancelled marker, never a user-facing error.
pub async fn capture(scope: Scope, include_cursor: bool, output: &Path) -> Result<PathBuf> {
    let conn = Connection::session().await
        .context("failed to connect to D-Bus session bus")?;

    let filename = output.to_str()
        .ok_or_else(|| anyhow!("output path is not valid UTF-8"))?;

    let (success, used): (bool, String) = match scope {
        Scope::Full => {
            // include_frame=false, flash=true
            call(&conn, "Screenshot", &(false, true, filename)).await?
        }
        Scope::Window => {
            // Captures the focused window. Our window is hidden before
            // dispatch, so focus has returned to the user's working window.
            // include_frame=true (decorations), cursor per setting, flash=true
            call(&conn, "ScreenshotWindow", &(true, include_cursor, true, filename)).await?
        }
        Scope::Region => {
            // Region is captured by capture_region() with explicit coordinates
            // from the in-app selector — capture() is never called for it.
            bail!("internal: region scope uses capture_region()");
        }
    };

    if !success {
        bail!("Cinnamon screenshot reported failure");
    }

    Ok(PathBuf::from(used))
}

/// Capture an explicit screen rectangle via the shell's ScreenshotArea.
/// Coordinates are absolute screen pixels (from the in-app region selector).
/// The shell writes PNG to `output`; the actually-used path is returned.
pub async fn capture_region(x: i32, y: i32, w: i32, h: i32, output: &Path) -> Result<PathBuf> {
    let conn = Connection::session().await
        .context("failed to connect to D-Bus session bus")?;

    let filename = output.to_str()
        .ok_or_else(|| anyhow!("output path is not valid UTF-8"))?;

    // flash=true
    let (success, used): (bool, String) =
        call(&conn, "ScreenshotArea", &(x, y, w, h, true, filename)).await?;

    if !success {
        bail!("Cinnamon ScreenshotArea reported failure");
    }

    Ok(PathBuf::from(used))
}

async fn call<B, R>(conn: &Connection, method: &str, body: &B) -> Result<R>
where
    B: zvariant::DynamicType + serde::Serialize,
    R: for<'d> zvariant::DynamicDeserialize<'d>,
{
    let reply = conn
        .call_method(Some(BUS_NAME), OBJ_PATH, Some(INTERFACE), method, body)
        .await
        .with_context(|| format!("shell screenshot call {method} failed"))?;
    reply.body().deserialize()
        .with_context(|| format!("unexpected reply shape from {method}"))
}
