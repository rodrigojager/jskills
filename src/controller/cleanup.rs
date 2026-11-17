//! Explicit historical cleanup. Inventory is read-only; execution holds the checkout
//! writer lock and revalidates both logical SQLite ownership and every owned path.
//! The shared database, sidecars, catalogs and advisory authority are never unlinked.
use crate::{domain::validation, resources::ControllerGuard, storage::Workspace};
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, types::ValueRef};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: PathBuf,
    pub kind: String,
    pub bytes: u64,
    pub modified_ns: u128,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rows {
    pub table: String,
    pub count: u64,
    pub logical_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub id: String,
    pub state: String,
    pub finished: bool,
    pub discards_resumability: bool,
    pub blockers: Vec<String>,
    pub roots: Vec<PathBuf>,
    pub entries: Vec<Entry>,
    pub rows: Vec<Rows>,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub contract_version: u32,
    pub checkout: PathBuf,
    pub data_root: PathBuf,
    pub finished_only: bool,
    pub runs: Vec<Run>,
    pub bytes: u64,
    /// Shared storage is deliberately retained, even when the last run is removed.
    pub retained_shared: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub contract_version: u32,
    pub approval_sha256: String,
    pub checkout: PathBuf,
    pub data_root: PathBuf,
    pub removed_runs: Vec<String>,
    pub removed_paths: Vec<PathBuf>,
    pub removed_rows: u64,
    pub removed_file_bytes: u64,
    pub retained_shared: Vec<PathBuf>,
    pub finished_only: bool,
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Reject links/reparse points in *every* component, including the selected root.
/// Canonical containment alone would permit an internal junction or swapped link.
fn checked(path: &Path) -> Result<PathBuf> {
    ensure!(path.is_absolute(), "cleanup paths must be absolute");
    let mut current = PathBuf::new();
    for component in path.components() {
        ensure!(
            !matches!(component, Component::ParentDir),
            "cleanup traversal is forbidden"
        );
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        let metadata = fs::symlink_metadata(&current)?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "cleanup rejects symlinks: {}",
            current.display()
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "cleanup rejects reparse points: {}",
                current.display()
            );
        }
    }
    fs::canonicalize(path).map_err(Into::into)
}

fn roots(workspace: &Workspace) -> Result<(PathBuf, PathBuf)> {
    let checkout = checked(&workspace.checkout)?;
    let data = checked(&workspace.data)?;
    let git = checked(&workspace.git_dir)?;
    ensure!(
        data != checkout && !checkout.starts_with(&data),
        "data root overlaps source checkout"
    );
    ensure!(
        !data.starts_with(&checkout) || (data.starts_with(&git) && data != git),
        "runtime data must be outside source or under the Git directory"
    );
    checked(&workspace.database())?;
    Ok((checkout, data))
}

fn collect(root: &Path, boundary: &Path, output: &mut Vec<Entry>) -> Result<()> {
    let path = checked(root)?;
    ensure!(
        path.starts_with(boundary) && path != boundary,
        "cleanup path escapes owned data root"
    );
    let metadata = fs::symlink_metadata(root)?;
    ensure!(
        metadata.is_dir() || metadata.is_file(),
        "unsupported cleanup file type"
    );
    let digest = if metadata.is_file() {
        let mut file = fs::File::open(root)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        format!("{hasher:x}", hasher = hasher.finalize())
    } else {
        String::new()
    };
    output.push(Entry {
        path,
        kind: if metadata.is_dir() {
            "directory"
        } else {
            "file"
        }
        .into(),
        bytes: if metadata.is_file() {
            metadata.len()
        } else {
            0
        },
        modified_ns: metadata
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        sha256: digest,
    });
    if metadata.is_dir() {
        let mut children = fs::read_dir(root)?
            .map(|r| r.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            collect(&child, boundary, output)?;
        }
    }
    Ok(())
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn tables(conn: &Connection) -> Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    Ok(statement
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}
fn table(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}

/// Select solely by durable ownership. Child tables without run_id inherit the
/// immutable attempt identity. Unknown storage must be explicitly understood.
fn predicate(conn: &Connection, name: &str) -> Result<String> {
    ensure!(
        name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "unsupported table name"
    );
    if name == "runs" {
        return Ok("id=?1".into());
    }
    let mut statement = conn.prepare(&format!("PRAGMA table_info(\"{name}\")"))?;
    let columns = statement
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if columns.iter().any(|c| c == "run_id") {
        return Ok("run_id=?1".into());
    }
    if columns.iter().any(|c| c == "attempt_id") {
        return Ok("attempt_id IN (SELECT id FROM attempts WHERE run_id=?1)".into());
    }
    bail!("unknown runtime table ownership: {name}; explicit management reconciliation required")
}

fn row_inventory(conn: &Connection, name: &str, run: &str) -> Result<Rows> {
    let filter = predicate(conn, name)?;
    let mut statement = conn.prepare(&format!(
        "SELECT * FROM \"{name}\" WHERE {filter} ORDER BY rowid"
    ))?;
    let columns = statement.column_count();
    let mut rows = statement.query([run])?;
    let mut count = 0;
    let mut bytes = 0;
    let mut digest = Sha256::new();
    while let Some(row) = rows.next()? {
        count += 1;
        digest.update([0xff]);
        for i in 0..columns {
            let (kind, value) = match row.get_ref(i)? {
                ValueRef::Null => (0, vec![]),
                ValueRef::Integer(v) => (1, v.to_le_bytes().to_vec()),
                ValueRef::Real(v) => (2, v.to_bits().to_le_bytes().to_vec()),
                ValueRef::Text(v) => (3, v.to_vec()),
                ValueRef::Blob(v) => (4, v.to_vec()),
            };
            bytes += value.len() as u64;
            digest.update([kind]);
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value);
        }
    }
    Ok(Rows {
        table: name.into(),
        count,
        logical_bytes: bytes,
        sha256: format!("{:x}", digest.finalize()),
    })
}

fn ids(conn: &Connection, name: &str, run: &str) -> Result<Vec<String>> {
    if !table(conn, name)? {
        return Ok(vec![]);
    }
    let mut q = conn.prepare(&format!(
        "SELECT id FROM {name} WHERE run_id=?1 ORDER BY id"
    ))?;
    Ok(q.query_map([run], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?)
}

fn unresolved(conn: &Connection, table_name: &str, run: &str, terminal: &str) -> Result<bool> {
    if !table(conn, table_name)? {
        return Ok(false);
    }
    Ok(conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table_name} WHERE run_id=?1 AND state NOT IN ({terminal}))"), [run], |r| r.get(0))?)
}

fn custody(path: &Path, blockers: &mut Vec<String>, required: bool) -> Result<()> {
    if !exists(path)? {
        if required {
            blockers.push(format!("missing custody proof: {}", path.display()));
        }
        return Ok(());
    }
    checked(path)?;
    let record = serde_json::from_slice::<crate::execution::custody::Record>(&fs::read(path)?);
    if !record.is_ok_and(|r| {
        r.empty
            && r.platform == std::env::consts::OS
            && r.scope
                .strip_prefix("jtasks-")
                .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
    }) {
        blockers.push(format!(
            "unresolved custody: {}; explicit management reconciliation required",
            path.display()
        ));
    }
    Ok(())
}

pub fn preview(
    workspace: &Workspace,
    selection: &[String],
    previous: bool,
    finished_only: bool,
) -> Result<Preview> {
    let conn = crate::read::projections::open(workspace)?;
    preview_with(&conn, workspace, selection, previous, finished_only)
}

/// Accept a snapshot connection so execution can inventory within its immediate
/// transaction, and tests can exercise SQL retirement entirely in memory.
pub fn preview_with(
    conn: &Connection,
    workspace: &Workspace,
    selection: &[String],
    previous: bool,
    finished_only: bool,
) -> Result<Preview> {
    let (checkout, data_root) = roots(workspace)?;
    let names = tables(conn)?;
    let selected = if previous {
        ensure!(
            selection.is_empty(),
            "previous and explicit IDs cannot be combined"
        );
        let mut q = conn.prepare("SELECT id FROM runs WHERE state='completed' ORDER BY id")?;
        q.query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?
    } else {
        selection.to_vec()
    };
    let selected: BTreeSet<_> = selected.into_iter().collect();
    let registry = crate::controller::admission::registry_path(workspace);
    if exists(&registry)? {
        checked(&registry)?;
    }
    let reservations = crate::resources::registry::list(&registry)?;
    let checkout_key = crate::resources::identity::checkout_key(&checkout)?;
    let mut runs = Vec::new();
    for id in selected {
        validation::identifier(&id)?;
        let (owner, state): (String, String) = conn
            .query_row("SELECT checkout,state FROM runs WHERE id=?1", [&id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?
            .context("unknown selected run")?;
        ensure!(
            Path::new(&owner) == checkout,
            "selected run belongs to another checkout"
        );
        let finished =
            state == "completed" && crate::read::status::from_connection(conn, &id)?.complete;
        // Automatic previous selection is always stricter than explicit history.
        if previous && !finished {
            continue;
        }
        let mut blockers = Vec::new();
        if !matches!(
            state.as_str(),
            "completed" | "incomplete" | "stopped" | "ready" | "paused"
        ) {
            blockers.push(format!("run is active or unresolved: {state}"));
        }
        if finished_only && !finished {
            blockers.push(
                "strict finished-run selection requires completed tasks and required operations"
                    .into(),
            );
        }
        for (name, terminal) in [
            ("attempts", "'retired','interrupted'"),
            (
                "managed_commands",
                "'queued','stopped','exited','launch_failed'",
            ),
            ("reviewer_calls", "'completed','failed','stale','stopped'"),
            ("checkpoint_receipts", "'applied'"),
        ] {
            if unresolved(conn, name, &id, terminal)? {
                blockers.push(format!(
                    "live/unresolved {name}; explicit management reconciliation required"
                ));
            }
        }
        if reservations
            .iter()
            .any(|r| r.owner.checkout == checkout_key && r.owner.run == id)
        {
            blockers.push(
                "advisory reservations remain; release/reconcile them explicitly before cleanup"
                    .into(),
            );
        }
        let mut owned_roots = vec![workspace.data.join("runs").join(&id)];
        for (table_name, directory) in [
            ("attempts", "attempts"),
            ("managed_commands", "commands"),
            ("reviewer_calls", "reviews"),
        ] {
            for child in ids(conn, table_name, &id)? {
                validation::identifier(&child)?;
                let root = workspace.data.join(directory).join(child);
                // Queued commands have not launched and require no custody file.
                let required = match table_name {
                    "attempts" => true,
                    "managed_commands" => conn.query_row("SELECT state!='queued' AND state!='launch_failed' FROM managed_commands WHERE id=?1", [root.file_name().unwrap().to_string_lossy().as_ref()], |r| r.get(0))?,
                    // invoked=0 is an observation of admission, not durable proof
                    // that an earlier root/snapshot scope was never launched.
                    // A bound=false, empty=true record proves a failed/no-launch
                    // scope; absent/torn records need management reconciliation.
                    _ => true,
                };
                if exists(&root)? {
                    checked(&root)?;
                }
                custody(&root.join("custody.json"), &mut blockers, required)?;
                if directory == "reviews" {
                    custody(&root.join("snapshot/custody.json"), &mut blockers, true)?;
                }
                owned_roots.push(root);
            }
        }
        owned_roots.sort();
        let mut entries = Vec::new();
        let mut canonical_roots = Vec::new();
        for root in owned_roots {
            if exists(&root)? {
                let canonical = checked(&root)?;
                canonical_roots.push(canonical);
                collect(&root, &data_root, &mut entries)?;
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        // Catch custody records nested in future output layouts too.
        for entry in &entries {
            if entry.kind == "file" && entry.path.file_name().is_some_and(|n| n == "custody.json") {
                custody(&entry.path, &mut blockers, true)?;
            }
        }
        blockers.sort();
        blockers.dedup();
        let rows = names
            .iter()
            .map(|name| row_inventory(conn, name, &id))
            .collect::<Result<Vec<_>>>()?;
        let bytes = entries.iter().map(|e| e.bytes).sum();
        runs.push(Run {
            id,
            state,
            finished,
            discards_resumability: !finished,
            blockers,
            roots: canonical_roots,
            entries,
            rows,
            bytes,
        });
    }
    let bytes = runs.iter().map(|r| r.bytes).sum();
    let mut retained_shared = vec![
        workspace.database(),
        registry,
        workspace.data.join("config"),
        workspace.data.join("catalog"),
    ];
    for suffix in ["-wal", "-shm", "-journal"] {
        retained_shared.push(PathBuf::from(format!(
            "{}{suffix}",
            workspace.database().display()
        )));
    }
    retained_shared.sort();
    retained_shared.dedup();
    Ok(Preview { contract_version: CONTRACT_VERSION, checkout, data_root, finished_only: finished_only || previous, runs, bytes, retained_shared, warnings: vec!["Removal is permanent; explicitly selected unfinished history loses same-task sessions and resumability.".into(), "Database byte reclamation is not promised: the shared SQLite database/sidecars, configuration and advisory registry are retained.".into()] })
}

/// The only destructive filesystem seam. Tests inject a recording implementation;
/// no real cleanup is needed to verify approval, custody and SQL isolation.
pub trait Removal {
    fn remove_file(&mut self, path: &Path) -> Result<()>;
    fn remove_directory(&mut self, path: &Path) -> Result<()>;
}
struct Filesystem;
impl Removal for Filesystem {
    fn remove_file(&mut self, path: &Path) -> Result<()> {
        fs::remove_file(path).map_err(Into::into)
    }
    fn remove_directory(&mut self, path: &Path) -> Result<()> {
        fs::remove_dir(path).map_err(Into::into)
    }
}

pub fn execute(workspace: &Workspace, approval: &Preview, discard_resume: bool) -> Result<Receipt> {
    let guard = ControllerGuard::acquire(&workspace.git_dir)?;
    checked(&workspace.database())?;
    let mut conn =
        Connection::open_with_flags(workspace.database(), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    execute_with(
        &mut conn,
        workspace,
        &guard,
        approval,
        discard_resume,
        &mut Filesystem,
    )
}

pub fn execute_with(
    conn: &mut Connection,
    workspace: &Workspace,
    _guard: &ControllerGuard,
    approval: &Preview,
    discard_resume: bool,
    removal: &mut impl Removal,
) -> Result<Receipt> {
    ensure!(
        approval.contract_version == CONTRACT_VERSION,
        "unsupported cleanup contract"
    );
    ensure!(!approval.runs.is_empty(), "empty cleanup selection");
    ensure!(
        approval.runs.iter().all(|r| r.blockers.is_empty()),
        "selected runs have blockers; reconcile explicitly and preview again"
    );
    ensure!(
        discard_resume || !approval.runs.iter().any(|r| r.discards_resumability),
        "unfinished history requires --discard-resume"
    );
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.pragma_update(None, "defer_foreign_keys", "ON")?;
    let selection = approval
        .runs
        .iter()
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let current = preview_with(&tx, workspace, &selection, false, approval.finished_only)?;
    ensure!(
        &current == approval,
        "cleanup ownership/inventory changed; a new preview and approval are required"
    );
    ensure!(
        current.runs.iter().all(|r| r.blockers.is_empty()),
        "custody changed; cleanup refused"
    );
    let mut removed_paths = Vec::new();
    let mut removed_rows = 0;
    for run in &current.runs {
        let mut entries = run.entries.clone();
        // Nonrecursive removal refuses new children; never follow a tree at deletion.
        entries.sort_by_key(|e| std::cmp::Reverse(e.path.components().count()));
        for entry in entries {
            let canonical = checked(&entry.path)?;
            ensure!(
                canonical == entry.path && canonical.starts_with(&current.data_root),
                "cleanup path changed"
            );
            if entry.kind == "file" {
                removal.remove_file(&entry.path)?;
            } else {
                removal.remove_directory(&entry.path)?;
            }
            removed_paths.push(entry.path);
        }
        // Delete dependent rows before attempts, whose identity defines child ownership.
        // Foreign keys are deferred to preserve all cross-table constraints at commit.
        for row in run
            .rows
            .iter()
            .filter(|r| r.table != "attempts" && r.table != "runs")
        {
            let filter = predicate(&tx, &row.table)?;
            removed_rows += tx.execute(
                &format!("DELETE FROM \"{}\" WHERE {filter}", row.table),
                [&run.id],
            )? as u64;
        }
        removed_rows += tx.execute("DELETE FROM attempts WHERE run_id=?1", [&run.id])? as u64;
        removed_rows += tx.execute("DELETE FROM runs WHERE id=?1", [&run.id])? as u64;
    }
    tx.commit()?;
    Ok(Receipt {
        contract_version: CONTRACT_VERSION,
        approval_sha256: hash(&serde_json::to_vec(approval)?),
        checkout: current.checkout,
        data_root: current.data_root,
        removed_runs: selection,
        removed_paths,
        removed_rows,
        removed_file_bytes: current.bytes,
        retained_shared: current.retained_shared,
        finished_only: current.finished_only,
    })
}
