//! Daemonless advisory authority, intentionally independent of task/process custody.
//! Each request is one immediate SQLite transaction; no source file is locked.
//! The versioned JSON request/receipt is the consumer boundary, not the DB schema.
//! Consumers must explicitly share one registry path for shared named identities;
//! the CLI defaults to its checkout's runtime directory. External jlock adoption
//! is a separate consumer change; this module never changes jlock's global state.
use super::identity::Identity;
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Shared,
    Exclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Active,
    Unknown,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub checkout: String,
    pub run: String,
    pub task: String,
    pub attempt: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub identity: Identity,
    pub mode: Mode,
}

// Modes sort only for deterministic full-set acquisition; exclusivity uses explicit matching.
impl PartialOrd for Mode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Mode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub id: String,
    pub resource: Resource,
    pub owner: Owner,
    /// An observation, never proof that work stopped or that a resource is unused.
    pub owner_observation: Observation,
    pub overridden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Reserve {
        owner: Owner,
        observation: Observation,
        resources: Vec<Resource>,
    },
    Release {
        reservation: String,
    },
    Override {
        reservation: String,
        to_owner: Owner,
        observation: Observation,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    pub request_id: String,
    pub actor: String,
    pub reason: String,
    pub operation: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub protocol_version: u32,
    pub request_id: String,
    pub actor: String,
    pub reason: String,
    pub observed_at_ms: u64,
    pub prior: Vec<Reservation>,
    pub current: Vec<Reservation>,
    pub deliberate_override: bool,
    pub processes_stopped: bool,
    pub attempts_dispatched: u32,
}

pub fn list(path: &Path) -> Result<Vec<Reservation>> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    ensure!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0))? == PROTOCOL_VERSION,
        "unsupported advisory registry version"
    );
    read_reservations(&conn)
}

fn read_reservations(conn: &Connection) -> Result<Vec<Reservation>> {
    let mut statement = conn.prepare("SELECT record FROM reservations ORDER BY id")?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
}

fn validate_owner(owner: &mut Owner) -> Result<()> {
    ensure!(
        !owner.checkout.trim().is_empty()
            && !owner.run.trim().is_empty()
            && !owner.task.trim().is_empty(),
        "owner checkout, run and task are required"
    );
    ensure!(
        owner.attempt.as_ref().is_none_or(|s| !s.trim().is_empty()),
        "attempt cannot be empty"
    );
    owner.checkout = super::identity::checkout_key(Path::new(&owner.checkout))?;
    Ok(())
}

pub fn apply(path: &Path, request: &Request) -> Result<Receipt> {
    ensure!(
        request.protocol_version == PROTOCOL_VERSION,
        "unsupported advisory protocol version"
    );
    ensure!(
        !request.request_id.trim().is_empty()
            && !request.actor.trim().is_empty()
            && !request.reason.trim().is_empty(),
        "request ID, actor and reason are required"
    );
    let mut request = request.clone();
    match &mut request.operation {
        Action::Reserve {
            owner, resources, ..
        } => {
            validate_owner(owner)?;
            ensure!(!resources.is_empty(), "at least one resource is required");
            resources.sort();
            resources.dedup();
            for resource in resources.iter_mut() {
                match &mut resource.identity {
                    Identity::Path { checkout, segments } => {
                        ensure!(
                            !checkout.is_empty()
                                && segments.iter().all(|s| !s.is_empty()
                                    && s != "."
                                    && s != ".."
                                    && !s.contains(['/', '\\'])),
                            "invalid normalized path identity"
                        );
                        ensure!(
                            super::identity::checkout_key(Path::new(checkout))? == owner.checkout,
                            "path resource belongs to another checkout"
                        );
                        resource.identity = Identity::path(
                            Path::new(&owner.checkout),
                            &segments.iter().collect::<std::path::PathBuf>(),
                        )?;
                    }
                    Identity::Named {
                        scope,
                        namespace,
                        name,
                    } => {
                        ensure!(
                            !namespace.trim().is_empty() && !name.trim().is_empty(),
                            "named resource identity is empty"
                        );
                        match scope {
                            super::identity::Scope::Checkout(checkout) => {
                                *checkout = super::identity::checkout_key(Path::new(checkout))?;
                                ensure!(
                                    checkout == &owner.checkout,
                                    "named resource belongs to another checkout"
                                );
                            }
                            super::identity::Scope::Shared(scope) => {
                                ensure!(!scope.trim().is_empty(), "shared identity is empty")
                            }
                        }
                    }
                }
            }
            resources.sort();
            resources.dedup();
        }
        Action::Override { to_owner, .. } => validate_owner(to_owner)?,
        _ => {}
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    ensure!(
        version == 0 || version == PROTOCOL_VERSION,
        "unsupported advisory registry version"
    );
    if version == 0 {
        tx.execute_batch("CREATE TABLE reservations(id TEXT PRIMARY KEY, record TEXT NOT NULL); CREATE TABLE receipts(id TEXT PRIMARY KEY, request TEXT NOT NULL, receipt TEXT NOT NULL); PRAGMA user_version=1;")?;
    }
    let encoded = serde_json::to_string(&request)?;
    if let Some((original, receipt)) = tx
        .query_row(
            "SELECT request,receipt FROM receipts WHERE id=?1",
            [&request.request_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        ensure!(
            original == encoded,
            "request ID reused with conflicting advisory mutation"
        );
        return Ok(serde_json::from_str(&receipt)?);
    }
    let existing = read_reservations(&tx)?;
    let mut receipt = Receipt {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id.clone(),
        actor: request.actor.clone(),
        reason: request.reason.clone(),
        observed_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_millis()
            .try_into()?,
        prior: vec![],
        current: vec![],
        deliberate_override: !matches!(request.operation, Action::Reserve { .. }),
        processes_stopped: false,
        attempts_dispatched: 0,
    };
    match &request.operation {
        Action::Reserve {
            owner,
            observation,
            resources,
        } => {
            // Validate the entire set before inserting anything: all or none.
            for wanted in resources {
                if let Some(held) = existing.iter().find(|held| {
                    held.owner != *owner
                        && held.resource.identity.overlaps(&wanted.identity)
                        && (held.resource.mode == Mode::Exclusive || wanted.mode == Mode::Exclusive)
                }) {
                    anyhow::bail!(
                        "advisory resource conflict with reservation {} owned by {}/{}",
                        held.id,
                        held.owner.run,
                        held.owner.task
                    );
                }
            }
            for resource in resources {
                let reservation = Reservation {
                    id: uuid::Uuid::new_v4().to_string(),
                    resource: resource.clone(),
                    owner: owner.clone(),
                    owner_observation: *observation,
                    overridden: false,
                };
                tx.execute(
                    "INSERT INTO reservations(id,record) VALUES(?1,?2)",
                    params![reservation.id, serde_json::to_string(&reservation)?],
                )?;
                receipt.current.push(reservation);
            }
        }
        Action::Release { reservation } | Action::Override { reservation, .. } => {
            let prior = existing
                .iter()
                .find(|r| &r.id == reservation)
                .context("unknown advisory reservation")?
                .clone();
            receipt.prior.push(prior.clone());
            if let Action::Override {
                to_owner,
                observation,
                ..
            } = &request.operation
            {
                // Cross-checkout transfer is valid only for explicit shared named resources.
                if !matches!(
                    &prior.resource.identity,
                    Identity::Named {
                        scope: super::identity::Scope::Shared(_),
                        ..
                    }
                ) {
                    ensure!(
                        prior.owner.checkout == to_owner.checkout,
                        "checkout-local reservation cannot transfer to another checkout"
                    );
                }
                let current = Reservation {
                    owner: to_owner.clone(),
                    owner_observation: *observation,
                    overridden: true,
                    ..prior
                };
                tx.execute(
                    "UPDATE reservations SET record=?2 WHERE id=?1",
                    params![reservation, serde_json::to_string(&current)?],
                )?;
                receipt.current.push(current);
            } else {
                tx.execute("DELETE FROM reservations WHERE id=?1", [reservation])?;
            }
        }
    }
    tx.execute(
        "INSERT INTO receipts(id,request,receipt) VALUES(?1,?2,?3)",
        params![
            request.request_id,
            encoded,
            serde_json::to_string(&receipt)?
        ],
    )?;
    tx.commit()?;
    Ok(receipt)
}
