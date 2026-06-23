// portal/screencast.rs — xdg-desktop-portal ScreenCast session

use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::os::unix::io::OwnedFd;
use zbus::{proxy, Connection, MessageStream};
use zvariant::{OwnedObjectPath, OwnedValue, Value};

use super::types::{CursorMode, PersistMode, PortalResponse, SourceType, Stream};
use crate::settings::Settings;

const SENDER_TOKEN_PREFIX: &str = "mediasnare";

#[proxy(
    interface = "org.freedesktop.portal.ScreenCast",
    default_service = "org.freedesktop.portal.Desktop",
    default_path = "/org/freedesktop/portal/desktop"
)]
trait ScreenCast {
    fn create_session(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<OwnedObjectPath>;
    fn select_sources(&self, session_handle: &zbus::zvariant::ObjectPath<'_>, options: HashMap<&str, Value<'_>>) -> zbus::Result<OwnedObjectPath>;
    fn start(&self, session_handle: &zbus::zvariant::ObjectPath<'_>, parent_window: &str, options: HashMap<&str, Value<'_>>) -> zbus::Result<OwnedObjectPath>;
    fn open_pipe_wire_remote(&self, session_handle: &zbus::zvariant::ObjectPath<'_>, options: HashMap<&str, Value<'_>>) -> zbus::Result<zbus::zvariant::OwnedFd>;
}

#[proxy(
    interface = "org.freedesktop.portal.Session",
    default_service = "org.freedesktop.portal.Desktop"
)]
trait Session {
    fn close(&self) -> zbus::Result<()>;
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

pub struct ScreencastSession {
    conn:           Connection,
    session_handle: OwnedObjectPath,
}

impl ScreencastSession {
    pub async fn new() -> Result<Self> {
        let conn  = Connection::session().await.context("failed to connect to D-Bus session bus")?;
        let proxy = ScreenCastProxy::new(&conn).await.context("failed to create ScreenCast proxy")?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        let handle_token = format!("{SENDER_TOKEN_PREFIX}_{}", std::process::id());
        options.insert("handle_token",         Value::from(handle_token.as_str()));
        options.insert("session_handle_token", Value::from(handle_token.as_str()));

        let request_path = proxy.create_session(options).await
            .context("CreateSession D-Bus call failed")?;

        let results = await_response(&conn, request_path).await
            .context("CreateSession portal response failed")?;

        let session_handle = results
            .get("session_handle")
            .and_then(|v| v.downcast_ref::<zvariant::ObjectPath<'_>>().ok())
            .map(|p| OwnedObjectPath::try_from(p.as_str()).unwrap())
            .ok_or_else(|| anyhow!("portal response missing session_handle"))?;

        Ok(Self { conn, session_handle })
    }

    pub async fn select_sources(
        &self,
        source_type: SourceType,
        cursor_mode: CursorMode,
        persist_mode: PersistMode,
        restore_token: Option<&str>,
    ) -> Result<()> {
        let proxy = ScreenCastProxy::new(&self.conn).await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        let handle_token = format!("{SENDER_TOKEN_PREFIX}_sel_{}", std::process::id());
        options.insert("handle_token",  Value::from(handle_token.as_str()));
        options.insert("types",         Value::from(source_type as u32));
        options.insert("multiple",      Value::from(true));
        options.insert("cursor_mode",   Value::from(cursor_mode as u32));
        options.insert("persist_mode",  Value::from(persist_mode as u32));

        if let Some(token) = restore_token {
            options.insert("restore_token", Value::from(token));
        }

        let request_path = proxy
            .select_sources(&*self.session_handle, options).await
            .context("SelectSources D-Bus call failed")?;

        await_response(&self.conn, request_path).await
            .context("SelectSources portal response failed")?;

        Ok(())
    }

    pub async fn start(&self) -> Result<(Vec<Stream>, Option<String>)> {
        let proxy = ScreenCastProxy::new(&self.conn).await?;

        let mut options: HashMap<&str, Value<'_>> = HashMap::new();
        let handle_token = format!("{SENDER_TOKEN_PREFIX}_start_{}", std::process::id());
        options.insert("handle_token", Value::from(handle_token.as_str()));

        let request_path = proxy
            .start(&*self.session_handle, "", options).await
            .context("Start D-Bus call failed")?;

        let results = await_response(&self.conn, request_path).await
            .context("Start portal response failed")?;

        let streams = parse_streams(&results)?;

        let restore_token = results
            .get("restore_token")
            .and_then(|v| v.downcast_ref::<zvariant::Str<'_>>().ok())
            .map(|s| s.as_str().to_owned());

        if let Some(ref token) = restore_token {
            Settings::get().set_portal_token(token);
        }

        Ok((streams, restore_token))
    }

    pub async fn open_pipewire_remote(&self) -> Result<OwnedFd> {
        let proxy = ScreenCastProxy::new(&self.conn).await?;

        let zfd = proxy
            .open_pipe_wire_remote(&*self.session_handle, HashMap::new()).await
            .context("OpenPipeWireRemote D-Bus call failed")?;

        Ok(zfd.into())
    }

    #[allow(dead_code)]
    pub async fn close(self) -> Result<()> {
        let proxy = SessionProxy::builder(&self.conn)
            .path(self.session_handle.as_ref())?
            .build()
            .await?;

        proxy.close().await.context("Session Close D-Bus call failed")?;
        Ok(())
    }
}

fn parse_streams(results: &HashMap<String, OwnedValue>) -> Result<Vec<Stream>> {
    let raw = results
        .get("streams")
        .ok_or_else(|| anyhow!("portal response missing streams"))?;

    let array = match raw.downcast_ref::<zvariant::Array>() {
        Ok(a) => a,
        Err(_) => return Ok(vec![]),
    };

    let mut streams = Vec::new();

    for item in array.iter() {
        let s = match item.downcast_ref::<zvariant::Structure>() {
            Ok(s) => s,
            Err(_) => continue,
        };

        let fields = s.fields();
        if fields.len() < 2 { continue; }

        let node_id: u32 = match fields[0].downcast_ref::<u32>() {
            Ok(n) => n,
            Err(_) => continue,
        };

        let props = match fields[1].downcast_ref::<zvariant::Dict>() {
            Ok(d) => d,
            Err(_) => {
                streams.push(Stream {
                    node_id,
                    position: (0, 0),
                    size: (0, 0),
                    source_type: SourceType::Monitor,
                });
                continue;
            }
        };

        let position = get_pair_prop(&props, "position").unwrap_or((0, 0));
        let size     = get_pair_prop(&props, "size").unwrap_or((0, 0));

        let src_key = "source_type".to_string();
        let source_type = props
            .get::<String, zvariant::Value>(&src_key)
            .ok()
            .flatten()
            .and_then(|v| v.downcast_ref::<u32>().ok().map(|n| n))
            .map(|t| match t {
                1 => SourceType::Monitor,
                2 => SourceType::Window,
                _ => SourceType::Virtual,
            })
            .unwrap_or(SourceType::Monitor);

        streams.push(Stream { node_id, position, size, source_type });
    }

    Ok(streams)
}

fn get_pair_prop(dict: &zvariant::Dict, key: &str) -> Option<(i32, i32)> {
    let key_owned = key.to_string();
    let val = dict.get::<String, zvariant::Value>(&key_owned).ok().flatten()?;
    let s   = val.downcast_ref::<zvariant::Structure>().ok()?;
    let f   = s.fields();
    if f.len() < 2 { return None; }
    let x = f[0].downcast_ref::<i32>().ok()?;
    let y = f[1].downcast_ref::<i32>().ok()?;
    Some((x, y))
}

pub async fn is_available() -> bool {
    match Connection::session().await {
        Ok(conn) => ScreenCastProxy::new(&conn).await.is_ok(),
        Err(_)   => false,
    }
}
