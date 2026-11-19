use crate::domain::types::Receipt;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
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
        #[serde(default)]
        actor: Option<String>,
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

/// The endpoint folder of one run. Each run has its own, so controllers started from two checkouts that share one
/// data folder never overwrite each other's endpoint.
pub fn run_folder(data: &Path, run: &str) -> PathBuf {
    data.join("controllers").join(run)
}

/// The control endpoint file of one run's task controller.
pub fn control_file(data: &Path, run: &str) -> PathBuf {
    run_folder(data, run).join("control.json")
}

/// The managed-command endpoint file: a run controller's own, or the standalone `exec serve` one.
pub fn commands_file(data: &Path, run: Option<&str>) -> PathBuf {
    match run {
        Some(run) => run_folder(data, run).join("commands-control.json"),
        None => data.join("commands-control.json"),
    }
}

/// Write an endpoint file (owner-only on Unix), creating the run's folder.
pub fn write_endpoint_file(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    Ok(())
}

/// Whether a loopback endpoint accepts a connection now. The file of a controller that is gone does not.
pub fn answers(endpoint: SocketAddr) -> bool {
    endpoint.ip().is_loopback()
        && TcpStream::connect_timeout(&endpoint, Duration::from_millis(300)).is_ok()
}

/// The live task controller endpoint of `run`: its own file, or the shared file an older controller wrote when that
/// file names `run`. A missing, unreadable or stale file, or one of another run, gives `None`.
pub fn control_endpoint(data: &Path, run: &str) -> Option<ControlEndpoint> {
    crate::domain::validation::identifier(run).ok()?;
    [control_file(data, run), data.join("control.json")]
        .iter()
        .filter_map(|path| std::fs::read(path).ok())
        .filter_map(|bytes| serde_json::from_slice::<ControlEndpoint>(&bytes).ok())
        .find(|endpoint| endpoint.run == run && answers(endpoint.endpoint))
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
