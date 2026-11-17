//! Explicit reviewed checkpoints. Ordinary completion never calls this module's apply.
//! Git and SQLite cannot transact together: record intent before updating the ref and
//! retain an actionable uncertainty record if the ref/index/receipt sequence breaks.
use crate::{
    domain::{
        operations::{Operation, Stage},
        types::Plan,
    },
    resources::ControllerGuard,
    storage::{Store, Workspace},
};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// Exact files, never directories, globs, or an implicit whole checkout.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    tree_entry: String,
    sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub contract_version: u32,
    pub checkout: PathBuf,
    pub data_root: PathBuf,
    pub run: String,
    pub operation: String,
    pub revision: String,
    pub head: String,
    pub branch: String,
    pub reviewer: String,
    /// Explicit responsibility statement for the changes since the clean baseline.
    pub attribution: String,
    pub message: String,
    entries: Vec<Entry>,
}

const SQL: &str = "CREATE TABLE IF NOT EXISTS checkpoint_baselines(run_id TEXT NOT NULL,operation_id TEXT NOT NULL,entries TEXT NOT NULL,dirty_paths TEXT NOT NULL,PRIMARY KEY(run_id,operation_id)); CREATE TABLE IF NOT EXISTS checkpoint_receipts(review_hash TEXT PRIMARY KEY,run_id TEXT NOT NULL,operation_id TEXT NOT NULL,state TEXT NOT NULL,commit_id TEXT NOT NULL,detail TEXT NOT NULL); CREATE TABLE IF NOT EXISTS checkpoint_check_inputs(command_id TEXT PRIMARY KEY,run_id TEXT NOT NULL REFERENCES runs(id),revision TEXT NOT NULL,entries TEXT NOT NULL);";

/// Bind the exact future checkpoint bytes to an actual matching check invocation,
/// including repair rechecks. A queued command gets its snapshot at admission.
pub fn capture_check_inputs(
    conn: &Connection,
    workspace: &Workspace,
    spec: &crate::execution::commands::CommandSpec,
    command: &str,
) -> Result<()> {
    let Some(operation) = &spec.operation else {
        return Ok(());
    };
    let plan: String = conn.query_row("SELECT plan FROM runs WHERE id=?1", [&spec.run], |r| {
        r.get(0)
    })?;
    let plan: Plan = serde_json::from_str(&plan)?;
    let Some(op) = plan.operations.iter().find(|op| &op.id == operation) else {
        return Ok(());
    };
    if !crate::domain::operation_lineage::matches_check(&op.initial, spec, &workspace.checkout) {
        return Ok(());
    }
    let paths: BTreeSet<String> = plan
        .operations
        .iter()
        .filter_map(|op| op.checkpoint.as_ref())
        .flat_map(|declaration| declaration.paths.iter())
        .filter(|path| {
            op.inputs
                .iter()
                .any(|input| crate::domain::operation_lineage::overlaps(input, path))
        })
        .cloned()
        .collect();
    if paths.is_empty() {
        return Ok(());
    }
    conn.execute_batch(SQL)?;
    let revision: String = conn.query_row(
        "SELECT revision FROM operations WHERE run_id=?1 AND id=?2",
        [&spec.run, operation],
        |r| r.get(0),
    )?;
    let entries = snapshot(workspace, &paths.into_iter().collect::<Vec<_>>())?;
    conn.execute(
        "INSERT INTO checkpoint_check_inputs(command_id,run_id,revision,entries) VALUES(?1,?2,?3,?4)",
        params![command, spec.run, revision, serde_json::to_string(&entries)?],
    )?;
    Ok(())
}

fn custody_proof(workspace: &Workspace, path: &Path) -> Result<()> {
    let mut current = workspace.data.clone();
    for component in path.strip_prefix(&workspace.data)?.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).with_context(|| {
            format!(
                "waiting: missing custody proof {}; explicitly reconcile its process scope",
                path.display()
            )
        })?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "waiting: linked custody proof {}",
            path.display()
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "waiting: reparse custody proof {}",
                path.display()
            );
        }
    }
    let proof = serde_json::from_slice::<crate::execution::custody::Record>(&fs::read(path)?);
    ensure!(
        proof.is_ok_and(|record| record.empty
            && record.platform == std::env::consts::OS
            && record
                .scope
                .strip_prefix("jtasks-")
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())),
        "waiting: unresolved custody proof {}; explicitly reconcile its process scope",
        path.display()
    );
    Ok(())
}

fn quiescent(conn: &Connection, workspace: &Workspace) -> Result<()> {
    for (table, directory, terminal) in [
        ("attempts", "attempts", "'retired','interrupted'"),
        (
            "managed_commands",
            "commands",
            "'stopped','exited','launch_failed'",
        ),
        (
            "reviewer_calls",
            "reviews",
            "'completed','failed','stale','stopped'",
        ),
    ] {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            continue;
        }
        let active: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE state NOT IN ({terminal}))"),
            [],
            |r| r.get(0),
        )?;
        ensure!(
            !active,
            "waiting: live/unresolved {table}; explicitly retire/reconcile writers and checks before checkpoint"
        );
        let mut statement = conn.prepare(&format!("SELECT id,state FROM {table}"))?;
        let rows = statement
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (id, state) in rows {
            crate::domain::validation::identifier(&id)?;
            if table == "managed_commands" && state == "launch_failed" {
                continue;
            }
            let root = workspace.data.join(directory).join(id);
            custody_proof(workspace, &root.join("custody.json"))?;
            if table == "reviewer_calls" {
                custody_proof(workspace, &root.join("snapshot/custody.json"))?;
            }
        }
    }
    Ok(())
}

pub fn validate(op: &Operation, declaration: &Declaration) -> Result<()> {
    ensure!(
        matches!(op.stage, Stage::Group { .. } | Stage::Final { .. }),
        "checkpoint {} requires an explicit group/final operation",
        op.id
    );
    ensure!(
        !declaration.paths.is_empty(),
        "checkpoint {} needs exact reviewed paths",
        op.id
    );
    let mut seen = BTreeSet::new();
    for path in &declaration.paths {
        ensure!(
            crate::domain::operation_lineage::valid_input(path)
                && !path.contains('\\')
                && Path::new(path)
                    .components()
                    .all(|p| matches!(p, Component::Normal(_)))
                && !path.ends_with('/')
                && !path.split('/').any(|p| p.eq_ignore_ascii_case(".git"))
                && seen.insert(if cfg!(windows) {
                    path.to_lowercase()
                } else {
                    path.clone()
                }),
            "checkpoint paths must be unique exact checkout-relative files: {path}"
        );
        ensure!(
            op.inputs
                .iter()
                .any(|input| crate::domain::operation_lineage::overlaps(input, path)),
            "checkpoint path {path} is outside operation inputs"
        );
    }
    Ok(())
}

fn git_command(workspace: &Workspace) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(git_path(&workspace.checkout))
        .arg("--literal-pathspecs");
    // Do not inherit an alternate index or object/ref redirection from the caller.
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_NAMESPACE",
    ] {
        command.env_remove(name);
    }
    command
}

/// Rust keeps canonical extended-length paths for identity/safety checks. Git for
/// Windows expects ordinary drive/UNC paths in argv and GIT_INDEX_FILE instead.
fn git_path(path: &Path) -> std::ffi::OsString {
    #[cfg(windows)]
    {
        let text = path.as_os_str().to_string_lossy();
        if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
            return format!(r"\\{unc}").into();
        }
        if let Some(drive) = text.strip_prefix(r"\\?\") {
            return drive.into();
        }
    }
    path.as_os_str().into()
}

fn output(command: &mut Command) -> Result<Vec<u8>> {
    let result = command.output().context("cannot invoke Git")?;
    ensure!(
        result.status.success(),
        "Git checkpoint refused: {}",
        String::from_utf8_lossy(&result.stderr).trim()
    );
    Ok(result.stdout)
}

fn git(workspace: &Workspace, args: &[&str]) -> Result<String> {
    Ok(
        String::from_utf8(output(git_command(workspace).args(args))?)?
            .trim()
            .into(),
    )
}

fn checked_path(workspace: &Workspace, path: &str) -> Result<PathBuf> {
    let target = workspace.checkout.join(path);
    ensure!(
        !target.starts_with(&workspace.data),
        "checkpoint scope overlaps runtime data: {path}"
    );
    let mut current = workspace.checkout.clone();
    for component in Path::new(path).components() {
        ensure!(
            matches!(component, Component::Normal(_)),
            "invalid checkpoint path: {path}"
        );
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                ensure!(
                    !metadata.file_type().is_symlink(),
                    "checkpoint rejects linked paths: {path}"
                );
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    ensure!(
                        metadata.file_attributes() & 0x400 == 0,
                        "checkpoint rejects reparse points: {path}"
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    ensure!(
        !target.exists() || target.is_file(),
        "checkpoint requires exact regular files: {path}"
    );
    Ok(target)
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn snapshot(workspace: &Workspace, paths: &[String]) -> Result<Vec<Entry>> {
    paths
        .iter()
        .map(|path| {
            let target = checked_path(workspace, path)?;
            let tree_entry = git(workspace, &["ls-tree", "HEAD", "--", path])?;
            ensure!(
                tree_entry.is_empty()
                    || tree_entry.starts_with("100644 blob ")
                    || tree_entry.starts_with("100755 blob "),
                "unsupported checkpoint tree entry: {path}"
            );
            let bytes = match fs::read(target) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            Ok(Entry {
                path: path.clone(),
                tree_entry,
                sha256: bytes.as_deref().map(digest),
            })
        })
        .collect()
}

pub fn capture_baselines(
    conn: &Connection,
    workspace: &Workspace,
    run: &str,
    plan: &Plan,
) -> Result<()> {
    if !plan.operations.iter().any(|op| op.checkpoint.is_some()) {
        return Ok(());
    }
    conn.execute_batch(SQL)?;
    for op in &plan.operations {
        let Some(declaration) = &op.checkpoint else {
            continue;
        };
        // An unborn branch has no attributable committed baseline.
        git(workspace, &["rev-parse", "--verify", "HEAD"])?;
        let entries = snapshot(workspace, &declaration.paths)?;
        let mut dirty = Vec::new();
        for entry in &entries {
            if (entry.tree_entry.is_empty() && entry.sha256.is_some())
                || !git(
                    workspace,
                    &[
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=all",
                        "--",
                        &entry.path,
                    ],
                )?
                .is_empty()
            {
                dirty.push(&entry.path);
            }
        }
        conn.execute("INSERT INTO checkpoint_baselines(run_id,operation_id,entries,dirty_paths) VALUES(?1,?2,?3,?4)", params![run, op.id, serde_json::to_string(&entries)?, serde_json::to_string(&dirty)?])?;
    }
    Ok(())
}

/// Check success and checkpoint application are separate facts. Never imply that
/// a passed command has committed anything, or repair an uncertain intent here.
pub fn status(
    conn: &Connection,
    run: &str,
    operation: &str,
    declaration: &Declaration,
) -> Result<serde_json::Value> {
    let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='checkpoint_receipts')", [], |r|r.get(0))?;
    let mut receipts = Vec::new();
    if exists {
        let mut statement = conn.prepare("SELECT review_hash,state,commit_id FROM checkpoint_receipts WHERE run_id=?1 AND operation_id=?2 ORDER BY rowid")?;
        receipts = statement.query_map([run, operation], |r|Ok(serde_json::json!({"review_hash":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"commit":r.get::<_,String>(2)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
    }
    Ok(
        serde_json::json!({"paths":declaration.paths,"application_state":receipts.last().map(|r|r["state"].clone()).unwrap_or(serde_json::json!("awaiting_explicit_review_and_apply")),"receipts":receipts}),
    )
}

fn applicable(
    conn: &Connection,
    workspace: &Workspace,
    run: &str,
    operation: &str,
) -> Result<Operation> {
    let (spec, state, checkout, revision): (String, String, String, String) = conn.query_row("SELECT o.spec,o.state,r.checkout,o.revision FROM operations o JOIN runs r ON r.id=o.run_id WHERE o.run_id=?1 AND o.id=?2", [run, operation], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    ensure!(
        Path::new(&checkout) == workspace.checkout,
        "checkpoint belongs to another checkout"
    );
    let mut op: Operation = serde_json::from_str(&spec)?;
    op.revision = revision;
    let declaration = op
        .checkpoint
        .as_ref()
        .context("operation has no explicit checkpoint declaration")?;
    validate(&op, declaration)?;
    ensure!(
        state == "passed",
        "waiting: checkpoint operation {operation} must pass its declared check"
    );
    let producers = match &op.stage {
        Stage::Group { tasks } | Stage::Final { tasks } => tasks,
        _ => unreachable!(),
    };
    for task in producers {
        let state: String = conn.query_row(
            "SELECT state FROM tasks WHERE run_id=?1 AND id=?2",
            [run, task],
            |r| r.get(0),
        )?;
        ensure!(
            state == "completed",
            "waiting: checkpoint producer {task} is {state}"
        );
    }
    quiescent(conn, workspace)?;
    let mut statement =
        conn.prepare("SELECT spec,state,revision,command_id FROM operations WHERE run_id=?1 AND (required=1 OR id=?2)")?;
    let rows = statement
        .query_map([run, operation], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let current = snapshot(workspace, &declaration.paths)?;
    for (spec, state, revision, command) in rows {
        let check: Operation = serde_json::from_str(&spec)?;
        if check.inputs.iter().any(|input| {
            declaration
                .paths
                .iter()
                .any(|path| crate::domain::operation_lineage::overlaps(input, path))
        }) {
            ensure!(
                state == "passed",
                "waiting: relevant check {} is {state}; pass it before applying checkpoint",
                check.id
            );
            let command = command
                .context("passed check has no attributable command; rerun declared check")?;
            let (checked_revision, entries): (String,String) = conn.query_row("SELECT revision,entries FROM checkpoint_check_inputs WHERE command_id=?1", [&command], |r|Ok((r.get(0)?,r.get(1)?))).context("passed check has no checkpoint-byte snapshot; rerun the declared check explicitly")?;
            let entries: Vec<Entry> = serde_json::from_str(&entries)?;
            ensure!(
                checked_revision == revision,
                "stale checkpoint check revision; rerun check {}",
                check.id
            );
            for entry in current.iter().filter(|entry| {
                check
                    .inputs
                    .iter()
                    .any(|input| crate::domain::operation_lineage::overlaps(input, &entry.path))
            }) {
                ensure!(entries.iter().any(|checked|checked.path == entry.path && checked.sha256 == entry.sha256), "stale passed check {}: reviewed bytes changed in {}; explicitly invalidate inputs and rerun check", check.id, entry.path);
            }
        }
    }
    Ok(op)
}

pub fn review(
    workspace: &Workspace,
    run: &str,
    operation: &str,
    reviewer: &str,
    attribution: &str,
    message: &str,
) -> Result<Review> {
    ensure!(
        !reviewer.trim().is_empty() && !attribution.trim().is_empty() && !message.trim().is_empty(),
        "reviewer, attribution statement and commit message are required"
    );
    let conn = Connection::open_with_flags(workspace.database(), OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let op = applicable(&conn, workspace, run, operation)?;
    let declaration = op.checkpoint.as_ref().unwrap();
    let (baseline, dirty): (String,String) = conn.query_row("SELECT entries,dirty_paths FROM checkpoint_baselines WHERE run_id=?1 AND operation_id=?2", [run, operation], |r|Ok((r.get(0)?,r.get(1)?))).context("checkpoint has no import-time baseline; author/import a new explicit checkpoint")?;
    let dirty: Vec<String> = serde_json::from_str(&dirty)?;
    ensure!(
        dirty.is_empty(),
        "ambiguous pre-existing edits in checkpoint scope: {}; leave them untouched and author a clean attributable checkpoint",
        dirty.join(", ")
    );
    let baseline: Vec<Entry> = serde_json::from_str(&baseline)?;
    let entries = snapshot(workspace, &declaration.paths)?;
    for (before, after) in baseline.iter().zip(&entries) {
        ensure!(
            before.path == after.path && before.tree_entry == after.tree_entry,
            "ambiguous HEAD change in {} since import; review a new checkpoint baseline",
            after.path
        );
        ensure!(
            git(
                workspace,
                &["diff", "--cached", "--name-only", "--", &after.path]
            )?
            .is_empty(),
            "ambiguous staged edits in {}; leave the existing index untouched and resolve attribution first",
            after.path
        );
        let flags = git(workspace, &["ls-files", "-v", "--", &after.path])?;
        ensure!(
            flags.is_empty() || flags.starts_with("H "),
            "unsupported sparse/assume-unchanged index entry: {}",
            after.path
        );
    }
    ensure!(
        baseline.len() == entries.len(),
        "checkpoint baseline differs from declared scope"
    );
    ensure!(
        baseline
            .iter()
            .zip(&entries)
            .any(|(a, b)| a.sha256 != b.sha256),
        "checkpoint contains no attributable changes"
    );
    Ok(Review {
        contract_version: 1,
        checkout: workspace.checkout.clone(),
        data_root: workspace.data.clone(),
        run: run.into(),
        operation: operation.into(),
        revision: op.revision,
        head: git(workspace, &["rev-parse", "HEAD"])?,
        branch: git(workspace, &["symbolic-ref", "-q", "HEAD"])
            .context("checkpoint requires an attached existing branch")?,
        reviewer: reviewer.into(),
        attribution: attribution.into(),
        message: message.into(),
        entries,
    })
}

/// Holds the ordinary Git index lock while constructing both scoped trees. RAII
/// removes only this invocation's unpublished scratch indexes and lock file.
struct IndexFiles {
    paths: Vec<PathBuf>,
}
impl Drop for IndexFiles {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = fs::remove_file(path);
        }
    }
}

fn feed(command: &mut Command, input: &[u8]) -> Result<String> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(input)?;
    let result = child.wait_with_output()?;
    ensure!(
        result.status.success(),
        "Git checkpoint refused: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().into())
}

fn index_update(
    workspace: &Workspace,
    index: &Path,
    path: &str,
    entry: Option<(&str, &str)>,
) -> Result<()> {
    let mut command = git_command(workspace);
    command.env("GIT_INDEX_FILE", git_path(index));
    if let Some((mode, oid)) = entry {
        command.args(["update-index", "--add", "--cacheinfo", mode, oid, path]);
    } else {
        command.args(["update-index", "--force-remove", "--", path]);
    }
    output(&mut command)?;
    Ok(())
}

pub fn apply(
    workspace: &Workspace,
    approval: &Review,
    wait_seconds: u64,
) -> Result<serde_json::Value> {
    ensure!(
        approval.contract_version == 1
            && approval.checkout == workspace.checkout
            && approval.data_root == workspace.data,
        "review identity differs from this checkout/data root"
    );
    ensure!(
        wait_seconds <= 300,
        "checkpoint wait is bounded to 300 seconds"
    );
    let deadline = Instant::now() + Duration::from_secs(wait_seconds);
    let guard = loop {
        match ControllerGuard::acquire(&workspace.git_dir) {
            Ok(guard) => break guard,
            Err(error) if Instant::now() >= deadline => return Err(error.context("waiting: foreground writer still owns checkout; retry checkpoint apply after writers/checks retire")),
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let store = Store::open(workspace, &guard)?;
    store.conn.execute_batch(SQL)?;
    let hash = digest(&serde_json::to_vec(approval)?);
    let previous: Option<(String, String)> = store
        .conn
        .query_row(
            "SELECT state,commit_id FROM checkpoint_receipts WHERE review_hash=?1",
            [&hash],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((state, commit)) = previous {
        ensure!(
            state == "applied",
            "checkpoint intent {commit} is uncertain; inspect checkpoint_receipts/journal, HEAD and index before any retry; never restage implicitly"
        );
        return Ok(
            serde_json::json!({"state":"applied","commit":commit,"review_hash":hash,"replayed":true}),
        );
    }
    let current = review(
        workspace,
        &approval.run,
        &approval.operation,
        &approval.reviewer,
        &approval.attribution,
        &approval.message,
    )?;
    ensure!(
        &current == approval,
        "review is stale: HEAD, scope, revision or reviewed bytes changed; inspect and generate a fresh checkpoint review"
    );
    let index_path = PathBuf::from(git(
        workspace,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
    )?);
    let lock_path = index_path.with_file_name("index.lock");
    let mut lock = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lock_path)
        .context("Git index is busy; wait for its writer and retry")?;
    let mut scratch = IndexFiles {
        paths: vec![lock_path.clone()],
    };
    let prefix = format!("jtasks-checkpoint-{}", uuid::Uuid::new_v4());
    let tree_index = workspace.git_dir.join(format!("{prefix}-tree.index"));
    let merged_index = workspace.git_dir.join(format!("{prefix}-merged.index"));
    scratch
        .paths
        .extend([tree_index.clone(), merged_index.clone()]);
    let original_index = fs::read(&index_path)?;
    fs::write(&merged_index, &original_index)?;
    output(
        git_command(workspace)
            .env("GIT_INDEX_FILE", git_path(&tree_index))
            .args(["read-tree", "HEAD"]),
    )?;
    for entry in &approval.entries {
        if entry.sha256.is_some() {
            let bytes = fs::read(checked_path(workspace, &entry.path)?)?;
            ensure!(
                Some(digest(&bytes)) == entry.sha256,
                "reviewed content changed: {}; review again",
                entry.path
            );
            let oid = feed(
                git_command(workspace).args(["hash-object", "-w", "--stdin"]),
                &bytes,
            )?;
            let mode = if entry.tree_entry.starts_with("100755 ") {
                "100755"
            } else {
                "100644"
            };
            index_update(workspace, &tree_index, &entry.path, Some((mode, &oid)))?;
            index_update(workspace, &merged_index, &entry.path, Some((mode, &oid)))?;
        } else {
            index_update(workspace, &tree_index, &entry.path, None)?;
            index_update(workspace, &merged_index, &entry.path, None)?;
        }
    }
    let tree = String::from_utf8(output(
        git_command(workspace)
            .env("GIT_INDEX_FILE", git_path(&tree_index))
            .args(["write-tree"]),
    )?)?
    .trim()
    .to_string();
    let old_tree = git(workspace, &["rev-parse", "HEAD^{tree}"])?;
    ensure!(tree != old_tree, "checkpoint would create an empty commit");
    let commit = feed(
        git_command(workspace).args(["commit-tree", &tree, "-p", &approval.head]),
        approval.message.as_bytes(),
    )?;
    lock.write_all(&fs::read(&merged_index)?)?;
    lock.sync_all()?;
    ensure!(
        fs::read(&index_path)? == original_index,
        "Git index changed concurrently; no ref updated"
    );
    ensure!(
        snapshot(
            workspace,
            &approval
                .entries
                .iter()
                .map(|e| e.path.clone())
                .collect::<Vec<_>>()
        )? == approval.entries,
        "reviewed files changed concurrently; no ref updated"
    );
    ensure!(
        git(workspace, &["symbolic-ref", "-q", "HEAD"])? == approval.branch,
        "branch changed concurrently; no ref updated"
    );
    store.conn.execute("INSERT INTO checkpoint_receipts(review_hash,run_id,operation_id,state,commit_id,detail) VALUES(?1,?2,?3,'intent',?4,?5)",params![hash,approval.run,approval.operation,commit,serde_json::to_string(approval)?])?;
    store.conn.execute("INSERT INTO journal(run_id,event,detail) VALUES(?1,'checkpoint_intent',?2)", params![approval.run,serde_json::json!({"review_hash":hash,"commit":commit,"paths":approval.entries.iter().map(|e|&e.path).collect::<Vec<_>>(),"reviewer":approval.reviewer,"attribution":approval.attribution}).to_string()])?;
    // Compare-and-swap refuses a concurrently advanced branch. No checkout, reset,
    // stash, hooks, blanket add, merge, push or worktree creation occurs here.
    git(
        workspace,
        &[
            "update-ref",
            "-m",
            "jtasks explicit scoped checkpoint",
            &approval.branch,
            &commit,
            &approval.head,
        ],
    )
    .context("checkpoint ref/index state requires inspection; persisted intent retained")?;
    drop(lock);
    if let Err(error) = fs::rename(&lock_path, &index_path) {
        // Keep the prepared index lock available for explicit recovery, not Drop.
        scratch.paths.retain(|p| p != &lock_path);
        bail!(
            "checkpoint commit {commit} exists but index publication failed: {error}; inspect persisted intent and retained index.lock before recovery"
        );
    }
    store.conn.execute(
        "UPDATE checkpoint_receipts SET state='applied' WHERE review_hash=?1",
        [&hash],
    )?;
    store.conn.execute(
        "INSERT INTO journal(run_id,event,detail) VALUES(?1,'checkpoint_applied',?2)",
        params![
            approval.run,
            serde_json::json!({"review_hash":hash,"commit":commit}).to_string()
        ],
    )?;
    Ok(serde_json::json!({"state":"applied","commit":commit,"review_hash":hash,"replayed":false}))
}
