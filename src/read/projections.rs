//! Version 1 read contract shared by CLI, terminal and HTTP consumers.
//! No runtime writer, transport control client or discovery call belongs here.
use crate::{controller, storage::Workspace};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CONTRACT_VERSION: u32 = 1;
pub const MAX_ITEMS: usize = 100;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum View {
    Runs,
    Status,
    Tasks,
    Task,
    Attempts,
    Attempt,
    Activity,
    Progress,
    Errors,
    Operations,
    Decisions,
    Questions,
    Commands,
    Command,
    Logs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub view: View,
    pub run: Option<String>,
    pub task: Option<String>,
    pub attempt: Option<String>,
    pub command: Option<String>,
    pub after: u64,
    pub limit: usize,
    pub stream: String,
    /// Entity-list cursors are scoped to this graph revision when supplied.
    pub revision: Option<u32>,
}
impl Request {
    pub fn new(view: View, run: Option<&str>) -> Self {
        Self {
            view,
            run: run.map(str::to_owned),
            task: None,
            attempt: None,
            command: None,
            after: 0,
            limit: MAX_ITEMS,
            stream: "stdout".into(),
            revision: None,
        }
    }
}

pub fn open(workspace: &Workspace) -> Result<Connection> {
    ensure!(
        workspace.database().is_file(),
        "no runtime database; read does not create state"
    );
    let conn = Connection::open_with_flags(
        workspace.database(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    ensure!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?
            == crate::storage::schema::VERSION,
        "unsupported runtime schema; read does not migrate"
    );
    conn.execute_batch("PRAGMA query_only=ON; BEGIN")?;
    Ok(conn)
}
fn table(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}
fn required(value: &Option<String>, name: &str) -> Result<String> {
    value
        .clone()
        .with_context(|| format!("{name} selector is required"))
}
fn sql_cursor(value: u64) -> Result<i64> {
    i64::try_from(value).context("cursor exceeds supported SQL range")
}
fn sql_limit(value: usize) -> Result<i64> {
    i64::try_from(value).context("page size exceeds supported SQL range")
}
fn nonnegative(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}
fn selected_run(conn: &Connection, request: &Request) -> Result<String> {
    if let Some(run) = &request.run {
        return Ok(run.clone());
    }
    let command_run = if let Some(id) = &request.command {
        ensure!(
            table(conn, "managed_commands")?,
            "command state unavailable"
        );
        Some(
            conn.query_row(
                "SELECT run_id FROM managed_commands WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .context("unknown command")?,
        )
    } else if let Some(id) = &request.attempt {
        Some(
            conn.query_row("SELECT run_id FROM attempts WHERE id=?1", [id], |r| {
                r.get::<_, String>(0)
            })
            .context("unknown attempt")?,
        )
    } else {
        None
    };
    if let Some(run) = command_run {
        return Ok(run);
    }
    let mut query = conn.prepare("SELECT id FROM runs ORDER BY id LIMIT 2")?;
    let ids = query
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(
        ids.len() == 1,
        "explicit run selector required: no run or ambiguous runs"
    );
    Ok(ids[0].clone())
}
fn revision(conn: &Connection, run: &str) -> Result<Option<u32>> {
    if !table(conn, "graph_baselines")? {
        return Ok(None);
    }
    Ok(conn
        .query_row(
            "SELECT revision FROM graph_baselines WHERE run_id=?1",
            [run],
            |r| r.get(0),
        )
        .optional()?)
}
fn metadata(conn: &Connection, workspace: &Workspace, run: Option<&str>) -> Result<Value> {
    let (cursor, as_of):(u64, Option<String>) = conn.query_row("SELECT coalesce(max(sequence),0),(SELECT recorded_at FROM journal WHERE (?1 IS NULL OR run_id=?1) ORDER BY sequence DESC LIMIT 1) FROM journal WHERE (?1 IS NULL OR run_id=?1)",[run],|r|Ok((nonnegative(r,0)?,r.get(1)?)))?;
    let state = run
        .map(|run| {
            conn.query_row("SELECT state FROM runs WHERE id=?1", [run], |r| {
                r.get::<_, String>(0)
            })
        })
        .transpose()?;
    let mut auxiliary = serde_json::Map::new();
    for name in [
        "attempt_activity",
        "task_clocks",
        "execution_reviews",
        "managed_commands",
        "graph_baselines",
        "autonomy_requests",
        "recovery_lineages",
        "recovery_invocations",
        "due_work",
    ] {
        auxiliary.insert(
            name.into(),
            json!(if table(conn, name)? {
                "available"
            } else {
                "not_initialized"
            }),
        );
    }
    Ok(
        json!({"contract_version":CONTRACT_VERSION,"identity":{"checkout":workspace.checkout,"run":run},"graph_revision":run.map(|r|revision(conn,r)).transpose()?.flatten(),"event_cursor":cursor,"as_of":as_of,"as_of_source":"latest persisted journal event; each clock retains its own checkpoint; not a live heartbeat","provenance":{"state":"sqlite_read_snapshot","effective_configuration":"immutable_attempt_profile_with_origins","result":"designated_receipt_when_present; summaries alone do not prove acceptance"},"availability":{"runtime":"available","controller":"unobserved","persisted_run_state":state,"auxiliary":auxiliary},"read_only":true,"native_helpers":{"state":"unobserved","count":null,"responsibility":"native_harness"}}),
    )
}
fn attach(mut payload: Value, metadata: Value) -> Value {
    let file_provenance = payload.get("provenance").cloned();
    payload
        .as_object_mut()
        .expect("projection object")
        .extend(metadata.as_object().unwrap().clone());
    if let Some(Value::Object(file)) = file_provenance {
        payload["provenance"].as_object_mut().unwrap().extend(file);
    }
    let mut omissions = Vec::new();
    bound(&mut payload, "", &mut omissions);
    payload["projection_limits"] = json!({"max_items_per_nested_list":MAX_ITEMS,"max_text_bytes":16384,"omissions":omissions,"state_snapshot_memory":"status and operations currently materialize persisted graph/history before bounding the response; raw transcripts are never loaded"});
    payload
}
fn bound(value: &mut Value, path: &str, omissions: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            // Raw bytes have their own explicit page bound and must stay lossless.
            if path == "/bytes" {
                return;
            }
            if items.len() > MAX_ITEMS {
                if omissions.len() < 32 {
                    omissions.push(
                        json!({"path":path,"reason":"list_limit","omitted":items.len()-MAX_ITEMS}),
                    );
                }
                items.truncate(MAX_ITEMS);
            }
            for (index, item) in items.iter_mut().enumerate() {
                bound(item, &format!("{path}/{index}"), omissions);
            }
        }
        Value::Object(fields) => {
            for (key, item) in fields {
                bound(item, &format!("{path}/{key}"), omissions);
            }
        }
        Value::String(text) if text.len() > 16384 => {
            // Log text is a bounded preview; truncating it is explicitly reported.
            let original = text.len();
            let mut end = 16384;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            if omissions.len() < 32 {
                omissions
                    .push(json!({"path":path,"reason":"text_limit","omitted_bytes":original-end}));
            }
        }
        _ => {}
    }
}
fn pagination(after: u64, returned: usize, limit: usize, total: u64, rev: Option<u32>) -> Value {
    let next = after + returned as u64;
    json!({"after":after,"next_after":next,"limit":limit,"has_more":next<total,"total":total,"graph_revision":rev})
}

/// One SQLite snapshot; retained files explicitly have separate byte cursors.
pub fn read(workspace: &Workspace, request: &Request) -> Result<Value> {
    ensure!(
        request.limit > 0
            && request.limit
                <= if matches!(request.view, View::Logs) {
                    super::output::MAX_PAGE_BYTES
                } else {
                    MAX_ITEMS
                },
        "read page limit exceeds contract bounds"
    );
    ensure!(
        request.after <= i64::MAX as u64,
        "cursor exceeds supported range"
    );
    let conn = open(workspace)?;
    if matches!(request.view, View::Runs) {
        let total = conn.query_row("SELECT count(*) FROM runs", [], |r| nonnegative(r, 0))?;
        let mut query =
            conn.prepare("SELECT id,checkout,state FROM runs ORDER BY id LIMIT ?1 OFFSET ?2")?;
        let runs=query.query_map(params![sql_limit(request.limit)?,sql_cursor(request.after)?],|r|Ok(json!({"run":r.get::<_,String>(0)?,"checkout":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        return Ok(attach(
            json!({"runs":runs,"page":pagination(request.after,runs.len(),request.limit,total,None)}),
            metadata(&conn, workspace, None)?,
        ));
    }
    let run = selected_run(&conn, request)?;
    let checkout: String = conn
        .query_row("SELECT checkout FROM runs WHERE id=?1", [&run], |r| {
            r.get(0)
        })
        .context("unknown run")?;
    ensure!(
        std::path::Path::new(&checkout) == workspace.checkout,
        "run belongs to another checkout"
    );
    let meta = metadata(&conn, workspace, Some(&run))?;
    let rev = revision(&conn, &run)?;
    if let Some(expected) = request.revision {
        ensure!(
            rev == Some(expected),
            "cursor graph revision changed; refetch the view"
        );
    }
    let payload = match request.view {
        View::Status => {
            let mut value = serde_json::to_value(super::status::from_connection(&conn, &run)?)?;
            let tasks = value["tasks"].as_array_mut().unwrap();
            let total = tasks.len() as u64;
            let items: Vec<_> = tasks
                .iter()
                .skip(request.after as usize)
                .take(request.limit)
                .cloned()
                .collect();
            value["tasks"] = json!(items);
            value["page"] = pagination(request.after, items.len(), request.limit, total, rev);
            value["pending_verification"] =
                json!(!value["verification_complete"].as_bool().unwrap_or(false));
            let mut query =
                conn.prepare("SELECT state,count(*) FROM tasks WHERE run_id=?1 GROUP BY state")?;
            let mut counts = serde_json::Map::new();
            for row in
                query.query_map([&run], |r| Ok((r.get::<_, String>(0)?, nonnegative(r, 1)?)))?
            {
                let (state, count) = row?;
                counts.insert(state, json!(count));
            }
            value["task_counts"] = json!(counts);
            value["elapsed"] = elapsed(&conn, &run)?;
            value["objective"] = conn
                .query_row(
                    "SELECT json_extract(plan,'$.context.brief') FROM runs WHERE id=?1",
                    [&run],
                    |r| r.get::<_, Option<String>>(0),
                )?
                .into();
            value["pending_required_operations"]=json!(conn.query_row("SELECT count(*) FROM operations WHERE run_id=?1 AND required=1 AND state<>'passed'",[&run],|r|nonnegative(r,0))?);
            value["next_automatic_action"] = if table(&conn, "due_work")? {
                conn.query_row("SELECT id,kind,due_ms,state FROM due_work WHERE run_id=?1 AND state IN ('pending','queued','admitted') ORDER BY due_ms,id LIMIT 1",[&run],|r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"due_ms":r.get::<_,i64>(2)?,"state":r.get::<_,String>(3)?,"source":"persisted_due_work; controller availability unobserved"}))).optional()?.unwrap_or(Value::Null)
            } else {
                Value::Null
            };
            value
        }
        View::Tasks | View::Task => tasks(&conn, workspace, &run, request, rev)?,
        View::Attempts => {
            let mut query=conn.prepare("SELECT id FROM attempts WHERE run_id=?1 AND (?2 IS NULL OR task_id=?2) ORDER BY rowid LIMIT ?3 OFFSET ?4")?;
            let ids = query
                .query_map(
                    params![
                        run,
                        request.task,
                        sql_limit(request.limit)?,
                        sql_cursor(request.after)?
                    ],
                    |r| r.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let items = ids
                .iter()
                .map(|id| attempt(&conn, workspace, &run, id))
                .collect::<Result<Vec<_>>>()?;
            let total: u64 = conn.query_row(
                "SELECT count(*) FROM attempts WHERE run_id=?1 AND (?2 IS NULL OR task_id=?2)",
                params![run, request.task],
                |r| nonnegative(r, 0),
            )?;
            json!({"run":run,"attempts":items,"page":pagination(request.after,items.len(),request.limit,total,rev)})
        }
        View::Attempt => {
            let id = required(&request.attempt, "attempt")?;
            json!({"run":run,"attempt":attempt(&conn,workspace,&run,&id)?})
        }
        View::Activity => {
            let task = required(&request.task, "task")?;
            check_task(&conn, &run, &task)?;
            let attempts = attempt_ids(&conn, &run, &task, request.after, request.limit)?;
            let total: u64 = conn.query_row(
                "SELECT count(*) FROM attempts WHERE run_id=?1 AND task_id=?2",
                [&run, &task],
                |r| nonnegative(r, 0),
            )?;
            let records = attempts
                .iter()
                .map(|id| controller::activity::projection(&conn, id))
                .collect::<Result<Vec<_>>>()?;
            json!({"run":run,"task":task,"clock":clock(&conn,&run,&task)?,"attempts":records,"page":pagination(request.after,records.len(),request.limit,total,rev)})
        }
        View::Operations => {
            let snapshot = super::status::from_connection(&conn, &run)?;
            let total = snapshot.operations.len() as u64;
            let operations: Vec<_> = snapshot
                .operations
                .into_iter()
                .skip(request.after as usize)
                .take(request.limit)
                .collect();
            json!({"run":run,"verification_complete":snapshot.verification_complete,"pending_verification":!snapshot.verification_complete,"page":pagination(request.after,operations.len(),request.limit,total,rev),"operations":operations})
        }
        View::Decisions | View::Questions => records(&conn, &run, request)?,
        View::Progress | View::Errors => recent_events(&conn, &run, request)?,
        View::Commands => commands(&conn, workspace, &run, request, rev)?,
        View::Command => command(
            &conn,
            workspace,
            &run,
            &required(&request.command, "command")?,
        )?,
        View::Logs => logs(&conn, workspace, &run, request)?,
        View::Runs => unreachable!(),
    };
    Ok(attach(payload, meta))
}

// The legacy schema has no run-start or completion timestamp. First launch is
// the earliest execution evidence; completed runs use their last journal event,
// explicitly a recorded span rather than an invented completion time.
fn elapsed(conn: &Connection, run: &str) -> Result<Value> {
    let (start, cutoff, completed): (Option<String>, String, bool) = conn.query_row(
        "SELECT (SELECT recorded_at FROM journal WHERE run_id=?1 AND event='launch_intent' ORDER BY sequence LIMIT 1),
         CASE WHEN state='completed' THEN (SELECT recorded_at FROM journal WHERE run_id=?1 ORDER BY sequence DESC LIMIT 1)
         ELSE strftime('%Y-%m-%dT%H:%M:%fZ','now') END, state='completed' FROM runs WHERE id=?1",
        [run], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if start.is_none() {
        return Ok(
            json!({"started_at":null,"measured_at":null,"elapsed_ms":null,
            "measurement":"unavailable","start_source":"no persisted launch intent",
            "end_source":"no execution interval recorded"}),
        );
    }
    let ms: Option<i64> = conn.query_row(
        "SELECT max(0,CAST(round((julianday(?2)-julianday(?1))*86400000) AS INTEGER))",
        params![start, cutoff],
        |r| r.get(0),
    )?;
    Ok(
        json!({"started_at":start,"measured_at":cutoff,"elapsed_ms":ms,
        "measurement":if completed {"recorded_span"} else {"wall_elapsed"},
        "start_source":"first persisted launch intent; includes pauses and controller downtime",
        "end_source":if completed {"last persisted journal event; exact completion time was not recorded"} else {"read snapshot wall clock; controller liveness unobserved"}}),
    )
}

/// Newest first, with an exclusive sequence cursor. This reads the run journal,
/// never a selected task's raw output or the whole retained transcript.
fn recent_events(conn: &Connection, run: &str, request: &Request) -> Result<Value> {
    let errors_only = matches!(request.view, View::Errors);
    let mut query = conn.prepare(
        "SELECT j.sequence,j.task_id,j.attempt_id,j.event,j.detail,j.recorded_at,
         CASE WHEN j.event IN ('result_accepted','process_retired') THEN a.summary ELSE NULL END
         FROM journal j LEFT JOIN attempts a ON a.id=j.attempt_id AND a.run_id=j.run_id
         WHERE j.run_id=?1 AND (?2=0 OR j.sequence<?2) AND
         (?3=0 OR (j.event='process_retired' AND j.detail IN ('failed','needs_intervention'))
          OR (j.event='activity' AND CASE WHEN json_valid(j.detail) THEN json_extract(j.detail,'$.severity')='error' ELSE 0 END))
         ORDER BY j.sequence DESC LIMIT ?4",
    )?;
    let mut events = query
        .query_map(
            params![
                run,
                sql_cursor(request.after)?,
                errors_only,
                sql_limit(request.limit + 1)?
            ],
            |r| {
                let raw: String = r.get(4)?;
                Ok(
                    json!({"sequence":nonnegative(r,0)?,"task":r.get::<_,Option<String>>(1)?,
            "attempt":r.get::<_,Option<String>>(2)?,"event":r.get::<_,String>(3)?,
            "detail":serde_json::from_str::<Value>(&raw).unwrap_or(Value::String(raw)),
            "recorded_at":r.get::<_,String>(5)?,"summary":r.get::<_,Option<String>>(6)?}),
                )
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let more = events.len() > request.limit;
    events.truncate(request.limit);
    let next = events
        .last()
        .map(|e| e["sequence"].as_u64().unwrap())
        .unwrap_or(request.after);
    Ok(
        json!({"events":events,"page":{"after":request.after,"next_after":next,"has_more":more,"limit":request.limit,"order":"sequence_desc","cursor":"exclusive_before_sequence"},
        "coverage":if errors_only {"Recorded activity errors and failed/intervention task retirements across this project. Raw stderr and unjournaled command failures are not classified."} else {"Persisted project events across all tasks, newest first. Raw executor output is available separately per attempt."}}),
    )
}

fn check_task(conn: &Connection, run: &str, task: &str) -> Result<()> {
    ensure!(
        conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE run_id=?1 AND id=?2)",
            [run, task],
            |r| r.get::<_, bool>(0)
        )?,
        "unknown task"
    );
    Ok(())
}
fn clock(conn: &Connection, run: &str, task: &str) -> Result<Value> {
    if table(conn, "task_clocks")?
        && conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM task_clocks WHERE run_id=?1 AND task_id=?2)",
            [run, task],
            |r| r.get::<_, bool>(0),
        )?
    {
        return controller::clock::projection(conn, run, task);
    }
    Ok(Value::Null)
}
fn attempt_ids(
    conn: &Connection,
    run: &str,
    task: &str,
    after: u64,
    limit: usize,
) -> Result<Vec<String>> {
    let mut query = conn.prepare(
        "SELECT id FROM attempts WHERE run_id=?1 AND task_id=?2 ORDER BY rowid LIMIT ?3 OFFSET ?4",
    )?;
    Ok(query
        .query_map(
            params![run, task, sql_limit(limit)?, sql_cursor(after)?],
            |r| r.get(0),
        )?
        .collect::<rusqlite::Result<_>>()?)
}
fn attempt(conn: &Connection, workspace: &Workspace, run: &str, id: &str) -> Result<Value> {
    crate::domain::validation::identifier(id)?;
    let mut value=conn.query_row("SELECT task_id,state,pid,outcome,summary,exit_code,effective_profile FROM attempts WHERE run_id=?1 AND id=?2",[run,id],|r|Ok(json!({"id":id,"task":r.get::<_,String>(0)?,"process_state":r.get::<_,String>(1)?,"pid":r.get::<_,Option<u32>>(2)?,"outcome":r.get::<_,Option<String>>(3)?,"summary":r.get::<_,Option<String>>(4)?,"exit_code":r.get::<_,Option<i32>>(5)?,"effective_profile":r.get::<_,String>(6)?}))).context("unknown attempt")?;
    value["effective_profile"] =
        serde_json::from_str(value["effective_profile"].as_str().unwrap())?;
    let receipt: Option<(String, String)> = conn
        .query_row(
            "SELECT id,payload FROM receipts WHERE attempt_id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    value["result_provenance"]=json!(receipt.map(|(id,payload)|->Result<Value>{Ok(json!({"receipt_id":id,"source":"designated_receipt","payload":serde_json::from_str::<Value>(&payload)?}))}).transpose()?);
    value["activity"] = controller::activity::projection(conn, id)?;
    value["review"] = controller::reviews::projection(conn, id)?;
    let task = value["task"].as_str().unwrap().to_owned();
    value["clock"] = clock(conn, run, &task)?;
    value["retry_authorizations"] = ledger(conn, run, &task)?;
    let mut command_request = Request::new(View::Commands, Some(run));
    command_request.task = Some(task.clone());
    value["commands"] = commands(conn, workspace, run, &command_request, revision(conn, run)?)?;
    value["reservations"] = reservations(workspace, run, &task, Some(id))?;
    let dir = workspace.data.join("attempts").join(id);
    let mut native = Value::Null;
    for executor in ["codex", "claude", "opencode"] {
        let path = dir.join(format!("{executor}-observation.json"));
        match std::fs::metadata(&path) {
            Ok(_) => {
                native = super::output::json_file(&path)?;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    value["native_observation"] = native;
    value["capture"] = json!({"state":"unknown_until_paged","raw_retained":null,"hidden_reasoning":"unavailable","native_helpers":{"state":"unobserved","count":null}});
    Ok(value)
}
fn tasks(
    conn: &Connection,
    workspace: &Workspace,
    run: &str,
    request: &Request,
    rev: Option<u32>,
) -> Result<Value> {
    let single = matches!(request.view, View::Task);
    if single {
        check_task(conn, run, &required(&request.task, "task")?)?;
    }
    let mut query=conn.prepare("SELECT id,state,spec FROM tasks WHERE run_id=?1 AND (?2 IS NULL OR id=?2) ORDER BY ordinal,id LIMIT ?3 OFFSET ?4")?;
    let mut rows = query.query(params![
        run,
        if single {
            request.task.as_deref()
        } else {
            None
        },
        sql_limit(request.limit)?,
        sql_cursor(request.after)?
    ])?;
    let mut items = Vec::new();
    while let Some(row) = rows.next()? {
        let id: String = row.get(0)?;
        let state: String = row.get(1)?;
        let spec: Value = serde_json::from_str(&row.get::<_, String>(2)?)?;
        let current:Option<String>=conn.query_row("SELECT id FROM attempts WHERE run_id=?1 AND task_id=?2 ORDER BY rowid DESC LIMIT 1",[run,&id],|r|r.get(0)).optional()?;
        let mut item = json!({"id":id,"state":state,"deliverable":spec["title"],"depends_on":spec["depends_on"],"spec":spec,"current_attempt":current,"lineage_root":controller::graph::root(conn,run,&id)?,"clock":clock(conn,run,&id)?,"cooldown":controller::cooldowns::read(conn,run,&id,controller::admission::now_ms())?,"verification_pending":state=="verification_pending"});
        if single {
            item["current_attempt_detail"] = current
                .as_ref()
                .map(|id| attempt(conn, workspace, run, id))
                .transpose()?
                .unwrap_or(Value::Null);
            item["retry_authorizations"] = ledger(conn, run, &id)?;
            item["reservations"] = reservations(workspace, run, &id, None)?;
            item["acceptance_policy"] = conn
                .query_row(
                    "SELECT json_extract(plan,'$.completion') FROM runs WHERE id=?1",
                    [run],
                    |r| r.get::<_, String>(0),
                )?
                .into();
            let total: u64 = conn.query_row(
                "SELECT count(*) FROM attempts WHERE run_id=?1 AND task_id=?2",
                [run, &id],
                |r| nonnegative(r, 0),
            )?;
            item["attempt_history"] = json!({"ids":attempt_ids(conn,run,&id,0,MAX_ITEMS)?,"total":total,"has_more":total>MAX_ITEMS as u64,"continuation":"read attempts --task <task> --after <offset>"});
        }
        items.push(item);
    }
    let total: u64 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE run_id=?1 AND (?2 IS NULL OR id=?2)",
        params![
            run,
            if single {
                request.task.as_deref()
            } else {
                None
            }
        ],
        |r| nonnegative(r, 0),
    )?;
    Ok(
        json!({"run":run,"tasks":items,"page":pagination(request.after,items.len(),request.limit,total,rev)}),
    )
}
fn ledger(conn: &Connection, run: &str, task: &str) -> Result<Value> {
    if !table(conn, "recovery_lineages")? || !table(conn, "recovery_invocations")? {
        return Ok(json!({"state":"not_initialized"}));
    }
    let root = controller::graph::root(conn, run, task)?;
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM recovery_lineages WHERE run_id=?1 AND task_id=?2)",
        [run, &root],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(json!({"state":"not_initialized"}));
    }
    Ok(
        json!({"state":"available","lineage_root":root,"ledger":controller::recovery::ledger(conn,run,task)?}),
    )
}
fn reservations(
    workspace: &Workspace,
    run: &str,
    task: &str,
    attempt: Option<&str>,
) -> Result<Value> {
    let path = controller::admission::registry_path(workspace);
    match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({"state":"not_initialized","records":null}));
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("PRAGMA query_only=ON; BEGIN")?;
    ensure!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))?
            == crate::resources::registry::PROTOCOL_VERSION,
        "unsupported resource registry schema"
    );
    let mut query=conn.prepare("SELECT record FROM reservations WHERE json_extract(record,'$.owner.run')=?1 AND json_extract(record,'$.owner.task')=?2 AND (?3 IS NULL OR json_extract(record,'$.owner.attempt')=?3) AND json_extract(record,'$.owner.checkout')=?4 ORDER BY id LIMIT 101")?;
    let checkout = crate::resources::identity::checkout_key(&workspace.checkout)?;
    let records = query
        .query_map(params![run, task, attempt, checkout], |r| {
            r.get::<_, String>(0)
        })?
        .map(|r| Ok(serde_json::from_str::<Value>(&r?)?))
        .collect::<Result<Vec<_>>>()?;
    Ok(
        json!({"state":"available","source":"separate_read_only_advisory_registry_snapshot; not process liveness","has_more":records.len()>MAX_ITEMS,"records":records.into_iter().take(MAX_ITEMS).collect::<Vec<_>>()}),
    )
}
fn records(conn: &Connection, run: &str, request: &Request) -> Result<Value> {
    let questions = matches!(request.view, View::Questions);
    let mut query=conn.prepare("SELECT sequence,task_id,attempt_id,event,detail,recorded_at FROM journal WHERE run_id=?1 AND sequence>?2 AND (?3 IS NULL OR task_id=?3) AND ((?4=1 AND event='restricted_question') OR (?4=0 AND event IN ('autonomous_decision','restricted_question','restricted_answer'))) ORDER BY sequence LIMIT ?5")?;
    let mut rows = query.query(params![
        run,
        sql_cursor(request.after)?,
        request.task,
        questions,
        sql_limit(request.limit)?
    ])?;
    let mut items = Vec::new();
    let mut next = request.after;
    while let Some(row) = rows.next()? {
        next = nonnegative(row, 0)?;
        let detail: Value = serde_json::from_str(&row.get::<_, String>(4)?)?;
        let record = json!({"sequence":next,"task":row.get::<_,Option<String>>(1)?,"attempt":row.get::<_,Option<String>>(2)?,"event":row.get::<_,String>(3)?,"detail":detail,"recorded_at":row.get::<_,String>(5)?,"provenance":"authoritative_journal"});
        if questions {
            let answer:Option<String>=conn.query_row("SELECT detail FROM journal WHERE run_id=?1 AND event='restricted_answer' AND json_extract(detail,'$.question_id')=?2 ORDER BY sequence LIMIT 1",params![run,detail["request_id"].as_str()],|r|r.get(0)).optional()?;
            let pending = answer.is_none();
            items.push(json!({"question":record,"answer":answer.map(|s|serde_json::from_str::<Value>(&s)).transpose()?,"pending":pending,"resume":"explicit bounded retry after designated blocked outcome and custody retirement"}));
        } else {
            items.push(record);
        }
    }
    let has_more:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM journal WHERE run_id=?1 AND sequence>?2 AND (?3 IS NULL OR task_id=?3) AND ((?4=1 AND event='restricted_question') OR (?4=0 AND event IN ('autonomous_decision','restricted_question','restricted_answer'))))",params![run,sql_cursor(next)?,request.task,questions],|r|r.get(0))?;
    Ok(
        json!({"run":run,"records":items,"page":{"after":request.after,"next_after":next,"limit":request.limit,"has_more":has_more,"cursor_kind":"journal_sequence"}}),
    )
}
fn command(conn: &Connection, workspace: &Workspace, run: &str, id: &str) -> Result<Value> {
    ensure!(
        table(conn, "managed_commands")?,
        "command state unavailable"
    );
    let mut value=conn.query_row("SELECT request_id,task_id,attempt_id,state,pid,exit_code,reason FROM managed_commands WHERE run_id=?1 AND id=?2",[run,id],|r|Ok(json!({"run":run,"command_id":id,"request_id":r.get::<_,String>(0)?,"task":r.get::<_,Option<String>>(1)?,"attempt":r.get::<_,Option<String>>(2)?,"state":r.get::<_,String>(3)?,"pid":r.get::<_,Option<u32>>(4)?,"exit_code":r.get::<_,Option<i32>>(5)?,"reason":r.get::<_,Option<String>>(6)?,"custody_observation":"persisted; current process liveness unobserved"}))).context("unknown command")?;
    value["recorded_state"] = value["state"].clone();
    value["state_source"] = json!("persisted receipt; active process custody unobserved");
    if matches!(
        value["state"].as_str(),
        Some("intent" | "running" | "stopping")
    ) {
        value["state"] = json!("unknown");
    }
    crate::domain::validation::identifier(id)?;
    for stream in ["stdout", "stderr"] {
        let path = workspace
            .data
            .join("commands")
            .join(id)
            .join(format!("{stream}.log"));
        let cursor = match std::fs::metadata(path) {
            Ok(meta) => {
                ensure!(meta.is_file(), "retained capture is not a file");
                Some(meta.len())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        value[format!("{stream}_cursor")] = json!(cursor);
    }
    Ok(value)
}
fn commands(
    conn: &Connection,
    workspace: &Workspace,
    run: &str,
    request: &Request,
    rev: Option<u32>,
) -> Result<Value> {
    if !table(conn, "managed_commands")? {
        return Ok(json!({"run":run,"commands":null,"state":"not_initialized"}));
    }
    let mut query=conn.prepare("SELECT id FROM managed_commands WHERE run_id=?1 AND (?2 IS NULL OR task_id=?2) ORDER BY rowid LIMIT ?3 OFFSET ?4")?;
    let ids = query
        .query_map(
            params![
                run,
                request.task,
                sql_limit(request.limit)?,
                sql_cursor(request.after)?
            ],
            |r| r.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let items = ids
        .iter()
        .map(|id| command(conn, workspace, run, id))
        .collect::<Result<Vec<_>>>()?;
    let total: u64 = conn.query_row(
        "SELECT count(*) FROM managed_commands WHERE run_id=?1 AND (?2 IS NULL OR task_id=?2)",
        params![run, request.task],
        |r| nonnegative(r, 0),
    )?;
    Ok(
        json!({"run":run,"commands":items,"page":pagination(request.after,items.len(),request.limit,total,rev)}),
    )
}
fn logs(conn: &Connection, workspace: &Workspace, run: &str, request: &Request) -> Result<Value> {
    ensure!(
        matches!(request.stream.as_str(), "stdout" | "stderr"),
        "stream must be stdout or stderr"
    );
    let (kind, id) = if let Some(id) = &request.command {
        command(conn, workspace, run, id)?;
        ("commands", id.clone())
    } else {
        let id = if let Some(id) = &request.attempt {
            id.clone()
        } else {
            let task = required(&request.task, "task or attempt or command")?;
            conn.query_row("SELECT id FROM attempts WHERE run_id=?1 AND task_id=?2 ORDER BY rowid DESC LIMIT 1",[run,&task],|r|r.get::<_,String>(0)).context("task has no attempt capture")?
        };
        ensure!(
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM attempts WHERE run_id=?1 AND id=?2)",
                [run, &id],
                |r| r.get::<_, bool>(0)
            )?,
            "unknown attempt in run"
        );
        ("attempts", id)
    };
    // Identity is verified in SQLite before forming an owned capture path.
    crate::domain::validation::identifier(&id)?;
    let path = workspace
        .data
        .join(kind)
        .join(&id)
        .join(format!("{}.log", request.stream));
    let mut value = super::output::page(&path, request.after, request.limit)?;
    value["run"] = json!(run);
    value["stream"] = json!(request.stream);
    value["capture_id"] = json!(id);
    value["capture_kind"] = json!(kind);
    if kind == "commands" {
        let receipt = command(conn, workspace, run, &id)?;
        value["command_id"] = json!(id);
        value["state"] = receipt["state"].clone();
        value["recorded_state"] = receipt["recorded_state"].clone();
        value["custody_observation"] = receipt["custody_observation"].clone();
    }
    value["provenance"] = json!({"source":"retained_raw_file","cursor_kind":"byte_offset","snapshot":"file_length_at_open; independent of journal snapshot"});
    Ok(value)
}
