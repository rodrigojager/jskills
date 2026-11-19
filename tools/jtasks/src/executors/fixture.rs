use crate::{
    domain::types::{FixtureOutcome, Outcome, Receipt},
    transport::{self, Request},
};
use anyhow::{Context, Result};
use std::{net::SocketAddr, time::Duration};

/// Deterministic executor launched as a separate OS process by the controller.
pub fn execute(
    outcome: FixtureOutcome,
    summary: String,
    delay_ms: u64,
    linger_ms: u64,
    exit_code: i32,
) -> Result<i32> {
    std::thread::sleep(Duration::from_millis(delay_ms));
    let designated = match outcome {
        FixtureOutcome::Completed => Some(Outcome::Completed),
        FixtureOutcome::Blocked => Some(Outcome::Blocked),
        FixtureOutcome::Failed => Some(Outcome::Failed),
        FixtureOutcome::Silent => None,
        FixtureOutcome::Prose => {
            println!("Completed: {summary}");
            None
        }
        FixtureOutcome::HelperJson => {
            println!("{{\"outcome\":\"completed\",\"summary\":\"helper report\"}}");
            None
        }
        FixtureOutcome::Malformed => {
            use std::{io::Write, net::TcpStream};
            let mut stream = TcpStream::connect(std::env::var("JTASKS_CONTROL_ENDPOINT")?)?;
            stream.write_all(b"arbitrary result prose\n")?;
            None
        }
    };
    if let Some(outcome) = designated {
        let receipt = Receipt {
            receipt_id: uuid::Uuid::new_v4().to_string(),
            attempt_id: std::env::var("JTASKS_ATTEMPT_ID")?,
            task_id: std::env::var("JTASKS_TASK_ID")?,
            outcome,
            summary,
        };
        let endpoint: SocketAddr = std::env::var("JTASKS_CONTROL_ENDPOINT")?.parse()?;
        let request = Request::Result {
            token: std::env::var("JTASKS_RESULT_TOKEN")
                .context("fixture needs a controller-issued result scope")?,
            receipt,
        };
        transport::send(endpoint, &request)?;
        // Exercise durable idempotency through the public wire protocol.
        transport::send(endpoint, &request)?;
    }
    std::thread::sleep(Duration::from_millis(linger_ms));
    Ok(exit_code)
}
