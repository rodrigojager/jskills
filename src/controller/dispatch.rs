use crate::{
    execution::launch,
    read::status,
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request, Response},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    process::Child,
    time::Duration,
};

struct Active {
    id: String,
    task: String,
    token: String,
    child: Child,
    codex: bool,
}

impl Drop for Active {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// The only runtime writer: transactions are finished before process/socket waits.
pub fn run(workspace: &Workspace, run: &str) -> Result<i32> {
    let guard = ControllerGuard::acquire(&workspace.git_dir)?;
    ensure!(
        workspace.database().is_file(),
        "no runtime database; import a plan first"
    );
    let mut store = Store::open(workspace, &guard)?;
    let mut commands = crate::controller::commands::Service::new(workspace, &store)?;
    store.check_checkout(run, &workspace.checkout)?;
    ensure!(
        !store.has_unretired(run)?,
        "unfinished launch/process requires reconciliation; no replacement was launched"
    );
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = ControlEndpoint {
        endpoint: listener.local_addr()?,
        token: uuid::Uuid::new_v4().to_string(),
        run: run.into(),
    };
    write_endpoint(workspace, &endpoint)?;
    store.set_run_state(run, "running")?;
    let mut active: Option<Active> = None;
    let mut stopped = false;
    loop {
        commands.poll(&store)?;
        // Drain requests before observing exit, so a designated result queued
        // just before process exit cannot be mistaken for missing evidence.
        while let Some(mut stream) = accept(&listener)? {
            // Winsock inherits the listener's nonblocking mode on acceptance.
            // Bound each individual request with timeouts instead.
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
            stream.set_write_timeout(Some(Duration::from_millis(100)))?;
            let result: Result<bool> = (|| match transport::read_message(&mut stream)? {
                Request::Result { token, receipt } => {
                    let active = active.as_ref().context("no active task attempt")?;
                    ensure!(token == active.token, "invalid result scope");
                    Ok(!store.accept(run, &active.id, &receipt)?)
                }
                Request::Stop {
                    token,
                    run: requested,
                } => {
                    ensure!(
                        token == endpoint.token && requested == run,
                        "invalid stop scope"
                    );
                    stopped = true;
                    Ok(false)
                }
            })();
            let response = match result {
                Ok(duplicate) => Response {
                    accepted: true,
                    duplicate,
                    error: None,
                },
                Err(error) => Response {
                    accepted: false,
                    duplicate: false,
                    error: Some(error.to_string()),
                },
            };
            // A disconnected client does not undo an already durable receipt.
            let _ = transport::write_message(&mut stream, &response);
        }
        if stopped {
            commands.stop_all(&store, "controller stopped")?;
        }
        if let Some(current) = active.as_mut() {
            if stopped {
                // T01 launches a direct fixture with no descendants. Broader
                // process-tree ownership/reconciliation is the custody increment.
                if current.child.try_wait()?.is_none() {
                    current.child.kill()?;
                }
                let exit = current.child.wait()?;
                if current.codex {
                    crate::executors::codex::capture_interrupted(
                        workspace,
                        &current.task,
                        &current.id,
                    )?;
                }
                store.retire(run, &current.task, &current.id, exit.code(), true)?;
                active = None;
            } else if let Some(exit) = current.child.try_wait()? {
                if current.codex {
                    crate::executors::codex::finish(
                        workspace,
                        &mut store,
                        run,
                        &current.task,
                        &current.id,
                        exit.success(),
                    )?;
                }
                store.retire(run, &current.task, &current.id, exit.code(), false)?;
                active = None;
            }
        }
        let operation_progress = if !stopped {
            crate::controller::operations::tick(workspace, &store, run, &mut commands)?
        } else {
            false
        };
        if active.is_none() {
            if stopped {
                if commands.has_live() {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                break;
            }
            let Some(task) = store.ready(run)? else {
                if commands.has_live() || operation_progress {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                break;
            };
            let attempt = uuid::Uuid::new_v4().to_string();
            let token = uuid::Uuid::new_v4().to_string();
            store.intent(run, &task.id, &attempt)?;
            match launch::task(workspace, &task, &attempt, endpoint.endpoint, &token) {
                Ok(child) => {
                    let pid = child.id();
                    // Retain the owned process even if post-spawn persistence
                    // fails; never create a second attempt on that uncertainty.
                    active = Some(Active {
                        id: attempt.clone(),
                        task: task.id.clone(),
                        token,
                        child,
                        codex: matches!(
                            task.executor,
                            crate::domain::types::Executor::Codex { .. }
                        ),
                    });
                    if let Err(error) = store.launched(&attempt, pid) {
                        let current = active.as_mut().unwrap();
                        let _ = current.child.kill();
                        let _ = current.child.wait();
                        return Err(error.context("spawned process retired after launch persistence failed; intent retained for reconciliation"));
                    }
                }
                Err(error) => {
                    store.retire(run, &task.id, &attempt, None, false)?;
                    store.set_run_state(run, "incomplete")?;
                    return Err(error.context("executor could not launch; task needs intervention"));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    crate::controller::operations::reconcile(&store, run)?;
    let snapshot = status::read(&workspace.database(), run)?;
    let state = if stopped {
        "stopped"
    } else if snapshot.complete {
        "completed"
    } else {
        "incomplete"
    };
    store.set_run_state(run, state)?;
    println!(
        "{}",
        serde_json::to_string(&status::read(&workspace.database(), run)?)?
    );
    Ok(if snapshot.complete && !stopped { 0 } else { 3 })
}

fn accept(listener: &TcpListener) -> Result<Option<TcpStream>> {
    match listener.accept() {
        Ok((stream, _)) => Ok(Some(stream)),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn write_endpoint(workspace: &Workspace, endpoint: &ControlEndpoint) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(workspace.data.join("control.json"))?;
    file.write_all(&serde_json::to_vec(endpoint)?)?;
    file.sync_all()?;
    Ok(())
}
