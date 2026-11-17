use crate::{
    domain::profiles::{self, ProfileConfig, ProfileFile},
    storage::Workspace,
};
use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};

#[derive(Args)]
pub struct Profiles {
    #[command(subcommand)]
    pub action: Action,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum Scope {
    Project,
    User,
}
#[derive(Subcommand)]
pub enum Action {
    /// List effective jtasks aliases and their source files.
    List,
    /// Resolve an alias, optionally using explicit task override/run-default JSON.
    Show {
        alias: String,
        #[arg(long)]
        overrides: Option<PathBuf>,
        #[arg(long)]
        defaults: Option<PathBuf>,
    },
    /// Replace one alias; native settings and runtime attempts are never edited.
    Set {
        alias: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
    /// Merge a versioned profiles file atomically, replacing matching aliases.
    Import {
        file: PathBuf,
        #[arg(long, value_enum, default_value = "project")]
        scope: Scope,
    },
}
fn config(path: Option<PathBuf>) -> Result<ProfileConfig> {
    match path {
        Some(path) => Ok(serde_json::from_slice(&fs::read(path)?)?),
        None => Ok(ProfileConfig::default()),
    }
}
pub fn execute(workspace: &Workspace, args: Profiles) -> Result<i32> {
    let target = |scope| -> Result<PathBuf> {
        match scope {
            Scope::Project => Ok(profiles::project_file(workspace)),
            Scope::User => profiles::user_file(),
        }
    };
    let output = match args.action {
        Action::List => {
            json!({"aliases":profiles::aliases(workspace)?.into_iter().map(|(alias,(configuration,source))| json!({"alias":alias,"configuration":configuration,"source":source})).collect::<Vec<_>>(),"identity":"jtasks aliases; native_profile/native_agent are separate fields"})
        }
        Action::Show {
            alias,
            overrides,
            defaults,
        } => json!(profiles::effective(
            workspace,
            &alias,
            &config(overrides)?,
            &config(defaults)?
        )?),
        Action::Set { alias, file, scope } => {
            let config: ProfileConfig = serde_json::from_slice(&fs::read(file)?)
                .context("expected profile configuration object")?;
            let path = target(scope)?;
            profiles::update(&path, BTreeMap::from([(alias.clone(), config)]))?;
            json!({"alias":alias,"saved":path})
        }
        Action::Import { file, scope } => {
            let file: ProfileFile = serde_json::from_slice(&fs::read(file)?)
                .context("expected versioned profiles file")?;
            profiles::validate_file(&file)?;
            let count = file.profiles.len();
            let path = target(scope)?;
            profiles::update(&path, file.profiles)?;
            json!({"imported":count,"saved":path})
        }
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(0)
}
