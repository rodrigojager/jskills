use crate::domain::types::Receipt;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

const MAX_REQUEST: u64 = 65536;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Result { token: String, receipt: Receipt },
    Stop { token: String, run: String },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
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
