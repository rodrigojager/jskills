//! `jproject map` and `jproject status`: the automatic sections of `_work/WORKSPACE.md`.

use crate::container::{Container, WORK_FOLDERS};
use crate::util;
use anyhow::Result;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Item {
    pub kind: &'static str,
    pub name: String,
    pub status: String,
    pub since: String,
    pub path: PathBuf,
}

/// Value of a `- Key: value` line in a Markdown item file.
pub fn field(text: &str, key: &str) -> Option<String> {
    let prefix = format!("- {key}:");
    text.lines()
        .find_map(|l| l.trim().strip_prefix(&prefix).map(|v| v.trim().to_string()))
        .filter(|v| !v.is_empty())
}

fn mtime_date(p: &Path) -> String {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .map(util::date_of)
        .unwrap_or_default()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Every `_variants/<artifact>/` folder under the work folder.
pub fn pending_variants(c: &Container) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in walkdir::WalkDir::new(c.work()).into_iter().filter_map(|e| e.ok()) {
        if e.file_type().is_dir() && e.file_name() == "_variants" {
            out.extend(subdirs(e.path()));
        }
    }
    out
}

pub fn active_items(c: &Container) -> Vec<Item> {
    let mut items = Vec::new();
    for dir in subdirs(&c.work_folder("60-intake")) {
        let text = std::fs::read_to_string(dir.join("INTAKE.md")).unwrap_or_default();
        items.push(Item {
            kind: "intake",
            name: name_of(&dir),
            status: field(&text, "Status").unwrap_or_else(|| "missing INTAKE.md".into()),
            since: field(&text, "Arrived").unwrap_or_else(|| mtime_date(&dir)),
            path: dir,
        });
    }
    for dir in subdirs(&c.work_folder("70-proposals")) {
        let text = std::fs::read_to_string(dir.join("PROPOSAL.md")).unwrap_or_default();
        let created = field(&text, "Created").map(|v| v.split_whitespace().next().unwrap_or("").to_string());
        items.push(Item {
            kind: "proposal",
            name: name_of(&dir),
            status: field(&text, "Status").unwrap_or_else(|| "missing PROPOSAL.md".into()),
            since: created.unwrap_or_else(|| mtime_date(&dir)),
            path: dir,
        });
    }
    if let Ok(rd) = std::fs::read_dir(c.work_folder("40-handoff")) {
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "md") && name_of(p) != "AGENTS.md")
            .collect();
        files.sort();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            items.push(Item {
                kind: "handoff",
                name: f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                status: "open".into(),
                since: field(&text, "Updated").unwrap_or_else(|| mtime_date(&f)),
                path: f,
            });
        }
    }
    for v in pending_variants(c) {
        items.push(Item {
            kind: "variants",
            name: util::rel(&c.work(), &v),
            status: "awaiting consolidation".into(),
            since: mtime_date(&v),
            path: v,
        });
    }
    items
}

/// (folder, physical location, is link) for the container's top folders and work folders.
pub fn storage_rows(c: &Container) -> Vec<(String, String, bool)> {
    let mut paths = vec![
        c.product(),
        c.secrets(),
        c.work(),
        c.root.join(".agents").join("skills"),
        c.root.join(".claude").join("skills"),
    ];
    paths.extend(WORK_FOLDERS.iter().map(|f| c.work_folder(f)));
    paths
        .into_iter()
        .filter(|p| p.exists() || util::is_link(p))
        .map(|p| {
            let link = util::is_link(&p);
            let physical = if link {
                util::link_target(&p).map(|t| t.display().to_string()).unwrap_or_else(|| "(broken link)".into())
            } else {
                p.display().to_string()
            };
            (util::rel(&c.root, &p), physical, link)
        })
        .collect()
}

/// (name, description) of every project skill in `.agents/skills/`.
pub fn project_skills(c: &Container) -> Vec<(String, String)> {
    subdirs(&c.root.join(".agents").join("skills"))
        .into_iter()
        .filter_map(|d| {
            let text = std::fs::read_to_string(d.join("SKILL.md")).ok()?;
            let fm = crate::container::parse_front_matter(&text).unwrap_or_default();
            Some((
                fm.get("name").cloned().unwrap_or_else(|| name_of(&d)),
                fm.get("description").cloned().unwrap_or_default(),
            ))
        })
        .collect()
}

/// Top-level folders of the product (hidden ones excluded).
pub fn product_folders(c: &Container) -> Vec<String> {
    subdirs(&c.product())
        .iter()
        .map(|p| name_of(p))
        .filter(|n| !n.starts_with('.'))
        .collect()
}

/// Descriptions of product folders taken from `product/WORKSPACE.md` rows like ``| `src/` | ... |``.
fn product_descriptions(c: &Container) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(c.product().join("WORKSPACE.md")).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let cells: Vec<&str> = l.trim().trim_matches('|').split('|').map(|s| s.trim()).collect();
            if cells.len() < 2 {
                return None;
            }
            let name = cells[0].trim_matches('`').trim_end_matches('/');
            (!name.is_empty() && !name.contains(' ') && cells[0].starts_with('`')).then(|| (name.to_string(), cells[1].to_string()))
        })
        .collect()
}

/// Folder names listed in section 7 of `_work/WORKSPACE.md`.
pub fn summary_folders(text: &str) -> Option<Vec<String>> {
    let body = auto_section(text, "product")?;
    let re = regex::Regex::new(r"^- `([^`]+?)/?`").unwrap();
    Some(
        body.lines()
            .filter_map(|l| re.captures(l.trim()).map(|cap| cap[1].trim_end_matches('/').to_string()))
            .collect(),
    )
}

fn markers(key: &str) -> (String, String) {
    (format!("<!-- jproject:auto:{key}:start -->"), format!("<!-- jproject:auto:{key}:end -->"))
}

pub fn auto_section<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let (s, e) = markers(key);
    let start = text.find(&s)? + s.len();
    let end = text[start..].find(&e)? + start;
    Some(&text[start..end])
}

/// Replaces the body between a pair of auto markers; inserts the markers under `heading` if missing.
pub fn replace_auto(text: &str, key: &str, heading: &str, body: &str) -> String {
    let (s, e) = markers(key);
    let block = format!("{s}\n{}{e}", if body.is_empty() { String::new() } else { format!("{}\n", body.trim_end()) });
    if let (Some(a), Some(b)) = (text.find(&s), text.find(&e)) {
        if a < b {
            return format!("{}{}{}", &text[..a], block, &text[b + e.len()..]);
        }
    }
    if let Some(h) = text.find(heading) {
        let line_end = text[h..].find('\n').map(|i| h + i + 1).unwrap_or(text.len());
        return format!("{}{}\n{}", &text[..line_end], block, &text[line_end..]);
    }
    format!("{}\n\n{heading}\n{block}\n", text.trim_end())
}

fn render_sections(c: &Container) -> [(&'static str, &'static str, String); 4] {
    let mut storage = String::from("| Folder | Physical location | Junction? |\n|---|---|---|\n");
    for (folder, physical, link) in storage_rows(c) {
        storage.push_str(&format!("| `{folder}` | {physical} | {} |\n", if link { "yes" } else { "no" }));
    }
    let mut active = String::from("| Kind | Name | Status | Since |\n|---|---|---|---|\n");
    for i in active_items(c) {
        active.push_str(&format!("| {} | {} | {} | {} |\n", i.kind, i.name, i.status, i.since));
    }
    let skills = project_skills(c)
        .into_iter()
        .map(|(n, d)| format!("- {n}: {d}"))
        .collect::<Vec<_>>()
        .join("\n");
    let skills = if skills.is_empty() { "- (none)".to_string() } else { skills };
    let desc = product_descriptions(c);
    let mut product: Vec<String> = product_folders(c)
        .into_iter()
        .map(|f| {
            let d = desc.iter().find(|(n, _)| n == &f).map(|(_, d)| d.clone())
                .unwrap_or_else(|| format!("(describe it in `{}/WORKSPACE.md`)", c.product_dir));
            format!("- `{f}/`: {d}")
        })
        .collect();
    if product.is_empty() {
        product.push("- (no folders yet)".into());
    }
    product.push(format!("\nDetails: `{}/WORKSPACE.md`.", c.product_dir));
    [
        ("storage", "## 4. Storage", storage),
        ("active", "## 5. Active items", active),
        ("skills", "## 6. Project skills", skills),
        ("product", "## 7. Product summary", product.join("\n")),
    ]
}

/// Rewrites the automatic sections; hand-written text is kept.
pub fn regenerate(c: &Container) -> Result<bool> {
    let path = c.workspace_md();
    let old = std::fs::read_to_string(&path)?;
    let mut text = old.clone();
    for (key, heading, body) in render_sections(c) {
        text = replace_auto(&text, key, heading, &body);
    }
    let changed = text != old;
    if changed {
        std::fs::write(&path, text)?;
    }
    Ok(changed)
}

pub fn map(c: &Container) -> Result<()> {
    if regenerate(c)? {
        println!("Updated {}", c.workspace_md().display());
    } else {
        println!("{} is up to date.", c.workspace_md().display());
    }
    Ok(())
}

pub fn status(c: &Container) -> Result<()> {
    println!("Container: {} ({})", c.root.display(), c.project);
    let items = active_items(c);
    println!("\nActive items:");
    if items.is_empty() {
        println!("  (none)");
    }
    for i in &items {
        println!("  {:<9} {:<32} {:<24} since {}", i.kind, i.name, i.status, i.since);
    }
    println!("\nWork folders:");
    for f in WORK_FOLDERS {
        let p = c.work_folder(f);
        if p.exists() {
            println!("  {:<18} {:>10}", f, util::human_size(util::dir_size(&p)));
        } else {
            println!("  {:<18} {:>10}", f, "missing");
        }
    }
    println!("\nStorage:");
    for (folder, physical, link) in storage_rows(c) {
        println!("  {:<24} {}{}", folder, physical, if link { "  (link)" } else { "" });
    }
    println!("\nTools: zvec-grep {}, jtasks {}  (`jproject tools`)",
        if crate::config::has_zvec_grep() { "installed" } else { "not installed" },
        if crate::config::has_jtasks() { "installed" } else { "not installed" });
    Ok(())
}
