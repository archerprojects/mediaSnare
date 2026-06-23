pub mod audio;
pub mod pipeline;
pub mod profile;
pub mod recording;
pub mod screenshot;

/// Marker error: the user cancelled the capture (Esc in a picker, Cancel in
/// a portal dialog). Dispatch recognises it via downcast and quietly re-arms
/// the UI — a cancel is never shown as a failure.
#[derive(Debug)]
pub struct Cancelled;

impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "capture cancelled by user")
    }
}

impl std::error::Error for Cancelled {}
