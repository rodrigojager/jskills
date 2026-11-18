//! Explicit self-update, independent of checkout/runtime state and optional hosts.
use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use semver::Version;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, process::Command, time::Duration};

#[derive(Args)]
#[command(
    after_help = "jtasks update --check --repository owner/repo\njtasks update --repository owner/repo\nThe source can also be set with JTASKS_UPDATE_REPOSITORY or embedded when building a release. Only stable GitHub releases are used. Updating replaces this installation, never a project database or another CLI installation. Existing processes keep running their current version."
)]
pub struct Update {
    /// Report availability without downloading a binary or writing files.
    #[arg(long)]
    pub check: bool,
    /// Explicit trusted GitHub release repository (owner/repo).
    #[arg(long, env = "JTASKS_UPDATE_REPOSITORY")]
    pub repository: Option<String>,
}

#[derive(Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}

#[derive(Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

pub fn repository(value: Option<&str>) -> Result<&str> {
    let value = value
        .or(option_env!("JTASKS_UPDATE_REPOSITORY"))
        .unwrap_or("");
    let parts: Vec<_> = value.split('/').collect();
    ensure!(
        parts.len() == 2
            && parts.iter().all(|part| {
                !part.is_empty()
                    && *part != "."
                    && *part != ".."
                    && part
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
            }),
        "Set --repository owner/repo or JTASKS_UPDATE_REPOSITORY to the trusted release source"
    );
    Ok(value)
}

pub fn target() -> Result<&'static str> {
    if cfg!(all(
        target_os = "windows",
        target_arch = "x86_64",
        target_env = "msvc"
    )) {
        Ok("x86_64-pc-windows-msvc")
    } else if cfg!(all(
        target_os = "linux",
        target_arch = "x86_64",
        target_env = "gnu"
    )) {
        Ok("x86_64-unknown-linux-gnu")
    } else {
        bail!("Self-update has no published package convention for this platform")
    }
}

fn fetch(url: &str, limit: u64) -> Result<Vec<u8>> {
    ensure!(
        url.starts_with("https://"),
        "Update downloads require HTTPS"
    );
    let config = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(45)))
        .build();
    let mut response = ureq::Agent::new_with_config(config)
        .get(url)
        .header("User-Agent", concat!("jtasks/", env!("CARGO_PKG_VERSION")))
        .call()
        .context("Cannot fetch update; check connectivity and the release repository")?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "Update response exceeds its size limit"
    );
    Ok(bytes)
}

pub fn latest_version(release: &Release) -> Result<Version> {
    let version = Version::parse(
        release
            .tag_name
            .strip_prefix('v')
            .unwrap_or(&release.tag_name),
    )?;
    ensure!(
        !release.draft && !release.prerelease && version.pre.is_empty(),
        "Only stable releases can be installed"
    );
    Ok(version)
}

fn asset<'a>(release: &'a Release, name: &str, repo: &str) -> Result<&'a str> {
    let matches: Vec<_> = release.assets.iter().filter(|a| a.name == name).collect();
    ensure!(
        matches.len() == 1,
        "Release must contain exactly one {name} asset"
    );
    let url = &matches[0].browser_download_url;
    ensure!(
        url.starts_with(&format!("https://github.com/{repo}/releases/download/")),
        "Asset is outside the selected release repository"
    );
    Ok(url)
}

pub fn verify(bytes: &[u8], checksum: &[u8], name: &str) -> Result<()> {
    let checksum = std::str::from_utf8(checksum)?.trim();
    let fields: Vec<_> = checksum.split_whitespace().collect();
    ensure!(
        fields.len() == 2
            && fields[1].trim_start_matches('*') == name
            && fields[0].len() == 64
            && fields[0].bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid checksum sidecar for {name}"
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(fields[0]),
        "Update SHA-256 mismatch; installed executable was preserved"
    );
    Ok(())
}

// Candidate and staging live beside the current executable, on its filesystem.
// No controller/database is opened, and no running process is stopped.
pub fn install_verified(
    current: &Path,
    bytes: &[u8],
    expected: &str,
    replace: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let parent = current
        .parent()
        .context("Executable has no parent directory")?;
    let stage = parent.join(format!(
        ".jtasks-update-{}{}",
        uuid::Uuid::new_v4(),
        if cfg!(windows) { ".exe" } else { "" }
    ));
    let mut created = false;
    let result = (|| {
        use std::io::Write;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&stage)
            .context("Cannot stage update beside this installation; use its package manager or a writable installation")?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stage, fs::Permissions::from_mode(0o755))?;
        }
        let mut command = Command::new(&stage);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        // Bounded candidate probe; a bad release must not strand the update command.
        let output =
            crate::executors::catalog::probe(command, &["--version"], Duration::from_secs(5))
                .map_err(|code| {
                    anyhow::anyhow!("Downloaded executable verification failed: {code}")
                })?;
        ensure!(
            output.trim() == format!("jtasks {expected}"),
            "Downloaded executable does not match release version; installed executable was preserved"
        );
        replace(&stage)
    })();
    // Only remove the unique file this call created, never a pre-existing path.
    if created {
        let _ = fs::remove_file(&stage);
    }
    result
}

pub fn perform(
    args: Update,
    get: impl Fn(&str, u64) -> Result<Vec<u8>>,
    current: &Path,
    replace: impl FnOnce(&Path) -> Result<()>,
) -> Result<Value> {
    let repo = repository(args.repository.as_deref())?;
    let release: Release = serde_json::from_slice(&get(
        &format!("https://api.github.com/repos/{repo}/releases/latest"),
        1024 * 1024,
    )?)?;
    let version = latest_version(&release)?;
    let current_version = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let available = version > current_version;
    let mut report = json!({"contract_version":1,"component":"cli","repository":repo,"current_version":current_version.to_string(),"latest_version":version.to_string(),"available":available,"updated":false,"check_only":args.check});
    if args.check || !available {
        return Ok(report);
    }
    let ownership = current
        .parent()
        .context("Executable has no parent")?
        .join("jtasks-installation.json");
    if ownership.exists() {
        let policy: Value = serde_json::from_slice(&fs::read(&ownership)?)?;
        ensure!(
            policy["self_update"] != false,
            "This installation is managed: {}",
            policy["reason"]
                .as_str()
                .unwrap_or("use its owning package updater")
        );
    }
    let name = format!(
        "jtasks-{version}-{}{}",
        target()?,
        if cfg!(windows) { ".exe" } else { ".bin" }
    );
    let checksum = get(asset(&release, &format!("{name}.sha256"), repo)?, 1024)?;
    let bytes = get(asset(&release, &name, repo)?, 128 * 1024 * 1024)?;
    verify(&bytes, &checksum, &name)?;
    install_verified(current, &bytes, &version.to_string(), replace)?;
    report["updated"] = json!(true);
    Ok(report)
}

pub fn execute(args: Update) -> Result<i32> {
    let current = std::env::current_exe()?;
    let report = perform(args, fetch, &current, |stage| {
        self_replace::self_replace(stage)
            .context("Could not replace this installation; running processes were not stopped")
    })?;
    println!("{report}");
    Ok(0)
}
