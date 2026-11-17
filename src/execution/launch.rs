use crate::{
    domain::types::{Executor, Task},
    storage::Workspace,
};
use anyhow::Result;
use std::{
    fs::{self, File},
    net::SocketAddr,
    process::{Command, Stdio},
};

pub fn task(
    workspace: &Workspace,
    task: &Task,
    attempt: &str,
    endpoint: SocketAddr,
    token: &str,
) -> Result<super::custody::OwnedProcess> {
    let ticket = crate::domain::tickets::write_attempt_ticket(workspace, task, attempt)?;
    if let Executor::Codex { config } = &task.executor {
        return crate::executors::codex::launch(
            workspace, task, config, attempt, endpoint, token, &ticket,
        );
    }
    let Executor::Fixture {
        outcome,
        summary,
        delay_ms,
        linger_ms,
        exit_code,
    } = &task.executor
    else {
        unreachable!()
    };
    let output_dir = workspace.data.join("attempts").join(attempt);
    fs::create_dir_all(&output_dir)?;
    let stdout = File::create(output_dir.join("stdout.log"))?;
    let stderr = File::create(output_dir.join("stderr.log"))?;
    let outcome = serde_json::to_value(outcome)?
        .as_str()
        .unwrap()
        .replace('_', "-");
    // Each argument is a separate OS argument. No shell expansion or quoting.
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("fixture")
        .arg("--outcome")
        .arg(outcome)
        .arg("--summary")
        .arg(summary)
        .arg("--delay-ms")
        .arg(delay_ms.to_string())
        .arg("--linger-ms")
        .arg(linger_ms.to_string())
        .arg(format!("--exit-code={exit_code}"))
        .env("JTASKS_CONTROL_ENDPOINT", endpoint.to_string())
        .env("JTASKS_RESULT_TOKEN", token)
        .env("JTASKS_ATTEMPT_ID", attempt)
        .env("JTASKS_TASK_ID", &task.id)
        .env("JTASKS_TICKET_PATH", ticket)
        .current_dir(&workspace.checkout)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    super::custody::OwnedProcess::spawn(&mut command, &output_dir)
}
