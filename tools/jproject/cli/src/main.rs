//! jproject: creates and lints AI workspace containers.

mod check;
mod config;
mod container;
mod init;
mod map;
mod ops;
mod plan;
mod setup;
mod templates;
mod tools;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use std::path::PathBuf;

const ABOUT: &str = "Creates and lints AI workspace containers.

A container is a project folder (not a repository) with:
  product/   the only git repository: what the project needs to build, run, test and ship
  _work/     everything agents produce (docs drafts, mocks, plans, handoffs, tools), not versioned
  _secrets/  real .env files and keys, outside git
Every folder explains itself with an AGENTS.md; _work/WORKSPACE.md is the map.
zvec-grep (search index) and jtasks (task runs) are optional companions: `jproject tools`.";

const MAIN_EXAMPLES: &str = "Examples:
  jproject setup                     answer the first-time questions
  jproject init my-game              create a container in ./my-game
  jproject check                     lint the container you are in
  jproject new proposal dark-mode    start a proposal in _work/70-proposals/
  jproject help init                 help for one command";

#[derive(Parser)]
#[command(name = "jproject", version, about = "Creates and lints AI workspace containers.", long_about = ABOUT,
    after_help = MAIN_EXAMPLES, arg_required_else_help = true, propagate_version = true)]
struct Cli {
    /// Container to act on (default: found by walking up from the current folder).
    #[arg(long, short = 'C', global = true, value_name = "DIR")]
    path: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ask the first-time questions and save the global configuration.
    #[command(after_help = "Asks each value with an explanation and saves <config-dir>/jproject/config.toml.
Run it again any time to change values. Every value also has a flag.

Examples:
  jproject setup
  jproject setup --non-interactive --product-dir product --tools-dir ~/tools
  jproject setup --use-jtasks false")]
    Setup(setup::SetupArgs),

    /// Create a container (or complete an existing one), or a parallel folder.
    #[command(after_help = "Creates the full container layout with the minimum files in each folder, from
the templates. In an existing folder it only creates what is missing, never overwrites,
and lists what it created. Creates a zvec-grep index when zvec-grep is installed and
`default_index` is on (`--no-index` skips it).

With --parallel it creates a parallel folder instead: AGENTS.md/CLAUDE.md that declare the
origin container read-only, out/ for the deliverable, _work/ and HANDOFF.md.

Without PATH: <projects_root>/<name> when --name and projects_root are set, else the current folder.

Examples:
  jproject init my-game
  jproject init . --name my-game --no-index
  jproject init --parallel ../my-game-crafting --origin ../my-game")]
    Init {
        /// Folder of the container (created if missing).
        path: Option<PathBuf>,
        /// Project name (default: the folder name).
        #[arg(long)]
        name: Option<String>,
        /// Do not run `git init` in the product folder.
        #[arg(long)]
        no_git: bool,
        /// Do not create the zvec-grep index.
        #[arg(long)]
        no_index: bool,
        /// Create a parallel folder instead of a container (needs --origin).
        #[arg(long, requires = "origin")]
        parallel: bool,
        /// The container the parallel folder reads from (read-only).
        #[arg(long, value_name = "CONTAINER")]
        origin: Option<PathBuf>,
    },

    /// Print the container, product, work, secrets and jtasks data paths.
    #[command(after_help = "Finds the container by walking up from the current folder (from inside product/
it resolves to the parent). Other tools can call this instead of reimplementing the rule.

Examples:
  jproject root
  jproject root --json")]
    Root {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },

    /// Lint the container: report everything that breaks the framework rules.
    #[command(after_help = "Rules: Markdown not allowed in the product; versioned names (_v2, _final, _old,
_copy, .bak-<date>); more than one backup per file; pending _variants/; intakes without
INTAKE.md; proposals without PROPOSAL.md or stale; stray files at the root of _work/;
outdated product summary; .scratch/ in the product; broken junctions; folders without
AGENTS.md. Exits with 1 when there is an error (warnings do not fail), so it fits a
pre-commit hook.

Examples:
  jproject check
  jproject check --product-only
  jproject check --json --stale-days 30")]
    Check {
        /// Only the rules about the product repository (used by the pre-commit hook).
        #[arg(long)]
        product_only: bool,
        /// Days without change after which a proposal is reported as stale.
        #[arg(long, default_value_t = 14)]
        stale_days: u64,
        /// Print the findings as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Create an intake, a proposal or a handoff from its template.
    #[command(after_help = "intake   -> _work/60-intake/<name>/INTAKE.md
proposal -> _work/70-proposals/<name>/PROPOSAL.md
handoff  -> _work/40-handoff/<name>.md
Then updates Active items in _work/WORKSPACE.md.

Examples:
  jproject new intake crafting-system
  jproject new proposal dark-mode
  jproject new handoff inventory")]
    New {
        /// What to create.
        #[arg(value_enum)]
        kind: ops::ItemKind,
        /// Name of the intake/proposal folder or the handoff workstream.
        name: String,
    },

    /// Summary: open intakes, proposals, variants and handoffs; folder sizes; disks.
    #[command(after_help = "Examples:
  jproject status
  jproject status -C ../my-game")]
    Status,

    /// Empty _work/00-tmp; list pending variants and proposals; optionally prune Docker.
    #[command(after_help = "Emptying 00-tmp needs no confirmation (it is ephemeral by definition). Variants and
proposals are only listed. With --docker, prunes dangling images labelled
jproject.project=<project> after a confirmation.

Examples:
  jproject clean
  jproject clean --dry-run
  jproject clean --docker --yes")]
    Clean {
        /// Also prune this project's dangling Docker images.
        #[arg(long)]
        docker: bool,
        /// Only show what would be removed.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },

    /// Prepare the planning folder for a different plan.
    #[command(subcommand)]
    Plan(PlanCmd),

    /// Regenerate the automatic sections of _work/WORKSPACE.md.
    #[command(after_help = "Rewrites Storage, Active items, Project skills and Product summary (between the
jproject:auto markers) and keeps everything written by hand.

Examples:
  jproject map")]
    Map,

    /// Move container folders to another disk, leaving a junction in place.
    #[command(subcommand)]
    Storage(StorageCmd),

    /// Install the framework section in the agents' global instructions.
    #[command(subcommand)]
    Rules(RulesCmd),

    /// Install the pre-commit hook in the product repository.
    #[command(subcommand)]
    Hook(HookCmd),

    /// Manage the optional companions: zvec-grep and jtasks.
    #[command(subcommand)]
    Tools(ToolsCmd),
}

#[derive(Subcommand)]
enum PlanCmd {
    /// Remove the previous plan only after reviewing pending work and the exact inventory.
    #[command(after_help = "First move unresolved work and its shared context to 10-docs/BACKLOG.md.
Review BACKLOG.md and 80-ideas/IDEAS.md with the user before choosing the next scope.
This command clears 30-plan except AGENTS.md and removes finished jtasks runs through
jtasks clean. Other work folders, ADRs, glossary, backlog and ideas are preserved.
Active, paused, unknown or externally stored runs block the reset. No archive is created.
The approval token binds the exact file contents; any change requires a new preview.
Plan files go to the Recycle Bin/trash. Native jtasks clean permanently removes run data.

Examples:
  jproject plan reset --dry-run
  jproject plan reset --pending-reviewed --approve <token-from-preview>")]
    Reset {
        /// Preview only; this is also the default without --approve.
        #[arg(long, conflicts_with = "approve")]
        dry_run: bool,
        /// Token from the exact inventory approved by the user.
        #[arg(long)]
        approve: Option<String>,
        /// Confirm unresolved work and shared context have been consolidated in BACKLOG.md.
        #[arg(long)]
        pending_reviewed: bool,
    },
}

#[derive(Subcommand)]
enum StorageCmd {
    /// Move a folder of the container to another location and leave a junction.
    #[command(after_help = "The logical path does not change, so agents do not notice. Records the physical
path in _work/WORKSPACE.md (Storage) and in the global storage map. Asks for confirmation.

Examples:
  jproject storage move _work/25-source-assets ../../overflow-disk/my-game/25-source-assets
  jproject storage move _work/20-design /mnt/big/my-game/20-design --dry-run")]
    Move {
        /// Folder inside the container (relative path).
        folder: String,
        /// New physical location (a new or empty folder).
        destination: PathBuf,
        /// Only show what would happen.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum RulesCmd {
    /// Add or update the framework section between <!-- jproject:start/end --> markers.
    #[command(after_help = "Default targets: ~/.claude/CLAUDE.md, ~/.codex/AGENTS.md, ~/.config/opencode/AGENTS.md.
Only the text between the markers is touched. Shows the diff and asks before writing.

Examples:
  jproject rules install
  jproject rules install --dry-run
  jproject rules install --target ./AGENTS.md --yes")]
    Install {
        /// Instruction file to update instead of the defaults (repeatable).
        #[arg(long, value_name = "FILE")]
        target: Vec<PathBuf>,
        /// Only show the diff.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Print the framework section.
    #[command(after_help = "Examples:
  jproject rules show")]
    Show,
}

#[derive(Subcommand)]
enum HookCmd {
    /// Write product/.git/hooks/pre-commit running `jproject check --product-only`.
    #[command(after_help = "Examples:
  jproject hook install
  jproject hook install --force")]
    Install {
        /// Replace an existing pre-commit hook.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum ToolsCmd {
    /// Show whether zvec-grep and jtasks are installed and used.
    #[command(after_help = "Examples:
  jproject tools status")]
    Status,
    /// Install a companion (shows every command and asks first).
    #[command(after_help = "zvec-grep: npm install -g @zvec/zvec-grep, `zg install` for the detected agents,
default embedding model.
jtasks: enable an already installed native v2 CLI on PATH. Install the CLI from a
trusted distribution separately; no release source is assumed.

Examples:
  jproject tools install zvec-grep
  jproject tools install jtasks --dry-run")]
    Install {
        #[arg(value_enum)]
        tool: tools::Tool,
        /// Only show the commands.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Update a companion.
    #[command(after_help = "Examples:
  jproject tools update jtasks")]
    Update {
        #[arg(value_enum)]
        tool: tools::Tool,
        /// Only show the commands.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Uninstall a companion (data in containers is kept).
    #[command(after_help = "Examples:
  jproject tools uninstall zvec-grep --dry-run")]
    Uninstall {
        #[arg(value_enum)]
        tool: tools::Tool,
        /// Only show the commands.
        #[arg(long)]
        dry_run: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(2);
        }
    }
}

fn run(cli: Cli) -> Result<i32> {
    let path = cli.path.as_deref();
    match cli.command {
        Command::Setup(args) => {
            setup::run(&args)?;
        }
        Command::Init { path: target, name, no_git, no_index, parallel, origin } => {
            let cfg = Config::load_or_setup()?;
            if parallel {
                init::init_parallel(target, origin.as_deref().unwrap(), &cfg)?;
            } else {
                init::init(init::InitOptions { path: target.or(cli.path), name, no_git, no_index }, &cfg)?;
            }
        }
        Command::Root { json } => {
            let c = container::locate(path, &Config::load_or_default()?)?;
            if json {
                let v = serde_json::json!({
                    "container": c.root, "project": c.project, "product": c.product(),
                    "work": c.work(), "secrets": c.secrets(), "jtasks_data": c.jtasks(),
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else {
                println!("container    {}\nproject      {}\nproduct      {}\nwork         {}\nsecrets      {}\njtasks_data  {}",
                    c.root.display(), c.project, c.product().display(), c.work().display(), c.secrets().display(), c.jtasks().display());
            }
        }
        Command::Check { product_only, stale_days, json } => {
            let c = container::locate(path, &Config::load_or_default()?)?;
            let findings = check::run(&c, &check::CheckOptions { product_only, stale_days });
            return check::print(&findings, json);
        }
        Command::New { kind, name } => {
            let cfg = Config::load_or_default()?;
            let c = container::locate(path, &cfg)?;
            ops::new_item(&c, kind, &name, &cfg)?;
        }
        Command::Status => map::status(&container::locate(path, &Config::load_or_default()?)?)?,
        Command::Clean { docker, dry_run, yes } => {
            ops::clean(&container::locate(path, &Config::load_or_default()?)?, docker, dry_run, yes)?
        }
        Command::Plan(PlanCmd::Reset { dry_run: _, approve, pending_reviewed }) => {
            plan::reset(&container::locate(path, &Config::load_or_default()?)?, approve.as_deref(), pending_reviewed)?;
        }
        Command::Map => map::map(&container::locate(path, &Config::load_or_default()?)?)?,
        Command::Storage(StorageCmd::Move { folder, destination, dry_run, yes }) => {
            let cfg = Config::load_or_setup()?;
            let c = container::locate(path, &cfg)?;
            ops::storage_move(&c, &folder, &destination, &cfg, dry_run, yes)?;
        }
        Command::Rules(RulesCmd::Install { target, dry_run, yes }) => ops::rules_install(target, dry_run, yes)?,
        Command::Rules(RulesCmd::Show) => print!("{}", templates::GLOBAL_RULES),
        Command::Hook(HookCmd::Install { force }) => {
            ops::hook_install(&container::locate(path, &Config::load_or_default()?)?, force)?
        }
        Command::Tools(cmd) => {
            let mut cfg = Config::load_or_default()?;
            match cmd {
                ToolsCmd::Status => tools::status(&cfg)?,
                ToolsCmd::Install { tool, dry_run, yes } => tools::install(tool, &mut cfg, dry_run, yes)?,
                ToolsCmd::Update { tool, dry_run, yes } => tools::update(tool, &cfg, dry_run, yes)?,
                ToolsCmd::Uninstall { tool, dry_run, yes } => tools::uninstall(tool, &mut cfg, dry_run, yes)?,
            }
        }
    }
    Ok(0)
}
