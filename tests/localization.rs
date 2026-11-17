mod support;
use jtasks::read::i18n::{self, choose};
use serde_json::Value;
use std::{collections::BTreeMap, fs, process::Command};
use support::{Fixture, task};

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_jtasks"));
    for name in ["JTASKS_LANG", "JLOCK_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
        command.env_remove(name);
    }
    command.env("LANG", "en_US.UTF-8");
    command
}

#[test]
fn precedence_regional_aliases_and_explicit_unknown_match_legacy() {
    let env = BTreeMap::from([("JTASKS_LANG", "es-MX"), ("JLOCK_LANG", "fr")]);
    let select = |flag, system| choose(flag, |key| env.get(key).map(|v| (*v).into()), system);
    assert_eq!(select(Some("PT_br.UTF-8"), Some("ja")).tag, "pt-BR");
    let choice = select(None, Some("ja"));
    assert_eq!((choice.tag, choice.source), ("es", "JTASKS_LANG"));
    let choice = select(Some("unsupported"), Some("ja"));
    assert_eq!(choice.tag, "en");
    assert!(choice.warning.unwrap().contains("using English"));
    assert_eq!(
        choose(
            None,
            |key| (key == "JLOCK_LANG").then(|| "de".into()),
            Some("ja")
        )
        .source,
        "JLOCK_LANG"
    );
    assert_eq!(
        choose(None, |_| Some("unsupported".into()), Some("zh_TW@variant")).tag,
        "zh-CN"
    );
    assert_eq!(choose(None, |_| None, Some("C.UTF-8")).source, "default");
    assert_eq!(i18n::resolve(" it-CH "), Some("it"));
    assert_eq!(i18n::resolve(""), None);
}

#[test]
fn eleven_selective_catalogs_retain_glossary_process_labels_and_fallback() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    assert_eq!(i18n::TAGS.len(), 11);
    for (tag, task_word) in i18n::TAGS.iter().zip([
        "task",
        "tarefa",
        "tarea",
        "tâche",
        "Aufgabe",
        "attività",
        "任务",
        "タスク",
        "задача",
        "مهمة",
        "कार्य",
    ]) {
        let value: Value =
            serde_json::from_slice(&fs::read(root.join(format!("{tag}.json"))).unwrap()).unwrap();
        assert_eq!(value["_glossary"]["task"], task_word, "{tag}");
        assert_eq!(i18n::lookup(tag, "_glossary.task"), Some(task_word));
        assert!(
            !i18n::lookup(tag, "status.command_state.running")
                .unwrap()
                .is_empty()
        );
        assert!(!i18n::lookup(tag, "view.tasks").unwrap().is_empty());
        assert_eq!(
            i18n::lookup(tag, "detail.checkpoint"),
            i18n::lookup("en", "detail.checkpoint")
        );
        for key in value.as_object().unwrap().keys() {
            assert!(
                ![
                    "keepalive",
                    "supervise",
                    "notices",
                    "wake",
                    "help.start",
                    "help.done"
                ]
                .iter()
                .any(|obsolete| key.contains(obsolete)),
                "{key}"
            );
        }
    }
    assert!(
        fs::read_to_string(root.join("LICENSE"))
            .unwrap()
            .contains("Copyright (c) 2026 Rodrigo Jäger")
    );
    assert_eq!(i18n::lookup("fr", "absent.key"), None);
}

#[test]
fn public_help_localizes_before_parsing_and_explains_execution_boundaries() {
    for tag in i18n::TAGS {
        let output = cli()
            .env("JTASKS_LANG", "ja")
            .args(["--lang", tag, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains(i18n::lookup(tag, "help.about").unwrap()),
            "{tag}: {text}"
        );
        assert!(text.contains(i18n::lookup(tag, "heading.monitoring").unwrap()));
        assert!(text.contains("general operation inputs are not automatically fingerprinted"));
        assert!(text.contains("escaped helpers remain unobserved"));
        assert!(!text.contains("keepalive"));
    }
    let output = cli()
        .args(["checkpoint", "--lang=pt-BR", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Checkpoints Git explícitos"));
    let output = cli()
        .env("JTASKS_LANG", "ja")
        .args(["--lang=unknown", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("Independent foreground task controller")
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("using English"));
    let output = cli()
        .env("JTASKS_LANG", "unsupported")
        .env("JLOCK_LANG", "es_MX")
        .arg("--help")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("Controlador independiente"));
}

#[test]
fn machine_validation_and_errors_keep_english_fields_without_human_stdout() {
    let f = Fixture::new();
    let plan = f.plan(serde_json::json!([task(
        "A",
        &[],
        "completed",
        "original summary"
    )]));
    let mut expected = None;
    for tag in i18n::TAGS {
        let output = cli()
            .arg("--root")
            .arg(&f.root)
            .args(["--lang", tag, "--json", "validate"])
            .arg(&plan)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let json: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(json["valid"], true);
        if let Some(expected) = &expected {
            assert_eq!(&json, expected);
        } else {
            expected = Some(json);
        }
    }
    assert!(!f.data.exists(), "validation creates no runtime state");
    let output = cli()
        .args(["--lang=unknown", "--json", "validate"])
        .arg(f.root.join("missing.json"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["error"]["code"], "read_or_command_failed");
    assert!(String::from_utf8_lossy(&output.stderr).contains("using English"));
}

#[test]
fn human_watch_translates_labels_while_json_retains_protocol_and_summary() {
    let f = Fixture::new();
    f.import(serde_json::json!([task(
        "A",
        &[],
        "completed",
        "original summary"
    )]));
    let output = f
        .command()
        .args(["--lang", "pt-BR", "watch", "--run", "demo", "--once"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Resumo final"));
    assert!(text.contains("Objetivo:"));
    assert!(text.contains("Tarefas:"));
    assert!(text.contains("Verificação obrigatória="));
    let output = f
        .command()
        .args([
            "--lang", "pt-BR", "--json", "watch", "--run", "demo", "--once",
        ])
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["event"], "final_summary");
    assert_eq!(value["view"]["status"]["tasks"][0]["state"], "pending");
    let snapshot = f.snapshot();
    assert_eq!(
        snapshot["tasks"][0]["state"], "pending",
        "monitoring never dispatched work"
    );
}

#[test]
fn dashboard_presentation_is_separate_read_only_data_with_english_fallback() {
    use jtasks::{read::web::Server, storage::Workspace};
    use std::{
        io::{Read, Write},
        net::TcpStream,
        time::{Duration, Instant},
    };
    let f = Fixture::new();
    let workspace = Workspace::resolve(&f.root, Some(&f.data)).unwrap();
    let mut server = Server::bind(&workspace, "demo").unwrap();
    let host = server
        .url()
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let client = std::thread::spawn(move || {
        let mut socket = TcpStream::connect(&host).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            socket,
            "GET /presentation.json HTTP/1.1\r\nHost: {host}\r\n\r\n"
        )
        .unwrap();
        let mut output = String::new();
        socket.read_to_string(&mut output).unwrap();
        output
    });
    let start = Instant::now();
    while !client.is_finished() {
        assert!(start.elapsed() < Duration::from_secs(4));
        server.poll().unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    let response = client.join().unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    let body: Value = serde_json::from_str(response.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["language"], "en");
    assert_eq!(body["labels"]["tasks"], "Tasks");
    assert!(
        !f.data.exists(),
        "presentation must not create or open runtime storage"
    );
}
