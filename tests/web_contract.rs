mod support;
use jtasks::{
    read::{
        projections::{self, Request, View},
        web::Server,
    },
    storage::Workspace,
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use support::{Fixture, task};

struct Serving {
    host: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Serving {
    fn new(workspace: &Workspace) -> Self {
        let mut server = Server::bind(workspace, "demo").unwrap();
        let host = server
            .url()
            .trim_start_matches("http://")
            .trim_end_matches('/')
            .to_owned();
        assert!(host.starts_with("127.0.0.1:"));
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                server.poll().unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
        });
        Self {
            host,
            stop,
            thread: Some(thread),
        }
    }
    fn get(&self, path: &str) -> (u16, Vec<u8>) {
        self.send("GET", path, &self.host, "")
    }
    fn send(&self, method: &str, path: &str, host: &str, headers: &str) -> (u16, Vec<u8>) {
        let mut socket = TcpStream::connect(&self.host).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            socket,
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\n{headers}\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        socket.read_to_end(&mut response).unwrap();
        let split = response.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let status = std::str::from_utf8(&response[..split])
            .unwrap()
            .split_whitespace()
            .nth(1)
            .unwrap()
            .parse()
            .unwrap();
        (status, response[split + 4..].to_vec())
    }
    fn json(&self, path: &str) -> Value {
        let (code, body) = self.get(path);
        assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }
}
impl Drop for Serving {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn files(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for entry in fs::read_dir(path).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            out.extend(files(&p));
        } else {
            out.insert(p.clone(), fs::read(p).unwrap());
        }
    }
    out
}

#[test]
fn real_http_uses_exact_shared_projections_without_control_or_runtime_writes() {
    let f = Fixture::new();
    f.import(serde_json::json!([task("A", &[], "silent", "")]));
    let w = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    fs::write(w.data.join("control.json"), "SENTINEL_SECRET_NEVER_READ").unwrap();
    let before = files(&w.data);
    let server = Serving::new(&w);
    for (view, query) in [
        (View::Status, "status"),
        (View::Tasks, "tasks"),
        (View::Task, "task&task=A"),
        (View::Operations, "operations"),
        (View::Decisions, "decisions"),
        (View::Commands, "commands"),
    ] {
        let mut request = Request::new(view, Some("demo"));
        request.limit = 25;
        if matches!(view, View::Task) {
            request.task = Some("A".into());
        }
        let http = server.json(&format!("/api/read?view={query}"));
        assert_eq!(http, projections::read(&w, &request).unwrap());
        assert!(!http.to_string().contains("SENTINEL_SECRET"));
    }
    assert_eq!(files(&w.data), before);
    for path in ["/", "/app.js", "/style.css"] {
        assert_eq!(server.get(path).0, 200);
    }
    let script = String::from_utf8(server.get("/app.js").1).unwrap();
    assert!(script.contains("fields.limit = 4096"));
    assert!(!script.contains("innerHTML"));
}

#[test]
fn only_scoped_read_routes_allow_get_and_incomplete_clients_do_not_block_poll() {
    let f = Fixture::new();
    f.import(serde_json::json!([task("A", &[], "silent", "")]));
    let w = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let server = Serving::new(&w);
    let before = files(&w.data);
    for route in [
        "/api/stop",
        "/api/retry",
        "/control.json",
        "/../runtime.sqlite",
    ] {
        assert_eq!(server.get(route).0, 404);
    }
    assert_eq!(server.send("POST", "/api/read", &server.host, "").0, 405);
    assert_eq!(server.send("GET", "/api/read", "evil.example", "").0, 400);
    assert_eq!(
        server
            .send(
                "GET",
                "/api/read",
                &server.host,
                "Origin: https://evil.example\r\n"
            )
            .0,
        400
    );
    for query in [
        "view=runs",
        "run=other",
        "view=status&view=tasks",
        "view=tasks&limit=101",
        "view=logs&limit=65537",
        "token=unknown",
        "task=%FF",
    ] {
        assert_eq!(server.get(&format!("/api/read?{query}")).0, 400);
    }
    let mut slow = TcpStream::connect(&server.host).unwrap();
    slow.write_all(b"GET / HTTP/1.1\r\n").unwrap();
    let start = Instant::now();
    assert_eq!(server.get("/").0, 200);
    assert!(start.elapsed() < Duration::from_secs(1));
    drop(slow);
    assert_eq!(files(&w.data), before);
}

#[test]
fn output_is_explicit_bounded_and_preserves_missing_capture_uncertainty() {
    let f = Fixture::new();
    f.import(serde_json::json!([task("A", &[], "silent", "")]));
    let w = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let guard = jtasks::resources::ControllerGuard::acquire(&w.git_dir).unwrap();
    let mut store = jtasks::storage::Store::open(&w, &guard).unwrap();
    let t = serde_json::from_value(task("A", &[], "silent", "")).unwrap();
    store.prepare_attempt(&w, "demo", &t, "a1").unwrap();
    drop(store);
    drop(guard);
    let server = Serving::new(&w);
    let missing = server.json("/api/read?view=logs&attempt=a1&limit=4096");
    assert_eq!(missing["capture_state"], "unknown");
    assert_eq!(missing["gap"]["reason"], "raw_capture_missing");
    let dir = w.data.join("attempts/a1");
    fs::create_dir_all(&dir).unwrap();
    let bytes = b"0123456789".repeat(1000);
    fs::write(dir.join("stdout.log"), &bytes).unwrap();
    let first = server.json("/api/read?view=logs&attempt=a1&limit=4096");
    assert_eq!(first["bytes"].as_array().unwrap().len(), 4096);
    assert_eq!(first["next_offset"], 4096);
    assert_eq!(first["has_more"], true);
    let second = server.json("/api/read?view=logs&attempt=a1&after=4096&limit=4096");
    assert_eq!(second["offset"], 4096);
    assert_eq!(second["next_offset"], 8192);
}
