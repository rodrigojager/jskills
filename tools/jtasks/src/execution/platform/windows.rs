//! Suspended launch/primary-thread resume adapted from jlock process_tree.rs,
//! revision a6396f2fa588f026d7b7b8e745b09b13eb8005d9 (clean read-only source).
// MIT License, Copyright (c) 2026 Rodrigo Jäger
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
// of the Software, and to permit persons to whom the Software is furnished to do
// so, subject to the following conditions: The above copyright notice and this
// permission notice shall be included in all copies or substantial portions of
// the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
// EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
// EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
// OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
// ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use super::super::custody::Record;
use std::{
    io,
    mem::{size_of, zeroed},
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command},
    ptr::{null, null_mut},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn checked(value: i32) -> io::Result<()> {
    if value == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn started(handle: HANDLE) -> io::Result<String> {
    unsafe {
        let mut creation = zeroed();
        let mut exit = zeroed();
        let mut kernel = zeroed();
        let mut user = zeroed();
        checked(GetProcessTimes(
            handle,
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        ))?;
        Ok(format!(
            "{}:{}",
            creation.dwHighDateTime, creation.dwLowDateTime
        ))
    }
}
pub fn current_identity() -> io::Result<String> {
    started(unsafe { GetCurrentProcess() })
}

pub struct Tree {
    job: Handle,
    child: Child,
    reaped: bool,
}
impl Tree {
    pub fn spawn(
        command: &mut Command,
        scope: &str,
        bound: impl FnOnce(u32, String) -> io::Result<()>,
    ) -> Result<Self, super::SpawnError> {
        unsafe {
            let name = wide(scope);
            let job = CreateJobObjectW(null(), name.as_ptr());
            if job.is_null() {
                return Err(io::Error::last_os_error().into());
            }
            let job = Handle(job);
            if GetLastError() == ERROR_ALREADY_EXISTS {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "custody scope collision",
                )
                .into());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            checked(SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of_val(&limits) as u32,
            ))?;
            // The job handle is private and noninheritable. No breakaway is allowed.
            let mut child = command
                .creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP)
                .spawn()?;
            let setup = (|| {
                checked(AssignProcessToJobObject(
                    job.0,
                    child.as_raw_handle() as HANDLE,
                ))?;
                bound(child.id(), started(child.as_raw_handle() as HANDLE)?)?;
                resume(&child)
            })();
            if let Err(error) = setup {
                // Even a suspended child must not turn a failed kill into an
                // unbounded wait. Only a witnessed exit authorizes reaping.
                let _ = child.kill();
                let _ = TerminateJobObject(job.0, 1);
                let limit = crate::controller::lifetime::Shutdown::configured()
                    .map(|s| s.force)
                    .unwrap_or(Duration::from_secs(5));
                let deadline = Instant::now() + limit;
                let mut empty = false;
                while Instant::now() < deadline {
                    if matches!(child.try_wait(), Ok(Some(_))) && active(job.0).ok() == Some(0) {
                        empty = true;
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                return Err(super::SpawnError { error, empty });
            }
            Ok(Self {
                job,
                child,
                reaped: false,
            })
        }
    }
    pub fn leader_exit(&mut self) -> io::Result<Option<Option<i32>>> {
        Ok(self.child.try_wait()?.map(|s| s.code()))
    }
    pub fn graceful(&mut self) -> io::Result<()> {
        // Console-less commands may not support a graceful control event. The
        // private group id scopes the request; forced job shutdown remains bounded.
        if self.leader_exit()?.is_none() {
            unsafe {
                GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, self.child.id());
            }
        }
        Ok(())
    }
    pub fn force(&mut self) -> io::Result<()> {
        unsafe { checked(TerminateJobObject(self.job.0, 1)) }
    }
    pub fn empty(&mut self) -> io::Result<bool> {
        active(self.job.0).map(|n| n == 0)
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
            unsafe {
                TerminateJobObject(self.job.0, 1);
            }
        }
    }
}
fn active(job: HANDLE) -> io::Result<u32> {
    unsafe {
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
        checked(QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of_val(&info) as u32,
            null_mut(),
        ))?;
        Ok(info.ActiveProcesses)
    }
}
// The suspended primary thread cannot exit or recycle its identity before resume.
fn resume(child: &Child) -> io::Result<()> {
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = Handle(snapshot);
        let mut entry: THREADENTRY32 = zeroed();
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut more = Thread32First(snapshot.0, &mut entry);
        while more != 0 {
            if entry.th32OwnerProcessID == child.id() {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = Handle(thread);
                if ResumeThread(thread.0) == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            more = Thread32Next(snapshot.0, &mut entry);
        }
        Err(io::Error::other("suspended primary thread unavailable"))
    }
}
pub fn reconcile(record: &Record, limit: Duration) -> io::Result<bool> {
    // Only a durable binding receipt authorizes opening this random scope. A job
    // object remains named while it has members, even after its last handle closes.
    unsafe {
        let name = wide(&record.scope);
        // JOB_OBJECT_QUERY (0x4) and JOB_OBJECT_TERMINATE (0x8), winnt.h.
        let handle = OpenJobObjectW(0x0004 | 0x0008, 0, name.as_ptr());
        if handle.is_null() {
            return if GetLastError() == ERROR_FILE_NOT_FOUND {
                Ok(true)
            } else {
                Err(io::Error::last_os_error())
            };
        }
        let job = Handle(handle);
        let leader = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            record.pid.unwrap(),
        );
        if !leader.is_null() {
            let leader = Handle(leader);
            if started(leader.0)?.as_str() != record.start.as_deref().unwrap() {
                return Ok(false);
            }
            let mut member = 0;
            checked(IsProcessInJob(leader.0, job.0, &mut member))?;
            if member == 0 && WaitForSingleObject(leader.0, 0) != WAIT_OBJECT_0 {
                return Ok(false);
            }
        } else if GetLastError() != ERROR_INVALID_PARAMETER {
            return Ok(false); // Access failure never authorizes PID-based takeover.
        }
        checked(TerminateJobObject(job.0, 1))?;
        let end = Instant::now() + limit;
        loop {
            if active(job.0)? == 0 {
                return Ok(true);
            }
            if Instant::now() >= end {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
