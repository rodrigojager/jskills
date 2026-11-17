use crate::{
    execution::launch,
    read::status,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request, Response},
};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    net::{TcpListener, TcpStream},
    time::Duration,
};

struct Active {
    id: String,
    task: String,
    token: String,
    child: crate::execution::custody::OwnedProcess,
    codex: bool,
    claude: bool,
    opencode: bool,
    native: Option<super::activity::NativeCursor>,
}

/// The only runtime writer: transactions are finished before process/socket waits.
pub fn run(workspace: &Workspace, run: &str) -> Result<i32> {
    let mut owner = super::lifetime::Owner::acquire(workspace, run)?;
    ensure!(
        workspace.database().is_file(),
        "no runtime database; import a plan first"
    );
    let mut store = Store::open(workspace, owner.guard())?;
    store.check_checkout(run, &workspace.checkout)?;
    let mut commands = crate::controller::commands::Service::new(workspace, &store)?;
    let mut clocks = super::clock::Clock::start(&store.conn, run, super::clock::utc_ms())?;
    super::reconcile::attempts(workspace, &mut store, run)?;
    super::reviews::reconcile(workspace, &store, run)?;
    super::recovery::reconcile(workspace, &store, run)?;
    super::admission::reconcile(workspace, &store, run)?;
    let unknown: bool = store.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND state='unknown')",
        [run],
        |r| r.get(0),
    )?;
    ensure!(
        !unknown,
        "managed command custody is unknown; affected run admission is paused"
    );
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
    listener.set_nonblocking(true)?;
    let endpoint = ControlEndpoint {
        endpoint: listener.local_addr()?,
        token: uuid::Uuid::new_v4().to_string(),
        run: run.into(),
    };
    write_endpoint(workspace, &endpoint)?;
    owner.endpoint(endpoint.endpoint)?;
    store.set_run_state(run, "running")?;
    let mut active: Vec<Active> = Vec::new();
    let mut stopped = false;
    let mut checker = super::due::Checker::new();
    let mut reviews = super::reviews::Service::configured()?;
    let mut scheduling_requested = true;
    clocks.checkpoint(&store.conn, run)?;
    loop {
        stopped |= super::lifetime::interrupted();
        if stopped {
            commands.stop_all(&store, "controller stopped")?;
        }
        let before_commands = store.conn.total_changes();
        commands.poll(&store)?;
        if store.conn.total_changes() != before_commands {
            clocks.checkpoint(&store.conn, run)?;
            scheduling_requested = true;
        }
        let due_scan = checker.ready();
        let mut queued = Vec::new();
        if due_scan {
            clocks.checkpoint(&store.conn, run)?;
            commands.check_deadlines(&store)?;
            commands.admit_queued(&store)?;
            // Queue snapshots only. Reviewer consumers admit/recheck and launch
            // asynchronously; no model or command wait belongs in this scan.
            queued = checker.scan(&store.conn, run)?;
            super::recovery::reconcile(workspace, &store, run)?;
            scheduling_requested = true;
        }
        // Drain requests before observing exit, so a designated result queued
        // just before process exit cannot be mistaken for missing evidence.
        for _ in 0..32 {
            let Some(mut stream) = accept(&listener)? else {
                break;
            };
            stopped |= super::lifetime::interrupted();
            // Winsock inherits the listener's nonblocking mode on acceptance.
            // Bound each individual request with timeouts instead.
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
            stream.set_write_timeout(Some(Duration::from_millis(100)))?;
            let mut retry = None;
            let result: Result<bool> = (|| match transport::read_message(&mut stream)? {
                Request::Log {
                    token,
                    attempt,
                    task,
                    kind,
                    severity,
                    content,
                } => {
                    ensure!(
                        !stopped
                            && active
                                .iter()
                                .any(|a| a.id == attempt && a.task == task && a.token == token),
                        "invalid log scope or inactive worker"
                    );
                    let event = super::activity::Event {
                        actor: super::activity::Actor::Worker,
                        source: "explicit_worker".into(),
                        kind,
                        severity,
                        content,
                    };
                    super::activity::commit(
                        &store.conn,
                        run,
                        &task,
                        &attempt,
                        &event,
                        super::clock::utc_ms(),
                    )?;
                    Ok(false)
                }
                Request::Retry {
                    token,
                    run: requested,
                    task,
                    reason,
                    request_id,
                } => {
                    ensure!(
                        !stopped && token == endpoint.token && requested == run,
                        "inactive controller or invalid retry scope"
                    );
                    scheduling_requested = true;
                    retry = Some(super::cooldowns::retry(
                        workspace,
                        &store,
                        run,
                        &task,
                        &reason,
                        &request_id,
                        true,
                    )?);
                    Ok(false)
                }
                Request::Result { token, receipt } => {
                    ensure!(!stopped, "controller stopped accepting results");
                    let active = active
                        .iter()
                        .find(|a| a.id == receipt.attempt_id && a.token == token)
                        .context("invalid result scope or inactive task attempt")?;
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
                    retry,
                    accepted: true,
                    duplicate,
                    error: None,
                },
                Err(error) => Response {
                    retry: None,
                    accepted: false,
                    duplicate: false,
                    error: Some(error.to_string()),
                },
            };
            // A disconnected client does not undo an already durable receipt.
            let _ = transport::write_message(&mut stream, &response);
        }
        stopped |= super::lifetime::interrupted();
        if stopped {
            commands.stop_all(&store, "controller stopped")?;
        }
        let mut retired = Vec::new();
        for current in &mut active {
            let review_stop = super::reviews::stop_requested(&store.conn, &current.id)?;
            if review_stop {
                commands.stop_task(
                    &store,
                    run,
                    &current.task,
                    "execution review requested stop",
                )?;
                current.child.request_stop()?;
            }
            if let Some(native) = &mut current.native {
                native.poll(
                    &store.conn,
                    &workspace.data.join("attempts").join(&current.id),
                    run,
                    &current.task,
                    &current.id,
                    super::clock::utc_ms(),
                )?;
            }
            if stopped {
                current.child.request_stop()?;
            }
            let observation = current.child.poll();
            if let Err(error) = observation {
                store.conn.execute(
                    "UPDATE attempts SET state='uncertain' WHERE id=?1",
                    [&current.id],
                )?;
                store.set_run_state(run, "custody_unknown")?;
                return Err(error);
            }
            if let Some(exit) = observation? {
                // Capture the entire final bounded backlog before retirement so
                // final native records retain their live worker attribution.
                if let Some(native) = &mut current.native {
                    native.drain(
                        &store.conn,
                        &workspace.data.join("attempts").join(&current.id),
                        run,
                        &current.task,
                        &current.id,
                        super::clock::utc_ms(),
                    )?;
                }
                clocks.checkpoint(&store.conn, run)?;
                if stopped {
                    if current.opencode {
                        crate::executors::opencode::capture_interrupted(
                            workspace,
                            &current.task,
                            &current.id,
                        )?;
                    }
                    if current.codex {
                        crate::executors::codex::capture_interrupted(
                            workspace,
                            &current.task,
                            &current.id,
                        )?;
                    }
                    if current.claude {
                        crate::executors::claude::capture_interrupted(
                            workspace,
                            &current.task,
                            &current.id,
                        )?;
                    }
                    store.retire(run, &current.task, &current.id, exit, true)?;
                } else {
                    if current.opencode {
                        crate::executors::opencode::finish(
                            workspace,
                            &mut store,
                            run,
                            &current.task,
                            &current.id,
                            exit == Some(0),
                        )?;
                    }
                    if current.codex {
                        crate::executors::codex::finish(
                            workspace,
                            &mut store,
                            run,
                            &current.task,
                            &current.id,
                            exit == Some(0),
                        )?;
                    }
                    if current.claude {
                        crate::executors::claude::finish(
                            workspace,
                            &mut store,
                            run,
                            &current.task,
                            &current.id,
                            exit == Some(0),
                        )?;
                    }
                    store.retire(run, &current.task, &current.id, exit, review_stop)?;
                }
                super::reviews::reconcile_actions(workspace, &store, run)?;
                super::recovery::after_retire(workspace, &store, run, &current.task, &current.id)?;
                if !super::recovery::live_commands(&store.conn, run, &current.task)? {
                    super::admission::release(workspace, run, &current.task, &current.id)?;
                }
                retired.push(current.id.clone());
            }
        }
        active.retain(|a| !retired.contains(&a.id));
        if !retired.is_empty() {
            scheduling_requested = true;
            clocks.checkpoint(&store.conn, run)?;
        }
        if stopped {
            reviews.stop_all()?;
        }
        let before_reviews = store.conn.total_changes();
        reviews.poll(workspace, &store, run, &mut clocks)?;
        if !stopped && due_scan {
            reviews.consume(workspace, &store, run, &queued, &checker, &mut clocks)?;
        }
        if store.conn.total_changes() != before_reviews {
            scheduling_requested = true;
        }
        let operation_progress = if !stopped && scheduling_requested {
            crate::controller::operations::tick(workspace, &store, run, &mut commands)?
        } else {
            false
        };
        if stopped {
            if commands.has_live() || !active.is_empty() || reviews.has_live() {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            break;
        }
        let mut admitted = false;
        let admission_checked = scheduling_requested;
        while scheduling_requested {
            let attempt = uuid::Uuid::new_v4().to_string();
            let Some(task) = super::admission::next(workspace, &store, run, &attempt)? else {
                break;
            };
            let token = uuid::Uuid::new_v4().to_string();
            let task = match store.prepare_attempt(workspace, run, &task, &attempt) {
                Ok(task) => task,
                Err(error) => {
                    super::admission::release(workspace, run, &task.id, &attempt)?;
                    return Err(error);
                }
            };
            admitted = true;
            match launch::task(workspace, &task, &attempt, endpoint.endpoint, &token) {
                Ok(child) => {
                    let pid = child.id();
                    let executor =
                        if matches!(task.executor, crate::domain::types::Executor::Codex { .. }) {
                            Some("codex")
                        } else if matches!(
                            task.executor,
                            crate::domain::types::Executor::Claude { .. }
                        ) {
                            Some("claude")
                        } else if matches!(
                            task.executor,
                            crate::domain::types::Executor::OpenCode { .. }
                        ) {
                            Some("opencode")
                        } else {
                            None
                        };
                    // Retain the owned process even if post-spawn persistence
                    // fails; never create a second attempt on that uncertainty.
                    active.push(Active {
                        id: attempt.clone(),
                        task: task.id.clone(),
                        token,
                        child,
                        native: None,
                        codex: matches!(
                            task.executor,
                            crate::domain::types::Executor::Codex { .. }
                        ),
                        claude: matches!(
                            task.executor,
                            crate::domain::types::Executor::Claude { .. }
                        ),
                        opencode: matches!(
                            task.executor,
                            crate::domain::types::Executor::OpenCode { .. }
                        ),
                    });
                    if let Some(executor) = executor {
                        active.last_mut().unwrap().native =
                            Some(super::activity::NativeCursor::new(
                                &workspace.data.join("attempts").join(&attempt),
                                executor,
                            )?);
                    }
                    if let Err(error) = store.launched(&attempt, pid) {
                        return Err(error.context("launch persistence failed; owned cleanup requested and intent retained for reconciliation"));
                    }
                    clocks.checkpoint(&store.conn, run)?;
                }
                Err(error) => {
                    if super::recovery::launch_failure(
                        workspace,
                        &store,
                        run,
                        &task.id,
                        &attempt,
                        &format!("{error:#}"),
                    )? {
                        store.retire(run, &task.id, &attempt, None, false)?;
                        super::recovery::after_retire(workspace, &store, run, &task.id, &attempt)?;
                        super::admission::release(workspace, run, &task.id, &attempt)?;
                    } else {
                        store.conn.execute(
                            "UPDATE attempts SET state='uncertain' WHERE id=?1",
                            [&attempt],
                        )?;
                        store.set_run_state(run, "custody_unknown")?;
                        return Err(error.context(
                            "executor launch custody is unknown; reconcile before admission",
                        ));
                    }
                    // A confirmed empty task launch failure is task-local. Keep
                    // supervising independent active workers and pending tasks.
                }
            }
        }
        scheduling_requested = false;
        if admission_checked
            && active.is_empty()
            && !commands.has_live()
            && !reviews.has_live()
            && !operation_progress
            && !admitted
            && !super::admission::cooling_down(&store, run)?
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(
            if active.is_empty() && !commands.has_live() && !reviews.has_live() {
                100
            } else {
                10
            },
        ));
    }
    clocks.checkpoint(&store.conn, run)?;
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
