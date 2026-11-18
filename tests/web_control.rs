mod support;
use jtasks::{read::web::Server, storage::Workspace};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    process::Output,
    time::Duration,
};
use support::Fixture;

fn call(f: &Fixture, args: &[&str]) -> Output {
    let mut c = f.command();
    c.env("JTASKS_CONFIG_HOME", f.root.join("isolated-user-config"));
    c.args(args).output().unwrap()
}
fn prepared() -> Fixture {
    let f = Fixture::new();
    let alias = json!({"executor":"claude","provider":"anthropic","model":"model-one","effort":"medium","executable":"python","launcher_args":["x.py"]});
    let file = f.root.join("alias-input.json");
    fs::write(&file, serde_json::to_vec(&alias).unwrap()).unwrap();
    let out = call(
        &f,
        &["profiles", "set", "standard", "--file", file.to_str().unwrap(), "--scope", "project"],
    );
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let exception = json!({"actor":"test","reason":"synthetic native protocol substitute, no account"});
    f.import(json!([
        {"id":"A","title":"First","unknown_capabilities":exception,"executor":{"kind":"profile","alias":"standard"}}
    ]));
    f
}
fn journal(f: &Fixture, event: &str) -> Vec<Value> {
    let c = Connection::open_with_flags(
        f.data.join("state.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    c.prepare("SELECT detail FROM journal WHERE event=?1 ORDER BY rowid")
        .unwrap()
        .query_map([event], |r| r.get::<_, String>(0))
        .unwrap()
        .map(|d| serde_json::from_str(&d.unwrap()).unwrap())
        .collect()
}

/// Sends one request, polling the server in between so the test needs no thread.
fn exchange(server: &mut Server, request: &str) -> (u16, String) {
    let host = server.url().trim_start_matches("http://").trim_end_matches('/').to_owned();
    let mut socket = TcpStream::connect(&host).unwrap();
    socket.set_read_timeout(Some(Duration::from_millis(5))).unwrap();
    socket.write_all(request.replace("@HOST@", &host).as_bytes()).unwrap();
    let mut response = Vec::new();
    for _ in 0..1000 {
        server.poll().unwrap();
        let mut chunk = [0; 4096];
        match socket.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => response.extend_from_slice(&chunk[..n]),
            Err(_) => (),
        }
    }
    let text = String::from_utf8(response).unwrap();
    let status = text.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, text.split_once("\r\n\r\n").unwrap().1.to_string())
}
fn post(server: &mut Server, path: &str, headers: &str, body: &Value) -> (u16, String) {
    let body = body.to_string();
    exchange(
        server,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: @HOST@\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}
fn page_token(server: &mut Server) -> String {
    let (code, page) = exchange(server, "GET / HTTP/1.1\r\nHost: @HOST@\r\n\r\n");
    assert_eq!(code, 200);
    let marker = r#"name="jtasks-token" content=""#;
    let start = page.find(marker).unwrap() + marker.len();
    page[start..start + page[start..].find('"').unwrap()].to_string()
}

#[test]
fn stop_and_agents_need_the_page_token_and_origin() {
    let f = prepared();
    let w = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let mut server = Server::bind(&w, "demo").unwrap();
    let token = page_token(&mut server);
    assert!(!token.is_empty() && !token.contains('@'));
    let own = |t: &str| format!("X-Jtasks-Token: {t}\r\n");
    let origin = |o: &str| format!("X-Jtasks-Token: {token}\r\nOrigin: {o}\r\n");
    let set = json!({"alias":"standard","model":"model-two","actor":"web-test"});

    // Refused: no token, another token, another origin, another method; nothing changes.
    for (path, body) in [("/api/stop", json!({"actor":"x"})), ("/api/agents", set.clone())] {
        assert_eq!(post(&mut server, path, "", &body).0, 403);
        assert_eq!(post(&mut server, path, &own("wrong"), &body).0, 403);
        assert_eq!(post(&mut server, path, &origin("http://evil.example"), &body).0, 403);
    }
    assert_eq!(
        exchange(&mut server, "PUT /api/stop HTTP/1.1\r\nHost: @HOST@\r\n\r\n").0,
        403
    );
    assert!(server.take_stop().is_none());
    assert!(journal(&f, "stop_requested").is_empty());
    assert!(journal(&f, "alias_changed").is_empty());

    // Agents with the token (own origin allowed explicitly) are journaled.
    let (code, body) = post(&mut server, "/api/agents", &own(&token), &set);
    assert_eq!(code, 200, "{body}");
    let entry: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(entry["alias"], "standard");
    assert_eq!(entry["configuration"]["model"], "model-two");
    let changes = journal(&f, "alias_changed");
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["actor"], "web-test");
    let (code, body) = exchange(&mut server, "GET /api/agents HTTP/1.1\r\nHost: @HOST@\r\n\r\n");
    assert_eq!(code, 200);
    let list: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(list["aliases"][0]["configuration"]["model"], "model-two");
    let (code, body) = exchange(
        &mut server,
        "GET /api/agents/choices?executor=claude HTTP/1.1\r\nHost: @HOST@\r\n\r\n",
    );
    assert_eq!(code, 200, "{body}");
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["executor"], "claude");

    // Stop with the token: 202, journaled with the actor, and the controller can see it.
    let own_origin = format!("X-Jtasks-Token: {token}\r\nOrigin: http://@HOST@\r\n");
    let (code, _) = post(&mut server, "/api/stop", &own_origin, &json!({"actor":"web-test"}));
    assert_eq!(code, 202);
    assert_eq!(server.take_stop().as_deref(), Some("web-test"));
    assert!(server.take_stop().is_none());
    let stops = journal(&f, "stop_requested");
    assert_eq!(stops.len(), 1);
    assert_eq!(stops[0]["actor"], "web-test");
}
