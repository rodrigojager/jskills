//! Durable command receipts owned by the foreground controller, never by clients.
use crate::{
    execution::commands::{CommandSpec, spawn},
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS managed_commands(id TEXT PRIMARY KEY, request_id TEXT NOT NULL UNIQUE, run_id TEXT NOT NULL REFERENCES runs(id), task_id TEXT, attempt_id TEXT, spec TEXT NOT NULL, state TEXT NOT NULL, pid INTEGER, exit_code INTEGER, reason TEXT);";
pub const MAX_CHUNK: usize = 65536;

#[derive(Debug, Serialize, Deserialize)]
pub struct Endpoint {
    pub endpoint: SocketAddr,
    pub token: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub token: String,
    pub action: Action,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Start {
        spec: CommandSpec,
    },
    Status {
        id: String,
    },
    Output {
        id: String,
        stream: String,
        offset: u64,
        limit: usize,
    },
    Stop {
        id: String,
        reason: String,
    },
    RepairBegin {
        run: String,
        operation: String,
        worker: String,
        request_id: String,
    },
    RepairFinish {
        report: crate::domain::operation_lineage::Recheck,
    },
    InputsChanged {
        run: String,
        request_id: String,
        revision: String,
        inputs: Vec<String>,
        reason: String,
    },
    Shutdown,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub value: Option<Value>,
    pub error: Option<String>,
}

struct Owned {
    child: crate::execution::custody::OwnedProcess,
    deadline: Option<Instant>,
    stopping: bool,
}
pub struct Service {
    listener: TcpListener,
    endpoint: Endpoint,
    data: PathBuf,
    workspace: Workspace,
    live: HashMap<String, Owned>,
    shutdown: bool,
    /// The run a task controller serves; None for standalone `exec serve`.
    run: Option<String>,
}

impl Service {
    /// `run` is the run a task controller serves; `None` is the standalone `exec serve` service.
    pub fn new(workspace: &Workspace, store: &Store, run: Option<&str>) -> Result<Self> {
        store.conn.execute_batch(SCHEMA)?;
        store.conn.execute_batch(super::operation_repair::SCHEMA)?;
        // A prior controller's PID alone cannot prove custody. Keep uncertainty visible;
        // identical replay returns this receipt, and never launches a replacement.
        super::reconcile::commands(workspace, store)?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let endpoint = Endpoint {
            endpoint: listener.local_addr()?,
            token: uuid::Uuid::new_v4().to_string(),
        };
        crate::transport::write_endpoint_file(
            &crate::transport::commands_file(&workspace.data, run),
            &endpoint,
        )?;
        let service = Self {
            listener,
            endpoint,
            data: workspace.data.clone(),
            workspace: workspace.clone(),
            live: HashMap::new(),
            shutdown: false,
            run: run.map(str::to_owned),
        };
        let retired = {
            let mut q = store.conn.prepare("SELECT id FROM managed_commands WHERE state IN ('stopped','exited','launch_failed')")?;
            q.query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in retired {
            service.release_resources(store, &id)?;
        }
        Ok(service)
    }
    pub fn has_live(&self) -> bool {
        !self.live.is_empty()
    }
    pub fn has_pending(&self, store: &Store, run: &str) -> Result<bool> {
        Ok(store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND state='queued')",
            [run],
            |r| r.get(0),
        )?)
    }
    /// Called only by the instance's periodic checker. Process/socket polling
    /// remains independent I/O observation and owns no task-specific timer.
    pub fn check_deadlines(&mut self, store: &Store) -> Result<()> {
        for (id, owned) in &mut self.live {
            if owned.deadline.is_some_and(|d| Instant::now() >= d) && !owned.stopping {
                owned.child.request_stop()?;
                owned.stopping = true;
                store.conn.execute("UPDATE managed_commands SET state='stopping',reason='command_deadline' WHERE id=?1", [id])?;
            }
        }
        Ok(())
    }
    pub fn poll(&mut self, store: &Store) -> Result<()> {
        let mut retired = Vec::new();
        for (id, owned) in &mut self.live {
            let observation = owned.child.poll();
            if let Err(error) = observation {
                store.conn.execute("UPDATE managed_commands SET state='unknown',reason='owned cleanup unconfirmed' WHERE id=?1", [id])?;
                return Err(error);
            }
            if let Some(exit) = observation? {
                store.conn.execute(
                    "UPDATE managed_commands SET state=?2,exit_code=?3 WHERE id=?1",
                    params![id, if owned.stopping { "stopped" } else { "exited" }, exit],
                )?;
                retired.push(id.clone());
            }
        }
        let had_retirement = !retired.is_empty();
        for id in retired {
            self.live.remove(&id);
            self.release_resources(store, &id)?;
        }
        // Bound admission per tick so busy clients cannot starve observation.
        for _ in 0..32 {
            let mut stream = match self.listener.accept() {
                Ok((stream, _)) => stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
            stream.set_nonblocking(false)?;
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
            stream.set_write_timeout(Some(Duration::from_millis(100)))?;
            let result = (|| {
                let request: Request = read_message(&mut stream)?;
                ensure!(
                    request.token == self.endpoint.token,
                    "invalid command control scope"
                );
                self.handle(store, request.action)
            })();
            let reply = match result {
                Ok(value) => Reply {
                    value: Some(value),
                    error: None,
                },
                Err(e) => Reply {
                    value: None,
                    error: Some(e.to_string()),
                },
            };
            // Client loss never rolls back accepted launch intent or its receipt.
            let _ = crate::transport::write_message(&mut stream, &reply);
        }
        if had_retirement {
            self.admit_queued(store)?;
        }
        Ok(())
    }
    fn handle(&mut self, store: &Store, action: Action) -> Result<Value> {
        match action {
            Action::Start { spec } => {
                let cycle = super::operation_repair::authorize_command(store, &spec)?;
                let receipt = self.start(store, spec.clone())?;
                if let Some(cycle) = cycle {
                    super::operation_repair::record_command(
                        store,
                        &spec,
                        cycle,
                        receipt["command_id"]
                            .as_str()
                            .context("missing command receipt")?,
                    )?;
                }
                Ok(receipt)
            }
            Action::RepairBegin {
                run,
                operation,
                worker,
                request_id,
            } => super::operation_repair::begin(store, &run, &operation, &worker, &request_id),
            Action::RepairFinish { report } => super::operation_repair::finish(store, &report),
            Action::InputsChanged {
                run,
                request_id,
                revision,
                inputs,
                reason,
            } => super::operation_repair::invalidate(
                store,
                &run,
                &request_id,
                &revision,
                &inputs,
                &reason,
            ),
            Action::Status { id } => status(store, &self.data, &id),
            Action::Output {
                id,
                stream,
                offset,
                limit,
            } => output(store, &self.data, &id, &stream, offset, limit),
            Action::Stop { id, reason } => {
                self.stop(store, &id, &reason)?;
                status(store, &self.data, &id)
            }
            Action::Shutdown => {
                self.stop_all(store, "controller_shutdown")?;
                self.shutdown = true;
                Ok(json!({"shutdown":true}))
            }
        }
    }
    pub(crate) fn start(&mut self, store: &Store, mut spec: CommandSpec) -> Result<Value> {
        ensure!(
            !self.shutdown && !super::lifetime::interrupted(),
            "command controller is shutting down"
        );
        if let Some(task) = &spec.task {
            let task_json: String = store.conn.query_row(
                "SELECT spec FROM tasks WHERE run_id=?1 AND id=?2",
                [&spec.run, task],
                |r| r.get(0),
            )?;
            let task: crate::domain::types::Task = serde_json::from_str(&task_json)?;
            spec.resources.extend(task.admission.resources);
            spec.resources.sort();
            spec.resources.dedup();
        }
        let serialized = serde_json::to_string(&spec)?;
        let previous: Option<(String, String)> = store
            .conn
            .query_row(
                "SELECT id,spec FROM managed_commands WHERE request_id=?1",
                [&spec.request_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, previous)) = previous {
            let mut receipt = if previous == serialized {
                status(store, &self.data, &id)?
            } else {
                // A later attempt of the same task asking for the same command
                // gets the command an earlier attempt started, so a retry
                // waits for it instead of starting it again.
                let earlier: CommandSpec = serde_json::from_str(&previous)?;
                let same_command = earlier.task.is_some()
                    && earlier.task == spec.task
                    && CommandSpec {
                        attempt: spec.attempt.clone(),
                        ..earlier.clone()
                    } == spec;
                ensure!(same_command, "conflicting command request id");
                let (Some(task), Some(attempt)) = (&spec.task, &spec.attempt) else {
                    bail!("task and attempt must be supplied together");
                };
                let active: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3 AND state='running')", [attempt,&spec.run,task], |r|r.get(0))?;
                ensure!(active, "command requires the current running task attempt");
                let mut receipt = status(store, &self.data, &id)?;
                receipt["started_by_attempt"] = json!(earlier.attempt);
                receipt
            };
            receipt["duplicate"] = json!(true);
            return Ok(receipt);
        }
        spec.validate()?;
        // An unresolved writer blocks this run's fresh launches, while exact
        // request replays above still return their original durable receipt.
        let unknown: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM managed_commands WHERE run_id=?1 AND state='unknown')",
            [&spec.run],
            |r| r.get(0),
        )?;
        ensure!(
            !unknown,
            "prior command custody is unknown; fresh launches for this run are paused"
        );
        let _deadline = spec
            .deadline_ms
            .map(|ms| {
                Instant::now()
                    .checked_add(Duration::from_millis(ms))
                    .context("command deadline is out of range")
            })
            .transpose()?;
        let exists: bool = store.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1)",
            [&spec.run],
            |r| r.get(0),
        )?;
        ensure!(exists, "unknown command run");
        if let (Some(task), Some(attempt)) = (&spec.task, &spec.attempt) {
            let active: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3 AND state='running')", [attempt,&spec.run,task], |r|r.get(0))?;
            ensure!(active, "command requires the current running task attempt");
        }
        let id = uuid::Uuid::new_v4().to_string();
        super::admission::resources(&self.workspace, &spec.resources)?;
        store.conn.execute("INSERT INTO managed_commands(id,request_id,run_id,task_id,attempt_id,spec,state) VALUES(?1,?2,?3,?4,?5,?6,'queued')",params![id,spec.request_id,spec.run,spec.task,spec.attempt,serialized])?;
        self.admit_queued(store)?;
        let mut receipt = status(store, &self.data, &id)?;
        receipt["duplicate"] = json!(false);
        Ok(receipt)
    }
    fn resource_owner(&self, spec: &CommandSpec, id: &str) -> crate::resources::registry::Owner {
        super::admission::owner(
            &self.workspace,
            &spec.run,
            spec.task.as_deref().unwrap_or("operation"),
            spec.attempt.as_deref().unwrap_or(id),
        )
    }
    fn release_resources(&self, store: &Store, id: &str) -> Result<()> {
        let spec: String =
            store
                .conn
                .query_row("SELECT spec FROM managed_commands WHERE id=?1", [id], |r| {
                    r.get(0)
                })?;
        let spec: CommandSpec = serde_json::from_str(&spec)?;
        super::admission::release_reserved(
            &self.workspace,
            self.resource_owner(&spec, id),
            &format!("command-{id}"),
        )?;
        if let (Some(task), Some(attempt)) = (&spec.task, &spec.attempt) {
            let retired: bool = store.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3 AND state IN ('retired','interrupted'))",
                [attempt, &spec.run, task],
                |r| r.get(0),
            )?;
            // A surviving command retains its worker's reservation. Release
            // that exact hold only after the worker and every owned command
            // are known retired; a live or uncertain scope keeps the hold.
            if retired && !super::recovery::live_commands(&store.conn, &spec.run, task)? {
                super::admission::release(&self.workspace, &spec.run, task, attempt)?;
                // Recovery waits while a retired attempt's command lives; now
                // that the last one ended, the run's own controller makes the
                // deferred decision (as its next start would), so a retry
                // can ask for the same command and read its real result.
                if self.run.as_deref() == Some(spec.run.as_str()) {
                    super::recovery::reconcile(&self.workspace, store, &spec.run)?;
                }
            }
        }
        Ok(())
    }
    pub fn admit_queued(&mut self, store: &Store) -> Result<()> {
        if self.shutdown || super::lifetime::interrupted() {
            return Ok(());
        }
        let rows = {
            let mut q = store.conn.prepare(
                "SELECT id,spec FROM managed_commands WHERE state='queued' ORDER BY rowid,id",
            )?;
            q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (id, json) in rows {
            let spec: CommandSpec = serde_json::from_str(&json)?;
            let capacity = super::admission::plan(store, &spec.run)?
                .scheduling
                .heavy_capacity;
            let used: i64 = store.conn.query_row("SELECT count(*) FROM managed_commands WHERE run_id=?1 AND state IN ('intent','running','stopping','unknown')", [&spec.run], |r|r.get(0))?;
            if usize::try_from(used)? >= capacity {
                continue;
            }
            if let (Some(task), Some(attempt)) = (&spec.task, &spec.attempt) {
                let current: bool = store.conn.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3 AND state='running')", [attempt,&spec.run,task], |r|r.get(0))?;
                if !current {
                    store.conn.execute("UPDATE managed_commands SET state='stopped',reason='task attempt retired before command admission' WHERE id=?1", [&id])?;
                    continue;
                }
            }
            if !super::admission::reserve(
                &self.workspace,
                self.resource_owner(&spec, &id),
                &spec.resources,
                &format!("command-{id}"),
            )? {
                continue;
            }
            // Recheck the entire invocation after its wait before persisting intent.
            if spec.validate().is_err() {
                store.conn.execute("UPDATE managed_commands SET state='launch_failed',reason='queued invocation is no longer usable' WHERE id=?1", [&id])?;
                self.release_resources(store, &id)?;
                continue;
            }
            let deadline = spec
                .deadline_ms
                .map(|ms| {
                    Instant::now()
                        .checked_add(Duration::from_millis(ms))
                        .context("command deadline is out of range")
                })
                .transpose()?;
            super::checkpoints::capture_check_inputs(&store.conn, &self.workspace, &spec, &id)?;
            store.conn.execute(
                "UPDATE managed_commands SET state='intent' WHERE id=?1 AND state='queued'",
                [&id],
            )?;
            match spawn(&spec, &self.data.join("commands").join(&id)) {
                Ok(child) => {
                    let pid = child.id();
                    self.live.insert(
                        id.clone(),
                        Owned {
                            child,
                            deadline,
                            stopping: false,
                        },
                    );
                    // Retain the Child even if receipt persistence fails. A replay sees
                    // intent and cannot repeat this launch across the SQL/spawn gap.
                    store.conn.execute(
                        "UPDATE managed_commands SET state='running',pid=?2 WHERE id=?1",
                        params![id, pid],
                    )?;
                }
                Err(_) => {
                    // Do not persist OS error text, which can contain private environment values.
                    let empty =
                        crate::execution::custody::reconcile(&self.data.join("commands").join(&id))
                            .unwrap_or(false);
                    store.conn.execute("UPDATE managed_commands SET state=?2,reason='program launch failed or custody unconfirmed' WHERE id=?1",rusqlite::params![id,if empty {"launch_failed"} else {"unknown"}])?;
                    if empty {
                        self.release_resources(store, &id)?;
                    }
                }
            }
        }
        Ok(())
    }
    fn stop(&mut self, store: &Store, id: &str, reason: &str) -> Result<()> {
        ensure!(!reason.trim().is_empty(), "stop reason is required");
        if let Some(owned) = self.live.get_mut(id) {
            store.conn.execute(
                "UPDATE managed_commands SET state='stopping',reason=?2 WHERE id=?1",
                [id, reason],
            )?;
            owned.child.request_stop()?;
            owned.stopping = true;
        } else {
            let receipt = status(store, &self.data, id)?;
            ensure!(
                receipt["state"] != "unknown" && receipt["state"] != "intent",
                "unresolved custody; cannot claim process stopped"
            );
            if receipt["state"] == "queued" {
                store.conn.execute(
                    "UPDATE managed_commands SET state='stopped',reason=?2 WHERE id=?1",
                    [id, reason],
                )?;
            }
        }
        Ok(())
    }
    /// Task-local reconciliation never shuts down unrelated command admission.
    pub fn stop_task(&mut self, store: &Store, run: &str, task: &str, reason: &str) -> Result<()> {
        let ids = {
            let mut q = store.conn.prepare("SELECT id FROM managed_commands WHERE run_id=?1 AND task_id=?2 AND state IN ('queued','intent','running','stopping','unknown')")?;
            q.query_map([run, task], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for id in ids {
            self.stop(store, &id, reason)?;
        }
        Ok(())
    }
    pub fn stop_all(&mut self, store: &Store, reason: &str) -> Result<()> {
        self.shutdown = true;
        let ids: Vec<_> = self.live.keys().cloned().collect();
        for id in ids {
            self.stop(store, &id, reason)?;
        }
        store.conn.execute(
            "UPDATE managed_commands SET state='stopped',reason=?1 WHERE state='queued'",
            [reason],
        )?;
        Ok(())
    }
}

pub fn serve(workspace: &Workspace) -> Result<i32> {
    let mut owner = super::lifetime::Owner::acquire(workspace, "managed-commands")?;
    ensure!(
        workspace.database().is_file(),
        "import a plan before serving commands"
    );
    let store = Store::open(workspace, owner.guard())?;
    let mut service = Service::new(workspace, &store, None)?;
    owner.endpoint(service.endpoint.endpoint)?;
    let mut checker = super::due::Checker::new();
    while !service.shutdown || service.has_live() {
        if super::lifetime::interrupted() {
            service.stop_all(&store, "controller interrupted")?;
        }
        if checker.ready() {
            service.check_deadlines(&store)?;
            service.admit_queued(&store)?;
        }
        service.poll(&store)?;
        std::thread::sleep(Duration::from_millis(if service.has_live() {
            10
        } else {
            100
        }));
    }
    Ok(0)
}

pub fn status(store: &Store, data: &Path, id: &str) -> Result<Value> {
    let (request, state, pid, exit, reason): (
        String,
        String,
        Option<u32>,
        Option<i32>,
        Option<String>,
    ) = store
        .conn
        .query_row(
            "SELECT request_id,state,pid,exit_code,reason FROM managed_commands WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .context("unknown command id")?;
    let length = |name| {
        fs::metadata(data.join("commands").join(id).join(name))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    Ok(
        json!({"command_id":id,"request_id":request,"state":state,"pid":pid,"exit_code":exit,"reason":reason,"stdout_cursor":length("stdout.log"),"stderr_cursor":length("stderr.log")}),
    )
}
pub fn output(
    store: &Store,
    data: &Path,
    id: &str,
    stream: &str,
    offset: u64,
    limit: usize,
) -> Result<Value> {
    ensure!(
        matches!(stream, "stdout" | "stderr"),
        "stream must be stdout or stderr"
    );
    ensure!(
        limit > 0 && limit <= MAX_CHUNK,
        "output limit must be 1..65536 bytes"
    );
    let receipt = status(store, data, id)?;
    let path = data.join("commands").join(id).join(format!("{stream}.log"));
    let mut bytes = Vec::new();
    let mut captured = 0;
    match File::open(path) {
        Ok(mut file) => {
            captured = file.metadata()?.len();
            ensure!(offset <= captured, "output cursor is beyond captured bytes");
            file.seek(SeekFrom::Start(offset))?;
            file.take(limit as u64).read_to_end(&mut bytes)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(offset == 0, "output cursor is beyond captured bytes");
        }
        Err(error) => return Err(error.into()),
    }
    Ok(
        json!({"command_id":id,"stream":stream,"offset":offset,"next_offset":offset+bytes.len() as u64,"captured_bytes":captured,"bytes":bytes,"state":receipt["state"]}),
    )
}
pub fn read_message<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    let mut line = String::new();
    BufReader::new(stream.take(1048577)).read_line(&mut line)?;
    ensure!(
        line.len() <= 1048576 && line.ends_with('\n'),
        "oversized or incomplete command frame"
    );
    Ok(serde_json::from_str(&line)?)
}
/// The command endpoint that serves `run`: its task controller's own file while that controller answers, otherwise
/// the standalone `exec serve` file. A stale file of a controller that is gone is ignored.
pub fn endpoint_for(data: &Path, run: Option<&str>) -> Result<Endpoint> {
    let read =
        |path: PathBuf| -> Option<Endpoint> { serde_json::from_slice(&fs::read(path).ok()?).ok() };
    if let Some(run) = run.filter(|run| crate::domain::validation::identifier(run).is_ok())
        && let Some(endpoint) = read(crate::transport::commands_file(data, Some(run)))
        && crate::transport::answers(endpoint.endpoint)
    {
        return Ok(endpoint);
    }
    read(crate::transport::commands_file(data, None))
        .context("no active command controller; use exec serve or run")
}

pub fn send(endpoint: &Endpoint, action: Action) -> Result<Value> {
    ensure!(
        endpoint.endpoint.ip().is_loopback(),
        "command endpoint must be loopback"
    );
    let mut stream = TcpStream::connect_timeout(&endpoint.endpoint, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    crate::transport::write_message(
        &mut stream,
        &Request {
            token: endpoint.token.clone(),
            action,
        },
    )?;
    let reply: Reply = read_message(&mut stream)?;
    if let Some(error) = reply.error {
        bail!("{error}");
    }
    reply.value.context("command controller omitted reply")
}
