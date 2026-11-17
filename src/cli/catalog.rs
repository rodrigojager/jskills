use crate::{
    executors::catalog::{self, Snapshot},
    storage::Workspace,
};
use anyhow::{Result, ensure};
use clap::Args;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, time::Duration};

#[derive(Args)]
pub struct Catalog {
    pub executor: String,
    /// Explicit bounded version/help and native catalog discovery; never a turn.
    #[arg(long, conflicts_with = "snapshot")]
    pub refresh: bool,
    /// Import richer scoped evidence explicitly; discovery cannot prove access.
    #[arg(long, conflicts_with = "refresh")]
    pub snapshot: Option<PathBuf>,
    #[arg(long)]
    pub executable: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pub launcher_arg: Vec<String>,
    /// Caller-managed configuration identity (change it after native config edits).
    #[arg(long, default_value = "native-default")]
    pub scope: String,
    #[arg(long, default_value_t=3000, value_parser=clap::value_parser!(u64).range(50..=10000))]
    pub timeout_ms: u64,
    #[arg(long)]
    pub provider: Option<String>,
    #[arg(long)]
    pub model: Option<String>,
    #[arg(long)]
    pub effort: Option<String>,
    #[arg(long)]
    pub variant: Option<String>,
    #[arg(long)]
    pub native_agent: Option<String>,
    #[arg(long)]
    pub native_profile: Option<String>,
}
pub fn view(snapshot: Snapshot) -> Value {
    let freshness = if snapshot.observed_at == 0 {
        "unavailable"
    } else if catalog::now() >= snapshot.recommended_refresh_at {
        "stale"
    } else {
        "cached_fresh"
    };
    json!({"snapshot":snapshot,"freshness":freshness,"supported_options_scope":"jtasks adapter; native_flags are observed help only", "configuration_fingerprint":"unavailable; scope identity is caller-managed", "access":"not_verified; catalog membership does not authorize an account", "metadata_helpers":"explicit refresh only; owned bounded helpers retired before return", "metadata_interfaces":{"codex":"App Server stdio model/list; --provider selects configured provider, otherwise native default", "opencode":"models --standalone; project-scoped provider/model identifiers", "claude":"installed help has no metadata-only model/reusable-agent catalog; agents manages background sessions"}})
}
pub fn execute(workspace: &Workspace, args: Catalog) -> Result<i32> {
    catalog::name(&args.executor)?;
    let scope = catalog::scope(
        workspace,
        args.executable.as_deref().unwrap_or(&args.executor),
        &args.launcher_arg,
        &args.scope,
    );
    let mut snapshot = if let Some(path) = args.snapshot {
        let snapshot: Snapshot = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(
            snapshot.executor == args.executor && snapshot.scope == scope,
            "snapshot executor/scope does not match request"
        );
        catalog::save(workspace, snapshot, true)?
    } else if args.refresh {
        catalog::save(
            workspace,
            catalog::refresh(
                &args.executor,
                scope,
                Duration::from_millis(args.timeout_ms),
                args.provider.as_deref(),
            )?,
            false,
        )?
    } else {
        catalog::read(workspace, &args.executor, &scope)?
            .unwrap_or_else(|| catalog::unavailable(&args.executor, scope))
    };
    let filters = [
        ("provider", args.provider),
        ("model", args.model),
        ("effort", args.effort),
        ("variant", args.variant),
        ("native_agent", args.native_agent),
        ("native_profile", args.native_profile),
    ];
    let filtered = filters.iter().any(|(_, v)| v.is_some());
    snapshot.choices.retain(|c| {
        filters.iter().all(|(field, value)| {
            value.as_ref().is_none_or(|value| {
                if c.field == *field {
                    c.value == *value
                } else if *field == "provider" {
                    c.provider.as_ref().is_none_or(|p| p == value)
                } else if *field == "model" {
                    c.model.as_ref().is_none_or(|m| m == value)
                } else {
                    true
                }
            })
        })
    });
    let mut output = view(snapshot);
    output["filtered"] = json!(filtered);
    println!("{}", serde_json::to_string(&output)?);
    Ok(0)
}
pub fn executors(workspace: &Workspace) -> Result<Value> {
    let mut snapshots = vec![];
    for executor in ["codex", "claude", "opencode"] {
        let scope = catalog::scope(workspace, executor, &[], "native-default");
        snapshots.push(view(
            catalog::read(workspace, executor, &scope)?
                .unwrap_or_else(|| catalog::unavailable(executor, scope)),
        ));
    }
    Ok(json!({"executors":snapshots,"read_only":true,"refresh":"catalog <executor> --refresh"}))
}
