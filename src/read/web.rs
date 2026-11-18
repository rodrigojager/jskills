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
    /// Per-controller page token: embedded in the served page, never in a URL or a log.
    token: String,
    stop: Option<String>,
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
            token: uuid::Uuid::new_v4().simple().to_string(),
            stop: None,
        })
    }
    /// The actor of a stop accepted (and already journaled) through the page, once.
    pub fn take_stop(&mut self) -> Option<String> {
        self.stop.take()
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
                } else if complete(&c.input) {
                    reads += 1;
                    let state = Posture {
                        workspace: &self.workspace,
                        run: &self.run,
                        host: &host,
                        token: &self.token,
                    };
                    c.output = Some(match route(&state, &mut self.stop, &c.input) {
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
struct Posture<'a> {
    workspace: &'a Workspace,
    run: &'a str,
    host: &'a str,
    token: &'a str,
}

fn forbidden() -> Vec<u8> {
    response(
        403,
        "application/json",
        br#"{"error":"forbidden: a valid page token and origin are required"}"#,
    )
}

/// Headers received and, when a body is announced, the whole announced body too.
fn complete(input: &[u8]) -> bool {
    let Some(end) = input.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&input[..end]).to_ascii_lowercase();
    let length = head
        .split("\r\n")
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    input.len() >= end + 4 + length
}

fn json_body(body: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str(body)? {
        serde_json::Value::Object(map) => Ok(map),
        _ => bail!("a JSON object is required"),
    }
}

fn text_field(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> Result<Option<String>> {
    match map.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() && s.len() <= 200 => {
            Ok(Some(s.clone()))
        }
        _ => bail!("{key} must be a non-empty string"),
    }
}

fn post(state: &Posture, stop: &mut Option<String>, path: &str, body: &str) -> Result<Vec<u8>> {
    let map = json_body(body)?;
    let actor = text_field(&map, "actor")?.unwrap_or_else(|| "web".into());
    let value = match path {
        "/api/stop" => {
            let conn = rusqlite::Connection::open_with_flags(
                state.workspace.database(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE
                    | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            conn.busy_timeout(crate::storage::WRITER_BUSY_TIMEOUT)?;
            conn.execute(
                "INSERT INTO journal(run_id,event,detail) VALUES(?1,'stop_requested',?2)",
                rusqlite::params![state.run, json!({"actor": actor}).to_string()],
            )?;
            *stop = Some(actor.clone());
            let body = json!({"stopping":true,"run":state.run,"actor":actor}).to_string();
            return Ok(response(202, "application/json", body.as_bytes()));
        }
        "/api/agents" => {
            let alias = text_field(&map, "alias")?.ok_or_else(|| anyhow::anyhow!("alias is required"))?;
            crate::cli::configure::set_alias(
                state.workspace,
                state.run,
                &alias,
                [
                    text_field(&map, "executor")?,
                    text_field(&map, "provider")?,
                    text_field(&map, "model")?,
                    text_field(&map, "effort")?,
                ],
                &actor,
            )?
        }
        _ => {
            return Ok(response(
                404,
                "application/json",
                br#"{"error":"unknown route"}"#,
            ));
        }
    };
    Ok(response(200, "application/json", value.to_string().as_bytes()))
}

fn route(state: &Posture, stop: &mut Option<String>, bytes: &[u8]) -> Result<Vec<u8>> {
    let (workspace, run, host) = (state.workspace, state.run, state.host);
    let split = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(bytes.len());
    let text = std::str::from_utf8(&bytes[..split])?;
    let body = std::str::from_utf8(&bytes[(split + 4).min(bytes.len())..])?;
    let mut lines = text.split("\r\n");
    let parts: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    ensure!(
        parts.len() == 3 && parts[2] == "HTTP/1.1",
        "invalid HTTP request"
    );
    let writes = parts[0] == "POST";
    if parts[0] != "GET" && !writes {
        return Ok(forbidden());
    }
    let mut hosts = 0;
    let mut supplied = None;
    let mut foreign = false;
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid header"))?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "host" => {
                ensure!(value == host, "invalid local Host");
                hosts += 1;
            }
            "origin" if writes => foreign |= value != format!("http://{host}"),
            "origin" => ensure!(
                value == format!("http://{host}"),
                "cross-origin reads refused"
            ),
            "sec-fetch-site" if writes => foreign |= value == "cross-site",
            "sec-fetch-site" => ensure!(value != "cross-site", "cross-site reads refused"),
            "x-jtasks-token" => supplied = Some(value.to_string()),
            "content-length" if !writes => ensure!(value == "0", "request body refused"),
            "transfer-encoding" => bail!("request body refused"),
            _ => (),
        }
    }
    ensure!(hosts == 1, "one local Host is required");
    let (path, query) = parts[1].split_once('?').unwrap_or((parts[1], ""));
    if writes {
        if foreign || supplied.as_deref() != Some(state.token) {
            return Ok(forbidden());
        }
        return post(state, stop, path, body);
    }
    let (kind, page): (&str, Vec<u8>) = match path {
        "/" => (
            "text/html",
            String::from_utf8_lossy(include_bytes!("../../web/index.html"))
                .replace("@@JTASKS_TOKEN@@", state.token)
                .into_bytes(),
        ),
        "/api/agents" => {
            let body = crate::cli::configure::list(workspace, run)?.to_string();
            return Ok(response(200, "application/json", body.as_bytes()));
        }
        "/api/agents/choices" => {
            let executor = query
                .split('&')
                .find_map(|p| p.strip_prefix("executor="))
                .map(decode)
                .transpose()?
                .ok_or_else(|| anyhow::anyhow!("executor is required"))?;
            let body = crate::cli::configure::choices(workspace, &executor)?.to_string();
            return Ok(response(200, "application/json", body.as_bytes()));
        }
        "/app.js" => ("text/javascript", include_bytes!("../../web/app.js").to_vec()),
        "/style.css" => ("text/css", include_bytes!("../../web/style.css").to_vec()),
        "/presentation.json" => {
            let body = serde_json::to_vec(&super::i18n::presentation())?;
            return Ok(response(200, "application/json", &body));
        }
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
    Ok(response(200, kind, &page))
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
