use super::platform;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub platform: String,
    pub scope: String,
    pub pid: Option<u32>,
    pub start: Option<String>,
    pub bound: bool,
    pub empty: bool,
}

/// The record is durable before spawn, and binding is durable before Windows
/// resumes its suspended child. An incomplete/torn record always means unknown.
fn write(path: &Path, record: &Record) -> Result<()> {
    let mut file = fs::File::create(path)?;
    file.write_all(&serde_json::to_vec(record)?)?;
    file.sync_all()?;
    Ok(())
}

pub struct OwnedProcess {
    tree: platform::Tree,
    record: Record,
    path: PathBuf,
    stop_at: Option<Instant>,
    force_at: Option<Instant>,
    limits: crate::controller::lifetime::Shutdown,
}
impl OwnedProcess {
    pub fn spawn(command: &mut Command, directory: &Path) -> Result<Self> {
        fs::create_dir_all(directory)?;
        let path = directory.join("custody.json");
        let mut record = Record {
            platform: std::env::consts::OS.into(),
            scope: format!("jtasks-{}", uuid::Uuid::new_v4()),
            pid: None,
            start: None,
            bound: false,
            empty: false,
        };
        write(&path, &record)?;
        let limits = crate::controller::lifetime::Shutdown::configured()?;
        let scope = record.scope.clone();
        let tree = platform::Tree::spawn(command, &scope, |pid, start| {
            record.pid = Some(pid);
            record.start = Some(start);
            record.bound = true;
            write(&path, &record).map_err(std::io::Error::other)
        });
        let tree = match tree {
            Ok(tree) => tree,
            Err(error) => {
                record.empty = error.empty;
                write(&path, &record)?;
                return Err(error.into());
            }
        };
        Ok(Self {
            tree,
            record,
            path,
            stop_at: None,
            force_at: None,
            limits,
        })
    }
    pub fn id(&self) -> u32 {
        self.record.pid.unwrap()
    }
    pub fn request_stop(&mut self) -> Result<()> {
        if self.stop_at.is_none() {
            self.tree.graceful()?;
            self.stop_at = Some(Instant::now());
        }
        Ok(())
    }
    /// A leader exit alone is never retirement. Force any remaining descendants
    /// and observe an empty scope before returning its original exit code.
    pub fn poll(&mut self) -> Result<Option<Option<i32>>> {
        let exit = self.tree.leader_exit()?;
        let force = exit.is_some()
            || self
                .stop_at
                .is_some_and(|t| t.elapsed() >= self.limits.grace);
        if force && self.force_at.is_none() {
            self.tree.force()?;
            self.force_at = Some(Instant::now());
        }
        if exit.is_some() && self.tree.empty()? {
            self.tree.reap()?;
            self.record.empty = true;
            write(&self.path, &self.record)?;
            return Ok(exit);
        }
        ensure!(
            !self
                .force_at
                .is_some_and(|t| t.elapsed() >= self.limits.force),
            "owned process cleanup is unconfirmed; custody remains unknown"
        );
        Ok(None)
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        if !self.record.empty {
            // Unwinding is bounded too; no wait() on a possibly living leader.
            let _ = self.tree.force();
            let deadline = Instant::now() + self.limits.force;
            while Instant::now() < deadline {
                if self.poll().ok().flatten().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}

/// Explicit controller resume only. Never reconstruct ownership from a PID.
pub fn reconcile(directory: &Path) -> Result<bool> {
    let path = directory.join("custody.json");
    let Ok(bytes) = fs::read(&path) else {
        return Ok(false);
    };
    let Ok(mut record) = serde_json::from_slice::<Record>(&bytes) else {
        return Ok(false);
    };
    if record.platform != std::env::consts::OS
        || !record
            .scope
            .strip_prefix("jtasks-")
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
    {
        return Ok(false);
    }
    if record.empty {
        return Ok(true);
    }
    if !record.bound || record.pid.is_none() || record.start.is_none() {
        return Ok(false);
    }
    let limit = crate::controller::lifetime::Shutdown::configured()?.force;
    if platform::reconcile(&record, limit).context("owned scope reconciliation failed")? {
        record.empty = true;
        write(&path, &record)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
