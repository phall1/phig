use std::{fs, path::Path, process::Command};

use assert_cmd::{Command as AssertCommand, cargo::cargo_bin_cmd};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use predicates::prelude::*;
use serde_json::Value;

#[test]
fn release_build_environment_keeps_macos_12_floor() {
    assert_eq!(option_env!("MACOSX_DEPLOYMENT_TARGET"), Some("12.0"));
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
fn repo() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.name", "Compose"]);
    git(
        d.path(),
        &["config", "user.email", "compose@example.invalid"],
    );
    fs::write(d.path().join("file.txt"), "one\n").unwrap();
    fs::write(d.path().join("alpha.txt"), "alpha\n").unwrap();
    fs::write(d.path().join("zeta.txt"), "zeta\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-qm", "base"]);
    git(d.path(), &["checkout", "-qb", "feature"]);
    fs::write(d.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(d.path(), &["commit", "-qam", "feature"]);
    fs::write(d.path().join("stash.txt"), "stash\n").unwrap();
    git(d.path(), &["add", "stash.txt"]);
    git(d.path(), &["stash", "push", "-qm", "saved"]);
    fs::write(d.path().join("file.txt"), "one\ntwo\nworking\n").unwrap();
    d
}
fn validate_protocol(value: &Value) {
    let schema_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/schema/phig-1.schema.json");
    let schema: Value = serde_json::from_slice(&fs::read(&schema_path).unwrap()).unwrap();
    assert_eq!(schema["oneOf"].as_array().unwrap().len(), 3);
    assert_eq!(
        schema["$defs"]["sha1Oid"]["properties"]["hex"]["pattern"],
        "^[0-9a-f]{40}$"
    );
    jsonschema::draft202012::meta::validate(&schema)
        .unwrap_or_else(|error| panic!("invalid protocol schema: {error}"));
    jsonschema::draft202012::validate(&schema, value)
        .unwrap_or_else(|error| panic!("schema validation failed: {error}"));
}

fn snapshot(repo: &Path, args: &[&str]) -> Value {
    let out = AssertCommand::new(assert_cmd::cargo::cargo_bin!("phig"))
        .arg("--repo")
        .arg(repo)
        .arg("snapshot")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    assert_eq!(out.stdout.last(), Some(&b'\n'));
    assert!(!out.stdout.contains(&0x1b));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["protocol"], "phig/1");
    assert_eq!(v["kind"], "snapshot");
    validate_protocol(&v);
    v
}

#[test]
fn protocol_golden_fixtures_match_the_versioned_schema_contract() {
    for fixture in ["version.json", "snapshot.json", "selection.json"] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let value: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(value["protocol"], "phig/1");
        validate_protocol(&value);
    }
}

#[test]
fn every_snapshot_target_is_clean_bounded_json() {
    let d = repo();
    for (args, target) in [
        (&["log"][..], "log"),
        (&["show", "HEAD"][..], "show"),
        (&["compare", "main", "feature"][..], "compare"),
        (&["diff", "main", "feature"][..], "diff"),
        (&["refs"][..], "refs"),
        (&["status"][..], "status"),
        (&["tree", "HEAD"][..], "tree"),
        (&["blame", "HEAD", "--", "file.txt"][..], "blame"),
        (&["stash"][..], "stash"),
    ] {
        let v = snapshot(d.path(), args);
        assert_eq!(v["payload"]["target"], target);
        assert!(v["payload"].get("truncated").is_some());
    }
}

#[test]
fn snapshot_offsets_are_consumable_and_singletons_reject_them() {
    let d = repo();
    fs::write(d.path().join("untracked-a"), "a").unwrap();
    fs::write(d.path().join("untracked-b"), "b").unwrap();
    let cfg = d.path().join("page.toml");
    fs::write(&cfg, "version=1\n[limits]\nsnapshot_items=1\n").unwrap();
    let run = |args: &[&str]| {
        let output = AssertCommand::new(assert_cmd::cargo::cargo_bin!("phig"))
            .arg("--config")
            .arg(&cfg)
            .arg("--repo")
            .arg(d.path())
            .arg("snapshot")
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    for (target, data_pointer) in [
        ("log", "/payload/data/commits/0/id/hex"),
        ("refs", "/payload/data/0/target/hex"),
        ("status", "/payload/data/entries/0/path/bytesBase64"),
        ("tree", "/payload/data/0/path/bytesBase64"),
        ("blame", "/payload/data/0/final_line"),
    ] {
        let first_args = if target == "blame" {
            vec![target, "HEAD", "--", "file.txt"]
        } else {
            vec![target]
        };
        let first = run(&first_args);
        assert_eq!(first["payload"]["offset"], 0);
        let next = first["payload"]["continuation"].as_u64().unwrap();
        let offset = next.to_string();
        let second_args = if target == "blame" {
            vec![target, "--offset", &offset, "HEAD", "--", "file.txt"]
        } else {
            vec![target, "--offset", &offset]
        };
        let second = run(&second_args);
        assert_eq!(second["payload"]["offset"], next);
        assert_ne!(
            first.pointer(data_pointer),
            second.pointer(data_pointer),
            "{target}"
        );
    }
    let stash = run(&["stash", "--offset", "1", "--format", "json"]);
    assert_eq!(stash["payload"]["offset"], 1);
    assert_eq!(stash["payload"]["data"].as_array().unwrap().len(), 0);
    let format_before = run(&["--format", "json", "refs"]);
    assert_eq!(format_before["payload"]["target"], "refs");

    cargo_bin_cmd!("phig")
        .arg("--repo")
        .arg(d.path())
        .args(["snapshot", "show", "--offset", "1", "HEAD"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("does not accept"));
}

#[test]
fn version_completions_and_manpage_are_pipe_clean() {
    let out = cargo_bin_cmd!("phig")
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["protocol"], "phig/1");
    validate_protocol(&v);
    for shell in ["bash", "zsh", "fish", "elvish", "powershell"] {
        cargo_bin_cmd!("phig")
            .args(["completions", shell])
            .assert()
            .success()
            .stdout(predicate::str::is_empty().not());
    }
    cargo_bin_cmd!("phig")
        .arg("manpage")
        .assert()
        .success()
        .stdout(
            predicate::str::contains(".TH PHIG 1").and(predicate::str::contains("phig Manual")),
        );
    let directory = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("phig")
        .args(["manpage", "--output-dir"])
        .arg(directory.path())
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    for page in ["phig.1", "phig-snapshot.1", "phig-config-check.1"] {
        assert!(directory.path().join(page).is_file(), "missing {page}");
    }
    let binary = assert_cmd::cargo::cargo_bin!("phig");
    let pipeline = format!("set -o pipefail; '{}' manpage | true", binary.display());
    let status = Command::new("bash")
        .args(["-c", &pipeline])
        .status()
        .unwrap();
    assert!(status.success(), "BrokenPipe was not handled cleanly");

    if Command::new("mandoc").arg("-V").output().is_ok() {
        for entry in fs::read_dir(directory.path()).unwrap() {
            let path = entry.unwrap().path();
            let output = Command::new("mandoc")
                .args(["-T", "lint"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "mandoc {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}

#[test]
fn xdg_and_environment_config_precedence_is_exact() {
    let d = tempfile::tempdir().unwrap();
    let xdg = d.path().join("xdg");
    let home = d.path().join("home");
    let expected_xdg = xdg.join("phig/config.toml");
    cargo_bin_cmd!("phig")
        .args(["config", "path"])
        .env("XDG_CONFIG_HOME", &xdg)
        .env("HOME", &home)
        .env_remove("PHIG_CONFIG")
        .assert()
        .success()
        .stdout(format!("{}\n", expected_xdg.display()));
    cargo_bin_cmd!("phig")
        .args(["config", "path"])
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", &home)
        .env_remove("PHIG_CONFIG")
        .assert()
        .success()
        .stdout(format!(
            "{}\n",
            home.join(".config/phig/config.toml").display()
        ));

    let explicit = d.path().join("from-env.toml");
    fs::write(&explicit, "version=1\n[limits]\nsnapshot_items=1\n").unwrap();
    cargo_bin_cmd!("phig")
        .args(["config", "check"])
        .env("PHIG_CONFIG", &explicit)
        .assert()
        .success()
        .stdout(predicate::str::contains(explicit.display().to_string()));
    let r = repo();
    let output = cargo_bin_cmd!("phig")
        .arg("--repo")
        .arg(r.path())
        .args(["snapshot", "refs"])
        .env("PHIG_CONFIG", &explicit)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["payload"]["data"].as_array().unwrap().len(), 1);
    fs::write(&explicit, "broken=true\n").unwrap();
    cargo_bin_cmd!("phig")
        .args(["--no-config", "config", "check"])
        .env("PHIG_CONFIG", &explicit)
        .env_remove("HOME")
        .env_remove("XDG_CONFIG_HOME")
        .assert()
        .success()
        .stdout(predicate::str::contains("built-in defaults"));
    cargo_bin_cmd!("phig")
        .args(["config", "check"])
        .env("PHIG_CONFIG", d.path().join("missing.toml"))
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("does not exist"));
}

#[test]
fn config_init_check_strictness_and_no_config_recovery() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("config.toml");
    cargo_bin_cmd!("phig")
        .args(["config", "check"])
        .env("XDG_CONFIG_HOME", d.path().join("absent"))
        .env_remove("PHIG_CONFIG")
        .assert()
        .success()
        .stdout(predicate::str::contains("built-in defaults"));
    cargo_bin_cmd!("phig")
        .arg("--config")
        .arg(&path)
        .args(["config", "init"])
        .assert()
        .success()
        .stdout(predicate::str::contains("config.toml"));
    cargo_bin_cmd!("phig")
        .arg("--config")
        .arg(&path)
        .args(["config", "check"])
        .assert()
        .success();
    fs::write(&path, "[ui]\npreview=true\n").unwrap();
    cargo_bin_cmd!("phig")
        .arg("--config")
        .arg(&path)
        .args(["config", "check"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("missing field `version`"));
    fs::write(&path, "version=1\n[ui]\nunknown=true\n").unwrap();
    cargo_bin_cmd!("phig")
        .arg("--config")
        .arg(&path)
        .args(["config", "check"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains("unknown field").and(predicate::str::contains("config.toml")),
        );
    let r = repo();
    cargo_bin_cmd!("phig")
        .arg("--no-config")
        .arg("--repo")
        .arg(r.path())
        .args(["snapshot", "log"])
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_are_exact_and_helpers_never_run() {
    use std::os::unix::ffi::OsStringExt;
    let d = repo();
    let raw = vec![b'n', 0xff, b'x'];
    if fs::write(
        d.path().join(std::ffi::OsString::from_vec(raw.clone())),
        "x",
    )
    .is_err()
    {
        // macOS filesystems reject arbitrary non-UTF-8 names; Linux exercises
        // the end-to-end path while domain golden tests cover encoding here.
        return;
    }
    let marker = d.path().join("helper-ran");
    let helper = d.path().join("helper.sh");
    fs::write(
        &helper,
        format!("#!/bin/sh\ntouch '{}'\nexit 99\n", marker.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
    let out = AssertCommand::new(assert_cmd::cargo::cargo_bin!("phig"))
        .arg("--repo")
        .arg(d.path())
        .args(["snapshot", "status"])
        .env("GIT_EXTERNAL_DIFF", &helper)
        .env("GIT_PAGER", &helper)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!marker.exists());
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    let entries = v["payload"]["data"]["entries"].as_array().unwrap();
    let encoded = entries
        .iter()
        .find_map(|e| e["path"]["bytesBase64"].as_str())
        .unwrap();
    assert!(
        entries
            .iter()
            .any(|e| e["path"]["bytesBase64"] == STANDARD.encode(&raw))
    );
    assert!(!encoded.is_empty());
}

#[test]
fn select_without_a_controlling_terminal_is_clean_unsupported_context() {
    let d = repo();
    cargo_bin_cmd!("phig")
        .arg("--repo")
        .arg(d.path())
        .args(["select", "--kind", "commit", "--format", "json"])
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("controlling terminal"));
}

#[test]
fn explicit_config_overrides_diff_and_snapshot_limits() {
    let d = repo();
    let cfg = d.path().join("phig.toml");
    fs::write(&cfg,"version=1\n[diff]\ncontext=0\nalgorithm='minimal'\nwhitespace='show'\n[limits]\nsnapshot_items=1\n").unwrap();
    let out = AssertCommand::new(assert_cmd::cargo::cargo_bin!("phig"))
        .arg("--config")
        .arg(&cfg)
        .arg("--repo")
        .arg(d.path())
        .args(["snapshot", "refs"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["payload"]["data"].as_array().unwrap().len(), 1);
    assert_eq!(v["payload"]["truncated"], true);
}
