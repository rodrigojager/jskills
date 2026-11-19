//! `jproject setup`: the interactive first-time configuration.

use crate::config::{self, Config};
use crate::util;
use anyhow::Result;
use clap::Args;

#[derive(Args, Debug, Default, Clone)]
pub struct SetupArgs {
    /// Do not ask anything: use the flags given and defaults for the rest.
    #[arg(long)]
    pub non_interactive: bool,
    /// Default parent folder for new containers.
    #[arg(long, value_name = "DIR")]
    pub projects_root: Option<String>,
    /// Name of the versioned folder inside each container.
    #[arg(long, value_name = "NAME")]
    pub product_dir: Option<String>,
    /// Name of the work folder inside each container.
    #[arg(long, value_name = "NAME")]
    pub work_dir: Option<String>,
    /// Name of the secrets folder inside each container.
    #[arg(long, value_name = "NAME")]
    pub secrets_dir: Option<String>,
    /// jtasks run data folder, relative to the container.
    #[arg(long, value_name = "PATH")]
    pub jtasks_data: Option<String>,
    /// Global folder for generic tools.
    #[arg(long, value_name = "DIR")]
    pub tools_dir: Option<String>,
    /// Path of the global STORAGE.md disk map.
    #[arg(long, value_name = "FILE")]
    pub storage_map: Option<String>,
    /// Folder whose files override the embedded templates.
    #[arg(long, value_name = "DIR")]
    pub templates_dir: Option<String>,
    /// Create a zvec-grep index on `init` (true/false).
    #[arg(long, value_name = "BOOL")]
    pub default_index: Option<bool>,
    /// Use jtasks in new containers (true/false).
    #[arg(long, value_name = "BOOL")]
    pub use_jtasks: Option<bool>,
}

fn ask(question: &str, explanation: &str, default: &str) -> Result<String> {
    println!("\n{explanation}");
    Ok(dialoguer::Input::<String>::new()
        .with_prompt(question)
        .default(default.to_string())
        .allow_empty(true)
        .interact_text()?)
}

fn ask_bool(question: &str, explanation: &str, default: bool) -> Result<bool> {
    println!("\n{explanation}");
    Ok(dialoguer::Confirm::new().with_prompt(question).default(default).interact()?)
}

pub fn run(args: &SetupArgs) -> Result<Config> {
    let mut cfg = Config::load()?.unwrap_or_default();
    let set = |v: &Option<String>, field: &mut String| {
        if let Some(v) = v {
            *field = v.clone();
        }
    };
    set(&args.projects_root, &mut cfg.projects_root);
    set(&args.product_dir, &mut cfg.product_dir);
    set(&args.work_dir, &mut cfg.work_dir);
    set(&args.secrets_dir, &mut cfg.secrets_dir);
    set(&args.jtasks_data, &mut cfg.jtasks_data);
    set(&args.tools_dir, &mut cfg.tools_dir);
    set(&args.storage_map, &mut cfg.storage_map);
    set(&args.templates_dir, &mut cfg.templates_dir);
    if let Some(v) = args.default_index {
        cfg.default_index = v;
    }
    if let Some(v) = args.use_jtasks {
        cfg.use_jtasks = v;
    }

    let interactive = !args.non_interactive && util::interactive();
    if !args.non_interactive && !interactive {
        println!("No terminal: saving the given values and defaults (use --non-interactive to silence this).");
    }
    if interactive {
        println!("jproject setup. Press Enter to keep the value in brackets.");
        cfg.projects_root = ask(
            "Projects root (optional)",
            "Where do you usually create project containers? `jproject init` suggests it. Leave empty to always use the current folder.",
            &cfg.projects_root,
        )?;
        cfg.product_dir = ask(
            "Product folder name",
            "Each container has one versioned folder: the only git repository, with only what the project needs to build, run, test and ship.",
            &cfg.product_dir,
        )?;
        cfg.work_dir = ask(
            "Work folder name",
            "Everything agents produce that is not product (docs drafts, mocks, plans, handoffs, tools) goes to a non-versioned work folder.",
            &cfg.work_dir,
        )?;
        cfg.secrets_dir = ask(
            "Secrets folder name",
            "Real .env files, keys and certificates live in a folder outside git.",
            &cfg.secrets_dir,
        )?;
        cfg.tools_dir = ask(
            "Global tools folder (optional)",
            "Tools agents create stay in each project. When you ask, a generic one is moved to this global folder. Leave empty if you do not want one.",
            &cfg.tools_dir,
        )?;
        let default_map = cfg.storage_map_path().to_string_lossy().into_owned();
        cfg.storage_map = ask(
            "Global storage map (STORAGE.md)",
            "When a folder moves to another disk, jproject records it in a global map: disk roots, project to disk, junctions.",
            if cfg.storage_map.is_empty() { &default_map } else { &cfg.storage_map },
        )?;
        cfg.templates_dir = ask(
            "Templates override folder (optional)",
            "Templates are built into jproject. A file with the same relative path in this folder replaces the built-in one.",
            &cfg.templates_dir,
        )?;
        cfg.default_index = ask_bool(
            "Create a zvec-grep search index on `jproject init`?",
            "zvec-grep (optional) gives agents semantic search over the container. The index excludes secrets and temporary files.",
            cfg.default_index,
        )?;
        cfg.use_jtasks = ask_bool(
            "Use jtasks in new containers?",
            "jtasks (optional) is a task list with dependencies and file ownership for agents.",
            cfg.use_jtasks,
        )?;
        if cfg.use_jtasks {
            cfg.jtasks_data = ask(
                "jtasks data folder",
                "Where should jtasks store run data (tasks, tickets, ledger)? It must NOT be inside the versioned folder. Relative to the container.",
                &cfg.jtasks_data,
            )?;
        }
    }
    let path = cfg.save()?;
    println!("Saved {}", path.display());

    // Suggest (and offer to install) the optional companions.
    let missing: Vec<(&str, bool)> = vec![
        ("zvec-grep", cfg.default_index && !config::has_zvec_grep()),
        ("jtasks", cfg.use_jtasks && !config::has_jtasks()),
    ];
    for (tool, is_missing) in missing {
        if !is_missing {
            continue;
        }
        println!("\n{tool} is enabled but not installed. Install it with `jproject tools install {tool}`.");
        if interactive && util::confirm(&format!("Install {tool} now?"), false)? {
            let t = crate::tools::Tool::from_name(tool)?;
            if let Err(e) = crate::tools::install(t, &mut cfg, false, true) {
                eprintln!("{tool} was not installed: {e:#}");
            }
        }
    }
    Ok(cfg)
}
