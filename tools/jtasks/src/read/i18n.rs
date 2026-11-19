//! Human-only catalogs adapted from legacy jtasks (MIT; locales/LICENSE).
//! Never translate persisted identifiers, machine projections or executor protocol.
use serde_json::Value;
use std::sync::OnceLock;

pub const TAGS: &[&str] = &[
    "en", "pt-BR", "es", "fr", "de", "it", "zh-CN", "ja", "ru", "ar", "hi",
];
const SOURCES: &[&str] = &[
    include_str!("../../locales/en.json"),
    include_str!("../../locales/pt-BR.json"),
    include_str!("../../locales/es.json"),
    include_str!("../../locales/fr.json"),
    include_str!("../../locales/de.json"),
    include_str!("../../locales/it.json"),
    include_str!("../../locales/zh-CN.json"),
    include_str!("../../locales/ja.json"),
    include_str!("../../locales/ru.json"),
    include_str!("../../locales/ar.json"),
    include_str!("../../locales/hi.json"),
];
static CATALOGS: OnceLock<Vec<Value>> = OnceLock::new();
static CURRENT: OnceLock<Choice> = OnceLock::new();

#[derive(Debug, PartialEq, Eq)]
pub struct Choice {
    pub tag: &'static str,
    pub source: &'static str,
    pub warning: Option<String>,
}

pub fn resolve(input: &str) -> Option<&'static str> {
    let tag = input.split(['.', '@']).next()?.trim().replace('_', "-");
    TAGS.iter()
        .copied()
        .find(|known| known.eq_ignore_ascii_case(&tag))
        .or_else(|| {
            let primary = tag.split('-').next()?;
            TAGS.iter().copied().find(|known| {
                known
                    .split('-')
                    .next()
                    .unwrap()
                    .eq_ignore_ascii_case(primary)
            })
        })
}

/// An unsupported explicit choice forces English; unsupported environment choices fall through.
pub fn choose(
    explicit: Option<&str>,
    env: impl Fn(&str) -> Option<String>,
    system: Option<&str>,
) -> Choice {
    if let Some(input) = explicit.filter(|s| !s.is_empty()) {
        return match resolve(input) {
            Some(tag) => Choice {
                tag,
                source: "--lang",
                warning: None,
            },
            None => Choice {
                tag: "en",
                source: "default",
                warning: Some(format!(
                    "unknown language {input:?} (available: {}); using English",
                    TAGS.join(", ")
                )),
            },
        };
    }
    for source in ["JTASKS_LANG", "JLOCK_LANG"] {
        if let Some(tag) = env(source).as_deref().and_then(resolve) {
            return Choice {
                tag,
                source,
                warning: None,
            };
        }
    }
    match system.and_then(resolve) {
        Some(tag) => Choice {
            tag,
            source: "system",
            warning: None,
        },
        None => Choice {
            tag: "en",
            source: "default",
            warning: None,
        },
    }
}

fn system_locale() -> Option<String> {
    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(name: *mut u16, capacity: i32) -> i32;
        }
        let mut name = [0u16; 85];
        // SAFETY: name points to a writable buffer of the supplied capacity.
        let count = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
        if count > 1 {
            return String::from_utf16(&name[..count as usize - 1]).ok();
        }
    }
    None
}

/// Select before Clap renders help. Stop scanning at the command payload separator.
pub fn initialize(args: &[std::ffi::OsString]) -> &'static Choice {
    CURRENT.get_or_init(|| {
        let mut explicit = None;
        let mut words = args.iter().skip(1);
        while let Some(word) = words.next() {
            if word == "--" {
                break;
            }
            if word == "--lang" {
                explicit = words.next().map(|w| w.to_string_lossy().into_owned());
            } else if let Some(value) = word.to_str().and_then(|w| w.strip_prefix("--lang=")) {
                explicit = Some(value.into());
            }
        }
        choose(
            explicit.as_deref(),
            |name| std::env::var(name).ok(),
            system_locale().as_deref(),
        )
    })
}

pub fn selected() -> &'static str {
    CURRENT.get().map_or("en", |choice| choice.tag)
}

pub fn lookup(tag: &str, key: &str) -> Option<&'static str> {
    let catalogs = CATALOGS.get_or_init(|| {
        SOURCES
            .iter()
            .map(|source| serde_json::from_str(source).expect("embedded catalog"))
            .collect()
    });
    let index = TAGS.iter().position(|known| *known == tag).unwrap_or(0);
    // Dotted keys remain compatible with legacy keys; glossary is nested JSON.
    let value = |catalog: &'static Value| -> Option<&'static str> {
        catalog.get(key).and_then(Value::as_str).or_else(|| {
            key.strip_prefix("_glossary.")
                .and_then(|word| catalog["_glossary"][word].as_str())
        })
    };
    value(&catalogs[index]).or_else(|| value(&catalogs[0]))
}

pub fn tr(key: &str) -> &str {
    lookup(selected(), key).unwrap_or(key)
}

/// Presentation dictionary is separate from the versioned runtime read contract.
pub fn presentation() -> Value {
    let keys = [
        "operations",
        "objective",
        "tasks",
        "states",
        "verification",
        "final",
        "overview",
        "decisions",
        "questions",
        "commands",
        "refresh",
        "readonly",
        "loading",
        "snapshot",
    ];
    let mut value = serde_json::Map::new();
    for key in keys {
        value.insert(key.into(), Value::String(tr(&format!("view.{key}")).into()));
    }
    let states: serde_json::Map<String, Value> = [
        "ended",
        "intent",
        "launch-failed",
        "running",
        "stopping",
        "unresolved",
    ]
    .iter()
    .map(|state| {
        (
            (*state).into(),
            Value::String(tr(&format!("status.command_state.{state}")).into()),
        )
    })
    .collect();
    serde_json::json!({"language":selected(),"labels":value,"command_states":states})
}
