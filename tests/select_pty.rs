#![cfg(unix)]

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    fs,
    io::{Read, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn git(repo: &std::path::Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
fn repository() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-q", "-b", "main"]);
    git(d.path(), &["config", "user.name", "Select"]);
    git(
        d.path(),
        &["config", "user.email", "select@example.invalid"],
    );
    fs::write(d.path().join("a"), "a\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-qm", "one"]);
    git(d.path(), &["checkout", "-qb", "feature"]);
    git(d.path(), &["mv", "a", "b"]);
    fs::write(d.path().join("b"), "a\nb\n").unwrap();
    git(d.path(), &["commit", "-qam", "two"]);
    d
}
fn assert_selection_prompt(output: &str, kind: &str, accept: &str, cancel: &str) {
    let mut remaining = output;
    for part in ["SELECT", kind, accept, "emit", cancel, "cancel"] {
        let offset = remaining
            .find(part)
            .unwrap_or_else(|| panic!("selection prompt omitted {part:?}: {output:?}"));
        remaining = &remaining[offset + part.len()..];
    }
}

fn validate_protocol(value: &serde_json::Value) {
    let schema_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/schema/phig-1.schema.json");
    let schema: serde_json::Value =
        serde_json::from_slice(&fs::read(schema_path).unwrap()).unwrap();
    jsonschema::draft202012::validate(&schema, value)
        .unwrap_or_else(|error| panic!("schema validation failed: {error}"));
}

fn run_script(repo: &std::path::Path, script: &str, key: u8) -> String {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut cmd = CommandBuilder::new("/bin/sh");
    cmd.args(["-c", script]);
    cmd.cwd(repo);
    cmd.env("TERM", "xterm-256color");
    cmd.env("NO_COLOR", "1");
    let mut child = pair.slave.spawn_command(cmd).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let read = thread::spawn(move || {
        let mut v = Vec::new();
        reader.read_to_end(&mut v).unwrap();
        v
    });
    let mut writer = pair.master.take_writer().unwrap();
    thread::sleep(Duration::from_millis(800));
    writer.write_all(&[key]).unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("selection timed out")
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(writer);
    drop(pair.master);
    String::from_utf8_lossy(&read.join().unwrap()).into_owned()
}

#[test]
fn command_substitution_reserves_stdout_for_exact_selection() {
    let d = repository();
    let bin = assert_cmd::cargo::cargo_bin!("phig");
    let script = format!(
        "value=\"$('{}' --no-alt-screen select --kind commit --format oid)\"; rc=$?; printf '\\nRESULT:%s:%s\\n' \"$rc\" \"$value\"",
        bin.display()
    );
    let output = run_script(d.path(), &script, b'\r');
    assert_selection_prompt(&output, "COMMIT", "Enter", "Esc/q");
    let marker = output.rsplit("RESULT:").next().unwrap();
    let mut fields = marker.lines().next().unwrap().split(':');
    assert_eq!(fields.next(), Some("0"));
    let oid = fields.next().unwrap();
    assert_eq!(oid.len(), 40);
    assert!(oid.bytes().all(|b| b.is_ascii_hexdigit()));
}

#[test]
fn every_selection_kind_returns_a_versioned_exact_locator() {
    let d = repository();
    let bin = assert_cmd::cargo::cargo_bin!("phig");
    for (kind, extra) in [
        ("ref", ""),
        ("file", ""),
        ("hunk", ""),
        ("line", " -- b"),
        ("compare", " main --base feature"),
    ] {
        let script = format!(
            "value=\"$('{}' --no-alt-screen select --kind {kind} --format json{extra})\"; rc=$?; printf '\\nRESULT:%s:%s\\n' \"$rc\" \"$value\"",
            bin.display()
        );
        let output = run_script(d.path(), &script, b'\r');
        assert_selection_prompt(&output, &kind.to_ascii_uppercase(), "Enter", "Esc/q");
        let result = output.rsplit("RESULT:0:").next().unwrap();
        let json = result.lines().next().unwrap();
        let value: serde_json::Value =
            serde_json::from_str(json).unwrap_or_else(|_| panic!("{kind}: {output:?}"));
        assert_eq!(value["protocol"], "phig/1");
        assert_eq!(value["payload"]["kind"], kind);
        assert!(value["payload"].get("repository").is_some());
        validate_protocol(&value);
        if kind == "compare" {
            let expected = Command::new("git")
                .arg("-C")
                .arg(d.path())
                .args(["rev-parse", "main"])
                .output()
                .unwrap();
            assert_eq!(
                value["payload"]["compare"]["head"]["hex"],
                String::from_utf8(expected.stdout).unwrap().trim()
            );
        }
        if kind == "line" {
            assert_eq!(value["payload"]["path"]["display"], "b");
        }
    }
}

#[test]
fn configured_keys_are_remaps_not_additive_aliases() {
    let d = repository();
    let config = d.path().join("keys.toml");
    fs::write(&config, "version=1\n[keys]\nquit='x'\nhelp='h'\n").unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.args(["--no-alt-screen", "--config"]);
    command.arg(config);
    command.cwd(d.path());
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let read = thread::spawn(move || {
        let mut value = Vec::new();
        reader.read_to_end(&mut value).unwrap();
        value
    });
    let mut writer = pair.master.take_writer().unwrap();
    thread::sleep(Duration::from_millis(500));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(200));
    assert!(
        child.try_wait().unwrap().is_none(),
        "old q alias still quit"
    );
    writer.write_all(b"h").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"x").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("remapped quit key did not exit");
        }
        thread::sleep(Duration::from_millis(20));
    }
    drop(writer);
    drop(pair.master);
    let output = String::from_utf8_lossy(&read.join().unwrap()).into_owned();
    assert!(
        output.contains("phig keys"),
        "remapped help key did not open help"
    );
}

#[test]
fn selection_cancellation_honors_semantic_quit_remap() {
    let d = repository();
    let config = d.path().join("select-keys.toml");
    fs::write(&config, "version=1\n[keys]\nquit='x'\n").unwrap();
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.args([
        "--no-alt-screen",
        "--config",
        config.to_str().unwrap(),
        "select",
        "--kind",
        "commit",
    ]);
    command.cwd(d.path());
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let read = thread::spawn(move || {
        let mut value = Vec::new();
        reader.read_to_end(&mut value).unwrap();
        value
    });
    let mut writer = pair.master.take_writer().unwrap();
    thread::sleep(Duration::from_millis(500));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(200));
    assert!(
        child.try_wait().unwrap().is_none(),
        "select retained the old q cancellation alias"
    );
    writer.write_all(b"x").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("remapped select quit key did not cancel");
        }
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.exit_code(), 1, "selection cancellation exit changed");
    drop(writer);
    drop(pair.master);
    let output = String::from_utf8_lossy(&read.join().unwrap()).into_owned();
    assert_selection_prompt(&output, "COMMIT", "Enter", "Esc/x");
}

#[test]
fn selection_cancel_is_exit_one_with_empty_stdout() {
    let d = repository();
    let bin = assert_cmd::cargo::cargo_bin!("phig");
    let script = format!(
        "value=\"$('{}' --no-alt-screen select --kind commit --format json)\"; rc=$?; printf '\\nRESULT:%s:%s\\n' \"$rc\" \"${{#value}}\"",
        bin.display()
    );
    let output = run_script(d.path(), &script, b'q');
    assert_selection_prompt(&output, "COMMIT", "Enter", "Esc/q");
    assert!(output.contains("RESULT:1:0"), "{output:?}");
}
