// portal/screenshot.rs — xdg-desktop-portal Screenshot interface

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use relm4::gtk::glib;
use std::collections::HashMap;
use std::path::PathBuf;
use zbus::{Connection, MessageStream};
use zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::types::PortalResponse;

#[zbus::proxy(
    interface = "org.freedesktop.portal.Screenshot",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait Screenshot {
    fn screenshot(
        &self,
        parent_window: &str,
        options: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;
}

/// Check whether xdg-desktop-portal Screenshot is reachable on the session bus.
pub async fn is_available() -> bool {
    match Connection::session().await {
        Ok(conn) => ScreenshotProxy::new(&conn).await.is_ok(),
        Err(_)   => false,
    }
}

/// Request a screenshot via xdg-desktop-portal.
/// `interactive` — if true the portal shows a region/window picker.
/// Returns the path to the file written by the compositor.
pub async fn request(interactive: bool) -> Result<PathBuf> {
    let conn  = Connection::session().await.context("failed to connect to D-Bus session bus")?;
    let proxy = ScreenshotProxy::new(&conn).await.context("failed to create Screenshot proxy")?;

    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    let handle_token = format!("mediasnare_{}", std::process::id());
    options.insert("handle_token", Value::from(handle_token.as_str()));
    options.insert("interactive",  Value::from(interactive));

    let request_path = proxy.screenshot("", options).await
        .context("Screenshot D-Bus call failed")?;

    let results = await_response(&conn, request_path).await
        .context("Screenshot portal response failed")?;

    let uri = results
        .get("uri")
        .and_then(|v| v.downcast_ref::<zvariant::Str<'_>>().ok())
        .map(|s| s.as_str().to_owned())
        .ok_or_else(|| anyhow!("portal response missing uri"))?;

    // The portal returns a percent-encoded file URI — screenshot backends
    // name files with spaces ("Screenshot from ...png"), which arrive as %20.
    // Stripping the scheme prefix leaves literal %20 in the path → ENOENT on
    // every subsequent file operation. filename_from_uri decodes properly.
    let (path, _) = glib::filename_from_uri(&uri)
        .with_context(|| format!("invalid URI in portal response: {uri}"))?;

    Ok(path)
}

async fn await_response(
    conn: &Connection,
    request_path: OwnedObjectPath,
) -> Result<HashMap<String, OwnedValue>> {
    let mut stream = MessageStream::from(conn.clone());

    while let Some(msg) = stream.next().await {
        let msg = msg.context("D-Bus message error")?;

        if msg.header().path().map(|p| p.as_str()) != Some(request_path.as_str()) {
            continue;
        }
        if msg.header().member().map(|m| m.as_str()) != Some("Response") {
            continue;
        }

        let (response_code, results): (u32, HashMap<String, OwnedValue>) =
            msg.body().deserialize().context("failed to deserialize portal Response")?;

        match PortalResponse::from(response_code) {
            PortalResponse::Success       => return Ok(results),
            PortalResponse::UserCancelled => {
                return Err(anyhow::Error::new(crate::capture::Cancelled));
            }
            PortalResponse::Other         => bail!("portal returned error response"),
        }
    }

    bail!("D-Bus stream ended before portal response")
}
