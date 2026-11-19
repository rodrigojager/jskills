//! Reuses the checkout's existing kernel lock; metadata is discovery only.
use crate::{resources::ControllerGuard, storage::Workspace};
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{
    fs,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

#[derive(Clone, Copy)]
pub struct Shutdown {
    pub grace: Duration,
    pub force: Duration,
}
impl Shutdown {
    pub fn configured() -> Result<Self> {
        fn value(name: &str, default: u64, minimum: u64) -> Result<Duration> {
            let n = match std::env::var(name) {
                Ok(v) => v
                    .parse::<u64>()
                    .with_context(|| format!("invalid {name}"))?,
                Err(std::env::VarError::NotPresent) => default,
                Err(e) => return Err(e.into()),
            };
            ensure!(
                (minimum..=60000).contains(&n),
                "{name} is outside the supported shutdown bound"
            );
            Ok(Duration::from_millis(n))
        }
        Ok(Self {
            grace: value("JTASKS_SHUTDOWN_GRACE_MS", 10000, 0)?,
            force: value("JTASKS_SHUTDOWN_FORCE_MS", 5000, 1)?,
        })
    }
}

pub struct Owner {
    guard: ControllerGuard,
    path: std::path::PathBuf,
    record: serde_json::Value,
}
impl Owner {
    pub fn acquire(workspace: &Workspace, run: &str) -> Result<Self> {
        let path = workspace.git_dir.join("jtasks-v2-controller.json");
        let guard = ControllerGuard::acquire(&workspace.git_dir).with_context(|| {
            let record = fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
            format!(
                "another foreground controller owns this checkout; discovery={}",
                record.unwrap_or(json!({"state":"unavailable"}))
            )
        })?;
        Shutdown::configured()?;
        install_signal_handler()?;
        let owner = Self {
            guard,
            path,
            record: json!({"instance":uuid::Uuid::new_v4(),"pid":std::process::id(),"start":crate::execution::platform::current_identity()?,"run":run,"endpoint":null}),
        };
        owner.write()?;
        Ok(owner)
    }
    pub fn guard(&self) -> &ControllerGuard {
        &self.guard
    }
    pub fn endpoint(&mut self, endpoint: impl ToString) -> Result<()> {
        self.record["endpoint"] = json!(endpoint.to_string());
        self.write()
    }
    fn write(&self) -> Result<()> {
        let mut f = fs::File::create(&self.path)?;
        f.write_all(&serde_json::to_vec(&self.record)?)?;
        f.sync_all()?;
        Ok(())
    }
}
#[cfg(windows)]
fn install_signal_handler() -> Result<()> {
    unsafe extern "system" fn handler(event: u32) -> i32 {
        use windows_sys::Win32::System::Console::*;
        if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT | CTRL_CLOSE_EVENT) {
            INTERRUPTED.store(true, Ordering::Relaxed);
            1
        } else {
            0
        }
    }
    INTERRUPTED.store(false, Ordering::Relaxed);
    // New process groups can inherit the console's ignore-Ctrl+C flag; explicit
    // foreground ownership opts back in before registering its scoped handler.
    unsafe {
        windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 0);
    }
    ensure!(
        unsafe { windows_sys::Win32::System::Console::SetConsoleCtrlHandler(Some(handler), 1) }
            != 0,
        "could not install controller interrupt handler"
    );
    Ok(())
}
#[cfg(target_os = "linux")]
fn install_signal_handler() -> Result<()> {
    extern "C" fn handler(_: i32) {
        INTERRUPTED.store(true, Ordering::Relaxed);
    }
    INTERRUPTED.store(false, Ordering::Relaxed);
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler as *const () as usize;
        libc::sigemptyset(&mut action.sa_mask);
        ensure!(
            libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) == 0
                && libc::sigaction(libc::SIGTERM, &action, std::ptr::null_mut()) == 0,
            "could not install controller interrupt handler"
        );
    }
    Ok(())
}
