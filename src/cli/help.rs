//! One terminology/contract reference for the public CLI; human text only.
use super::Cli;
use crate::read::i18n;
use clap::{Command, CommandFactory};

fn section(name: &str) -> String {
    format!(
        "{}:\n  {}",
        i18n::tr(&format!("heading.{name}")),
        i18n::tr(&format!("detail.{name}"))
    )
}

pub fn command() -> Command {
    let args: Vec<_> = std::env::args_os().collect();
    let choice = i18n::initialize(&args);
    if let Some(warning) = &choice.warning {
        eprintln!("{warning}");
    }
    let overview = [
        "execution",
        "monitoring",
        "operations",
        "autonomy",
        "checkpoint",
        "estimator",
        "cleanup",
    ]
    .iter()
    .map(|name| section(name))
    .collect::<Vec<_>>()
    .join("\n\n");
    let mut command = Cli::command()
        .about(i18n::tr("help.about"))
        .after_help(overview);
    command = command
        .mut_arg("lang", |arg| arg.help(i18n::tr("help.lang")))
        .mut_arg("json", |arg| arg.help(i18n::tr("help.json")));
    for (name, topic) in [
        ("run", "execution"),
        ("stop", "execution"),
        ("watch", "monitoring"),
        ("read", "monitoring"),
        ("operations", "operations"),
        ("checkpoint", "checkpoint"),
        ("clean", "cleanup"),
    ] {
        command = command.mut_subcommand(name, |sub| {
            let prior = sub.get_after_help().map(ToString::to_string);
            sub.after_help(match prior {
                Some(prior) => format!("{prior}\n\n{}", section(topic)),
                None => section(topic),
            })
        });
    }
    command
}
