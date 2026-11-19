//! `jproject tools`: status, install, update and uninstall of the optional companions
//! zvec-grep (semantic search) and jtasks (task runs for agents).

use crate::config::{self, Config};
use crate::util;
use anyhow::{bail, Result};
use std::path::PathBuf;

const ZVEC_PACKAGE: &str = "@zvec/zvec-grep";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Tool {
    #[value(name = "zvec-grep")]
    ZvecGrep,
    Jtasks,
}

impl Tool {
    pub fn from_name(name: &str) -> Result<Tool> {
        match name {
            "zvec-grep" => Ok(Tool::ZvecGrep),
            "jtasks" => Ok(Tool::Jtasks),
            _ => bail!("Unknown tool `{name}`; use `zvec-grep` or `jtasks`."),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Tool::ZvecGrep => "zvec-grep",
            Tool::Jtasks => "jtasks",
        }
    }
}

/// One step of an install/update/uninstall plan.
struct Step {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
}

impl Step {
    fn new(program: &str, args: &[&str]) -> Self {
        Step { program: program.into(), args: args.iter().map(|s| s.to_string()).collect(), cwd: None }
    }
    fn show(&self) -> String {
        let cmd = format!("{} {}", self.program, self.args.join(" "));
        match &self.cwd {
            Some(d) => format!("(in {}) {cmd}", d.display()),
            None => cmd,
        }
    }
}

fn execute(steps: &[Step], notes: &[String], what: &str, dry_run: bool, yes: bool) -> Result<bool> {
    println!("{what}:");
    for s in steps {
        println!("  $ {}", s.show());
    }
    for n in notes {
        println!("  note: {n}");
    }
    if steps.is_empty() || dry_run {
        return Ok(false);
    }
    if !util::confirm("Run these commands?", yes)? {
        println!("Nothing changed.");
        return Ok(false);
    }
    for s in steps {
        let args: Vec<&str> = s.args.iter().map(|a| a.as_str()).collect();
        util::run(&s.program, &args, s.cwd.as_deref())?;
    }
    Ok(true)
}

pub fn status(cfg: &Config) -> Result<()> {
    let zg = util::output("zg", &["version"]);
    println!("zvec-grep  {}", match &zg {
        Some(v) => format!("installed ({v})"),
        None => "not installed  -> jproject tools install zvec-grep".into(),
    });
    println!("           used by `jproject init`: {}", if cfg.default_index { "yes (default_index = true)" } else { "no" });
    let version = util::output("jtasks", &["--version"]);
    println!("jtasks     {}", version.as_deref().unwrap_or("not installed"));
    println!("           used in new containers: {}", if cfg.use_jtasks { "yes (use_jtasks = true)" } else { "no" });
    Ok(())
}

fn require(program: &str, why: &str) -> Result<()> {
    if util::which(program).is_none() {
        bail!("`{program}` is required to {why} but was not found on PATH.");
    }
    Ok(())
}

pub fn install(tool: Tool, cfg: &mut Config, dry_run: bool, yes: bool) -> Result<()> {
    match tool {
        Tool::ZvecGrep => {
            let mut steps = Vec::new();
            if !config::has_zvec_grep() {
                require("npm", "install zvec-grep")?;
                steps.push(Step::new("npm", &["install", "-g", ZVEC_PACKAGE]));
            }
            steps.push(Step::new("zg", &["install", "--target", "auto", "--yes"]));
            steps.push(Step::new("zg", &["config", "model", "set", &cfg.zvec_embedding, "--default"]));
            let notes = vec!["zg install connects zvec-grep to the agents it detects (MCP + guidance). Restart the agents afterwards.".to_string()];
            if execute(&steps, &notes, "Install zvec-grep", dry_run, yes)? {
                cfg.default_index = true;
                cfg.save()?;
                println!("zvec-grep installed. New containers get an index on `jproject init`.");
            }
        }
        Tool::Jtasks => {
            let help = util::output("jtasks", &["--help"]).unwrap_or_default();
            if !help.contains("Independent foreground task controller") {
                bail!("Install the native jtasks v2 distribution and put it on PATH first; no trusted release source is configured.");
            }
            println!("Native jtasks v2 is available on PATH. Enable it for new containers.");
            if !dry_run && util::confirm("Enable jtasks for new containers?", yes)? {
                cfg.use_jtasks = true;
                cfg.save()?;
            }
        }
    }
    Ok(())
}

pub fn update(tool: Tool, _cfg: &Config, dry_run: bool, yes: bool) -> Result<()> {
    let steps = match tool {
        Tool::ZvecGrep => {
            require("npm", "update zvec-grep")?;
            vec![Step::new("npm", &["install", "-g", &format!("{ZVEC_PACKAGE}@latest")])]
        }
        Tool::Jtasks => {
            require("jtasks", "update jtasks v2")?;
            if !util::output("jtasks", &["--help"]).unwrap_or_default().contains("Independent foreground task controller") {
                bail!("The jtasks on PATH is not the native v2 controller.");
            }
            vec![Step::new("jtasks", &["update"])]
        }
    };
    execute(&steps, &[], &format!("Update {}", tool.name()), dry_run, yes)?;
    Ok(())
}

pub fn uninstall(tool: Tool, cfg: &mut Config, dry_run: bool, yes: bool) -> Result<()> {
    let (steps, notes) = match tool {
        Tool::ZvecGrep => {
            let mut s = Vec::new();
            if config::has_zvec_grep() {
                s.push(Step::new("zg", &["uninstall", "--target", "all", "--yes"]));
            }
            if util::which("npm").is_some() {
                s.push(Step::new("npm", &["uninstall", "-g", ZVEC_PACKAGE]));
            }
            (s, vec!["Existing `.zvec-grep/` index folders in containers are left in place; delete them by hand if you want.".to_string()])
        }
        Tool::Jtasks => {
            println!("Disable jtasks for new containers; the separately installed native CLI, plugins and run data are retained.");
            if !dry_run && util::confirm("Disable jtasks for new containers?", yes)? {
                cfg.use_jtasks = false;
                cfg.save()?;
            }
            return Ok(());
        }
    };
    if execute(&steps, &notes, &format!("Uninstall {}", tool.name()), dry_run, yes)? {
        match tool {
            Tool::ZvecGrep => cfg.default_index = false,
            Tool::Jtasks => cfg.use_jtasks = false,
        }
        cfg.save()?;
    }
    Ok(())
}
