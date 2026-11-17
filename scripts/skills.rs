//! Build local harness instruction packages from canonical sources; no installation.
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub const AGENTS: &[&str] = &["codex", "claude", "opencode"];
pub const SKILLS: &[&str] = &["to-jtasks", "jtasks"];

fn harness(agent: &str, skill: &str) -> &'static str {
    match (agent, skill) {
        ("codex", "jtasks") => {
            "The controller invokes Codex exec directly. Return the adapter-supplied final JSON schema through the designated native final channel, or use the scoped result client. Tool and helper messages have no completion authority. Resume only an explicitly recorded same-task attempt; never use the last session implicitly."
        }
        ("claude", "jtasks") => {
            "The controller invokes Claude Code directly. Its root result structured_output may submit the adapter-supplied final JSON schema; the scoped result client is also available. Nested helper results have no completion authority. Native helper management belongs to Claude Code."
        }
        ("opencode", "jtasks") => {
            "The controller invokes OpenCode directly. Submit through the scoped result client: streamed prose, JSON text and step records do not designate an outcome. Native helper management belongs to OpenCode; no chat wake or plugin supervisor is required."
        }
        ("codex", _) => {
            "Prepare JSON intent for the independent CLI. Codex's own tools can author files; they do not schedule jtasks workers. A jtasks profile alias is distinct from a native Codex config profile."
        }
        ("claude", _) => {
            "Prepare JSON intent for the independent CLI. Claude Code's own tools can author files; no plugin scheduler or copied execution core is required. A jtasks profile alias is distinct from a native Claude agent."
        }
        ("opencode", _) => {
            "Prepare JSON intent for the independent CLI. OpenCode's own tools can author files; no plugin server or copied execution core is required. A jtasks profile alias is distinct from a native OpenCode agent or variant."
        }
        _ => unreachable!(),
    }
}

fn collect(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            return Err(io::Error::other("skill source must not contain symlinks"));
        }
        if entry.file_type()?.is_dir() {
            collect(base, &entry.path(), out)?;
        } else {
            out.insert(
                entry.path().strip_prefix(base).unwrap().into(),
                fs::read(entry.path())?,
            );
        }
    }
    Ok(())
}

/// Expected bytes are derived, never maintained as three independent instructions.
pub fn expected(root: &Path) -> io::Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for skill in SKILLS {
        let source = root.join("skills/source").join(skill);
        let mut files = BTreeMap::new();
        collect(&source, &source, &mut files)?;
        for agent in AGENTS {
            for (relative, bytes) in &files {
                let bytes = if relative == Path::new("SKILL.md") {
                    let text = std::str::from_utf8(bytes).map_err(io::Error::other)?;
                    if text.matches("{{HARNESS_GUIDANCE}}").count() != 1 {
                        return Err(io::Error::other(
                            "SKILL.md needs exactly one harness marker",
                        ));
                    }
                    text.replace("{{HARNESS_GUIDANCE}}", harness(agent, skill))
                        .into_bytes()
                } else {
                    bytes.clone()
                };
                result.insert(
                    PathBuf::from("skills")
                        .join(agent)
                        .join(skill)
                        .join(relative),
                    bytes,
                );
            }
        }
    }
    Ok(result)
}

/// Refuse unexpected package files rather than silently deleting user content.
pub fn check(root: &Path) -> io::Result<()> {
    let expected = expected(root)?;
    let mut actual = BTreeMap::new();
    for agent in AGENTS {
        let path = root.join("skills").join(agent);
        collect(root, &path, &mut actual)?;
    }
    if actual != expected {
        return Err(io::Error::other(
            "skill package drift; rebuild from skills/source and inspect unexpected files",
        ));
    }
    Ok(())
}

pub fn build(root: &Path) -> io::Result<()> {
    let expected = expected(root)?;
    for (relative, bytes) in expected {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, bytes)?;
    }
    check(root)
}

#[allow(dead_code)]
fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().unwrap_or_else(|| "check".into());
    let root = args
        .next()
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    if args.next().is_some() {
        return Err(io::Error::other(
            "usage: skills [build|check] [product-root]",
        ));
    }
    match mode.as_str() {
        "build" => build(&root)?,
        "check" => check(&root)?,
        _ => {
            return Err(io::Error::other(
                "usage: skills [build|check] [product-root]",
            ));
        }
    }
    println!(
        "Local skill packages match canonical source for Codex, Claude Code and OpenCode. No global paths changed."
    );
    Ok(())
}
