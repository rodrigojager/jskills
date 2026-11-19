use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const WORK_FOLDERS: [&str; 10] = [
    "00-tmp", "10-docs", "20-design", "25-source-assets", "30-plan",
    "40-handoff", "50-tools", "60-intake", "70-proposals", "80-ideas",
];

struct Env {
    _tmp: TempDir,
    root: PathBuf,
    config: PathBuf,
}

impl Env {
    fn new() -> Self {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path().join("projects").join("my-game");
        let config = tmp.path().join("config");
        Env { _tmp: tmp, root, config }
    }
    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("jproject").unwrap();
        c.env("JPROJECT_CONFIG_DIR", &self.config);
        c
    }
    fn setup(&self, extra: &[&str]) {
        let mut args = vec!["setup", "--non-interactive", "--default-index", "false", "--use-jtasks", "true"];
        args.extend_from_slice(extra);
        self.cmd().args(&args).assert().success();
    }
    fn init(&self) {
        self.setup(&[]);
        self.cmd().arg("init").arg(&self.root).arg("--no-index").assert().success();
    }
    fn check(&self) -> assert_cmd::assert::Assert {
        self.cmd().arg("check").arg("-C").arg(&self.root).assert()
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn write(&self, rel: &str, text: &str) {
        let p = self.p(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }
}

fn subcommands() -> Vec<Vec<&'static str>> {
    vec![
        vec!["setup"], vec!["init"], vec!["root"], vec!["check"], vec!["new"], vec!["status"],
        vec!["clean"], vec!["map"], vec!["storage", "move"], vec!["rules", "install"], vec!["rules", "show"],
        vec!["hook", "install"], vec!["tools", "status"], vec!["tools", "install"], vec!["tools", "update"],
        vec!["tools", "uninstall"], vec!["plan", "reset"],
    ]
}

#[test]
fn every_command_has_help_with_an_example() {
    let env = Env::new();
    for sub in subcommands() {
        let mut args = sub.clone();
        args.push("--help");
        env.cmd().args(&args).assert().success().stdout(predicate::str::contains("Example"));
        let mut help = vec!["help"];
        help.extend(sub.iter());
        env.cmd().args(&help).assert().success().stdout(predicate::str::contains("Example"));
    }
}

#[test]
fn no_arguments_prints_help_and_does_nothing() {
    let env = Env::new();
    env.cmd().assert().stderr(predicate::str::contains("Commands:").and(predicate::str::contains("init")));
    assert!(!env.config.exists());
    env.cmd().arg("--help").assert().success().stdout(predicate::str::contains("Examples:"));
    env.cmd().arg("--version").assert().success().stdout(predicate::str::contains("jproject"));
}

#[test]
fn init_creates_the_full_tree() {
    let env = Env::new();
    env.init();
    let mut expected = vec![
        "AGENTS.md", "CLAUDE.md", ".agents/skills", ".claude/settings.json", ".claude/skills",
        "product/.git", "product/AGENTS.md", "product/WORKSPACE.md", "product/README.md",
        "product/.gitignore", "product/.env.example", "_secrets/.env", "_work/WORKSPACE.md",
        "_work/10-docs/GLOSSARY.md", "_work/10-docs/BACKLOG.md", "_work/10-docs/adr", "_work/10-docs/_render",
        "_work/20-design/mocks-img", "_work/20-design/mock-html", "_work/20-design/previews",
        "_work/30-plan/PLAN.md", "_work/30-plan/PROGRESS.md", "_work/30-plan/DIVERGENCES.md",
        "_work/30-plan/jtasks", "_work/80-ideas/IDEAS.md",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    for f in WORK_FOLDERS {
        expected.push(format!("_work/{f}/AGENTS.md"));
    }
    for e in &expected {
        assert!(env.p(e).exists(), "missing {e}");
    }
    // The work folder holds exactly WORKSPACE.md and the ten folders.
    let mut names: Vec<String> = fs::read_dir(env.p("_work")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    let mut want: Vec<String> = WORK_FOLDERS.iter().map(|s| s.to_string()).collect();
    want.push("WORKSPACE.md".into());
    want.sort();
    assert_eq!(names, want);
    // .claude/skills is the same physical folder as .agents/skills.
    fs::write(env.p(".agents/skills/probe.txt"), "x").unwrap();
    assert!(env.p(".claude/skills/probe.txt").exists());
    // Front matter.
    let ws = fs::read_to_string(env.p("_work/WORKSPACE.md")).unwrap();
    assert!(ws.starts_with("---\njproject: 1\nproject: my-game\nproduct_dir: product\n"));
    assert!(!ws.contains("{{"));
    // CLAUDE.md imports AGENTS.md.
    assert_eq!(fs::read_to_string(env.p("CLAUDE.md")).unwrap().trim(), "@AGENTS.md");
}

#[test]
fn every_folder_agents_md_follows_the_model() {
    let env = Env::new();
    env.init();
    for f in WORK_FOLDERS {
        let text = fs::read_to_string(env.p(&format!("_work/{f}/AGENTS.md"))).unwrap();
        assert!(text.starts_with(&format!("# {f}\n")), "{f}: title");
        let headings: Vec<&str> = text.lines().filter(|l| l.starts_with("## ")).collect();
        assert_eq!(headings, ["## Purpose", "## What goes here", "## What does NOT go here", "## Conventions", "## When to delete"], "{f}");
        assert!(text.lines().count() < 40, "{f} is too long");
    }
}

#[test]
fn init_on_an_existing_container_overwrites_nothing() {
    let env = Env::new();
    env.init();
    env.write("_work/30-plan/PLAN.md", "my plan");
    env.write("AGENTS.md", "custom");
    fs::remove_file(env.p("_work/80-ideas/IDEAS.md")).unwrap();
    env.cmd().arg("init").arg(&env.root).arg("--no-index").assert().success()
        .stdout(predicate::str::contains("_work/80-ideas/IDEAS.md"));
    assert_eq!(fs::read_to_string(env.p("_work/30-plan/PLAN.md")).unwrap(), "my plan");
    assert_eq!(fs::read_to_string(env.p("AGENTS.md")).unwrap(), "custom");
    env.cmd().arg("init").arg(&env.root).arg("--no-index").assert().success()
        .stdout(predicate::str::contains("nothing was created"));
}

#[test]
fn a_fresh_container_has_no_violations() {
    let env = Env::new();
    env.init();
    env.check().success().stdout(predicate::str::contains("No violations"));
}

#[test]
fn custom_folder_names_and_discovery_from_inside_the_product() {
    let env = Env::new();
    env.setup(&["--product-dir", "app", "--work-dir", "ai-work", "--secrets-dir", "keys", "--jtasks-data", "ai-work/30-plan/runs"]);
    env.cmd().arg("init").arg(&env.root).arg("--no-index").assert().success();
    assert!(env.p("app/AGENTS.md").exists());
    assert!(env.p("ai-work/WORKSPACE.md").exists());
    assert!(env.p("keys/.env").exists());
    fs::create_dir_all(env.p("app/src/deep")).unwrap();
    // Discovery with a default config (names come from the front matter).
    let out = Command::cargo_bin("jproject").unwrap()
        .env("JPROJECT_CONFIG_DIR", env.root.parent().unwrap().join("nowhere"))
        .current_dir(env.p("app/src/deep"))
        .args(["root", "--json"]).assert().success().get_output().stdout.clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(PathBuf::from(v["container"].as_str().unwrap()), env.root);
    assert!(v["product"].as_str().unwrap().ends_with("app"));
    assert!(v["jtasks_data"].as_str().unwrap().replace('\\', "/").ends_with("ai-work/30-plan/runs"));
    env.cmd().current_dir(env.p("app")).arg("map").assert().success();
    env.cmd().current_dir(env.p("app")).arg("check").assert().success();
}

#[test]
fn outside_a_container_the_error_suggests_init() {
    let env = Env::new();
    fs::create_dir_all(&env.root).unwrap();
    env.cmd().current_dir(&env.root).arg("check").assert().failure()
        .stderr(predicate::str::contains("Not inside a jproject container").and(predicate::str::contains("jproject init")));
}

fn assert_rule(env: &Env, rule: &str, fails: bool) {
    let a = env.check();
    let a = if fails { a.failure() } else { a.success() };
    a.stdout(predicate::str::contains(format!("[{rule}]")));
}

#[test]
fn check_md_in_product() {
    let env = Env::new();
    env.init();
    env.write("product/notes.md", "x");
    assert_rule(&env, "md-in-product", true);
    // Listed in product/WORKSPACE.md: allowed.
    let ws = fs::read_to_string(env.p("product/WORKSPACE.md")).unwrap();
    fs::write(env.p("product/WORKSPACE.md"), ws + "\n- `notes.md`: published notes\n").unwrap();
    env.check().success();
    // AGENTS.md deeper than the second level is not allowed.
    env.write("product/a/b/c/AGENTS.md", "x");
    assert_rule(&env, "md-in-product", true);
}

#[test]
fn check_versioned_names() {
    let env = Env::new();
    env.init();
    env.write("_work/10-docs/plan_v2.md", "x");
    assert_rule(&env, "versioned-name", true);
    fs::remove_file(env.p("_work/10-docs/plan_v2.md")).unwrap();
    env.write("product/config.json.bak-20261023-hud2", "x");
    assert_rule(&env, "versioned-name", true);
    fs::remove_file(env.p("product/config.json.bak-20261023-hud2")).unwrap();
    env.write("product/src/mock_final.png", "x");
    assert_rule(&env, "versioned-name", true);
}

#[test]
fn check_skips_folders_the_product_repository_ignores() {
    let env = Env::new();
    env.init();
    // `build/` is in product/.gitignore: generated files there are not product content.
    env.write("product/androidApp/build/intermediates/values-v21/values-v21.xml", "x");
    env.write("product/androidApp/build/out/settings.json.bak-1", "x");
    // A nested .gitignore and .git/info/exclude are honoured too.
    env.write("product/app/.gitignore", "generated/\n");
    env.write("product/app/generated/model_v2.rs", "x");
    env.write("product/.git/info/exclude", "cache/\n");
    env.write("product/cache/data_old.bin", "x");
    env.cmd().arg("map").arg("-C").arg(&env.root).assert().success();
    env.check().success().stdout(predicate::str::contains("No violations"));
    // The same name outside an ignored folder is still reported.
    env.write("product/src/values-v21.xml", "x");
    assert_rule(&env, "versioned-name", true);
}

#[test]
fn check_multiple_backups() {
    let env = Env::new();
    env.init();
    env.write("_work/50-tools/settings.json.bak", "x");
    env.check().success();
    env.write("_work/50-tools/settings.json.1.bak", "x");
    assert_rule(&env, "multiple-backups", true);
}

#[test]
fn check_pending_variants_is_a_warning() {
    let env = Env::new();
    env.init();
    env.write("_work/20-design/mocks-img/_variants/inventory-screen/a.png", "x");
    assert_rule(&env, "pending-variants", false);
}

#[test]
fn check_intake_without_md() {
    let env = Env::new();
    env.init();
    env.write("_work/60-intake/crafting/out.rs", "x");
    assert_rule(&env, "intake-without-md", true);
}

#[test]
fn check_proposal_without_md() {
    let env = Env::new();
    env.init();
    env.write("_work/70-proposals/dark-mode/mock.png", "x");
    assert_rule(&env, "proposal-without-md", true);
}

#[test]
fn check_stale_proposal() {
    let env = Env::new();
    env.init();
    env.cmd().args(["new", "proposal", "dark-mode", "-C"]).arg(&env.root).assert().success();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 86_400);
    let f = fs::File::options().write(true).open(env.p("_work/70-proposals/dark-mode/PROPOSAL.md")).unwrap();
    f.set_modified(old).unwrap();
    assert_rule(&env, "stale-proposal", false);
}

#[test]
fn check_stray_in_work_root() {
    let env = Env::new();
    env.init();
    env.write("_work/notes.txt", "x");
    assert_rule(&env, "stray-in-work-root", true);
}

#[test]
fn check_outdated_summary() {
    let env = Env::new();
    env.init();
    fs::create_dir_all(env.p("product/src")).unwrap();
    assert_rule(&env, "outdated-summary", false);
    env.cmd().arg("map").arg("-C").arg(&env.root).assert().success();
    env.check().success().stdout(predicate::str::contains("No violations"));
}

#[test]
fn check_scratch_in_product() {
    let env = Env::new();
    env.init();
    fs::create_dir_all(env.p("product/.scratch/jtasks")).unwrap();
    assert_rule(&env, "scratch-in-product", true);
}

#[test]
fn check_broken_link() {
    let env = Env::new();
    env.init();
    let target = env.root.parent().unwrap().join("elsewhere");
    fs::create_dir_all(&target).unwrap();
    make_link(&env.p("_work/00-tmp/link"), &target);
    env.check().success();
    fs::remove_dir_all(&target).unwrap();
    assert_rule(&env, "broken-link", true);
}

#[test]
fn check_missing_agents_md() {
    let env = Env::new();
    env.init();
    fs::remove_file(env.p("_work/50-tools/AGENTS.md")).unwrap();
    assert_rule(&env, "missing-agents-md", true);
}

#[test]
fn check_product_only_ignores_work() {
    let env = Env::new();
    env.init();
    env.write("_work/notes.txt", "x");
    env.cmd().args(["check", "--product-only", "-C"]).arg(&env.root).assert().success();
    env.write("product/plan.md", "x");
    env.cmd().args(["check", "--product-only", "-C"]).arg(&env.root).assert().failure();
}

#[cfg(windows)]
fn make_link(link: &Path, target: &Path) {
    junction::create(target, link).unwrap();
    assert!(link.exists());
}
#[cfg(not(windows))]
fn make_link(link: &Path, target: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[test]
fn new_items_and_active_items() {
    let env = Env::new();
    env.init();
    for (kind, name, file) in [
        ("intake", "crafting", "_work/60-intake/crafting/INTAKE.md"),
        ("proposal", "dark-mode", "_work/70-proposals/dark-mode/PROPOSAL.md"),
        ("handoff", "inventory", "_work/40-handoff/inventory.md"),
    ] {
        env.cmd().args(["new", kind, name, "-C"]).arg(&env.root).assert().success();
        let text = fs::read_to_string(env.p(file)).unwrap();
        assert!(text.contains(name) && !text.contains("{{"), "{file}");
    }
    let ws = fs::read_to_string(env.p("_work/WORKSPACE.md")).unwrap();
    assert!(ws.contains("| intake | crafting | pending |"));
    assert!(ws.contains("| proposal | dark-mode | in-progress |"));
    assert!(ws.contains("| handoff | inventory | open |"));
    env.cmd().args(["new", "proposal", "dark-mode", "-C"]).arg(&env.root).assert().failure();
    env.cmd().args(["new", "proposal", "../evil", "-C"]).arg(&env.root).assert().failure();
    env.check().success();
}

#[test]
fn map_keeps_hand_written_text() {
    let env = Env::new();
    env.init();
    let ws = fs::read_to_string(env.p("_work/WORKSPACE.md")).unwrap();
    fs::write(env.p("_work/WORKSPACE.md"), ws.replace("## 3. Routing", "## 3. Routing\nHAND WRITTEN NOTE")).unwrap();
    fs::create_dir_all(env.p(".agents/skills/deploy")).unwrap();
    fs::write(env.p(".agents/skills/deploy/SKILL.md"), "---\nname: deploy\ndescription: Deploys the game\n---\n").unwrap();
    env.cmd().arg("map").arg("-C").arg(&env.root).assert().success();
    let ws = fs::read_to_string(env.p("_work/WORKSPACE.md")).unwrap();
    assert!(ws.contains("HAND WRITTEN NOTE"));
    assert!(ws.contains("- deploy: Deploys the game"));
}

#[test]
fn clean_empties_tmp_only() {
    let env = Env::new();
    env.init();
    env.write("_work/00-tmp/log.txt", "x");
    env.write("_work/00-tmp/sub/dump.bin", "x");
    env.write("_work/20-design/mocks-img/_variants/hud/a.png", "x");
    env.cmd().args(["clean", "--dry-run", "-C"]).arg(&env.root).assert().success();
    assert!(env.p("_work/00-tmp/log.txt").exists());
    env.cmd().args(["clean", "-C"]).arg(&env.root).assert().success().stdout(predicate::str::contains("variants"));
    let left: Vec<_> = fs::read_dir(env.p("_work/00-tmp")).unwrap().collect();
    assert_eq!(left.len(), 1);
    assert!(env.p("_work/00-tmp/AGENTS.md").exists());
    assert!(env.p("_work/20-design/mocks-img/_variants/hud/a.png").exists());
}

#[test]
fn storage_move_leaves_a_link_and_updates_both_maps() {
    let env = Env::new();
    env.init();
    env.write("_work/25-source-assets/hero.blend", "blend");
    let dest = env.root.parent().unwrap().parent().unwrap().join("other-disk").join("hero-assets");
    env.cmd().args(["storage", "move", "_work/25-source-assets"]).arg(&dest).args(["--yes", "-C"]).arg(&env.root)
        .assert().success();
    assert_eq!(fs::read_to_string(dest.join("hero.blend")).unwrap(), "blend");
    assert_eq!(fs::read_to_string(env.p("_work/25-source-assets/hero.blend")).unwrap(), "blend");
    let ws = fs::read_to_string(env.p("_work/WORKSPACE.md")).unwrap();
    assert!(ws.contains("| `_work/25-source-assets` |") && ws.contains("| yes |"));
    let map = fs::read_to_string(env.config.join("STORAGE.md")).unwrap();
    assert!(map.contains("`_work/25-source-assets`") && map.contains("hero-assets"));
    env.check().success();
}

#[test]
fn rules_install_is_idempotent_and_keeps_other_text() {
    let env = Env::new();
    let target = env.root.parent().unwrap().join("CLAUDE.md");
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(&target, "# Mine\n\nkeep me\n").unwrap();
    for _ in 0..2 {
        env.cmd().args(["rules", "install", "--yes", "--target"]).arg(&target).assert().success();
    }
    let text = fs::read_to_string(&target).unwrap();
    assert!(text.starts_with("# Mine\n\nkeep me\n"));
    assert_eq!(text.matches("<!-- jproject:start -->").count(), 1);
    assert!(text.contains("## AI workspace framework"));
    env.cmd().args(["rules", "install", "--yes", "--target"]).arg(&target).assert().success()
        .stdout(predicate::str::contains("up to date"));
}

#[test]
fn hook_install_writes_pre_commit() {
    let env = Env::new();
    env.init();
    env.cmd().args(["hook", "install", "-C"]).arg(&env.root).assert().success();
    let hook = fs::read_to_string(env.p("product/.git/hooks/pre-commit")).unwrap();
    assert!(hook.contains("jproject check --product-only"));
}

#[test]
fn parallel_folder() {
    let env = Env::new();
    env.init();
    let par = env.root.parent().unwrap().join("my-game-crafting");
    env.cmd().args(["init", "--parallel"]).arg(&par).arg("--origin").arg(&env.root).assert().success();
    for f in ["AGENTS.md", "CLAUDE.md", "HANDOFF.md", "_work/AGENTS.md", "out"] {
        assert!(par.join(f).exists(), "{f}");
    }
    let agents = fs::read_to_string(par.join("AGENTS.md")).unwrap();
    assert!(agents.contains(&env.root.display().to_string()) && agents.contains("only to read"));
    // Inside the origin: refused.
    env.cmd().args(["init", "--parallel"]).arg(env.p("_work/00-tmp/x")).arg("--origin").arg(&env.root).assert().failure();
}

#[test]
fn templates_and_rules_are_portable() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let re = regex::Regex::new(r"(?i)\b[a-z]:[\\/]|/users/|/home/|organizacao|rodrigo").unwrap();
    let mut checked = 0;
    for dir in ["templates", "global-rules", "cli/src"] {
        for e in walkdir::WalkDir::new(base.join(dir)).into_iter().filter_map(|e| e.ok()).filter(|e| e.file_type().is_file()) {
            let text = fs::read_to_string(e.path()).unwrap();
            for (i, line) in text.lines().enumerate() {
                // The jtasks repository URL is a public address, not a local path.
                if line.contains("rodrigojager/jtasks") {
                    continue;
                }
                assert!(!re.is_match(line), "{}:{}: {line}", e.path().display(), i + 1);
            }
            checked += 1;
        }
    }
    assert!(checked > 20);
}

fn reset_token(env: &Env) -> String {
    let output = env.cmd().args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root)
        .assert().success().get_output().stdout.clone();
    String::from_utf8(output).unwrap().lines()
        .find_map(|l| l.strip_prefix("Approval token: ")).unwrap().to_string()
}

#[test]
fn plan_reset_previews_then_clears_only_previous_planning_artifacts() {
    let env = Env::new();
    env.init();
    env.write("_work/30-plan/PLAN.md", "old objective");
    env.write("_work/30-plan/details/prompt.md", "old prompt");
    env.write("_work/10-docs/BACKLOG.md", "# Backlog\n\n## Context\nExisting product.\n\n## Open items\n- B01: still needs device validation.");
    env.write("_work/80-ideas/IDEAS.md", "# Ideas\n\n## Future idea\nDeferred.");
    env.write("_work/10-docs/adr/0001-storage.md", "accepted decision");
    env.write("_work/40-handoff/other.md", "another workstream");
    let protected = ["_work/10-docs/BACKLOG.md", "_work/80-ideas/IDEAS.md", "_work/10-docs/GLOSSARY.md",
        "_work/10-docs/adr/0001-storage.md", "_work/30-plan/AGENTS.md", "_work/40-handoff/other.md", "_secrets/.env", "product/README.md"];
    let before: Vec<_> = protected.iter().map(|p| fs::read(env.p(p)).unwrap()).collect();
    let token = reset_token(&env);
    assert_eq!(fs::read_to_string(env.p("_work/30-plan/PLAN.md")).unwrap(), "old objective");
    env.cmd().args(["plan", "reset", "--approve", &token, "-C"]).arg(&env.root)
        .assert().failure().stderr(predicate::str::contains("--pending-reviewed"));
    assert!(env.p("_work/30-plan/PLAN.md").exists());
    env.cmd().args(["plan", "reset", "--approve", &token, "--pending-reviewed", "-C"]).arg(&env.root)
        .assert().success().stdout(predicate::str::contains("Previous plan cleared"));
    for p in ["PLAN.md", "PROGRESS.md", "DIVERGENCES.md", "details"] {
        assert!(!env.p(&format!("_work/30-plan/{p}")).exists(), "{p} survived reset");
    }
    for (p, expected) in protected.iter().zip(before) {
        assert_eq!(fs::read(env.p(p)).unwrap(), expected, "{p} changed");
    }
    // A new plan can now be written without old child files or status carrying over.
    env.write("_work/30-plan/PLAN.md", "new objective");
    env.write("_work/30-plan/PROGRESS.md", "# Progress\n\n## Next\nNew work.");
    assert!(!env.p("_work/30-plan/details/prompt.md").exists());
    env.check().success();
}

#[test]
fn plan_reset_invalidates_approval_when_plan_backlog_or_ideas_change() {
    let env = Env::new();
    env.init();
    for path in ["_work/30-plan/PROGRESS.md", "_work/10-docs/BACKLOG.md", "_work/80-ideas/IDEAS.md", "_work/30-plan/extra.md"] {
        let token = reset_token(&env);
        env.write(path, &format!("changed {path}"));
        env.cmd().args(["plan", "reset", "--pending-reviewed", "--approve", &token, "-C"]).arg(&env.root)
            .assert().failure().stderr(predicate::str::contains("Inventory changed"));
        assert!(env.p("_work/30-plan/PLAN.md").exists());
    }
}

#[test]
fn plan_reset_refuses_links_unknown_data_and_parent_traversal() {
    let env = Env::new();
    env.init();
    let link = env.p("_work/30-plan/reference");
    let target = env.p("_work/10-docs");
    #[cfg(windows)]
    junction::create(&target, &link).unwrap();
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    env.cmd().args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root).assert().failure()
        .stderr(predicate::str::contains("refuses links"));
    #[cfg(windows)]
    fs::remove_dir(&link).unwrap();
    #[cfg(not(windows))]
    fs::remove_file(&link).unwrap();
    assert!(target.join("BACKLOG.md").exists());
    env.write("_work/30-plan/jtasks/draft.json", "unsubmitted work");
    env.cmd().args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root).assert().failure()
        .stderr(predicate::str::contains("Unrecognized jtasks data"));
    let marker = env.p("_work/WORKSPACE.md");
    let ws = fs::read_to_string(&marker).unwrap();
    fs::write(&marker, ws.replace("jtasks_data: _work/30-plan/jtasks", "jtasks_data: _work/30-plan/../10-docs")).unwrap();
    env.cmd().args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root).assert().failure()
        .stderr(predicate::str::contains("Unsafe reset path"));
    assert!(env.p("_work/30-plan/PLAN.md").exists());
}


#[test]
fn plan_reset_refuses_legacy_run_data() {
    let env = Env::new();
    env.init();
    env.write("_work/30-plan/jtasks/runs/old/run.json", "legacy history");
    env.cmd().args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root)
        .assert().failure().stderr(predicate::str::contains("Unrecognized jtasks data"));
    assert!(env.p("_work/30-plan/jtasks/runs/old/run.json").exists());
}

#[test]
fn native_v2_run_is_blocked_until_finished_then_cleaned_with_approval() {
    let Some(exe) = std::env::var_os("JTASKS_V2_TEST_BIN") else {
        eprintln!("Set JTASKS_V2_TEST_BIN to exercise the native v2 integration");
        return;
    };
    let env = Env::new();
    env.init();
    let plan = env.p("_work/00-tmp/fixture.json");
    fs::write(&plan, serde_json::json!({
        "schema_version":1,"completion":"judgment","operation_placement":"none",
        "tasks":[{"id":"T1","title":"Fixture","executor":{"kind":"fixture","outcome":"completed","summary":"Done"}}]
    }).to_string()).unwrap();
    let invoke = |args: &[&str]| {
        let out = std::process::Command::new(&exe).args(args).arg("--root").arg(&env.root)
            .arg("--data-dir").arg(env.p("_work/30-plan/jtasks/v2")).output().unwrap();
        assert!(out.status.success(), "stdout: {} stderr: {}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    };
    invoke(&["import", "--run", "fixture", plan.to_str().unwrap()]);
    let bin = PathBuf::from(&exe).parent().unwrap().to_path_buf();
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    env.cmd().env("PATH", &path).args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root)
        .assert().failure().stderr(predicate::str::contains("not completed"));
    invoke(&["run", "--run", "fixture"]);
    let out = env.cmd().env("PATH", &path).args(["plan", "reset", "--dry-run", "-C"]).arg(&env.root)
        .assert().success().get_output().stdout.clone();
    let text = String::from_utf8(out).unwrap();
    assert!(text.contains("fixture"));
    let token = text.lines().find_map(|l| l.strip_prefix("Approval token: ")).unwrap();
    env.cmd().env("PATH", &path).args(["plan", "reset", "--pending-reviewed", "--approve", token, "-C"]).arg(&env.root).assert().success();
    assert!(env.p("_work/30-plan/jtasks/v2/state.sqlite3").exists());
    assert!(!env.p("_work/30-plan/PLAN.md").exists());
}
