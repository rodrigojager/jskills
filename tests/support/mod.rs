use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{Duration, Instant},
};

pub struct Fixture {
    pub root: PathBuf,
    pub data: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        // Preserve all test output inside the project container, never system temp.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("_work/00-tmp/t01-fixtures")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&root).unwrap();
        assert!(
            Command::new("git")
                .arg("init")
                .arg("--quiet")
                .arg(&root)
                .status()
                .unwrap()
                .success()
        );
        Self {
            data: root.join("runtime"),
            root,
        }
    }

    pub fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_jtasks"));
        command
            .arg("--root")
            .arg(&self.root)
            .arg("--data-dir")
            .arg(&self.data);
        command
    }

    pub fn call(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }
    pub fn plan(&self, tasks: Value) -> PathBuf {
        let path = self.root.join("plan.json");
        fs::write(
            &path,
            serde_json::to_vec(&json!({"schema_version":1,"completion":"judgment","tasks":tasks}))
                .unwrap(),
        )
        .unwrap();
        path
    }
    pub fn import(&self, tasks: Value) {
        let path = self.plan(tasks);
        let output = self.call(&["import", path.to_str().unwrap(), "--run", "demo"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    pub fn snapshot(&self) -> Value {
        let output = self.call(&["status", "--run", "demo"]);
        serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stderr)))
    }
    pub fn await_status(&self, predicate: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let value = self.snapshot();
            if predicate(&value) {
                return value;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "status deadline: {value}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

pub fn task(id: &str, dependencies: &[&str], outcome: &str, summary: &str) -> Value {
    json!({"id":id,"title":format!("Fixture {id}"),"depends_on":dependencies,"executor":{"kind":"fixture","outcome":outcome,"summary":summary}})
}
