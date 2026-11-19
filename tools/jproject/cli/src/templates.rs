//! Templates embedded in the binary, overridable file by file from `templates_dir`.

use crate::config::Config;

macro_rules! embed {
    ($($path:literal),* $(,)?) => {
        &[$(($path, include_str!(concat!("../../templates/", $path)))),*]
    };
}

pub const EMBEDDED: &[(&str, &str)] = embed![
    "container/AGENTS.md",
    "container/CLAUDE.md",
    "container/claude-settings.json",
    "product/AGENTS.md",
    "product/WORKSPACE.md",
    "product/README.md",
    "product/gitignore",
    "product/env.example",
    "work/WORKSPACE.md",
    "work/00-tmp/AGENTS.md",
    "work/10-docs/AGENTS.md",
    "work/10-docs/GLOSSARY.md",
    "work/10-docs/BACKLOG.md",
    "work/20-design/AGENTS.md",
    "work/25-source-assets/AGENTS.md",
    "work/30-plan/AGENTS.md",
    "work/30-plan/PLAN.md",
    "work/30-plan/PROGRESS.md",
    "work/30-plan/DIVERGENCES.md",
    "work/40-handoff/AGENTS.md",
    "work/50-tools/AGENTS.md",
    "work/60-intake/AGENTS.md",
    "work/70-proposals/AGENTS.md",
    "work/80-ideas/AGENTS.md",
    "work/80-ideas/IDEAS.md",
    "items/INTAKE.md",
    "items/PROPOSAL.md",
    "items/HANDOFF.md",
    "parallel/AGENTS.md",
    "parallel/CLAUDE.md",
    "parallel/HANDOFF.md",
    "parallel/work-AGENTS.md",
];

pub const GLOBAL_RULES: &str = include_str!("../../global-rules/framework-section.md");

/// Returns a template by its relative path; a file with the same path under
/// `templates_dir` wins over the embedded copy.
pub fn get(rel: &str, cfg: &Config) -> String {
    if let Some(dir) = cfg.templates_dir() {
        if let Ok(text) = std::fs::read_to_string(dir.join(rel)) {
            return text;
        }
    }
    EMBEDDED
        .iter()
        .find(|(p, _)| *p == rel)
        .map(|(_, t)| t.to_string())
        .unwrap_or_else(|| panic!("missing embedded template {rel}"))
}

/// Embedded templates under a prefix, as (path without prefix, content).
pub fn under(prefix: &str) -> Vec<(&'static str, &'static str)> {
    EMBEDDED
        .iter()
        .filter_map(|(p, t)| p.strip_prefix(prefix).map(|rest| (rest, *t)))
        .collect()
}
