//! Version-1 advisory identities. Explicit path scopes include their descendants;
//! glob patterns are rejected rather than silently approximated.
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Identity {
    Path {
        checkout: String,
        segments: Vec<String>,
    },
    Named {
        scope: Scope,
        namespace: String,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "identity", rename_all = "snake_case")]
pub enum Scope {
    Checkout(String),
    /// Explicit host/environment identity, shared across participating checkouts.
    Shared(String),
}

pub fn checkout_key(checkout: &Path) -> Result<String> {
    Ok(normalize(
        &std::fs::canonicalize(checkout)?.to_string_lossy(),
    ))
}

fn normalize(text: &str) -> String {
    if cfg!(windows) {
        text.replace('\\', "/").to_lowercase()
    } else {
        text.into()
    }
}

impl Identity {
    pub fn path(checkout: &Path, value: &Path) -> Result<Self> {
        ensure!(
            !value.to_string_lossy().contains(['*', '?', '[', ']']),
            "explicit file/directory scopes required; globs are unsupported"
        );
        let checkout = std::fs::canonicalize(checkout)?;
        let candidate = if value.is_absolute() {
            value.to_path_buf()
        } else {
            checkout.join(value)
        };
        let mut normalized = PathBuf::new();
        for component in candidate.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    ensure!(normalized.pop(), "path escapes root");
                }
                other => normalized.push(other.as_os_str()),
            }
        }
        // Resolve existing ancestors (including symlinks) even for a new file.
        let mut ancestor = normalized.as_path();
        let mut suffix = Vec::new();
        while !ancestor.exists() {
            suffix.push(
                ancestor
                    .file_name()
                    .context("cannot resolve path ancestor")?
                    .to_os_string(),
            );
            ancestor = ancestor.parent().context("cannot resolve path ancestor")?;
        }
        let mut resolved = std::fs::canonicalize(ancestor)?;
        for part in suffix.into_iter().rev() {
            resolved.push(part);
        }
        let mut lexical = PathBuf::new();
        for component in resolved.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    ensure!(lexical.pop(), "path escapes root");
                }
                other => lexical.push(other.as_os_str()),
            }
        }
        let root_key = checkout_key(&checkout)?;
        let resolved_key = normalize(&lexical.to_string_lossy());
        let relative = resolved_key
            .strip_prefix(&root_key)
            .context("path is outside checkout")?;
        ensure!(
            relative.is_empty() || relative.starts_with('/'),
            "path is outside checkout"
        );
        let segments = relative
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(Self::Path {
            checkout: root_key,
            segments,
        })
    }

    pub fn named(
        checkout: &Path,
        shared_scope: Option<&str>,
        namespace: &str,
        name: &str,
    ) -> Result<Self> {
        ensure!(
            !namespace.trim().is_empty() && !name.trim().is_empty(),
            "namespace and resource name are required"
        );
        let scope = match shared_scope {
            Some(scope) => {
                ensure!(
                    !scope.trim().is_empty(),
                    "shared host/environment scope is required"
                );
                Scope::Shared(scope.into())
            }
            None => Scope::Checkout(checkout_key(checkout)?),
        };
        Ok(Self::Named {
            scope,
            namespace: namespace.into(),
            name: name.into(),
        })
    }

    pub fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::Path {
                    checkout: a,
                    segments: x,
                },
                Self::Path {
                    checkout: b,
                    segments: y,
                },
            ) => a == b && (x.starts_with(y) || y.starts_with(x)),
            _ => self == other,
        }
    }
}
