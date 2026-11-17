//! Startup is the only repair path. Read projections never call this module.
use crate::{
    execution::custody,
    storage::{Store, Workspace},
};
use anyhow::{Result, ensure};

pub fn attempts(workspace: &Workspace, store: &mut Store, run: &str) -> Result<()> {
    let rows = {
        let mut statement = store.conn.prepare("SELECT id,task_id FROM attempts WHERE run_id=?1 AND state IN ('launch_intent','running','uncertain')")?;
        statement
            .query_map([run], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut unknown = false;
    for (id, task) in rows {
        if custody::reconcile(&workspace.data.join("attempts").join(&id)).unwrap_or(false) {
            store.retire(run, &task, &id, None, true)?;
        } else {
            store
                .conn
                .execute("UPDATE attempts SET state='uncertain' WHERE id=?1", [&id])?;
            store.conn.execute("INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,'custody_unknown','no replacement admitted')", [run,&task,&id])?;
            unknown = true;
        }
    }
    if unknown {
        store.set_run_state(run, "custody_unknown")?;
    }
    ensure!(
        !unknown,
        "unfinished launch/process custody is unknown; no replacement was launched"
    );
    Ok(())
}

pub fn commands(workspace: &Workspace, store: &Store) -> Result<()> {
    let rows = {
        let mut statement = store.conn.prepare("SELECT id FROM managed_commands WHERE state IN ('intent','running','stopping','unknown')")?;
        statement
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    for id in rows {
        let empty = custody::reconcile(&workspace.data.join("commands").join(&id)).unwrap_or(false);
        store.conn.execute(
            "UPDATE managed_commands SET state=?2,reason=?3 WHERE id=?1",
            rusqlite::params![
                id,
                if empty { "stopped" } else { "unknown" },
                if empty {
                    "prior owned scope observed empty"
                } else {
                    "prior controller custody requires reconciliation"
                }
            ],
        )?;
    }
    Ok(())
}
