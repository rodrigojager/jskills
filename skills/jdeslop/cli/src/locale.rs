use std::ffi::OsString;
use std::sync::LazyLock;

use clap::Command;
use clap::error::{ContextKind, Error, ErrorKind};
use serde_json::Value;

static MESSAGES: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("locales.json")).expect("valid message catalog")
});

pub(crate) fn preference_env(primary: &'static str, legacy: &'static str) -> &'static str {
    if std::env::var_os(primary).is_none() && std::env::var_os(legacy).is_some() {
        legacy
    } else {
        primary
    }
}

pub fn parse_language(value: &str) -> Result<String, String> {
    let language = if value.eq_ignore_ascii_case("pt") {
        Some("pt-BR")
    } else {
        MESSAGES.as_object().and_then(|locales| {
            locales
                .keys()
                .find(|code| code.eq_ignore_ascii_case(value))
                .map(String::as_str)
        })
    };
    language.map(str::to_owned).ok_or_else(|| {
        MESSAGES
            .as_object()
            .expect("languages")
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    })
}

pub struct Locale(pub String);

impl Locale {
    pub fn from_args(args: &[OsString]) -> Self {
        let mut candidate = std::env::var(preference_env("JDESLOP_LANG", "ANTI_SLOP_LANG")).ok();
        let mut args = args.iter().skip(1).filter_map(|arg| arg.to_str());
        while let Some(arg) = args.next() {
            if arg == "--" {
                break;
            }
            if arg == "--lang" {
                candidate = args.next().map(str::to_owned);
            } else if let Some(value) = arg.strip_prefix("--lang=") {
                candidate = Some(value.to_owned());
            }
        }
        Self(
            candidate
                .and_then(|value| parse_language(&value).ok())
                .unwrap_or_else(|| "en".into()),
        )
    }

    pub fn t<'a>(&self, key: &'a str) -> &'a str {
        let messages = &MESSAGES[&self.0];
        messages[key]
            .as_str()
            .or_else(|| {
                messages[format!("{key}.")]
                    .as_str()
                    .and_then(|text| text.strip_suffix('.'))
            })
            .unwrap_or(key)
    }

    pub fn localize_command(&self, mut command: Command) -> Command {
        if let Some(about) = command.get_about().map(ToString::to_string) {
            command = command.about(clap::builder::StyledStr::from(self.t(&about).to_owned()));
        }
        let args: Vec<_> = command
            .get_arguments()
            .map(|arg| {
                let id = arg.get_id();
                let help = arg
                    .get_help()
                    .map(ToString::to_string)
                    .or_else(|| match id.as_str() {
                        "run" => Some("Run directory".to_owned()),
                        "surface" => Some("Surface to audit (repeatable)".to_owned()),
                        "source_root" => Some("Source code directory".to_owned()),
                        "title" => Some("Review title".to_owned()),
                        "python" => Some("Python executable".to_owned()),
                        _ => None,
                    });
                (id.clone(), help)
            })
            .collect();
        for (id, help) in args {
            if let Some(help) = help {
                let localized = clap::builder::StyledStr::from(self.t(&help).to_owned());
                command = command.mut_arg(id, |arg| arg.help(localized));
            }
        }
        if self.0 != "en" {
            // Clap generates the argument table and highlights. Translate its built-in headings after rendering.
            let mut rendered = command.render_long_help().to_string();
            for source in ["env", "default", "possible values"] {
                rendered =
                    rendered.replace(&format!("[{source}:"), &format!("[{}:", self.t(source)));
            }
            for source in ["Usage", "Arguments", "Options", "Commands"] {
                rendered = rendered.replace(
                    &format!(
                        "
{source}:"
                    ),
                    &format!(
                        "
{}:",
                        self.t(source)
                    ),
                );
            }
            for source in [
                "Print help",
                "Print version",
                "Print this message or the help of the given subcommand(s)",
            ] {
                rendered = rendered.replace(source, self.t(source));
            }
            let subcommands: Vec<_> = command
                .get_subcommands()
                .map(|cmd| {
                    (
                        cmd.get_name().to_owned(),
                        cmd.get_about().map(ToString::to_string),
                    )
                })
                .collect();
            for (_, about) in &subcommands {
                if let Some(about) = about {
                    rendered = rendered.replace(about, self.t(about));
                }
            }
            for (name, _) in subcommands {
                let bin_name = match name.as_str() {
                    "init" => concat!(env!("CARGO_PKG_NAME"), " init"),
                    "audit" => concat!(env!("CARGO_PKG_NAME"), " audit"),
                    "review" => concat!(env!("CARGO_PKG_NAME"), " review"),
                    "status" => concat!(env!("CARGO_PKG_NAME"), " status"),
                    _ => env!("CARGO_PKG_NAME"),
                };
                command = command.mut_subcommand(name, |child| {
                    self.localize_command(child.bin_name(bin_name))
                });
            }
            command = command.override_help(clap::builder::StyledStr::from(rendered));
        }
        command
    }

    pub fn report_parse_error(&self, error: Error, command: &Command) -> ! {
        if self.0 == "en"
            || matches!(
                error.kind(),
                ErrorKind::DisplayHelp
                    | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                    | ErrorKind::DisplayVersion
            )
        {
            error.exit();
        }
        let message = match error.kind() {
            ErrorKind::UnknownArgument | ErrorKind::InvalidSubcommand => {
                "Unknown argument or command."
            }
            ErrorKind::MissingRequiredArgument => "A required argument is missing.",
            ErrorKind::MissingSubcommand => "Provide a command.",
            ErrorKind::ArgumentConflict => "An argument was provided more than once.",
            ErrorKind::InvalidValue | ErrorKind::ValueValidation => {
                "An argument has an invalid value."
            }
            _ => "Invalid argument.",
        };
        eprintln!("jdeslop: {}", self.t(message));
        for context in [
            ContextKind::InvalidArg,
            ContextKind::InvalidValue,
            ContextKind::InvalidSubcommand,
        ] {
            if let Some(value) = error.get(context) {
                eprintln!("  {value}");
            }
        }
        eprintln!(
            "
{} --help",
            command.get_name()
        );
        std::process::exit(error.exit_code());
    }
}
