//! One combined ordering graph: task development, effective completion and checks.
use super::types::Plan;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    None,
    Task,
    Group,
    Final,
    Mixed,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Stage {
    Prerequisite { consumers: Vec<String> },
    Task { task: String },
    Group { tasks: Vec<String> },
    Final { tasks: Vec<String> },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Dependency {
    Operation { id: String },
    TaskCandidate { id: String },
    TaskCompleted { id: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub script: Option<String>,
    /// Relative directories resolve against the canonical checkout.
    pub cwd: PathBuf,
    #[serde(default)]
    pub environment_refs: BTreeMap<String, String>,
    #[serde(default)]
    pub resources: Vec<String>,
    pub deadline_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: String,
    pub stage: Stage,
    pub required: bool,
    /// Author-declared stable input revision; never changed by a command receipt.
    pub revision: String,
    pub inputs: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<Dependency>,
    pub initial: Invocation,
    pub repair_task: Option<String>,
    /// Optional explicit source-control checkpoint, applied only by the checkpoint CLI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<crate::controller::checkpoints::Declaration>,
}

pub fn task_checks<'a>(plan: &'a Plan, task: &'a str) -> impl Iterator<Item = &'a Operation> {
    plan.operations.iter().filter(move |op| {
        op.required && matches!(&op.stage, Stage::Task { task: id } if id == task)
    })
}

pub fn validate(plan: &Plan) -> Result<()> {
    ensure!(
        plan.operation_placement != Placement::None || plan.operations.is_empty(),
        "none placement cannot declare operations"
    );
    let tasks: HashSet<_> = plan.tasks.iter().map(|t| t.id.as_str()).collect();
    let mut operations = HashSet::new();
    for op in &plan.operations {
        if let Some(checkpoint) = &op.checkpoint {
            crate::controller::checkpoints::validate(op, checkpoint)?;
        }
        super::validation::identifier(&op.id)?;
        ensure!(
            operations.insert(op.id.as_str()),
            "duplicate operation {}",
            op.id
        );
        ensure!(
            !op.revision.trim().is_empty(),
            "operation {} requires an input revision",
            op.id
        );
        ensure!(
            !op.inputs.is_empty() && op.inputs.iter().all(|i| !i.trim().is_empty()),
            "operation {} requires explicit inputs",
            op.id
        );
        ensure!(
            op.inputs
                .iter()
                .all(|input| super::operation_lineage::valid_input(input)),
            "operation {} inputs must be explicit checkout-relative paths without parent traversal or globs",
            op.id
        );
        ensure!(
            !op.initial.program.trim().is_empty() && !op.initial.cwd.as_os_str().is_empty(),
            "operation {} requires an initial program and cwd",
            op.id
        );
        ensure!(
            op.initial.deadline_ms != Some(0),
            "operation deadline must be positive"
        );
        for (name, reference) in &op.initial.environment_refs {
            ensure!(
                !name.is_empty() && !name.contains(['=', '\0']) && !reference.is_empty(),
                "invalid operation environment reference"
            );
        }
        let (placement, ids) = match &op.stage {
            Stage::Prerequisite { consumers } => (None, consumers.clone()),
            Stage::Task { task } => (Some(Placement::Task), vec![task.clone()]),
            Stage::Group { tasks } => (Some(Placement::Group), tasks.clone()),
            Stage::Final { tasks } => (Some(Placement::Final), tasks.clone()),
        };
        ensure!(
            placement.is_none()
                || plan.operation_placement == Placement::Mixed
                || placement == Some(plan.operation_placement),
            "operation {} conflicts with selected placement",
            op.id
        );
        ensure!(
            !ids.is_empty(),
            "operation {} needs explicit consumers/producers",
            op.id
        );
        let mut unique = HashSet::new();
        for id in ids {
            ensure!(
                tasks.contains(id.as_str()) && unique.insert(id.clone()),
                "operation {} has unknown or repeated task {id}",
                op.id
            );
        }
        if matches!(op.stage, Stage::Prerequisite { .. }) {
            ensure!(op.required, "prerequisites must be required");
        }
        if matches!(op.stage, Stage::Group { .. } | Stage::Final { .. }) {
            ensure!(
                op.repair_task
                    .as_deref()
                    .is_some_and(|id| tasks.contains(id)),
                "group/final operation {} needs explicit repair_task responsibility",
                op.id
            );
        }
        if let Some(id) = &op.repair_task {
            ensure!(
                tasks.contains(id.as_str()),
                "unknown operation repair task {id}"
            );
        }
    }
    // Edges express prerequisites, so visiting prerequisites detects every cycle
    // across development and completion, including checks requiring their own result.
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for task in &plan.tasks {
        graph.insert(
            format!("start:{}", task.id),
            task.depends_on
                .iter()
                .map(|id| format!("done:{id}"))
                .collect(),
        );
        graph.insert(
            format!("candidate:{}", task.id),
            vec![format!("start:{}", task.id)],
        );
        graph.insert(
            format!("done:{}", task.id),
            vec![format!("candidate:{}", task.id)],
        );
    }
    for op in &plan.operations {
        graph.insert(format!("op:{}", op.id), Vec::new());
    }
    for op in &plan.operations {
        let key = format!("op:{}", op.id);
        match &op.stage {
            Stage::Prerequisite { consumers } => {
                for id in consumers {
                    graph
                        .get_mut(&format!("start:{id}"))
                        .unwrap()
                        .push(key.clone());
                }
            }
            Stage::Task { task } => {
                graph
                    .get_mut(&key)
                    .unwrap()
                    .push(format!("candidate:{task}"));
                if op.required {
                    graph
                        .get_mut(&format!("done:{task}"))
                        .unwrap()
                        .push(key.clone());
                }
            }
            Stage::Group { tasks } | Stage::Final { tasks } => {
                for id in tasks {
                    graph.get_mut(&key).unwrap().push(format!("done:{id}"));
                }
            }
        }
        for dep in &op.depends_on {
            let node = match dep {
                Dependency::Operation { id } => {
                    ensure!(
                        operations.contains(id.as_str()),
                        "unknown operation dependency {id}"
                    );
                    format!("op:{id}")
                }
                Dependency::TaskCandidate { id } => {
                    ensure!(tasks.contains(id.as_str()), "unknown task dependency {id}");
                    format!("candidate:{id}")
                }
                Dependency::TaskCompleted { id } => {
                    ensure!(tasks.contains(id.as_str()), "unknown task dependency {id}");
                    format!("done:{id}")
                }
            };
            let edges = graph.get_mut(&key).unwrap();
            ensure!(
                !edges.contains(&node),
                "operation {} repeats dependency",
                op.id
            );
            edges.push(node);
        }
    }
    fn visit(
        node: &str,
        graph: &HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(node) {
            return Ok(());
        }
        if !visiting.insert(node.into()) {
            bail!("combined task/operation ordering cycle at {node}");
        }
        for dependency in &graph[node] {
            visit(dependency, graph, visiting, done)?;
        }
        visiting.remove(node);
        done.insert(node.into());
        Ok(())
    }
    let mut visiting = HashSet::new();
    let mut done = HashSet::new();
    for key in graph.keys() {
        visit(key, &graph, &mut visiting, &mut done)?;
    }
    Ok(())
}
