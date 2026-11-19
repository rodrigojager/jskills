//! Owned process boundaries. Windows uses a private noninheritable Job; Linux
//! uses a pinned process-group leader. Deliberate Unix setsid escapes are outside
//! this boundary; reconciliation never signals an unpinned historical group.
#[derive(Debug)]
pub struct SpawnError {
    pub error: std::io::Error,
    pub empty: bool,
}
impl From<std::io::Error> for SpawnError {
    fn from(error: std::io::Error) -> Self {
        Self { error, empty: true }
    }
}
impl std::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for SpawnError {}
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(not(any(windows, target_os = "linux")))]
compile_error!("process custody currently supports Windows and Linux only");
