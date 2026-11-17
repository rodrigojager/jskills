use crate::{
    domain::autonomy::{Answer, Question},
    resources::ControllerGuard,
    storage::{Store, Workspace},
    transport::{self, ControlEndpoint, Request},
};
use anyhow::{Result, ensure};
use clap::Args;
use rusqlite::{Connection, OpenFlags};
use std::fs;

#[derive(Args)]
pub struct Questions {
    #[arg(long)]
    pub run: String,
}
#[derive(Args)]
pub struct Ask {
    #[command(flatten)]
    pub worker: super::decisions::Worker,
    #[arg(long)]
    pub request_id: String,
    #[arg(long)]
    pub question: String,
    #[arg(long)]
    pub blocked_requirement: String,
    #[arg(long = "option")]
    pub options: Vec<String>,
}
#[derive(Args)]
pub struct AnswerArgs {
    pub id: String,
    #[arg(long)]
    pub run: String,
    #[arg(long)]
    pub request_id: String,
    #[arg(long)]
    pub actor: String,
    #[arg(long)]
    pub text: String,
}
pub fn ask(args: Ask) -> Result<i32> {
    let w = args.worker;
    println!(
        "{}",
        serde_json::to_string(&transport::send(
            w.endpoint,
            &Request::Question {
                token: w.token,
                attempt: w.attempt,
                task: w.task,
                question: Question {
                    request_id: args.request_id,
                    question: args.question,
                    blocked_requirement: args.blocked_requirement,
                    options: args.options
                }
            }
        )?)?
    );
    Ok(0)
}
pub fn read(workspace: &Workspace, run: &str, questions: bool) -> Result<i32> {
    ensure!(workspace.database().is_file(), "no runtime database");
    let conn = Connection::open_with_flags(
        workspace.database(),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("BEGIN")?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM runs WHERE id=?1)",
        [run],
        |r| r.get(0),
    )?;
    ensure!(exists, "unknown run");
    let records = if questions {
        crate::controller::questions::questions(&conn, run)?
    } else {
        crate::controller::questions::read(&conn, run)?
    };
    println!(
        "{}",
        serde_json::json!({"run":run,"records":records,"read_only":true})
    );
    Ok(0)
}
pub fn answer(workspace: &Workspace, args: AnswerArgs) -> Result<i32> {
    let answer = Answer {
        question_id: args.id,
        request_id: args.request_id,
        actor: args.actor,
        text: args.text,
    };
    // Only management clients can acquire the controller lock or read its token.
    // A worker's attempt token cannot authorize an answer.
    if let Ok(guard) = ControllerGuard::acquire(&workspace.git_dir) {
        ensure!(workspace.database().is_file(), "no runtime database");
        let store = Store::open(workspace, &guard)?;
        store.check_checkout(&args.run, &workspace.checkout)?;
        let duplicate =
            crate::controller::questions::answer(workspace, &store, &args.run, &answer)?;
        println!(
            "{}",
            serde_json::json!({"accepted":true,"duplicate":duplicate,"resume":"explicit bounded retry required"})
        );
    } else {
        let endpoint: ControlEndpoint =
            serde_json::from_slice(&fs::read(workspace.data.join("control.json"))?)?;
        ensure!(endpoint.run == args.run, "controller serves another run");
        println!(
            "{}",
            serde_json::to_string(&transport::send(
                endpoint.endpoint,
                &Request::Answer {
                    token: endpoint.token,
                    run: args.run,
                    answer
                }
            )?)?
        );
    }
    Ok(0)
}
