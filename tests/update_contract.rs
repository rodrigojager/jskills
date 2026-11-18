use jtasks::cli::update::{self, Update};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn fixture() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("_work/00-tmp/update-tests")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&path).unwrap();
    path
}
fn release(version: &str, name: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"tag_name":format!("v{version}"),"draft":false,"prerelease":false,"assets":[
        {"name":name,"browser_download_url":format!("https://github.com/example/jtasks/releases/download/v{version}/{name}")},
        {"name":format!("{name}.sha256"),"browser_download_url":format!("https://github.com/example/jtasks/releases/download/v{version}/{name}.sha256")}
    ]})).unwrap()
}

#[test]
fn check_is_read_only_and_semver_never_downgrades() {
    for (version, check, available) in [
        ("0.1.1", true, true),
        ("0.1.0", false, false),
        ("0.0.9", false, false),
        ("0.10.0", true, true),
    ] {
        let calls = AtomicUsize::new(0);
        let result = update::perform(
            Update {
                check,
                repository: Some("example/jtasks".into()),
            },
            |url, _| {
                calls.fetch_add(1, Ordering::Relaxed);
                assert!(url.ends_with("/releases/latest"));
                Ok(release(version, "unused"))
            },
            std::path::Path::new("does-not-exist"),
            |_| panic!("check must never replace"),
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        assert_eq!(result["available"], available);
        assert_eq!(result["updated"], false);
    }
    assert!(update::repository(Some("owner/../repo")).is_err());
    assert!(update::repository(Some("https://github.com/owner/repo")).is_err());
    let prerelease: update::Release =
        serde_json::from_value(json!({"tag_name":"v9.0.0-beta.1","assets":[]})).unwrap();
    assert!(update::latest_version(&prerelease).is_err());
}

#[test]
fn bad_checksum_or_missing_asset_preserves_installation_before_execution() {
    let root = fixture();
    let current = root.join("installed");
    fs::write(&current, "original").unwrap();
    let name = format!(
        "jtasks-0.1.1-{}{}",
        update::target().unwrap(),
        if cfg!(windows) { ".exe" } else { ".bin" }
    );
    for missing in [false, true] {
        let result = update::perform(
            Update {
                check: false,
                repository: Some("example/jtasks".into()),
            },
            |url, _| {
                if url.ends_with("/latest") {
                    return Ok(release(
                        "0.1.1",
                        if missing { "wrong-target" } else { &name },
                    ));
                }
                if url.ends_with(".sha256") {
                    return Ok(format!("{}  {name}", "0".repeat(64)).into_bytes());
                }
                Ok(b"not executable".to_vec())
            },
            &current,
            |_| panic!("must not replace"),
        )
        .unwrap_err();
        assert!(!result.to_string().is_empty());
        assert_eq!(fs::read_to_string(&current).unwrap(), "original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
    }
    assert!(update::verify(b"bytes", b"no checksum", "file").is_err());
    fs::write(
        root.join("jtasks-installation.json"),
        r#"{"self_update":false,"reason":"Use owning package"}"#,
    )
    .unwrap();
    let result = update::perform(
        Update {
            check: false,
            repository: Some("example/jtasks".into()),
        },
        |url, _| {
            assert!(url.ends_with("/latest"));
            Ok(release("0.1.1", &name))
        },
        &current,
        |_| panic!("managed installation must not replace"),
    )
    .unwrap_err();
    assert!(result.to_string().contains("Use owning package"));
}

// Invoked only in a copied test executable; never replaces the cargo/test binary.
#[test]
fn replacement_child() {
    let Ok(directory) = std::env::var("JTASKS_UPDATE_CHILD") else {
        return;
    };
    let root = PathBuf::from(directory);
    let bytes = fs::read(root.join(if cfg!(windows) {
        "candidate.exe"
    } else {
        "candidate"
    }))
    .unwrap();
    let name = format!(
        "jtasks-0.1.1-{}{}",
        update::target().unwrap(),
        if cfg!(windows) { ".exe" } else { ".bin" }
    );
    let checksum = format!("{:x}  {name}", Sha256::digest(&bytes));
    let result = update::perform(
        Update {
            check: false,
            repository: Some("example/jtasks".into()),
        },
        |url, _| {
            Ok(if url.ends_with("/latest") {
                release("0.1.1", &name)
            } else if url.ends_with(".sha256") {
                checksum.as_bytes().to_vec()
            } else {
                bytes.clone()
            })
        },
        &std::env::current_exe().unwrap(),
        |file| Ok(self_replace::self_replace(file)?),
    )
    .unwrap();
    assert_eq!(result["updated"], true);
}

#[test]
fn verified_native_replacement_and_wrong_version_rejection() {
    let root = fixture();
    let source = root.join("candidate.rs");
    let candidate = root.join(if cfg!(windows) {
        "candidate.exe"
    } else {
        "candidate"
    });
    fs::write(&source, "fn main() { println!(\"jtasks 0.1.1\"); }").unwrap();
    assert!(
        Command::new("rustc")
            .arg(&source)
            .arg("-o")
            .arg(&candidate)
            .status()
            .unwrap()
            .success()
    );
    let bytes = fs::read(&candidate).unwrap();
    let original = root.join("original");
    fs::write(&original, "untouched").unwrap();
    assert!(
        update::install_verified(&original, &bytes, "0.1.2", |_| panic!(
            "wrong version must not replace"
        ))
        .is_err()
    );
    assert_eq!(fs::read_to_string(&original).unwrap(), "untouched");
    let child = root.join(if cfg!(windows) {
        "updater.exe"
    } else {
        "updater"
    });
    fs::copy(std::env::current_exe().unwrap(), &child).unwrap();
    let result = Command::new(&child)
        .args(["--exact", "replacement_child", "--nocapture"])
        .env("JTASKS_UPDATE_CHILD", &root)
        .env("TMP", &root)
        .env("TEMP", &root)
        .env("TMPDIR", &root)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let result = Command::new(&child).arg("--version").output().unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stdout).trim(),
        "jtasks 0.1.1"
    );
}
