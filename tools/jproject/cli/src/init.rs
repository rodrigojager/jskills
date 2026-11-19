//! `jproject init`: creates a container (§4.1, §10.4) or a parallel folder (§4.2).

use crate::config::{self, Config};
use crate::container::{self, Container, WORK_FOLDERS};
use crate::{map, templates, util};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct InitOptions {
    pub path: Option<PathBuf>,
    pub name: Option<String>,
    pub no_git: bool,
    pub no_index: bool,
}

/// What `init` did, for the report.
#[derive(Default)]
struct Report {
    root: PathBuf,
    created: Vec<String>,
    notes: Vec<String>,
}

impl Report {
    fn add(&mut self, path: &Path, created: bool) {
        if created {
            self.created.push(util::rel(&self.root, path));
        }
    }
}

/// The container folder: the given path; without one, `<projects_root>/<name>` when both
/// are known; otherwise the current folder.
fn target_dir(path: Option<PathBuf>, name: Option<&str>, cfg: &Config) -> Result<PathBuf> {
    let p = match (path, name) {
        (Some(p), _) => p,
        (None, Some(n)) if !cfg.projects_root.trim().is_empty() => PathBuf::from(&cfg.projects_root).join(n),
        (None, _) => std::env::current_dir()?,
    };
    Ok(std::path::absolute(p)?)
}

pub fn init(opts: InitOptions, cfg: &Config) -> Result<()> {
    let root = target_dir(opts.path, opts.name.as_deref(), cfg)?;
    if root.is_file() {
        bail!("{} is a file; pass a folder to `jproject init`.", root.display());
    }
    std::fs::create_dir_all(&root)?;

    // Values already recorded in an existing container win over the global config.
    let existing = container::container_at(&root, cfg);
    let mut c = existing.clone().unwrap_or_else(|| {
        let empty = BTreeMap::new();
        Container::from_front_matter(&root, &empty, cfg)
    });
    if let Some(name) = &opts.name {
        if existing.is_none() {
            c.project = name.clone();
        }
    }
    if c.product_dir == c.work_dir || c.product_dir == c.secrets_dir || c.work_dir == c.secrets_dir {
        bail!("product_dir, work_dir and secrets_dir must be different; fix them with `jproject setup`.");
    }

    let vars = vars_for(&c);
    let mut r = Report { root: root.clone(), ..Default::default() };

    // Container root.
    let t = |rel: &str| util::render(&templates::get(rel, cfg), &vars);
    r.add(&root.join("AGENTS.md"), util::write_new(&root.join("AGENTS.md"), &t("container/AGENTS.md"))?);
    r.add(&root.join("CLAUDE.md"), util::write_new(&root.join("CLAUDE.md"), &t("container/CLAUDE.md"))?);
    let skills = root.join(".agents").join("skills");
    r.add(&skills, util::mkdir_new(&skills)?);
    let claude_dir = root.join(".claude");
    r.add(&claude_dir.join("settings.json"), util::write_new(&claude_dir.join("settings.json"), &t("container/claude-settings.json"))?);
    let claude_skills = claude_dir.join("skills");
    if !claude_skills.exists() && !util::is_link(&claude_skills) {
        util::create_dir_link(&claude_skills, &skills)?;
        r.created.push(".claude/skills -> .agents/skills (link)".into());
    }

    // Product.
    let product = c.product();
    r.add(&product, util::mkdir_new(&product)?);
    if !opts.no_git && !product.join(".git").exists() {
        match util::run("git", &["init", "--quiet"], Some(&product)) {
            Ok(()) => r.created.push(format!("{}/.git (git init)", c.product_dir)),
            Err(e) => r.notes.push(format!("git init skipped: {e:#}")),
        }
    }
    for (file, tpl) in [
        ("AGENTS.md", "product/AGENTS.md"),
        ("WORKSPACE.md", "product/WORKSPACE.md"),
        ("README.md", "product/README.md"),
        (".gitignore", "product/gitignore"),
        (".env.example", "product/env.example"),
    ] {
        let p = product.join(file);
        r.add(&p, util::write_new(&p, &t(tpl))?);
    }

    // Secrets.
    let env = c.secrets().join(".env");
    r.add(&env, util::write_new(&env, "")?);

    // Work.
    let workspace_md = c.workspace_md();
    let new_workspace = util::write_new(&workspace_md, &t("work/WORKSPACE.md"))?;
    r.add(&workspace_md, new_workspace);
    for folder in WORK_FOLDERS {
        let dir = c.work_folder(folder);
        r.add(&dir, util::mkdir_new(&dir)?);
        for (rest, _) in templates::under(&format!("work/{folder}/")) {
            let p = dir.join(rest);
            r.add(&p, util::write_new(&p, &t(&format!("work/{folder}/{rest}")))?);
        }
    }
    for sub in ["10-docs/adr", "10-docs/_render", "20-design/mocks-img", "20-design/mock-html", "20-design/previews"] {
        let p = c.work().join(sub);
        r.add(&p, util::mkdir_new(&p)?);
    }
    if cfg.use_jtasks {
        let p = c.jtasks();
        r.add(&p, util::mkdir_new(&p)?);
    }

    if new_workspace {
        map::regenerate(&c)?;
    }

    // Optional companions.
    if !opts.no_index && cfg.default_index {
        index(&c, cfg, &mut r);
    }
    if cfg.use_jtasks && !config::has_jtasks() {
        r.notes.push("jtasks is enabled but not installed: `jproject tools install jtasks`.".into());
    }

    if r.created.is_empty() {
        println!("Container {} is complete; nothing was created.", root.display());
    } else {
        println!("Container {} ({}): created", root.display(), c.project);
        for p in &r.created {
            println!("  + {p}");
        }
    }
    for n in &r.notes {
        println!("note: {n}");
    }
    println!("Open your agent in {} (not in {}/).", root.display(), c.product_dir);
    Ok(())
}

pub fn vars_for(c: &Container) -> BTreeMap<&'static str, String> {
    BTreeMap::from([
        ("project", c.project.clone()),
        ("product_dir", c.product_dir.clone()),
        ("work_dir", c.work_dir.clone()),
        ("secrets_dir", c.secrets_dir.clone()),
        ("jtasks_data", c.jtasks_data.clone()),
        ("date", util::today()),
    ])
}

/// The zvec-grep exclusions (§6.12), as `-g` globs relative to the container.
pub fn index_globs(c: &Container) -> Vec<String> {
    let mut g = vec![
        format!("!{}/**", c.secrets_dir),
        format!("!{}/00-tmp/**", c.work_dir),
        format!("!{}/**", c.jtasks_data),
        "!**/node_modules/**".into(),
        "!**/.git/**".into(),
        "!**/target/**".into(),
        "!**/dist/**".into(),
        "!**/build/**".into(),
        "!**/obj/**".into(),
        "!**/bin/**".into(),
        "!**/.env".into(),
        "!**/.env.*".into(),
        "!.zvec-grep/**".into(),
    ];
    g.dedup();
    g
}

fn index(c: &Container, cfg: &Config, r: &mut Report) {
    if c.root.join(".zvec-grep").exists() {
        return;
    }
    if !config::has_zvec_grep() {
        r.notes.push("zvec-grep is not installed; no search index created. Install it with `jproject tools install zvec-grep`, then run `jproject init` again.".into());
        return;
    }
    let root = c.root.to_string_lossy().into_owned();
    let mut args: Vec<String> = vec!["index".into(), root, "--hidden".into()];
    for g in index_globs(c) {
        args.push("-g".into());
        args.push(g);
    }
    if !cfg.zvec_embedding.is_empty() {
        args.push("--embedding".into());
        args.push(cfg.zvec_embedding.clone());
    }
    println!("Creating the zvec-grep index (zg {})...", args.join(" "));
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match util::run("zg", &refs, Some(&c.root)) {
        Ok(()) => r.created.push(".zvec-grep (search index)".into()),
        Err(e) => r.notes.push(format!("zvec-grep index not created: {e:#}")),
    }
}

/// `jproject init --parallel [path] --origin <container>`.
pub fn init_parallel(path: Option<PathBuf>, origin: &Path, cfg: &Config) -> Result<()> {
    let origin_c = container::discover(origin, cfg)
        .with_context(|| format!("{} is not inside a jproject container; pass the origin container with --origin.", origin.display()))?;
    let root = std::path::absolute(match path {
        Some(p) => p,
        None => std::env::current_dir()?,
    })?;
    if root.starts_with(&origin_c.root) {
        bail!("The parallel folder must be outside the origin container ({}).", origin_c.root.display());
    }
    std::fs::create_dir_all(&root)?;
    let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "parallel".into());
    let vars = BTreeMap::from([
        ("name", name.clone()),
        ("origin", origin_c.root.to_string_lossy().into_owned()),
        ("date", util::today()),
    ]);
    let t = |rel: &str| util::render(&templates::get(rel, cfg), &vars);
    let mut r = Report { root: root.clone(), ..Default::default() };
    for (file, tpl) in [
        ("AGENTS.md", "parallel/AGENTS.md"),
        ("CLAUDE.md", "parallel/CLAUDE.md"),
        ("HANDOFF.md", "parallel/HANDOFF.md"),
        ("_work/AGENTS.md", "parallel/work-AGENTS.md"),
    ] {
        let p = root.join(file);
        r.add(&p, util::write_new(&p, &t(tpl))?);
    }
    let out = root.join("out");
    r.add(&out, util::mkdir_new(&out)?);
    println!("Parallel folder {} (origin: {}): created", root.display(), origin_c.root.display());
    for p in &r.created {
        println!("  + {p}");
    }
    println!(
        "When done, move out/ and HANDOFF.md to {}/60-intake/{name}/ (HANDOFF.md becomes INTAKE.md).",
        origin_c.work().display()
    );
    Ok(())
}
