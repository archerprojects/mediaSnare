// portal/notify.rs — desktop notifications via org.freedesktop.Notifications
//
// GLib's GNotification selects a backend at startup and on Cinnamon/MATE it
// picks org.gtk.Notifications, which nothing implements there — notifications
// vanish silently with no warning. The freedesktop spec interface below is
// implemented by every notification daemon (Cinnamon, MATE, GNOME, KDE,
// dunst, …) and is display-server agnostic, so this works unchanged on
// X11 today and Wayland later.

use std::collections::HashMap;
use zbus::Connection;
use zvariant::Value;

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    #[allow(clippy::too_many_arguments)]
    fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<&str>,
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Send a notification. Best-effort — a missing daemon logs a warning and
/// never surfaces to the user.
pub async fn send(summary: &str, body: &str) {
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("notification skipped — no session bus: {e}");
            return;
        }
    };
    let proxy = match NotificationsProxy::new(&conn).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!("notification skipped — proxy failed: {e}");
            return;
        }
    };

    match proxy
        .notify(
            "mediaSnare",
            0,
            "mediasnare",      // installed app icon
            summary,
            body,
            Vec::new(),
            HashMap::new(),
            -1,                // daemon default timeout
        )
        .await
    {
        Ok(id) => tracing::info!("notification delivered, daemon id {id}"),
        Err(e) => tracing::warn!("notification failed: {e}"),
    }
}
