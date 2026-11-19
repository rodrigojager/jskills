//! A reviewed transition between plans. Semantic carry-over belongs to the planning skill.

use crate::{container::Container, util};
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

struct Inventory {
    files: Vec<PathBuf>,
    runs: Vec<(String, PathBuf)>,
    token: String,
    preview: Option<Value>,
}

fn children(path: &Path) -> Result<Vec<PathBuf>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut paths = std::fs::read_dir(path)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    Ok(paths)
}

/// No parent traversal, external paths, junctions or symlinks in destructive scopes.
fn local(c: &Container, path: &Path) -> Result<()> {
    let rel = path.strip_prefix(&c.root).context("Reset paths must stay inside the container")?;
    if rel.components().any(|p| !matches!(p, Component::Normal(_))) || rel.as_os_str().is_empty() {
        bail!("Unsafe reset path: {}", path.display());
    }
    let mut ancestor = c.root.clone();
    for part in rel.components() {
        ancestor.push(part);
        if util::is_link(&ancestor) {
            bail!("Reset refuses links: {}. No target was followed.", ancestor.display());
        }
    }
    if path.exists() && !path.canonicalize()?.starts_with(c.root.canonicalize()?) {
        bail!("Reset path resolves outside the container: {}", path.display());
    }
    Ok(())
}

fn tree(c: &Container, path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    local(c, path)?;
    out.push(path.to_path_buf());
    if path.is_dir() {
        for child in children(path)? {
            tree(c, &child, out)?;
        }
    } else if !path.is_file() {
        bail!("Unsupported reset item: {}", path.display());
    }
    Ok(())
}

fn jtasks(c: &Container, args: &[&str]) -> Result<Value> {
    let exe = util::which("jtasks").context("Run data exists but jtasks is unavailable; install it before resetting")?;
    let output = Command::new(exe).args(args).arg("--json")
        .arg("--root").arg(&c.root).arg("--data-dir").arg(c.jtasks().join("v2"))
        .current_dir(&c.root).output()?;
    let response: Value = serde_json::from_slice(&output.stdout).context("jtasks did not return valid JSON; nothing else was removed")?;
    if !output.status.success() {
        bail!("jtasks refused the reset: {} {}", response, String::from_utf8_lossy(&output.stderr));
    }

    Ok(response)
}

fn inventory(c: &Container) -> Result<Inventory> {
    let plan = c.work_folder("30-plan");
    local(c, &plan)?;
    local(c, &c.jtasks())?;
    if c.jtasks().parent() != Some(plan.as_path()) {
        bail!("Reset requires jtasks_data to be a direct child of 30-plan; external or shared run stores must be handled separately.");
    }
    // A malformed WORKSPACE must never turn product/secrets into the planning area.
    for protected in [c.product(), c.secrets(), c.work_folder("10-docs"), c.work_folder("80-ideas")] {
        let rel = protected.strip_prefix(&c.root).context("Protected folders must stay inside the container")?;
        if rel.components().any(|p| !matches!(p, Component::Normal(_))) || rel.as_os_str().is_empty() {
            bail!("Unsafe protected folder: {}", protected.display());
        }
        let resolved = if protected.exists() { protected.canonicalize()? } else { protected.clone() };
        let resolved_plan = if plan.exists() { plan.canonicalize()? } else { plan.clone() };
        if plan.starts_with(&protected) || protected.starts_with(&plan)
            || resolved_plan.starts_with(&resolved) || resolved.starts_with(&resolved_plan) {
            bail!("Planning and protected folders overlap: {}", protected.display());
        }
    }
    let files: Vec<PathBuf> = children(&plan)?.into_iter()
        .filter(|p| p.file_name().is_none_or(|n| n != "AGENTS.md") && p != &c.jtasks())
        .collect();
    let mut entries = Vec::new();
    for file in &files {
        tree(c, file, &mut entries)?;
    }
    let mut runs = Vec::new();
    let mut native_previews = Vec::new();
    let mut preview = None;
    let data = c.jtasks();
    let runtime = data.join("v2");
    let mut data_entries = Vec::new();
    if data.exists() {
        tree(c, &data, &mut data_entries)?;
        for p in children(&data)? {
            if p != runtime || !p.is_dir() {
                bail!("Unrecognized jtasks data (possibly legacy or a draft): {}. Resolve it before resetting.", p.display());
            }
        }
    }
    if runtime.exists() && !runtime.join("state.sqlite3").is_file() && !children(&runtime)?.is_empty() {
        bail!("Unrecognized jtasks v2 data without a database; reset refused.");
    }
    if runtime.join("state.sqlite3").is_file() {
        let mut seen = BTreeSet::new();
        let mut after = 0u64;
        loop {
            let cursor = after.to_string();
            let response = jtasks(c, &["runs", "--limit", "100", "--after", &cursor])?;
            if response.get("contract_version").and_then(Value::as_u64) != Some(1) {
                bail!("Unsupported jtasks read contract; reset refused.");
            }
            for run in response.get("runs").and_then(Value::as_array).context("Missing jtasks runs")? {
                let id = run.get("run").and_then(Value::as_str).context("Missing run id")?;
                util::validate_name(id)?;
                if run.get("state").and_then(Value::as_str) != Some("completed") {
                    bail!("Run {id} is not completed. Resolve its work before starting a different plan.");
                }
                let checkout = PathBuf::from(run.get("checkout").and_then(Value::as_str).context("Missing run checkout")?);
                if checkout.canonicalize()? != c.product().canonicalize()? || !seen.insert(id.to_owned()) {
                    bail!("Run {id} belongs to another checkout or is duplicated; reset refused.");
                }
            }
            let page = response.get("page").context("Missing runs page")?;
            let next = page.get("next_after").and_then(Value::as_u64).context("Missing runs cursor")?;
            if page.get("has_more").and_then(Value::as_bool).context("Missing runs pagination")? {
                if next <= after { bail!("Invalid runs cursor; reset refused."); }
                after = next;
            } else { break; }
        }
        let value = jtasks(c, &["clean", "--previous", "--finished-only"])?;
        if value.get("contract_version").and_then(Value::as_u64) != Some(1)
            || value.get("finished_only").and_then(Value::as_bool) != Some(true) {
            bail!("Unsupported jtasks cleanup preview; reset refused.");
        }
        for (key, expected) in [("checkout", c.product()), ("data_root", runtime.clone())] {
            let actual = PathBuf::from(value.get(key).and_then(Value::as_str).context("Missing cleanup root")?);
            if actual.canonicalize()? != expected.canonicalize()? { bail!("jtasks cleanup {key} differs from the local inventory"); }
        }
        let mut selected = BTreeSet::new();
        for run in value.get("runs").and_then(Value::as_array).context("Missing cleanup runs")? {
            let id = run.get("id").and_then(Value::as_str).context("Missing cleanup id")?;
            if run.get("finished").and_then(Value::as_bool) != Some(true)
                || run.get("discards_resumability").and_then(Value::as_bool) != Some(false)
                || !run.get("blockers").and_then(Value::as_array).context("Missing cleanup blockers")?.is_empty()
                || !selected.insert(id.to_owned()) {
                bail!("Run {id} is incomplete or blocked; reset refused.");
            }
            runs.push((id.to_owned(), runtime.clone()));
        }
        if selected != seen { bail!("Some jtasks runs are unfinished or missing from the cleanup preview; reset refused."); }
        native_previews.push(serde_json::to_vec(&value)?);
        preview = Some(value);
    }
    entries.extend(data_entries);
    // Bind semantic review and protected documents to the same approval, even though they are not deleted.
    for p in [c.workspace_md(), plan.join("AGENTS.md"), c.work_folder("10-docs").join("BACKLOG.md"), c.work_folder("80-ideas").join("IDEAS.md")] {
        local(c, &p)?;
        entries.push(p);
    }
    entries.sort();
    // Deterministic FNV-1a change detector, not an authentication mechanism.
    let mut hash = 0xcbf29ce484222325u64;
    let mut add = |bytes: &[u8]| {
        for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    };
    add(c.root.to_string_lossy().as_bytes());
    for preview in native_previews { add(&preview); }
    for p in entries {
        add(util::rel(&c.root, &p).as_bytes());
        if p.is_file() {
            add(b"file");
            add(&std::fs::read(&p)?);
        } else {
            add(if p.exists() { b"directory" } else { b"missing" });
        }
    }
    runs.sort();
    Ok(Inventory { files, runs, token: format!("{hash:016x}"), preview })
}

fn trash_program() -> Result<PathBuf> {
    #[cfg(windows)]
    let name = "powershell";
    #[cfg(target_os = "macos")]
    let name = "osascript";
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let name = "gio";
    util::which(name).with_context(|| format!("{name} is required to send plan files to the system trash; nothing was removed"))
}

fn recycle(program: &Path, path: &Path) -> Result<()> {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
        command.args(["-NoProfile", "-NonInteractive", "-Command", r#"$ErrorActionPreference = 'Stop'; Add-Type -AssemblyName Microsoft.VisualBasic; $p = $env:JPROJECT_RECYCLE_PATH; if ([System.IO.Directory]::Exists($p)) { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteDirectory($p, 'OnlyErrorDialogs', 'SendToRecycleBin', 'ThrowException') } else { [Microsoft.VisualBasic.FileIO.FileSystem]::DeleteFile($p, 'OnlyErrorDialogs', 'SendToRecycleBin', 'ThrowException') }"#]);
        command.env("JPROJECT_RECYCLE_PATH", path);
    }
    #[cfg(target_os = "macos")]
    command.args(["-e", "on run argv\ntell application \"Finder\" to delete POSIX file (item 1 of argv)\nend run", "--"]).arg(path);
    #[cfg(all(not(windows), not(target_os = "macos")))]
    command.args(["trash", "--"]).arg(path);
    let output = command.output()?;
    if !output.status.success() {
        bail!("Could not recycle {}: {}. Reset is incomplete; do not write the next plan.", path.display(), String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

pub fn reset(c: &Container, approve: Option<&str>, pending_reviewed: bool) -> Result<()> {
    let inv = inventory(c)?;
    println!("Previous-plan inventory (other work folders are preserved):");
    for p in &inv.files {
        let mut items = Vec::new();
        tree(c, p, &mut items)?;
        for item in items {
            println!("  RECYCLE {}", item.display());
        }
    }
    if let Some(value) = &inv.preview {
        println!("  PERMANENT jtasks cleanup preview: {}", serde_json::to_string_pretty(value)?);
    }
    println!("Approval token: {}", inv.token);
    let Some(approval) = approve else {
        println!("Preview only. Consolidate unresolved work and context into BACKLOG.md, ask which backlog items and ideas to include, then obtain approval for this exact list.");
        return Ok(());
    };
    if approval != inv.token {
        bail!("Inventory changed or token is invalid; run `jproject plan reset --dry-run` and review it again.");
    }
    if !pending_reviewed || !c.work_folder("10-docs").join("BACKLOG.md").is_file() {
        bail!("Before resetting, preserve unresolved work and context in 10-docs/BACKLOG.md and pass --pending-reviewed. Completed history does not belong there.");
    }
    let program = trash_program()?;
    // Recheck immediately before the first mutation. jtasks clean checks busy tasks again itself.
    if inventory(c)?.token != inv.token {
        bail!("Inventory changed during review; reset refused.");
    }
    if let Some(preview) = inv.preview.as_ref().filter(|_| !inv.runs.is_empty()) {
        use std::io::Write;
        std::fs::create_dir_all(c.work_folder("00-tmp"))?;
        let mut approval_file = tempfile::NamedTempFile::new_in(c.work_folder("00-tmp"))?;
        serde_json::to_writer(&mut approval_file, preview)?;
        approval_file.flush()?;
        let path = approval_file.path().to_str().context("Non-Unicode approval path")?;
        let receipt = jtasks(c, &["clean", "--approve", path, "--yes"])?;
        let removed: BTreeSet<_> = receipt.get("removed_runs").and_then(Value::as_array)
            .context("Missing cleanup receipt")?.iter().filter_map(Value::as_str).map(str::to_owned).collect();
        let expected: BTreeSet<_> = inv.runs.iter().map(|(id, _)| id.clone()).collect();
        if removed != expected { bail!("jtasks cleanup receipt differs from approval; reset is incomplete."); }
    }
    for p in &inv.files {
        recycle(&program, p)?;
    }
    println!("Previous plan cleared. BACKLOG.md, IDEAS.md, ADRs, glossary and AGENTS.md were preserved. Write the new plan now.");
    Ok(())
}
