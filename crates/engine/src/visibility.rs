//! Session-owned physical-controller hiding; no input/output algorithms here.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VisibilityStatus {
    pub requested: bool,
    pub active: bool,
    pub hidden_paths: Vec<String>,
    pub error: Option<String>,
}

pub mod policy;
#[cfg(windows)]
mod session;
#[cfg(windows)]
mod topology;
#[cfg(windows)]
pub use session::{acquire, heartbeat, preparing, recover, release, run_helper, verify_outputs};

/// Read-only inventory used to protect native ViGEm output created by this session.
#[cfg(windows)]
pub(crate) fn virtual_output_ids() -> Vec<String> {
    topology::virtual_ids(&topology::inventory())
}

#[cfg(not(windows))]
pub fn release() -> anyhow::Result<VisibilityStatus> {
    Ok(VisibilityStatus::default())
}
#[cfg(not(windows))]
pub fn heartbeat() -> anyhow::Result<Option<VisibilityStatus>> {
    Ok(None)
}
#[cfg(not(windows))]
pub fn preparing() -> anyhow::Result<()> {
    Ok(())
}
#[cfg(not(windows))]
pub fn recover() -> anyhow::Result<VisibilityStatus> {
    Ok(VisibilityStatus::default())
}
#[cfg(not(windows))]
pub fn acquire(_: &str) -> anyhow::Result<Option<VisibilityStatus>> {
    anyhow::bail!("自动隐藏需要 Windows 和 HidHide")
}
#[cfg(not(windows))]
pub fn verify_outputs(_: Vec<String>, _: bool) -> anyhow::Result<()> {
    anyhow::bail!("自动隐藏需要 Windows")
}
