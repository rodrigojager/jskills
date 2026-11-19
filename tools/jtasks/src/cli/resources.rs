use crate::{
    resources::{
        identity::{Identity, checkout_key},
        registry::{self, Action, Mode, Observation, Owner, Request},
    },
    storage::Workspace,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args)]
#[command(
    after_help = "The default registry is <runtime-data>/advisory.sqlite3 and is checkout local.\nCross-checkout participants must select the SAME --registry path (or JTASKS_ADVISORY_REGISTRY) and the SAME explicit --shared-scope host/environment. Names are case sensitive. No global registry is discovered or modified automatically.\n\nExample: jtasks resources --registry /shared/advisory.sqlite3 reserve --run demo --task T1 --resource ports/8080 --shared-scope host-a/dev --request claim-1 --actor agent-a --reason declared-work\nExample: jtasks resources --registry /shared/advisory.sqlite3 override <id> --to-run demo --to-task T2 --request move-1 --actor agent-b --reason correct-assignment\n\nOverride/release deliberately accept conflicting work from active or unknown owners. They only change advisory records: they never stop processes or dispatch task attempts. Owner state is an assertion/observation, not verified liveness.\nThe version-1 apply JSON request/receipt is the consumer boundary; direct jlock CLI integration is a separate consumer change. The registry does not replace controller task/process custody."
)]
pub struct Resources {
    /// Explicit shared advisory authority. All participating checkouts must use the same path.
    #[arg(long, global = true, env = "JTASKS_ADVISORY_REGISTRY")]
    registry: Option<PathBuf>,
    /// Output is versioned JSON for both terminal and consumer use.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    action: ResourceAction,
}

#[derive(Args)]
struct Audit {
    #[arg(long)]
    request: String,
    #[arg(long)]
    actor: String,
    #[arg(long)]
    reason: String,
}

#[derive(Args)]
struct OwnerArgs {
    #[arg(long)]
    run: String,
    #[arg(long)]
    task: String,
    #[arg(long)]
    attempt: Option<String>,
    #[arg(long, value_enum, default_value = "unknown")]
    owner_state: Observation,
}

#[derive(Subcommand)]
enum ResourceAction {
    /// Read only; does not create/repair a missing registry.
    List,
    /// Atomically reserve the full explicit set, or report its conflict.
    Reserve {
        #[command(flatten)]
        audit: Audit,
        #[command(flatten)]
        owner: OwnerArgs,
        #[arg(long, value_enum, default_value = "exclusive")]
        mode: Mode,
        #[arg(long)]
        file: Vec<PathBuf>,
        /// Named resource as namespace/name; names are case sensitive.
        #[arg(long)]
        resource: Vec<String>,
        /// Explicit host/environment identity; otherwise named resources are checkout local.
        #[arg(long)]
        shared_scope: Option<String>,
    },
    /// Deliberate release, including active/unknown owners; never stops processes.
    Release {
        reservation: String,
        #[command(flatten)]
        audit: Audit,
    },
    /// Deliberate reassignment; no previous-owner handshake and no task dispatch.
    Override {
        reservation: String,
        #[command(flatten)]
        audit: Audit,
        #[arg(long)]
        to_task: String,
        #[arg(long)]
        to_run: String,
        #[arg(long)]
        to_attempt: Option<String>,
        #[arg(long, value_enum, default_value = "unknown")]
        owner_state: Observation,
    },
    /// Apply the public version-1 JSON protocol from a file (also supports cross-checkout owners).
    Apply { request_file: PathBuf },
}

pub fn execute(workspace: &Workspace, args: Resources) -> Result<i32> {
    let path = args
        .registry
        .unwrap_or_else(|| workspace.data.join("advisory.sqlite3"));
    let path = if path.is_absolute() {
        path
    } else {
        workspace.checkout.join(path)
    };
    let owner = |run: String, task: String, attempt: Option<String>| -> Result<Owner> {
        Ok(Owner {
            checkout: checkout_key(&workspace.checkout)?,
            run,
            task,
            attempt,
        })
    };
    let (audit, operation) = match args.action {
        ResourceAction::List => {
            println!(
                "{}",
                serde_json::json!({"protocol_version":registry::PROTOCOL_VERSION,"reservations":registry::list(&path)?})
            );
            return Ok(0);
        }
        ResourceAction::Reserve {
            audit,
            owner: who,
            mode,
            file,
            resource,
            shared_scope,
        } => {
            let mut resources = Vec::new();
            for file in file {
                resources.push(registry::Resource {
                    identity: Identity::path(&workspace.checkout, &file)?,
                    mode,
                });
            }
            for name in resource {
                let (namespace, name) = name
                    .split_once('/')
                    .context("resource must be namespace/name")?;
                resources.push(registry::Resource {
                    identity: Identity::named(
                        &workspace.checkout,
                        shared_scope.as_deref(),
                        namespace,
                        name,
                    )?,
                    mode,
                });
            }
            (
                audit,
                Action::Reserve {
                    owner: owner(who.run, who.task, who.attempt)?,
                    observation: who.owner_state,
                    resources,
                },
            )
        }
        ResourceAction::Release { reservation, audit } => (audit, Action::Release { reservation }),
        ResourceAction::Override {
            reservation,
            audit,
            to_task,
            to_run,
            to_attempt,
            owner_state,
        } => (
            audit,
            Action::Override {
                reservation,
                to_owner: owner(to_run, to_task, to_attempt)?,
                observation: owner_state,
            },
        ),
        ResourceAction::Apply { request_file } => {
            let request: Request = serde_json::from_slice(&std::fs::read(request_file)?)?;
            println!(
                "{}",
                serde_json::to_string(&registry::apply(&path, &request)?)?
            );
            return Ok(0);
        }
    };
    let receipt = registry::apply(
        &path,
        &Request {
            protocol_version: registry::PROTOCOL_VERSION,
            request_id: audit.request,
            actor: audit.actor,
            reason: audit.reason,
            operation,
        },
    )?;
    println!("{}", serde_json::to_string(&receipt)?);
    Ok(0)
}
