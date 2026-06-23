// portal/types.rs — xdg-desktop-portal shared D-Bus types
//
// CursorMode, SourceType, PersistMode map to portal spec uint32 values.
// Stream carries per-monitor node info returned by Session::start().

use zvariant::Type;
use serde::{Deserialize, Serialize};

// ── Portal D-Bus types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[repr(u32)]
pub enum CursorMode {
    Hidden   = 1,
    Embedded = 2,
    Metadata = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[repr(u32)]
pub enum SourceType {
    Monitor = 1,
    Window  = 2,
    Virtual = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[repr(u32)]
pub enum PersistMode {
    DoNotPersist  = 0,
    TransientOnly = 1,
    Persistent    = 2,
}

/// A single PipeWire stream returned by Session::start().
#[derive(Debug, Clone)]
pub struct Stream {
    /// PipeWire node id — passed to pipewiresrc node-id property.
    pub node_id:     u32,
    /// Position on the logical screen (x, y).
    pub position:    (i32, i32),
    /// Dimensions in pixels.
    #[allow(dead_code)]
    pub size:        (i32, i32),
    #[allow(dead_code)]
    pub source_type: SourceType,
}

/// Response code from portal D-Bus calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalResponse {
    Success,
    UserCancelled,
    Other,
}

impl From<u32> for PortalResponse {
    fn from(v: u32) -> Self {
        match v {
            0 => Self::Success,
            1 => Self::UserCancelled,
            _ => Self::Other,
        }
    }
}
