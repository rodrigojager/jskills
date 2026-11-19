//! `new`, `clean`, `storage move`, `rules install`, `hook install`.

use crate::config::Config;
use crate::container::Container;
use crate::{map, templates, util};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- new

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum ItemKind {
    Intake,
    Proposal,
    Handoff,
}

pub fn new_item(c: &Container, kind: ItemKind, name: &str, cfg: &Config) -> Result<()> {
    util::validate_name(name)?;
    let vars = BTreeMap::from([("name", name.to_string()), ("date", util::today()), ("project", c.project.clone())]);
    let (path, tpl) = match kind {
        ItemKind::Intake => (c.work_folder("60-intake").join(name).join("INTAKE.md"), "items/INTAKE.md"),
        ItemKind::Proposal => (c.work_folder("70-proposals").join(name).join("PROPOSAL.md"), "items/PROPOSAL.md"),
        ItemKind::Handoff => (c.work_folder("40-handoff").join(format!("{name}.md")), "items/HANDOFF.md"),
    };
    if !util::write_new(&path, &util::render(&templates::get(tpl, cfg), &vars))? {
        bail!("{} already exists; edit it in place.", path.display());
    }
    map::regenerate(c)?;
    println!("Created {}", path.display());
    println!("Updated Active items in {}", c.workspace_md().display());
    Ok(())
}

// ---------------------------------------------------------------- clean

pub fn clean(c: &Container, docker: bool, dry_run: bool, yes: bool) -> Result<()> {
    let tmp = c.work_folder("00-tmp");
    let mut removed = 0usize;
    for e in std::fs::read_dir(&tmp).into_iter().flatten().flatten() {
        let p = e.path();
        if e.file_name() == "AGENTS.md" {
            continue;
        }
        if dry_run {
            println!("would delete {}", util::rel(&c.root, &p));
        } else if util::is_link(&p) {
            util::remove_dir_link(&p).or_else(|_| std::fs::remove_file(&p))?;
        } else if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
        removed += 1;
    }
    println!("{} {removed} item(s) from {}/00-tmp.", if dry_run { "Would remove" } else { "Removed" }, c.work_dir);

    let pending: Vec<map::Item> = map::active_items(c).into_iter().filter(|i| i.kind == "variants" || i.kind == "proposal").collect();
    if !pending.is_empty() {
        println!("\nPending (not deleted; only the user decides):");
        for i in pending {
            println!("  {:<9} {:<24} {:<24} {}", i.kind, i.name, i.status, util::rel(&c.root, &i.path));
        }
    }

    if docker {
        let args = ["image", "prune", "-f", "--filter", &format!("label=jproject.project={}", c.project)];
        let shown = format!("docker {}", args.join(" "));
        if dry_run {
            println!("\nwould run: {shown}");
        } else if util::which("docker").is_none() {
            println!("\ndocker was not found on PATH; nothing pruned.");
        } else if util::confirm(&format!("Run `{shown}`?"), yes)? {
            util::run("docker", &args, None)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- storage

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    for e in walkdir::WalkDir::new(from).follow_links(false) {
        let e = e?;
        let dest = to.join(e.path().strip_prefix(from)?);
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&dest)?;
        } else if e.file_type().is_symlink() {
            bail!("{} contains a link ({}); move it by hand.", from.display(), e.path().display());
        } else {
            std::fs::copy(e.path(), &dest)?;
        }
    }
    Ok(())
}

pub fn storage_move(c: &Container, folder: &str, destination: &Path, cfg: &Config, dry_run: bool, yes: bool) -> Result<()> {
    let source = std::path::absolute(c.root.join(folder))?;
    if !source.starts_with(&c.root) || source == c.root {
        bail!("`{folder}` must be a folder inside the container {}.", c.root.display());
    }
    if util::is_link(&source) {
        bail!("`{folder}` is already a link to {}.", util::link_target(&source).map(|p| p.display().to_string()).unwrap_or_default());
    }
    if !source.is_dir() {
        bail!("`{folder}` is not a folder of the container.");
    }
    if source == c.product() || c.product().starts_with(&source) {
        bail!("Moving the product repository is not supported; move work folders such as `{}/25-source-assets`.", c.work_dir);
    }
    let dest = std::path::absolute(destination)?;
    if dest.starts_with(&c.root) {
        bail!("The destination must be outside the container.");
    }
    if dest.exists() && std::fs::read_dir(&dest)?.next().is_some() {
        bail!("{} exists and is not empty; pass a new or empty folder.", dest.display());
    }
    let rel = util::rel(&c.root, &source);
    println!("Move {rel} -> {}\nand leave a link in its place; update WORKSPACE.md and {}.", dest.display(), cfg.storage_map_path().display());
    if dry_run {
        return Ok(());
    }
    if !util::confirm("Proceed?", yes)? {
        println!("Nothing changed.");
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if dest.exists() {
        std::fs::remove_dir(&dest)?;
    }
    if std::fs::rename(&source, &dest).is_err() {
        copy_dir(&source, &dest).with_context(|| format!("cannot copy to {}", dest.display()))?;
        std::fs::remove_dir_all(&source)?;
    }
    util::create_dir_link(&source, &dest)?;
    map::regenerate(c)?;
    update_storage_map(&cfg.storage_map_path(), c, &rel, &dest)?;
    println!("Moved. {rel} is now a link to {}.", dest.display());
    Ok(())
}

const STORAGE_HEADER: &str = "# Storage map\n\nGlobal map of project folders that live on another disk (maintained by `jproject storage move`).\n\n## Disk roots\n| Disk | Root | Notes |\n|---|---|---|\n\n## Links\n| Project | Container | Folder | Physical location |\n|---|---|---|---|\n";

fn update_storage_map(path: &Path, c: &Container, folder: &str, dest: &Path) -> Result<()> {
    let text = std::fs::read_to_string(path).unwrap_or_else(|_| STORAGE_HEADER.to_string());
    let container = c.root.display().to_string();
    let row = format!("| {} | {} | `{}` | {} |", c.project, container, folder, dest.display());
    let key = format!("| {} | {} | `{}` |", c.project, container, folder);
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with(&key)).map(String::from).collect();
    lines.push(row);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, lines.join("\n") + "\n")?;
    Ok(())
}

// ---------------------------------------------------------------- rules

pub const RULES_START: &str = "<!-- jproject:start -->";
pub const RULES_END: &str = "<!-- jproject:end -->";

/// The global instruction files of Claude Code, Codex and OpenCode.
pub fn default_rule_targets() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    vec![
        home.join(".claude").join("CLAUDE.md"),
        home.join(".codex").join("AGENTS.md"),
        home.join(".config").join("opencode").join("AGENTS.md"),
    ]
}

pub fn with_rules(existing: &str) -> String {
    let block = format!("{RULES_START}\n{}\n{RULES_END}", templates::GLOBAL_RULES.trim_end());
    if let (Some(a), Some(b)) = (existing.find(RULES_START), existing.find(RULES_END)) {
        if a < b {
            return format!("{}{}{}", &existing[..a], block, &existing[b + RULES_END.len()..]);
        }
    }
    if existing.trim().is_empty() {
        format!("{block}\n")
    } else {
        format!("{}\n\n{block}\n", existing.trim_end())
    }
}

fn section(text: &str) -> &str {
    match (text.find(RULES_START), text.find(RULES_END)) {
        (Some(a), Some(b)) if a < b => &text[a..b + RULES_END.len()],
        _ => "",
    }
}

pub fn rules_install(targets: Vec<PathBuf>, dry_run: bool, yes: bool) -> Result<()> {
    let targets = if targets.is_empty() { default_rule_targets() } else { targets };
    for path in targets {
        let old = std::fs::read_to_string(&path).unwrap_or_default();
        let new = with_rules(&old);
        if new == old {
            println!("{}: up to date.", path.display());
            continue;
        }
        println!("\n{}{}:", path.display(), if path.exists() { "" } else { " (new file)" });
        for l in section(&old).lines() {
            println!("- {l}");
        }
        for l in section(&new).lines() {
            println!("+ {l}");
        }
        if dry_run {
            continue;
        }
        if util::confirm(&format!("Write the framework section to {}?", path.display()), yes)? {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, new)?;
            println!("Updated {}", path.display());
        } else {
            println!("Skipped {}", path.display());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- hook

pub fn hook_install(c: &Container, force: bool) -> Result<()> {
    let git = c.product().join(".git");
    if !git.is_dir() {
        bail!("{} is not a git repository; run `git init` there first (or `jproject init` without --no-git).", c.product().display());
    }
    let hook = git.join("hooks").join("pre-commit");
    let body = "#!/bin/sh\n# Installed by `jproject hook install`: blocks commits that break the jproject rules.\nexec jproject check --product-only\n";
    if let Ok(existing) = std::fs::read_to_string(&hook) {
        if existing.contains("jproject check") {
            println!("{} already runs `jproject check`.", hook.display());
            return Ok(());
        }
        if !force {
            bail!("{} already exists. Add `jproject check --product-only` to it by hand, or pass --force to replace it.", hook.display());
        }
    }
    std::fs::create_dir_all(hook.parent().unwrap())?;
    std::fs::write(&hook, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))?;
    }
    println!("Installed {}", hook.display());
    Ok(())
}
