use crate::domain::types::Receipt;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

/// Largest accepted control message, newline included.
pub const MAX_REQUEST: u64 = 65536;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Decision {
        token: String,
        attempt: String,
        task: String,
        decision: crate::domain::autonomy::Decision,
    },
    Question {
        token: String,
        attempt: String,
        task: String,
        question: crate::domain::autonomy::Question,
    },
    Answer {
        token: String,
        run: String,
        answer: crate::domain::autonomy::Answer,
    },
    Graph {
        token: String,
        run: String,
        proposal: Box<crate::domain::graph::Proposal>,
    },
    /// A graph proposal too large for one message, passed by reference: a file
    /// in the data folder's proposal directory and the SHA-256 of its bytes.
    GraphFile {
        token: String,
        run: String,
        path: std::path::PathBuf,
        sha256: String,
    },
    Log {
        token: String,
        attempt: String,
        task: String,
        kind: crate::controller::activity::Kind,
        severity: crate::controller::activity::Severity,
        content: Option<String>,
    },
    Result {
        token: String,
        receipt: Receipt,
    },
    Stop {
        token: String,
        run: String,
    },
    Retry {
        token: String,
        run: String,
        task: String,
        reason: String,
        request_id: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry: Option<serde_json::Value>,
    pub accepted: bool,
    pub duplicate: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ControlEndpoint {
    pub endpoint: SocketAddr,
    pub token: String,
    pub run: String,
}

pub fn send(endpoint: SocketAddr, request: &Request) -> Result<Response> {
    ensure!(
        endpoint.ip().is_loopback(),
        "control endpoint must be loopback"
    );
    let mut stream = TcpStream::connect_timeout(&endpoint, Duration::from_secs(2))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write_message(&mut stream, request)?;
    let response: Response = read_message(&mut stream)?;
    ensure!(
        response.accepted,
        "control request rejected: {}",
        response.error.as_deref().unwrap_or("unknown error")
    );
    Ok(response)
}

/// Whether a request fits in one control message.
pub fn fits(request: &Request) -> Result<bool> {
    Ok(serde_json::to_vec(request)?.len() as u64 + 1 <= MAX_REQUEST)
}

pub fn read_message<T: serde::de::DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    use std::io::Read;
    let mut line = String::new();
    BufReader::new(stream.take(MAX_REQUEST + 1)).read_line(&mut line)?;
    ensure!(
        line.len() as u64 <= MAX_REQUEST && line.ends_with('\n'),
        "invalid or oversized control message"
    );
    Ok(serde_json::from_str(&line)?)
}

pub fn write_message(stream: &mut TcpStream, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *stream, value)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}
