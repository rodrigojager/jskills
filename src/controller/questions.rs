//! Only explicit scoped requests enter this journal. Reads never repair files.
use crate::{
    domain::autonomy::{Answer, Decision, Question},
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::{fs, io::Write};

fn text(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= 8192,
        "nonempty concise text required (maximum 8192 bytes)"
    );
    Ok(())
}
pub fn initialize(conn: &Connection) -> Result<()> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS autonomy_requests(request_id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id), task_id TEXT, attempt_id TEXT, event TEXT NOT NULL, payload TEXT NOT NULL, sequence INTEGER NOT NULL UNIQUE REFERENCES journal(sequence));")?;
    Ok(())
}
/// A designated blocked result retains a stopped question wait. Explicit retry
/// consumes the existing work allowance; asking/answering never grants one.
pub fn reconcile(conn: &Connection, run: &str) -> Result<()> {
    let mut query=conn.prepare("SELECT DISTINCT a.task_id FROM attempts a JOIN autonomy_requests q ON q.attempt_id=a.id AND q.event='restricted_question' JOIN tasks t ON t.run_id=a.run_id AND t.id=a.task_id WHERE a.run_id=?1 AND a.state='retired' AND a.outcome='blocked' AND t.state='blocked'")?;
    let tasks = query
        .query_map([run], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in tasks {
        let root = super::graph::root(conn, run, &task)?;
        conn.execute("UPDATE recovery_lineages SET next_kind='work',resume_attempt=NULL WHERE run_id=?1 AND task_id=?2 AND next_kind='initial'",[run,root.as_str()])?;
    }
    Ok(())
}
fn append(
    conn: &Connection,
    run: &str,
    task: &str,
    attempt: &str,
    id: &str,
    event: &str,
    payload: &Value,
) -> Result<bool> {
    crate::domain::validation::identifier(id)?;
    let payload = serde_json::to_string(payload)?;
    let tx = conn.unchecked_transaction()?;
    let prior: Option<(String,String,String,String,String)> = tx.query_row("SELECT run_id,task_id,attempt_id,event,payload FROM autonomy_requests WHERE request_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    if let Some(prior) = prior {
        ensure!(
            prior
                == (
                    run.into(),
                    task.into(),
                    attempt.into(),
                    event.into(),
                    payload
                ),
            "conflicting management request ID"
        );
        return Ok(true);
    }
    tx.execute(
        "INSERT INTO journal(run_id,task_id,attempt_id,event,detail) VALUES(?1,?2,?3,?4,?5)",
        params![run, task, attempt, event, payload],
    )?;
    tx.execute("INSERT INTO autonomy_requests(request_id,run_id,task_id,attempt_id,event,payload,sequence) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,run,task,attempt,event,payload,tx.last_insert_rowid()])?;
    tx.commit()?;
    Ok(false)
}
fn worker(conn: &Connection, run: &str, task: &str, attempt: &str) -> Result<Value> {
    let profile:String=conn.query_row("SELECT effective_profile FROM attempts WHERE id=?1 AND run_id=?2 AND task_id=?3 AND state='running'",[attempt,run,task],|r|r.get(0)).context("request must designate the active task attempt")?;
    Ok(serde_json::from_str(&profile)?)
}
pub fn decide(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    task: &str,
    attempt: &str,
    decision: &Decision,
) -> Result<bool> {
    worker(&store.conn, run, task, attempt)?;
    text(&decision.situation)?;
    text(&decision.decision)?;
    text(&decision.deviation)?;
    let duplicate = append(
        &store.conn,
        run,
        task,
        attempt,
        &decision.request_id,
        "autonomous_decision",
        &serde_json::to_value(decision)?,
    )?;
    project(workspace, &store.conn, run)?;
    Ok(duplicate)
}
pub fn ask(
    workspace: &Workspace,
    store: &Store,
    run: &str,
    task: &str,
    attempt: &str,
    question: &Question,
) -> Result<bool> {
    let profile = worker(&store.conn, run, task, attempt)?;
    ensure!(
        profile["values"]["autonomy"] == "restricted",
        "structured questions require explicitly restricted execution; decide routine matters and continue"
    );
    text(&question.question)?;
    text(&question.blocked_requirement)?;
    ensure!(question.options.len() <= 16, "maximum 16 options");
    for option in &question.options {
        text(option)?;
    }
    let duplicate = append(
        &store.conn,
        run,
        task,
        attempt,
        &question.request_id,
        "restricted_question",
        &serde_json::to_value(question)?,
    )?;
    project(workspace, &store.conn, run)?;
    Ok(duplicate)
}
pub fn answer(workspace: &Workspace, store: &Store, run: &str, answer: &Answer) -> Result<bool> {
    text(&answer.actor)?;
    text(&answer.text)?;
    crate::domain::validation::identifier(&answer.question_id)?;
    let (task,attempt,payload):(String,String,String)=store.conn.query_row("SELECT task_id,attempt_id,payload FROM autonomy_requests WHERE run_id=?1 AND request_id=?2 AND event='restricted_question'",[run,&answer.question_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).context("unknown question in this run")?;
    let _question: Question = serde_json::from_str(&payload)?;
    // A question itself never completes, blocks, stops, or resumes a worker.
    // The designated blocked outcome and an explicit bounded retry remain required.
    let answers = read(&store.conn, run)?;
    for record in answers.as_array().unwrap() {
        if record["event"] == "restricted_answer"
            && record["detail"]["question_id"] == answer.question_id
        {
            ensure!(
                record["detail"] == serde_json::to_value(answer)?,
                "question already answered; answers are immutable"
            );
        }
    }
    let duplicate = append(
        &store.conn,
        run,
        &task,
        &attempt,
        &answer.request_id,
        "restricted_answer",
        &serde_json::to_value(answer)?,
    )?;
    project(workspace, &store.conn, run)?;
    Ok(duplicate)
}
/// The SQLite journal is authoritative, including after a failed file write.
pub fn read(conn: &Connection, run: &str) -> Result<Value> {
    let mut query=conn.prepare("SELECT sequence,task_id,attempt_id,event,detail FROM journal WHERE run_id=?1 AND event IN ('autonomous_decision','restricted_question','restricted_answer') ORDER BY sequence")?;
    let mut out = Vec::new();
    for row in query.query_map([run], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })? {
        let (sequence, task, attempt, event, detail) = row?;
        out.push(json!({"sequence":sequence,"task":task,"attempt":attempt,"event":event,"detail":serde_json::from_str::<Value>(&detail)?}));
    }
    Ok(json!(out))
}
pub fn questions(conn: &Connection, run: &str) -> Result<Value> {
    let records = read(conn, run)?;
    let records = records.as_array().unwrap();
    let out:Vec<_>=records.iter().filter(|r|r["event"]=="restricted_question").map(|q| {
        let answer=records.iter().find(|a|a["event"]=="restricted_answer" && a["detail"]["question_id"]==q["detail"]["request_id"]);
        json!({"question":q,"answer":answer,"pending":answer.is_none(),"resume":"explicit bounded retry after designated blocked outcome and custody retirement"})
    }).collect();
    Ok(json!(out))
}
/// Controller/import recovery only; deterministic replacement is idempotent.
pub fn project(workspace: &Workspace, conn: &Connection, run: &str) -> Result<()> {
    crate::domain::validation::identifier(run)?;
    let records = read(conn, run)?;
    if records.as_array().unwrap().is_empty() {
        return Ok(());
    }
    let directory = workspace.data.join("runs").join(run);
    fs::create_dir_all(&directory)?;
    let body = format!(
        "# Decisions and restricted questions\n\nJournal sequence is authoritative. Explicit recorded claims only; no private reasoning or secrets.\n\n```json\n{}\n```\n",
        serde_json::to_string_pretty(&records)?
    );
    let pending = directory.join("decisions.pending.md");
    let mut file = fs::File::create(&pending)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(pending, directory.join("decisions.md"))?;
    Ok(())
}
