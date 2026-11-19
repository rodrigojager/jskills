use super::super::custody::Record;
use std::{
    fs, io,
    os::unix::process::CommandExt,
    process::{Child, Command},
    time::Duration,
};

fn stat(pid: u32) -> io::Result<(char, i32, String)> {
    let text = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let (_, fields) = text
        .rsplit_once(") ")
        .ok_or_else(|| io::Error::other("invalid proc stat"))?;
    let fields: Vec<_> = fields.split_whitespace().collect();
    if fields.len() < 20 {
        return Err(io::Error::other("incomplete proc stat"));
    }
    Ok((
        fields[0].chars().next().unwrap(),
        fields[2].parse().map_err(io::Error::other)?,
        fields[19].into(),
    ))
}
pub fn current_identity() -> io::Result<String> {
    identity(std::process::id())
}
fn identity(pid: u32) -> io::Result<String> {
    Ok(format!(
        "{}:{}",
        fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim(),
        stat(pid)?.2
    ))
}

pub struct Tree {
    child: Child,
    group: i32,
    reaped: bool,
}
impl Tree {
    pub fn spawn(
        command: &mut Command,
        _scope: &str,
        bound: impl FnOnce(u32, String) -> io::Result<()>,
    ) -> Result<Self, super::SpawnError> {
        // The only pre-exec operation is async-signal-safe. Never use the host group.
        command.process_group(0);
        let child = command.spawn()?;
        let mut tree = Self {
            group: child.id() as i32,
            child,
            reaped: false,
        };
        if let Err(e) = identity(tree.child.id()).and_then(|start| bound(tree.child.id(), start)) {
            let _ = tree.force();
            let deadline = std::time::Instant::now()
                + crate::controller::lifetime::Shutdown::configured()
                    .map(|s| s.force)
                    .unwrap_or(Duration::from_secs(5));
            while std::time::Instant::now() < deadline {
                if tree.leader_exit().ok().flatten().is_some() && tree.empty().ok() == Some(true) {
                    let _ = tree.reap();
                    return Err(super::SpawnError {
                        error: e,
                        empty: true,
                    });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            return Err(super::SpawnError {
                error: e,
                empty: false,
            });
        }
        Ok(tree)
    }
    pub fn leader_exit(&mut self) -> io::Result<Option<Option<i32>>> {
        if self.reaped {
            return Err(io::Error::other("custody leader already reaped"));
        }
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            if libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            ) == -1
            {
                return Err(io::Error::last_os_error());
            }
            if info.si_pid() == 0 {
                Ok(None)
            } else {
                Ok(Some(if info.si_code == libc::CLD_EXITED {
                    Some(info.si_status())
                } else {
                    None
                }))
            }
        }
    }
    fn signal(&mut self, signal: i32) -> io::Result<()> {
        if self.reaped {
            return Ok(());
        }
        // WNOWAIT pins this child and thus its group identity even after leader exit.
        if unsafe { libc::kill(-self.group, signal) } == -1 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ESRCH) {
                return Err(e);
            }
        }
        Ok(())
    }
    pub fn graceful(&mut self) -> io::Result<()> {
        self.signal(libc::SIGTERM)
    }
    pub fn force(&mut self) -> io::Result<()> {
        self.signal(libc::SIGKILL)
    }
    pub fn empty(&mut self) -> io::Result<bool> {
        // Zombies cannot execute; keep the leader pinned through both snapshots.
        for _ in 0..2 {
            for entry in fs::read_dir("/proc")? {
                let entry = entry?;
                let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                    continue;
                };
                match stat(pid) {
                    Ok((state, group, _))
                        if group == self.group && state != 'Z' && state != 'X' =>
                    {
                        return Ok(false);
                    }
                    Ok(_) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(true)
    }
    pub fn reap(&mut self) -> io::Result<()> {
        self.child.wait()?;
        self.reaped = true;
        Ok(())
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.force();
        }
    }
}
pub fn reconcile(record: &Record, _limit: Duration) -> io::Result<bool> {
    // After controller loss the leader is no longer our waitable child. There is
    // no atomic identity-bound killpg: never signal a historically recorded PGID.
    // Only a proven absent group can be retired; otherwise preserve uncertainty.
    let group = record.pid.unwrap() as i32;
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        match stat(pid) {
            Ok((_, pg, _)) if pg == group => return Ok(false),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}
