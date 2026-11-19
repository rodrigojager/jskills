//! Small helpers shared by every command: dates, prompts, links, sizes, process lookup.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Today's date (UTC) as `YYYY-MM-DD`.
pub fn today() -> String {
    date_of(SystemTime::now())
}

/// A system time as `YYYY-MM-DD` (UTC).
pub fn date_of(t: SystemTime) -> String {
    let secs = t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant's days-to-civil algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Replaces `{{key}}` placeholders.
pub fn render(template: &str, vars: &BTreeMap<&str, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

pub fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

/// Asks a yes/no question. `--yes` answers yes; without a terminal the answer is no.
pub fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    if !interactive() {
        println!("{question} [no terminal: answering no; pass --yes to confirm]");
        return Ok(false);
    }
    Ok(dialoguer::Confirm::new()
        .with_prompt(question)
        .default(false)
        .interact()?)
}

/// Writes `content` to `path` only if the file does not exist. Returns true if written.
pub fn write_new(path: &Path, content: &str) -> Result<bool> {
    if path.exists() || is_link(path) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(true)
}

/// Creates a directory if missing. Returns true if created.
pub fn mkdir_new(path: &Path) -> Result<bool> {
    if path.exists() || is_link(path) {
        return Ok(false);
    }
    std::fs::create_dir_all(path).with_context(|| format!("cannot create {}", path.display()))?;
    Ok(true)
}

/// True for symlinks and Windows junctions (name-surrogate reparse points).
pub fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink() || is_junction(path))
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_junction(path: &Path) -> bool {
    junction::exists(path).unwrap_or(false)
}
#[cfg(not(windows))]
fn is_junction(_path: &Path) -> bool {
    false
}

/// Where a link points to (absolute when possible).
pub fn link_target(path: &Path) -> Option<PathBuf> {
    #[cfg(windows)]
    if let Ok(t) = junction::get_target(path) {
        return Some(t);
    }
    let t = std::fs::read_link(path).ok()?;
    Some(if t.is_absolute() { t } else { path.parent()?.join(t) })
}

/// Creates a directory link: a junction on Windows, a symlink elsewhere.
pub fn create_dir_link(link: &Path, target: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let abs = std::path::absolute(target)?;
        junction::create(&abs, link)
            .with_context(|| format!("cannot create junction {} -> {}", link.display(), abs.display()))?;
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, link)
            .with_context(|| format!("cannot create symlink {} -> {}", link.display(), target.display()))?;
    }
    Ok(())
}

/// Removes a directory link without touching its target.
pub fn remove_dir_link(link: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        std::fs::remove_dir(link)?;
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_file(link)?;
    }
    Ok(())
}

/// Total size in bytes of the files under `path` (links are not followed).
pub fn dir_size(path: &Path) -> u64 {
    walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Finds an executable on PATH (honours PATHEXT on Windows, so `npm` finds `npm.cmd`).
pub fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Runs a program found on PATH, inheriting stdio. Fails on a non-zero exit.
pub fn run(program: &str, args: &[&str], cwd: Option<&Path>) -> Result<()> {
    let exe = which(program)
        .with_context(|| format!("`{program}` was not found on PATH"))?;
    let mut cmd = Command::new(exe);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    let status = cmd.status().with_context(|| format!("cannot start `{program}`"))?;
    if !status.success() {
        bail!("`{program} {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

/// Runs a program and captures stdout; None if it is missing or fails.
pub fn output(program: &str, args: &[&str]) -> Option<String> {
    let exe = which(program)?;
    let out = Command::new(exe).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Path relative to `base`, with forward slashes, for display and matching.
pub fn rel(base: &Path, path: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// A name usable as a single folder or file name.
pub fn validate_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name != "."
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        bail!("Invalid name `{name}`: use letters, digits, `-`, `_` or `.` (for example `inventory-screen`).");
    }
    Ok(())
}
