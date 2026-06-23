#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingState {
    #[default]
    Idle,
    Ready,
    #[allow(dead_code)]
    Delayed,
    Recording,
    Paused,
    Flushing,
}
