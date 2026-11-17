use super::types::{Plan, Task};
use anyhow::{Result, bail, ensure};
use std::collections::{HashMap, HashSet};

pub fn identifier(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c)),
        "invalid identifier: use 1-128 ASCII letters, digits, underscores or hyphens"
    );
    Ok(())
}

pub fn validate(plan: &Plan) -> Result<()> {
    crate::controller::admission::validate(plan)?;
    super::profiles::validate(&plan.profile_defaults)?;
    ensure!(
        plan.schema_version == 1,
        "unsupported schema_version (expected 1)"
    );
    ensure!(!plan.tasks.is_empty(), "plan must contain tasks");
    let mut requirements = HashSet::new();
    for requirement in &plan.requirements {
        identifier(&requirement.id)?;
        ensure!(
            !requirement.text.trim().is_empty(),
            "requirement needs original text"
        );
        ensure!(
            requirements.insert(&requirement.id),
            "duplicate requirement {}",
            requirement.id
        );
    }
    let mut tasks = HashMap::new();
    for task in &plan.tasks {
        let mut mapped = HashSet::new();
        for id in &task.requirements {
            ensure!(
                requirements.contains(id) && mapped.insert(id),
                "task {} has unknown or duplicate requirement {id}",
                task.id
            );
        }
        super::reviews::validate(task.estimate_seconds, &task.reviews)?;
        if let super::types::Executor::Profile { alias, overrides } = &task.executor {
            identifier(alias)?;
            super::profiles::validate(overrides)?;
        }
        identifier(&task.id)?;
        ensure!(
            !task.title.trim().is_empty(),
            "task {} needs a title",
            task.id
        );
        ensure!(
            tasks.insert(task.id.as_str(), task).is_none(),
            "duplicate task {}",
            task.id
        );
    }
    for task in &plan.tasks {
        let mut seen = HashSet::new();
        for dep in &task.depends_on {
            ensure!(
                tasks.contains_key(dep.as_str()),
                "task {} has unknown dependency {dep}",
                task.id
            );
            ensure!(
                seen.insert(dep),
                "task {} repeats dependency {dep}",
                task.id
            );
        }
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for task in &plan.tasks {
        visit(task, &tasks, &mut visiting, &mut visited)?;
    }
    super::operations::validate(plan)?;
    for requirement in &plan.requirements {
        ensure!(
            plan.tasks
                .iter()
                .any(|task| task.requirements.contains(&requirement.id)),
            "requirement {} needs explicit work coverage",
            requirement.id
        );
    }
    Ok(())
}

// Selectively ported dependency traversal from jtasks src/validate.ts (MIT).
// Source: 6caa1413da4f5246a06e5b7e22bbc3f10e437b65; no runtime code is shared.
fn visit<'a>(
    task: &'a Task,
    tasks: &HashMap<&str, &'a Task>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> Result<()> {
    if visited.contains(task.id.as_str()) {
        return Ok(());
    }
    if !visiting.insert(task.id.as_str()) {
        bail!("dependency cycle at {}", task.id);
    }
    for dep in &task.depends_on {
        visit(tasks[dep.as_str()], tasks, visiting, visited)?;
    }
    visiting.remove(task.id.as_str());
    visited.insert(task.id.as_str());
    Ok(())
}

/* MIT License - Copyright (c) 2026 Rodrigo Jäger
Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions: The above copyright notice and this
permission notice shall be included in all copies or substantial portions of
the Software. THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO
EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES
OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE,
ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE. */
