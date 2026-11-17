//! Thin consumers of the shared read contract; no control endpoint required.
use crate::{
    read::projections::{self, Request, View},
    storage::Workspace,
};
use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct Page {
    #[arg(long)]
    pub run: Option<String>,
    #[arg(long, default_value_t = 0)]
    pub after: u64,
    #[arg(long, default_value_t = 100)]
    pub limit: usize,
    #[arg(long)]
    pub revision: Option<u32>,
}
#[derive(Args)]
pub struct Read {
    #[arg(value_enum)]
    pub view: View,
    #[command(flatten)]
    pub page: Page,
    #[arg(long)]
    pub task: Option<String>,
    #[arg(long)]
    pub attempt: Option<String>,
    #[arg(long)]
    pub command: Option<String>,
    #[arg(long, default_value = "stdout")]
    pub stream: String,
}
#[derive(Args)]
pub struct Entity {
    pub id: String,
    #[command(flatten)]
    pub page: Page,
}
#[derive(Args)]
pub struct Logs {
    pub task: Option<String>,
    #[command(flatten)]
    pub page: Page,
    #[arg(long)]
    pub attempt: Option<String>,
    #[arg(long)]
    pub command: Option<String>,
    #[arg(long, default_value = "stdout")]
    pub stream: String,
}
impl Page {
    pub fn request(self, view: View) -> Request {
        let mut request = Request::new(view, self.run.as_deref());
        request.after = self.after;
        request.limit = self.limit;
        request.revision = self.revision;
        request
    }
}
pub fn execute(workspace: &Workspace, args: Read) -> Result<i32> {
    let mut request = args.page.request(args.view);
    request.task = args.task;
    request.attempt = args.attempt;
    request.command = args.command;
    request.stream = args.stream;
    print(workspace, &request)
}
pub fn print(workspace: &Workspace, request: &Request) -> Result<i32> {
    let value = projections::read(workspace, request)?;
    let unfinished = matches!(request.view, View::Status) && value["complete"] != true;
    println!("{}", serde_json::to_string(&value)?);
    Ok(if unfinished { 3 } else { 0 })
}
pub fn entity(workspace: &Workspace, args: Entity, view: View) -> Result<i32> {
    let mut request = args.page.request(view);
    match view {
        View::Task => request.task = Some(args.id),
        View::Attempt => request.attempt = Some(args.id),
        _ => unreachable!(),
    }
    print(workspace, &request)
}
pub fn logs(workspace: &Workspace, args: Logs) -> Result<i32> {
    let mut request = args.page.request(View::Logs);
    request.task = args.task;
    request.attempt = args.attempt;
    request.command = args.command;
    request.stream = args.stream;
    print(workspace, &request)
}
