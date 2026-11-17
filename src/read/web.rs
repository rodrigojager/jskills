//! Controller-owned loopback HTTP reader. Polling this service never schedules work.
use super::projections::{self, Request, View};
use crate::storage::Workspace;
use anyhow::{Result, bail, ensure};
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    time::{Duration, Instant},
};

const MAX_HEADERS: usize = 8192;
const MAX_CONNECTIONS: usize = 8;
struct Connection {
    stream: TcpStream,
    input: Vec<u8>,
    output: Option<Vec<u8>>,
    written: usize,
    started: Instant,
}

pub struct Server {
    listener: TcpListener,
    workspace: Workspace,
    run: String,
    connections: Vec<Connection>,
}
impl Server {
    pub fn bind(workspace: &Workspace, run: &str) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            workspace: workspace.clone(),
            run: run.into(),
            connections: Vec::new(),
        })
    }
    /// Actual address allocated by this instance; contains no control credential.
    pub fn url(&self) -> String {
        format!(
            "http://{}/",
            self.listener.local_addr().expect("bound listener")
        )
    }
    /// Bounded nonblocking socket work in the foreground controller's own loop.
    /// Slow/closed readers are discarded and never become scheduling signals.
    pub fn poll(&mut self) -> Result<()> {
        for _ in self.connections.len()..MAX_CONNECTIONS {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true)?;
                    self.connections.push(Connection {
                        stream,
                        input: Vec::new(),
                        output: None,
                        written: 0,
                        started: Instant::now(),
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            }
        }
        let host = self.listener.local_addr()?.to_string();
        let mut reads = 0;
        self.connections.retain_mut(|c| {
            if c.started.elapsed() > Duration::from_secs(2) {
                return false;
            }
            if c.output.is_none() && reads < 2 {
                let mut bytes = [0; 2048];
                match c.stream.read(&mut bytes) {
                    Ok(0) => return false,
                    Ok(n) => c.input.extend_from_slice(&bytes[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(_) => return false,
                }
                if c.input.len() > MAX_HEADERS {
                    c.output = Some(response(
                        431,
                        "application/json",
                        br#"{"error":"headers too large"}"#,
                    ));
                } else if c.input.windows(4).any(|w| w == b"\r\n\r\n") {
                    reads += 1;
                    c.output = Some(match route(&self.workspace, &self.run, &host, &c.input) {
                        Ok(value) => value,
                        Err(e) => response(
                            400,
                            "application/json",
                            json!({"error":e.to_string(),"read_only":true})
                                .to_string()
                                .as_bytes(),
                        ),
                    });
                }
            }
            if let Some(output) = &c.output {
                // Bound each write even when a snapshot is comparatively large.
                let end = (c.written + 65536).min(output.len());
                match c.stream.write(&output[c.written..end]) {
                    Ok(0) => return false,
                    Ok(n) => c.written += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => (),
                    Err(_) => return false,
                }
                if c.written == output.len() {
                    return false;
                }
            }
            true
        });
        Ok(())
    }
}

fn response(status: u16, kind: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 {status} Response\r\nContent-Type: {kind}; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
fn route(workspace: &Workspace, run: &str, host: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(bytes)?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    ensure!(
        parts.len() == 3 && parts[2] == "HTTP/1.1",
        "invalid HTTP request"
    );
    if parts[0] != "GET" {
        return Ok(response(
            405,
            "application/json",
            br#"{"error":"read routes accept GET only"}"#,
        ));
    }
    let mut hosts = 0;
    for line in lines.take_while(|s| !s.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid header"))?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "host" => {
                ensure!(value == host, "invalid local Host");
                hosts += 1;
            }
            "origin" => ensure!(
                value == format!("http://{host}"),
                "cross-origin reads refused"
            ),
            "sec-fetch-site" => ensure!(value != "cross-site", "cross-site reads refused"),
            "content-length" => ensure!(value == "0", "request body refused"),
            "transfer-encoding" => bail!("request body refused"),
            _ => (),
        }
    }
    ensure!(hosts == 1, "one local Host is required");
    let (path, query) = parts[1].split_once('?').unwrap_or((parts[1], ""));
    let (kind, body): (&str, &[u8]) = match path {
        "/" => ("text/html", include_bytes!("../../web/index.html")),
        "/app.js" => ("text/javascript", include_bytes!("../../web/app.js")),
        "/style.css" => ("text/css", include_bytes!("../../web/style.css")),
        "/api/read" => {
            let request = request(run, query)?;
            let body = serde_json::to_vec(&projections::read(workspace, &request)?)?;
            return Ok(response(200, "application/json", &body));
        }
        _ => {
            return Ok(response(
                404,
                "application/json",
                br#"{"error":"unknown read route"}"#,
            ));
        }
    };
    Ok(response(200, kind, body))
}
fn decode(text: &str) -> Result<String> {
    let mut bytes = Vec::new();
    let mut input = text.as_bytes().iter().copied();
    while let Some(byte) = input.next() {
        bytes.push(match byte {
            b'+' => b' ',
            b'%' => {
                let hex: Vec<_> = input.by_ref().take(2).collect();
                ensure!(hex.len() == 2, "invalid percent encoding");
                u8::from_str_radix(std::str::from_utf8(&hex)?, 16)?
            }
            _ => byte,
        });
    }
    Ok(String::from_utf8(bytes)?)
}
fn request(run: &str, query: &str) -> Result<Request> {
    let mut fields = BTreeMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (key, value) = pair
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("invalid query"))?;
        let key = decode(key)?;
        ensure!(
            [
                "view", "run", "task", "attempt", "command", "after", "limit", "stream", "revision"
            ]
            .contains(&key.as_str()),
            "unknown read parameter"
        );
        ensure!(
            fields.insert(key, decode(value)?).is_none(),
            "duplicate read parameter"
        );
    }
    if let Some(selected) = fields.remove("run") {
        ensure!(selected == run, "dashboard is scoped to its controller run");
    }
    let view: View = serde_json::from_value(json!(
        fields.remove("view").unwrap_or_else(|| "status".into())
    ))?;
    ensure!(
        !matches!(view, View::Runs),
        "dashboard is scoped to its controller run"
    );
    let mut request = Request::new(view, Some(run));
    request.task = fields.remove("task");
    request.attempt = fields.remove("attempt");
    request.command = fields.remove("command");
    request.after = fields
        .remove("after")
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(0);
    request.limit = fields
        .remove("limit")
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(25);
    request.stream = fields.remove("stream").unwrap_or_else(|| "stdout".into());
    request.revision = fields.remove("revision").map(|v| v.parse()).transpose()?;
    Ok(request)
}
