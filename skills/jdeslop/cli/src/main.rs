use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, bail, ensure};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

mod locale;
use locale::{Locale, preference_env};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

#[cfg(windows)]
mod windows;

#[derive(Parser)]
#[command(about = "Audit and review UI for AI slop from one directory.", version)]
struct Cli {
    #[arg(
        long,
        env = preference_env("JDESLOP_SKILLS_ROOT", "ANTI_SLOP_SKILLS_ROOT"),
        global = true,
        help = "Directory containing the jdeslop skill"
    )]
    skills_root: Option<PathBuf>,
    #[arg(
        long,
        env = preference_env("JDESLOP_PYTHON", "ANTI_SLOP_PYTHON"),
        global = true,
        default_value = "python"
    )]
    python: OsString,
    #[arg(long, global = true, env = preference_env("JDESLOP_LANG", "ANTI_SLOP_LANG"), default_value = "en",
          value_parser = locale::parse_language,
          help = "Language for messages and the initial review interface")]
    lang: String,
    #[arg(long, global = true, env = preference_env("JDESLOP_THEME", "ANTI_SLOP_THEME"), default_value = "system",
          value_parser = ["light", "dark", "system"],
          help = "Initial color theme for the review interface")]
    theme: String,
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Create an audit and review manifest for one or more surfaces.
    Init {
        run: PathBuf,
        #[arg(long, required = true)]
        surface: Vec<String>,
        #[arg(long)]
        source_root: Option<PathBuf>,
        #[arg(long)]
        title: Option<String>,
    },
    /// Validate the full audit ledger. Exit 2 indicates evidence gaps.
    Audit { run: PathBuf },
    /// Open the local approval panel or build a portable HTML review.
    Review {
        run: PathBuf,
        #[arg(
            long,
            default_value_t = 8765,
            help = "Bind port (0 chooses a free port)"
        )]
        port: u16,
        #[arg(long, help = "Build the preview without starting a source queue")]
        portable: bool,
    },
    /// Report audit readiness, staged revisions and source jobs.
    Status { run: PathBuf },
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunConfig {
    version: u8,
    review_id: String,
    source_root: Option<PathBuf>,
}

impl RunConfig {
    fn open(run: &Path, locale: &Locale) -> Result<Self> {
        let path = run.join("run.json");
        ensure!(
            path.canonicalize().with_context(|| format!(
                "{} {}",
                locale.t("Open"),
                path.display()
            ))? == path,
            "{}",
            locale.t("run.json must be a file in the run directory.")
        );
        let config: Self = serde_json::from_reader(File::open(&path)?)
            .with_context(|| format!("{} {}", locale.t("Read"), path.display()))?;
        ensure!(
            config.version == 1 && !config.review_id.is_empty(),
            "{}",
            locale.t("Invalid run.json.")
        );
        if let Some(root) = &config.source_root {
            ensure!(
                root.is_absolute(),
                "{}",
                locale.t("The recorded source root must be absolute.")
            );
        }
        Ok(config)
    }
}

struct Skills {
    root: PathBuf,
    python: OsString,
    locale: Locale,
    theme: String,
}

impl Skills {
    fn command(&self, relative: &str) -> Result<Command> {
        let script = self.root.join(relative);
        ensure!(
            script.is_file(),
            "{} {}. {}",
            self.locale.t("Missing"),
            script.display(),
            self.locale.t("Set --skills-root to the jdeslop directory.")
        );
        let mut command = Command::new(&self.python);
        command
            .args(["-B", "-u"])
            .arg(script)
            .env("JDESLOP_LANG", &self.locale.0)
            .env("JDESLOP_THEME", &self.theme)
            .env("PYTHONIOENCODING", "utf-8");
        Ok(command)
    }

    fn coverage(&self) -> Result<Command> {
        self.command("scripts/coverage.py")
    }

    fn review(&self, run: &Path, config: RunConfig) -> Result<Command> {
        let mut command = self.command("review/scripts/run_review.py")?;
        command.arg(run).arg("--review-id").arg(config.review_id);
        if let Some(root) = config.source_root {
            command.arg("--source-root").arg(root);
        }
        Ok(command)
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = File::create_new(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    file.write_all(b"\n")?;
    Ok(())
}

fn init(
    skills: &Skills,
    run: &Path,
    surfaces: &[String],
    source_root: Option<PathBuf>,
    title: Option<String>,
) -> Result<()> {
    let locale = &skills.locale;
    let name = run
        .file_name()
        .context(locale.t("Choose a new run directory."))?;
    let parent = run
        .parent()
        .context(locale.t("Choose a new run directory."))?;
    fs::create_dir_all(parent)?;
    let parent = parent.canonicalize()?;
    let run = parent.join(name);
    ensure!(
        !run.try_exists()?,
        "{} {}. {}",
        locale.t("Run already exists:"),
        run.display(),
        locale.t("Resume with audit, review or status.")
    );
    let source_root = source_root
        .map(|root| {
            let resolved = root
                .canonicalize()
                .with_context(|| format!("{} {}", locale.t("Open"), root.display()))?;
            ensure!(
                resolved.is_dir(),
                "{}",
                locale.t("Source root must be a directory.")
            );
            Ok(resolved)
        })
        .transpose()?;
    let config = RunConfig {
        version: 1,
        review_id: format!("jdeslop-{}", Uuid::new_v4().simple()),
        source_root,
    };
    let staging = tempfile::Builder::new()
        .prefix(".jdeslop-")
        .tempdir_in(parent)?;
    let pending = staging.path().join("run");
    fs::create_dir(&pending)?;
    let mut coverage = skills.coverage()?;
    coverage.arg("init").arg(pending.join("coverage.json"));
    for surface in surfaces {
        coverage.arg("--surface").arg(surface);
    }
    let result = coverage
        .output()
        .context(locale.t("Start Python for the coverage ledger"))?;
    if !result.status.success() {
        bail!("{}", String::from_utf8_lossy(&result.stderr).trim());
    }
    let manifest_surfaces: Vec<_> = surfaces
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let sequence = index + 1;
            json!({
                "id": format!("surface-{sequence}"),
                "title": name.trim(),
                "baseline": format!("baseline-{sequence}.html"),
                "surface": name.trim()
            })
        })
        .collect();
    let manifest = json!({
        "reviewId": config.review_id,
        "title": title.unwrap_or_else(|| name.to_string_lossy().into_owned()),
        "context": "",
        "ledger": "coverage.json",
        "surfaces": manifest_surfaces,
        "fixes": []
    });
    write_json(&pending.join("review.json"), &manifest)?;
    write_json(&pending.join("run.json"), &config)?;
    fs::rename(&pending, &run)
        .with_context(|| format!("{} {}", locale.t("Create"), run.display()))?;
    println!("{} {}", locale.t("Created run:"), display_path(&run));
    println!(
        "{}",
        locale.t(
            "Fill coverage.json with audit evidence and stage baselines and fixes in review.json."
        )
    );
    Ok(())
}

fn display_path(path: &Path) -> String {
    let path = path.display().to_string();
    if let Some(network) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{network}")
    } else {
        path.strip_prefix(r"\\?\").unwrap_or(&path).to_owned()
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let mut root = cli
        .skills_root
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(".."));
    if !root.join("scripts/coverage.py").is_file() && root.join("jdeslop/SKILL.md").is_file() {
        // Existing configurations point to the parent of the skill directory.
        root = root.join("jdeslop");
    }
    let skills = Skills {
        root,
        python: cli.python,
        locale: Locale(cli.lang),
        theme: cli.theme,
    };
    let locale = &skills.locale;
    let (path, action) = match cli.action {
        Action::Init {
            run,
            surface,
            source_root,
            title,
        } => {
            init(
                &skills,
                &std::env::current_dir()?.join(run),
                &surface,
                source_root,
                title,
            )?;
            return Ok(ExitCode::SUCCESS);
        }
        Action::Audit { ref run } | Action::Review { ref run, .. } | Action::Status { ref run } => {
            (
                run.canonicalize()
                    .with_context(|| format!("{} {}", locale.t("Open run"), run.display()))?,
                cli.action,
            )
        }
    };
    let config = RunConfig::open(&path, locale)?;
    let status = match action {
        Action::Audit { .. } => {
            let ledger = path.join("coverage.json");
            ensure!(
                ledger.canonicalize()? == ledger,
                "{}",
                locale.t("coverage.json must be a file in the run directory.")
            );
            skills.coverage()?.arg("verify").arg(ledger).status()
        }
        Action::Status { .. } => skills.review(&path, config)?.arg("--status").status(),
        Action::Review { port, portable, .. } => {
            let mut command = skills.review(&path, config)?;
            command.arg("--port").arg(port.to_string());
            if portable {
                command.arg("--portable");
            }
            run_review(command)
        }
        Action::Init { .. } => unreachable!(),
    }
    .context(locale.t("Run the Python skill backend"))?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(1)
        .into())
}

#[cfg(unix)]
fn run_review(mut command: Command) -> io::Result<std::process::ExitStatus> {
    use std::os::unix::process::CommandExt;
    Err(command.exec())
}

#[cfg(windows)]
fn run_review(command: Command) -> io::Result<std::process::ExitStatus> {
    windows::run_review(command)
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let locale = Locale::from_args(&args);
    let command = locale.localize_command(Cli::command());
    let cli = Cli::from_arg_matches(
        &command
            .clone()
            .try_get_matches_from(&args)
            .unwrap_or_else(|error| locale.report_parse_error(error, &command)),
    )
    .unwrap_or_else(|error| locale.report_parse_error(error, &command));
    match run(cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("jdeslop: {error:#}");
            ExitCode::FAILURE
        }
    }
}
