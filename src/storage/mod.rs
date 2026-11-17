pub mod schema;

use crate::domain::{
    types::{Plan, Receipt, Task},
    validation,
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone)]
pub struct Workspace {
    pub checkout: PathBuf,
    pub git_dir: PathBuf,
    pub data: PathBuf,
}

impl Workspace {
    pub fn resolve(start: &Path, data_override: Option<&Path>) -> Result<Self> {
        let start = fs::canonicalize(start).context("cannot resolve workspace root")?;
        let container = start.ancestors().find_map(|root| {
            let text = fs::read_to_string(root.join("_work/WORKSPACE.md")).ok()?;
            let fields = front_matter(&text);
            (fields.get("jproject").map(String::as_str) == Some("1"))
                .then(|| (root.to_path_buf(), fields))
        });
        let candidate = match &container {
            Some((root, fields)) if !start.join(".git").exists() => root.join(
                fields
                    .get("product_dir")
                    .map(String::as_str)
                    .unwrap_or("product"),
            ),
            _ => start.clone(),
        };
        let git_path = |arg: &str| -> Result<PathBuf> {
            let output = Command::new("git")
                .arg("-C")
                .arg(&candidate)
                .args(["rev-parse", arg])
                .output()?;
            ensure!(output.status.success(), "no git checkout at workspace root");
            Ok(fs::canonicalize(String::from_utf8(output.stdout)?.trim())?)
        };
        let checkout = git_path("--show-toplevel")?;
        let git_dir = git_path("--absolute-git-dir")?;
        let base = container
            .as_ref()
            .map(|(r, _)| r.as_path())
            .unwrap_or(&checkout);
        let data = if let Some(path) = data_override {
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                base.join(path)
            }
        } else if let Some((root, fields)) = &container {
            root.join(
                fields
                    .get("jtasks_data")
                    .map(String::as_str)
                    .unwrap_or("_work/30-plan/jtasks"),
            )
            .join("v2")
        } else {
            git_dir.join("jtasks-v2")
        };
        Ok(Self {
            checkout,
            git_dir,
            data,
        })
    }

    pub fn database(&self) -> PathBuf {
        self.data.join("state.sqlite3")
    }
}

// The narrow front-matter convention follows legacy workspace.ts, without
// legacy global configuration or its fallback data directory.
fn front_matter(text: &str) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next().map(str::trim) != Some("---") {
        return fields;
    }
    for line in lines {
        if matches!(line.trim(), "---" | "...") {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(
                key.trim().into(),
                value
                    .split(" #")
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches(['\'', '"'])
                    .into(),
            );
        }
    }
    fields
}

pub struct Store {
    pub conn: Connection,
}

impl Store {
    /// Only a caller holding ControllerGuard may open a runtime writer.
    pub fn open(workspace: &Workspace, _guard: &crate::resources::ControllerGuard) -> Result<Self> {
        fs::create_dir_all(&workspace.data)?;
        let conn = Connection::open(workspace.database())?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            conn.execute_batch(schema::SQL)?;
        } else {
            ensure!(version == schema::VERSION, "unsupported runtime schema");
        }
        crate::controller::graph::initialize(&conn)?;
        crate::controller::recovery::initialize(&conn)?;
        crate::controller::cooldowns::initialize(&conn)?;
        crate::controller::activity::initialize(&conn)?;
        crate::controller::clock::initialize(&conn)?;
        crate::controller::due::initialize(&conn)?;
        crate::controller::reviews::initialize(&conn)?;
        crate::domain::autonomy::initialize(&conn)?;
        crate::controller::questions::initialize(&conn)?;
        Ok(Self { conn })
    }

    pub fn import(&mut self, workspace: &Workspace, run: &str, plan: &Plan) -> Result<()> {
        validation::identifier(run)?;
        validation::validate(plan)?;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO runs(id,checkout,plan,state) VALUES(?1,?2,?3,'ready')",
            params![
                run,
                workspace.checkout.to_string_lossy(),
                serde_json::to_string(plan)?
            ],
        )
        .context("run already exists or cannot be imported")?;
        for (ordinal, task) in plan.tasks.iter().enumerate() {
            tx.execute(
                "INSERT INTO tasks(run_id,id,ordinal,spec,state) VALUES(?1,?2,?3,?4,'pending')",
                params![run, task.id, ordinal as i64, serde_json::to_string(task)?],
            )?;
            crate::controller::recovery::ensure_lineage(&tx, run, &task.id, &task.recovery)?;
        }
        for (ordinal, operation) in plan.operations.iter().enumerate() {
            tx.execute("INSERT INTO operations(run_id,id,ordinal,spec,required,revision,state,request_id) VALUES(?1,?2,?3,?4,?5,?6,'pending',?7)", params![run, operation.id, ordinal as i64, serde_json::to_string(operation)?, operation.required, operation.revision, format!("operation-{}", uuid::Uuid::new_v4())])?;
        }
        tx.execute(
            "INSERT INTO journal(run_id,event,detail) VALUES(?1,'imported',?2)",
            params![run, "explicit judgment completion policy"],
        )?;
        tx.execute(
            "INSERT INTO graph_baselines(run_id,original_plan) VALUES(?1,?2)",
            params![run, serde_json::to_string(plan)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn check_checkout(&self, run: &str, checkout: &Path) -> Result<()> {
        let owner: String = self
            .conn
            .query_row("SELECT checkout FROM runs WHERE id=?1", [run], |r| r.get(0))
            .context("unknown run; import a plan first")?;
        ensure!(
            Path::new(&owner) == checkout,
            "run belongs to another checkout"
        );
        Ok(())
    }

    pub fn set_run_state(&self, run: &str, state: &str) -> Result<()> {
        self.conn
            .execute("UPDATE runs SET state=?2 WHERE id=?1", [run, state])?;
        Ok(())
    }

    pub fn ready(&self, run: &str) -> Result<Option<Task>> {
        let mut stmt = self
            .conn
            .prepare("SELECT spec,state,id FROM tasks WHERE run_id=?1 ORDER BY ordinal")?;
        let rows = stmt
            .query_map([run], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let states: HashMap<_, _> = rows
            .iter()
            .map(|(_, state, id)| (id.as_str(), state.as_str()))
            .collect();
        for (spec, state, _) in &rows {
            if state != "pending" && state != "interrupted" {
                continue;
            }
            let task: Task = serde_json::from_str(spec)?;
            if task
                .depends_on
                .iter()
                .all(|dep| states.get(dep.as_str()) == Some(&"completed"))
                && crate::controller::operations::task_ready(self, run, &task.id)?
            {
                return Ok(Some(task));
            }
        }
        Ok(None)
    }

    pub fn has_unretired(&self, run: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row("SELECT count(*) FROM attempts WHERE run_id=?1 AND state IN ('launch_intent','running','uncertain')", [run], |r|r.get(0))?;
        Ok(count > 0)
    }

    pub fn intent(&mut self, run: &str, task: &str, attempt: &str) -> Result<()> {
        self.intent_with_profile(run, task, attempt, "null")
    }

    fn intent_with_profile(
        &mut self,
        run: &str,
        task: &str,
        attempt: &str,
        snapshot: &str,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO attempts(id,run_id,task_id,state,effective_profile) VALUES(?1,?2,?3,'launch_intent',?4)",
            [attempt, run, task, snapshot],
        )?;
        crate::controller::recovery::reserve(&tx, run, task, attempt)?;
        tx.execute(
            "UPDATE tasks SET state='running' WHERE run_id=?1 AND id=?2",
            [run, task],
        )?;
        tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'launch_intent','persisted before spawn')", [run,task,attempt])?;
        tx.commit()?;
        Ok(())
    }

    /// Resolve once at admission and persist exactly what this attempt launches.
    pub fn prepare_attempt(
        &mut self,
        workspace: &Workspace,
        run: &str,
        task: &Task,
        attempt: &str,
    ) -> Result<Task> {
        let plan: String =
            self.conn
                .query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
        let plan: Plan = serde_json::from_str(&plan)?;
        let (mut effective_task, mut snapshot) =
            crate::domain::profiles::resolve(workspace, task, &plan.profile_defaults)?;
        crate::controller::recovery::configure(&self.conn, run, &mut effective_task)?;
        crate::domain::autonomy::effective(&self.conn, run, &mut effective_task, &mut snapshot)?;
        snapshot.values["recovery_executor"] = serde_json::to_value(&effective_task.executor)?;
        self.intent_with_profile(run, &task.id, attempt, &serde_json::to_string(&snapshot)?)?;
        Ok(effective_task)
    }

    pub fn launched(&self, attempt: &str, pid: u32) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE attempts SET state='running',pid=?2 WHERE id=?1 AND state='launch_intent'",
            params![attempt, pid],
        )?;
        crate::controller::recovery::dispatched(&tx, attempt)?;
        let now = crate::controller::clock::utc_ms();
        crate::controller::activity::begin(&tx, attempt, now)?;
        crate::controller::reviews::begin(&tx, attempt, now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn accept(&mut self, run: &str, active: &str, receipt: &Receipt) -> Result<bool> {
        validation::identifier(&receipt.receipt_id)?;
        ensure!(
            !receipt.summary.trim().is_empty(),
            "a nonempty result summary is required"
        );
        ensure!(receipt.attempt_id == active, "stale attempt identity");
        let payload = serde_json::to_string(receipt)?;
        let tx = self.conn.transaction()?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT payload FROM receipts WHERE id=?1",
                [&receipt.receipt_id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(previous) = previous {
            ensure!(previous == payload, "conflicting receipt id");
            return Ok(false);
        }
        let identity: Option<(String, String)> = tx
            .query_row(
                "SELECT task_id,state FROM attempts WHERE id=?1 AND run_id=?2",
                [&receipt.attempt_id, run],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((task, state)) = identity else {
            bail!("unknown attempt");
        };
        ensure!(
            task == receipt.task_id && state == "running",
            "result does not designate the active task attempt"
        );
        tx.execute(
            "INSERT INTO receipts(id,attempt_id,payload) VALUES(?1,?2,?3)",
            params![receipt.receipt_id, receipt.attempt_id, payload],
        )
        .context("attempt already has a designated result")?;
        tx.execute(
            "UPDATE attempts SET outcome=?2,summary=?3 WHERE id=?1",
            params![
                receipt.attempt_id,
                receipt.outcome.as_str(),
                receipt.summary
            ],
        )?;
        tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'result_accepted',?4)", params![run,receipt.task_id,receipt.attempt_id,receipt.receipt_id])?;
        tx.commit()?;
        Ok(true)
    }

    pub fn retire(
        &mut self,
        run: &str,
        task: &str,
        attempt: &str,
        exit: Option<i32>,
        interrupted: bool,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        let outcome: Option<String> =
            tx.query_row("SELECT outcome FROM attempts WHERE id=?1", [attempt], |r| {
                r.get(0)
            })?;
        let state = if interrupted {
            "interrupted"
        } else {
            "retired"
        };
        tx.execute(
            "UPDATE attempts SET state=?2,exit_code=?3 WHERE id=?1",
            params![attempt, state, exit],
        )?;
        let mut task_state = outcome.as_deref().unwrap_or(if interrupted {
            "interrupted"
        } else {
            "needs_intervention"
        });
        let plan: String =
            tx.query_row("SELECT plan FROM runs WHERE id=?1", [run], |r| r.get(0))?;
        let plan: Plan = serde_json::from_str(&plan)?;
        if task_state == "completed"
            && crate::domain::operations::task_checks(&plan, task)
                .next()
                .is_some()
        {
            task_state = "verification_pending";
        }
        tx.execute(
            "UPDATE tasks SET state=?3 WHERE run_id=?1 AND id=?2",
            [run, task, task_state],
        )?;
        tx.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'process_retired',?4)", [run,task,attempt,task_state])?;
        tx.commit()?;
        Ok(())
    }
}
