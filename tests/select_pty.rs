#![cfg(unix)]

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    fs,
    io::{Read, Write},
    process::Command,
    sync::{Arc, Mutex},
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
    let normalized = output.to_ascii_lowercase();
    for part in ["select", kind, accept, "emit", cancel, "cancel"] {
        let part = part.to_ascii_lowercase();
        assert!(
            normalized.contains(&part),
            "selection prompt omitted {part:?}: {output:?}"
        );
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

fn wait_for_output(output: &Arc<Mutex<Vec<u8>>>, marker: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let snapshot = output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if String::from_utf8_lossy(&snapshot).contains(marker) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "PTY omitted readiness marker {marker:?}: {:?}",
            String::from_utf8_lossy(&snapshot)
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn read_live(mut reader: Box<dyn Read + Send>) -> (Arc<Mutex<Vec<u8>>>, thread::JoinHandle<()>) {
    let output = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&output);
    let thread = thread::spawn(move || {
        let mut chunk = [0; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => shared
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    (output, thread)
}

fn retry_key_until_exit(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    writer: &mut dyn Write,
    key: &[u8],
    timeout: Duration,
) -> portable_pty::ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("PTY child ignored retryable exit key");
        }
        writer.write_all(key).unwrap();
        writer.flush().unwrap();
        thread::sleep(Duration::from_millis(40));
    }
}

fn assert_running_for(
    child: &mut Box<dyn portable_pty::Child + Send + Sync>,
    duration: Duration,
    context: &str,
) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        assert!(child.try_wait().unwrap().is_none(), "{context}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn run_script(repo: &std::path::Path, script: &str, ready: &str, key: u8) -> String {
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
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, read) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_output(&output, ready, Duration::from_secs(5));
    if key == b'q' {
        let _ = retry_key_until_exit(&mut child, &mut writer, &[key], Duration::from_secs(8));
    } else {
        writer.write_all(&[key]).unwrap();
        writer.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if child.try_wait().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                panic!("selection timed out after readiness marker {ready:?}")
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
    drop(writer);
    drop(pair.master);
    read.join().unwrap();
    String::from_utf8_lossy(
        &output
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
    .into_owned()
}

#[test]
fn command_substitution_reserves_stdout_for_exact_selection() {
    let d = repository();
    let bin = assert_cmd::cargo::cargo_bin!("phig");
    let script = format!(
        "value=\"$('{}' --no-alt-screen select --kind commit --format oid)\"; rc=$?; printf '\\nRESULT:%s:%s\\n' \"$rc\" \"$value\"",
        bin.display()
    );
    let output = run_script(d.path(), &script, "two (HEAD", b'\r');
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
    let blame = Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "blame",
            "--porcelain",
            "HEAD",
            "--",
            "b",
        ])
        .output()
        .unwrap();
    let blame_oid = String::from_utf8(blame.stdout)
        .unwrap()
        .split_ascii_whitespace()
        .next()
        .unwrap()
        .chars()
        .take(8)
        .collect::<String>();
    for (kind, extra, ready) in [
        ("ref", "", "branch"),
        ("file", "", "similarity"),
        ("hunk", "", "similarity"),
        ("line", " -- b", "blame-oid"),
        ("compare", " main --base feature", "ahead"),
    ] {
        let script = format!(
            "value=\"$('{}' --no-alt-screen select --kind {kind} --format json{extra})\"; rc=$?; printf '\\nRESULT:%s:%s\\n' \"$rc\" \"$value\"",
            bin.display()
        );
        let ready = if kind == "line" { &blame_oid } else { ready };
        let output = run_script(d.path(), &script, ready, b'\r');
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
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, read) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_output(&output, "phig", Duration::from_secs(5));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    assert_running_for(
        &mut child,
        Duration::from_millis(250),
        "old q alias still quit",
    );
    writer.write_all(b"h").unwrap();
    writer.flush().unwrap();
    wait_for_output(&output, "Help", Duration::from_secs(3));
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    let status = retry_key_until_exit(&mut child, &mut writer, b"x", Duration::from_secs(5));
    assert_eq!(status.exit_code(), 0);
    drop(writer);
    drop(pair.master);
    read.join().unwrap();
    let output = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
    assert!(
        output.contains("Help"),
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
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, read) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_output(&output, "select commit", Duration::from_secs(5));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    assert_running_for(
        &mut child,
        Duration::from_millis(250),
        "select retained the old q cancellation alias",
    );
    let status = retry_key_until_exit(&mut child, &mut writer, b"x", Duration::from_secs(5));
    assert_eq!(status.exit_code(), 1, "selection cancellation exit changed");
    drop(writer);
    drop(pair.master);
    read.join().unwrap();
    let output = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
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
    let output = run_script(d.path(), &script, "two (HEAD", b'q');
    assert_selection_prompt(&output, "COMMIT", "Enter", "Esc/q");
    assert!(output.contains("RESULT:1:0"), "{output:?}");
}
