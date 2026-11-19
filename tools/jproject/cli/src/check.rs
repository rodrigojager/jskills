//! `jproject check`: the framework lint (§10.5).

use crate::container::{Container, WORK_FOLDERS};
use crate::{map, util};
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub rule: &'static str,
    pub path: String,
    pub message: String,
    pub fix: String,
}

pub struct CheckOptions {
    pub product_only: bool,
    pub stale_days: u64,
}

/// Folders never walked: VCS data, dependencies, build output, indexes.
const SKIP_DIRS: [&str; 4] = [".git", "node_modules", "target", ".zvec-grep"];

struct Ctx<'a> {
    c: &'a Container,
    out: Vec<Finding>,
}

impl Ctx<'_> {
    fn push(&mut self, severity: Severity, rule: &'static str, path: &Path, message: impl Into<String>, fix: impl Into<String>) {
        self.out.push(Finding {
            severity,
            rule,
            path: util::rel(&self.c.root, path),
            message: message.into(),
            fix: fix.into(),
        });
    }
}

fn versioned_name_re() -> Regex {
    // A suffix on the file stem (`report_v2.md`, `mock-final.png`, `plan (1).md`, `x - Copy.txt`)
    // or a dated/tagged backup (`opencode.json.bak-20261023`).
    Regex::new(r"(?i)(?:[_-](?:v\d+|final|old|copy|backup|new|novo)$|\s-\s?copy(?:\s\(\d+\))?$|\s\(\d+\)$)").unwrap()
}

fn dated_bak_re() -> Regex {
    Regex::new(r"(?i)\.bak[-_.]?.+$").unwrap()
}

/// The text of the "Allowed Markdown files" section of `product/WORKSPACE.md`.
fn allowed_md_section(ws: &str) -> String {
    let Some(start) = ws.find("## Allowed Markdown files") else { return String::new() };
    let body = &ws[start + 2..];
    let end = body.find("\n## ").map(|i| i + 1).unwrap_or(body.len());
    body[..end].to_string()
}

/// A Markdown path is allowed when the section names it, or names one of its folders (`templates/`).
fn listed_md(section: &str, rel: &str) -> bool {
    if section.contains(&format!("`{rel}`")) {
        return true;
    }
    let mut prefix = String::new();
    for part in rel.split('/').take(rel.split('/').count() - 1) {
        prefix.push_str(part);
        prefix.push('/');
        if section.contains(&format!("`{prefix}`")) {
            return true;
        }
    }
    false
}

pub fn run(c: &Container, opts: &CheckOptions) -> Vec<Finding> {
    let mut ctx = Ctx { c, out: Vec::new() };
    let product = c.product();
    let scan_root = if opts.product_only { product.clone() } else { c.root.clone() };
    let product_ws = std::fs::read_to_string(product.join("WORKSPACE.md")).unwrap_or_default();
    let allowed_md = allowed_md_section(&product_ws);
    let versioned = versioned_name_re();
    let dated_bak = dated_bak_re();
    let mut baks: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();

    let secrets = c.secrets();
    let ignored = git_ignored_dirs(&product);
    let mut walker = walkdir::WalkDir::new(&scan_root).follow_links(false).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !(e.depth() > 0 && (SKIP_DIRS.contains(&name.as_ref()) || e.path() == secrets))
    });
    while let Some(entry) = walker.next() {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let in_product = path.starts_with(&product);

        // Links: broken junctions/symlinks.
        if util::is_link(path) {
            if std::fs::metadata(path).is_err() {
                ctx.push(Severity::Error, "broken-link", path, "junction/symlink points to a missing folder",
                    "recreate the target folder, or remove the link and update the Storage section (`jproject map`)");
            }
            continue;
        }

        if entry.file_type().is_dir() {
            if in_product && name == ".scratch" {
                ctx.push(Severity::Error, "scratch-in-product", path, "`.scratch/` inside the product repository",
                    format!("move tool data to `{}` (e.g. jtasks runs) and delete this folder", c.jtasks_data));
            }
            if name == "_variants" && !opts.product_only {
                for v in std::fs::read_dir(path).into_iter().flatten().flatten() {
                    ctx.push(Severity::Warning, "pending-variants", &v.path(), "variants awaiting consolidation",
                        "ask the user to choose one; then replace the canonical file and delete this folder");
                }
            }
            // A folder the product repository ignores (build output, caches) is not product content:
            // the folder-level rules above still apply to it, but its files are not checked.
            if in_product && ignored.contains(path) {
                walker.skip_current_dir();
            }
            continue;
        }

        // Files.
        let stem = Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if dated_bak.is_match(&name) {
            ctx.push(Severity::Error, "versioned-name", path, "dated or tagged backup file",
                "keep at most one `<file>.bak` without a date, and delete it once the change is confirmed");
        } else if versioned.is_match(&stem) {
            ctx.push(Severity::Error, "versioned-name", path, "versioned copy of a file (`_v2`, `_final`, `_old`, `_copy`...)",
                "keep one canonical file edited in place; alternatives go in `_variants/<artifact>/`");
        }
        if let Some(i) = name.to_ascii_lowercase().find(".bak") {
            // `x.json.bak`, `x.json.1.bak` and `x.json.bak2` are backups of the same `x.json`.
            let stem = Regex::new(r"[._-]?\d+$").unwrap().replace(&name[..i], "").into_owned();
            let base = path.parent().unwrap().join(stem);
            baks.entry(base).or_default().push(path.to_path_buf());
        }

        if in_product && name.to_ascii_lowercase().ends_with(".md") {
            let r = util::rel(&product, path);
            let depth = r.matches('/').count();
            let allowed = name == "README.md"
                || (name == "AGENTS.md" && depth <= 2)
                || (name == "WORKSPACE.md" && depth == 0)
                || listed_md(&allowed_md, &r);
            if !allowed {
                ctx.push(Severity::Error, "md-in-product", path, "Markdown file not allowed in the product",
                    format!("move it to `{}/10-docs/` (or another work folder); if the user placed it on purpose, list `{r}` in `{}/WORKSPACE.md`", c.work_dir, c.product_dir));
            }
        }
    }
    for (base, files) in baks {
        if files.len() > 1 {
            ctx.push(Severity::Error, "multiple-backups", &base, format!("{} backups of the same file", files.len()),
                "keep at most one `<file>.bak` and delete the others");
        }
    }

    structure(&mut ctx, opts);
    ctx.out
}

/// Folders inside the product that its git repository ignores (`.gitignore` files at any level,
/// `.git/info/exclude`, the global excludes file). Only folders whose own path matches an ignore
/// rule count, so a folder that merely holds individually ignored files (e.g. `*.bak-*`) is still
/// walked and its files still checked. Empty when the product is not a git repository or git is
/// missing.
fn git_ignored_dirs(product: &Path) -> HashSet<PathBuf> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let Some(git) = util::which("git") else { return HashSet::new() };
    let Ok(out) = Command::new(&git)
        .arg("-C").arg(product)
        .args(["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"])
        .stderr(Stdio::null())
        .output()
    else {
        return HashSet::new();
    };
    if !out.status.success() {
        return HashSet::new();
    }
    // `ls-files --directory` also lists folders that only contain ignored files; `check-ignore`
    // keeps those whose own path is ignored.
    let candidates: Vec<u8> = out.stdout
        .split(|b| *b == 0)
        .filter(|p| p.ends_with(b"/"))
        .flat_map(|p| p.iter().copied().chain(std::iter::once(0)))
        .collect();
    if candidates.is_empty() {
        return HashSet::new();
    }
    let Ok(mut child) = Command::new(&git)
        .arg("-C").arg(product)
        .args(["check-ignore", "-z", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return HashSet::new();
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&candidates);
    });
    let Ok(out) = child.wait_with_output() else { return HashSet::new() };
    let _ = writer.join();
    // Exit code 1 means "nothing ignored"; anything above is an error.
    if out.status.code().map_or(true, |c| c > 1) {
        return HashSet::new();
    }
    out.stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| {
            let rel = String::from_utf8_lossy(p);
            product.join(rel.trim_end_matches('/'))
        })
        .collect()
}

fn structure(ctx: &mut Ctx, opts: &CheckOptions) {
    let c = ctx.c;
    let product = c.product();
    if !product.join("AGENTS.md").exists() {
        ctx.push(Severity::Error, "missing-agents-md", &product, "the product has no AGENTS.md", "run `jproject init` (it only creates what is missing)");
    }
    if opts.product_only {
        return;
    }
    if !c.root.join("AGENTS.md").exists() {
        ctx.push(Severity::Error, "missing-agents-md", &c.root, "the container has no AGENTS.md", "run `jproject init` (it only creates what is missing)");
    }
    for f in WORK_FOLDERS {
        let dir = c.work_folder(f);
        if !dir.join("AGENTS.md").exists() {
            ctx.push(Severity::Error, "missing-agents-md", &dir, format!("{f} has no AGENTS.md"), "run `jproject init` (it only creates what is missing)");
        }
    }

    // Stray entries at the root of the work folder.
    for e in std::fs::read_dir(c.work()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let is_dir = e.path().is_dir();
        let ok = (name == "WORKSPACE.md" && !is_dir) || (is_dir && WORK_FOLDERS.contains(&name.as_str()));
        if !ok {
            ctx.push(Severity::Error, "stray-in-work-root", &e.path(), "not part of the work folder layout",
                format!("move it to the right folder (see the routing table in `{}/WORKSPACE.md`)", c.work_dir));
        }
    }

    // Intakes and proposals.
    let stale = Duration::from_secs(opts.stale_days * 86_400);
    for e in std::fs::read_dir(c.work_folder("60-intake")).into_iter().flatten().flatten() {
        if e.path().is_dir() && !e.path().join("INTAKE.md").exists() {
            ctx.push(Severity::Error, "intake-without-md", &e.path(), "intake without INTAKE.md",
                "write INTAKE.md (`jproject new intake <name>` creates the template)");
        }
    }
    for e in std::fs::read_dir(c.work_folder("70-proposals")).into_iter().flatten().flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let md = dir.join("PROPOSAL.md");
        if !md.exists() {
            ctx.push(Severity::Error, "proposal-without-md", &dir, "proposal without PROPOSAL.md",
                "write PROPOSAL.md (`jproject new proposal <name>` creates the template)");
            continue;
        }
        let newest = walkdir::WalkDir::new(&dir).into_iter().filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
            .filter_map(|e| e.metadata().ok()?.modified().ok())
            .max();
        if let Some(t) = newest {
            if SystemTime::now().duration_since(t).unwrap_or_default() > stale {
                ctx.push(Severity::Warning, "stale-proposal", &dir, format!("no change in more than {} days", opts.stale_days),
                    "ask the user to integrate or reject it (rejected proposals are deleted without a trace)");
            }
        }
    }

    // Product summary in WORKSPACE.md.
    if let Ok(text) = std::fs::read_to_string(c.workspace_md()) {
        let mut listed = map::summary_folders(&text).unwrap_or_default();
        let mut actual = map::product_folders(c);
        listed.sort();
        actual.sort();
        if listed != actual {
            ctx.push(Severity::Warning, "outdated-summary", &c.workspace_md(), "the product summary does not match the product folders",
                "run `jproject map`");
        }
    }
}

pub fn print(findings: &[Finding], json: bool) -> Result<i32> {
    let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
    let warnings = findings.len() - errors;
    if json {
        println!("{}", serde_json::to_string_pretty(findings)?);
    } else {
        for f in findings {
            let tag = match f.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            println!("{tag}[{}] {}: {}\n    fix: {}", f.rule, f.path, f.message, f.fix);
        }
        if findings.is_empty() {
            println!("No violations.");
        } else {
            println!("\n{errors} error(s), {warnings} warning(s).");
        }
    }
    Ok(if errors > 0 { 1 } else { 0 })
}
