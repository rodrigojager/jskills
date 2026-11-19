pub mod identity;
pub mod registry;
use anyhow::{Context, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

/// A kernel-held lock is released only when this controller's handle retires.
/// It belongs to the actual checkout, independently of the selected data root.
pub struct ControllerGuard {
    _file: File,
}

impl ControllerGuard {
    pub fn acquire(git_dir: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(git_dir.join("jtasks-v2-controller.lock"))?;
        file.try_lock()
            .context("another foreground controller owns this checkout")?;
        Ok(Self { _file: file })
    }
}
