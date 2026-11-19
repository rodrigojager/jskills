//! Bounded presentation of persisted read projections. No scheduling or control.
use super::{
    i18n,
    projections::{self, Request, View},
};
use crate::storage::Workspace;
use anyhow::Result;
use serde_json::{Value, json};
use std::io::{self, Write};

const ROWS: usize = 20;

/// Each component retains its own cursor: these are separate read snapshots.
pub fn snapshot(workspace: &Workspace, run: &str) -> Result<Value> {
    Ok(json!({
        "status": projections::read(workspace, &Request::new(View::Status, Some(run)))?,
        "assignments": projections::read(workspace, &Request::new(View::Tasks, Some(run)))?,
        "questions": projections::read(workspace, &Request::new(View::Questions, Some(run)))?,
        "snapshot_scope": "independent read snapshots; each component retains its own cursor/as_of"
    }))
}

fn text(value: &Value) -> String {
    match value {
        Value::Null => "unknown".into(),
        Value::String(s) => s.chars().filter(|c| !c.is_control()).take(160).collect(),
        _ => value.to_string(),
    }
}
fn list(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or(&[])
}
fn number(value: &Value) -> u64 {
    value.as_u64().unwrap_or(0)
}
fn flag(value: &Value) -> &'static str {
    match value.as_bool() {
        Some(true) => "complete",
        Some(false) => "pending",
        None => "unknown",
    }
}
fn assignment(task: &Value, declared: &Value) -> String {
    if let Some(attempt) = current_attempt(task, declared) {
        let profile = &attempt["effective_profile"];
        let values = &profile["values"];
        format!(
            "effective executor={} profile={} native_agent={} native_profile={} model={} effort={}; attempt={} recorded_process={} (current liveness unobserved)",
            text(&values["executor"]),
            text(&profile["alias"]),
            text(&values["native_agent"]),
            text(&values["native_profile"]),
            text(&values["model"]),
            text(&values["effort"]),
            text(&attempt["id"]),
            text(&attempt["process_state"])
        )
    } else if declared["current_attempt"].is_string() {
        format!(
            "current attempt={} assignment omitted from bounded history; use read attempt",
            text(&declared["current_attempt"])
        )
    } else {
        let executor = &declared["spec"]["executor"];
        format!(
            "declared executor={} profile={}; effective assignment unavailable until attempt",
            text(&executor["kind"]),
            text(&executor["alias"])
        )
    }
}

fn current_attempt<'a>(task: &'a Value, declared: &Value) -> Option<&'a Value> {
    let id = declared["current_attempt"].as_str()?;
    list(&task["attempts"]).iter().find(|a| a["id"] == id)
}

/// Render only supplied facts. Completion booleans are owned by the projection.
pub fn render(value: &Value, dashboard: Option<&str>, final_summary: bool) -> String {
    use std::fmt::Write;
    let s = &value["status"];
    let graph = &s["graph"];
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}jtasks {} | recorded state={} | controller={}",
        if final_summary {
            format!("{} | ", i18n::tr("view.final"))
        } else {
            String::new()
        },
        text(&s["identity"]["run"]),
        text(&s["state"]),
        text(&s["availability"]["controller"])
    );
    let _ = writeln!(
        out,
        "{}: {}",
        i18n::tr("view.objective"),
        text(&s["objective"])
    );
    let _ = writeln!(
        out,
        "Persisted as-of={} | cursor={} | graph revision={}",
        text(&s["as_of"]),
        text(&s["event_cursor"]),
        text(&s["graph_revision"])
    );
    let _ = writeln!(
        out,
        "{}: completed={} current={} original={} delta={} | development={} | run={}",
        i18n::tr("view.tasks"),
        text(&graph["completed"]),
        text(&graph["current_total"]),
        text(&graph["original_total"]),
        text(&graph["delta"]),
        flag(&s["development_complete"]),
        flag(&s["complete"])
    );
    let _ = writeln!(
        out,
        "{}: {}",
        i18n::tr("view.states"),
        text(&s["task_counts"])
    );
    let coverage = list(&graph["coverage"]);
    let _ = writeln!(
        out,
        "Requirement coverage (visible): {}/{}; graph revisions explain denominator changes",
        coverage.iter().filter(|r| r["satisfied"] == true).count(),
        coverage.len()
    );
    let _ = writeln!(
        out,
        "{}={} | pending required operations={}",
        i18n::tr("view.verification"),
        flag(&s["verification_complete"]),
        text(&s["pending_required_operations"])
    );
    for operation in list(&s["operations"]).iter().take(ROWS) {
        let _ = writeln!(
            out,
            "  operation {}: {} | required={} | placement={} | command={}",
            text(&operation["id"]),
            text(&operation["state"]),
            text(&operation["required"]),
            text(&operation["stage"]),
            text(&operation["command_id"])
        );
    }
    let assignments = list(&value["assignments"]["tasks"]);
    for task in list(&s["tasks"]).iter().take(ROWS) {
        let declared = assignments
            .iter()
            .find(|a| a["id"] == task["id"])
            .unwrap_or(&Value::Null);
        let _ = writeln!(
            out,
            "  task {}: {} | {}",
            text(&task["id"]),
            text(&task["state"]),
            assignment(task, declared)
        );
        let cooldown = &task["cooldown"];
        if !cooldown["effective_due_ms"].is_null() {
            let _ = writeln!(
                out,
                "    cooldown: due_ms={} state={} reason={}",
                text(&cooldown["effective_due_ms"]),
                text(&cooldown["latest"]["state"]),
                text(&cooldown["latest"]["reason"])
            );
        }
        if let Some(attempt) = current_attempt(task, declared) {
            let review = &attempt["review"];
            if !review["state"].is_null() && review["state"] != "unconfigured" {
                let _ = writeln!(
                    out,
                    "    review: {} reason={} calls={}/{}",
                    text(&review["state"]),
                    text(&review["reason"]),
                    text(&review["actual_calls"]),
                    text(&review["max_calls"])
                );
            }
        }
        let resources = list(&declared["spec"]["admission"]["resources"]);
        if !resources.is_empty() {
            let _ = writeln!(
                out,
                "    declared resources={} (ownership/wait cause not established by task state)",
                text(&declared["spec"]["admission"]["resources"])
            );
        }
    }
    for item in list(&value["questions"]["records"]).iter().take(ROWS) {
        let question = &item["question"];
        let _ = writeln!(
            out,
            "  question {} task={} pending={}: {} | resume={}",
            text(&question["detail"]["request_id"]),
            text(&question["task"]),
            text(&item["pending"]),
            text(&question["detail"]["question"]),
            text(&item["resume"])
        );
    }
    let next = &s["next_automatic_action"];
    let _ = writeln!(
        out,
        "Next persisted action: {}",
        if next.is_null() {
            "none recorded; controller availability unobserved".into()
        } else {
            format!(
                "{} kind={} due_ms={} state={}",
                text(&next["id"]),
                text(&next["kind"]),
                text(&next["due_ms"]),
                text(&next["state"])
            )
        }
    );
    let _ = writeln!(
        out,
        "Dashboard: {}",
        dashboard.unwrap_or("unavailable in this read snapshot")
    );
    let _ = writeln!(
        out,
        "Native helpers: unobserved | bounded view (up to {ROWS} rows per section); use read/logs for explicit detail"
    );
    for (name, component) in [
        ("status", s),
        ("assignments", &value["assignments"]),
        ("questions", &value["questions"]),
    ] {
        if component["page"]["has_more"] == true
            || !list(&component["projection_limits"]["omissions"]).is_empty()
        {
            let _ = writeln!(
                out,
                "  {name}: additional persisted items omitted; use paginated read"
            );
        }
    }
    out
}

/// Ignore clock/cursor churn when deciding whether a non-TTY transition matters.
fn meaningful(value: &Value) -> Value {
    let mut value = value.clone();
    fn prune(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                for key in [
                    "as_of",
                    "event_cursor",
                    "recorded_at",
                    "remaining_ms",
                    "clock",
                ] {
                    fields.remove(key);
                }
                for value in fields.values_mut() {
                    prune(value);
                }
            }
            Value::Array(items) => {
                for item in items {
                    prune(item);
                }
            }
            _ => {}
        }
    }
    prune(&mut value);
    value
}

/// TTY uses one alternate-screen overview; pipes receive actual transitions only.
pub struct Panel<W: Write> {
    output: W,
    tty: bool,
    json: bool,
    entered: bool,
    previous: Option<Value>,
}
impl<W: Write> Panel<W> {
    pub fn new(output: W, tty: bool, json: bool) -> Self {
        Self {
            output,
            tty: tty && !json,
            json,
            entered: false,
            previous: None,
        }
    }
    pub fn update(
        &mut self,
        value: &Value,
        dashboard: Option<&str>,
        final_summary: bool,
    ) -> io::Result<bool> {
        let signature = json!([meaningful(value), dashboard]);
        if !final_summary && self.previous.as_ref() == Some(&signature) {
            return Ok(false);
        }
        if self.tty && final_summary && self.entered {
            write!(self.output, "\x1b[?25h\x1b[?1049l")?;
            self.entered = false;
        }
        if self.tty && !final_summary {
            if !self.entered {
                write!(self.output, "\x1b[?1049h\x1b[?25l")?;
                self.entered = true;
            }
            write!(self.output, "\x1b[H\x1b[2J")?;
        }
        if self.json {
            writeln!(
                self.output,
                "{}",
                json!({"event":if final_summary {"final_summary"} else {"transition"},"view":value,"dashboard_url":dashboard})
            )?;
        } else {
            write!(self.output, "{}", render(value, dashboard, final_summary))?;
        }
        self.output.flush()?;
        self.previous = Some(signature);
        Ok(true)
    }
}
impl<W: Write> Drop for Panel<W> {
    fn drop(&mut self) {
        if self.entered {
            let _ = write!(self.output, "\x1b[?25h\x1b[?1049l");
            let _ = self.output.flush();
        }
    }
}

pub fn complete(value: &Value) -> bool {
    value["status"]["complete"] == true
}
pub fn cursor(value: &Value) -> u64 {
    number(&value["status"]["event_cursor"])
}
