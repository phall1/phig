#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, ExitStatus, PtySize, native_pty_system};

static PTY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_live(mut reader: Box<dyn Read + Send>) -> (Arc<Mutex<Vec<u8>>>, thread::JoinHandle<()>) {
    let output = Arc::new(Mutex::new(Vec::new()));
    let shared = Arc::clone(&output);
    let thread = thread::spawn(move || {
        let mut chunk = [0; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => shared.lock().unwrap().extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    (output, thread)
}

fn output_text(output: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8_lossy(&output.lock().unwrap()).into_owned()
}

fn wait_for_marker(output: &Arc<Mutex<Vec<u8>>>, marker: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        if output_text(output).contains(marker) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "PTY omitted marker {marker:?}: {}",
            output_text(output)
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_for_marker_count(
    output: &Arc<Mutex<Vec<u8>>>,
    marker: &str,
    minimum: usize,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    loop {
        if output_text(output).matches(marker).count() >= minimum {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "PTY omitted marker occurrence {minimum} for {marker:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn retry_key_until_exit(
    child: &mut Box<dyn Child + Send + Sync>,
    writer: &mut dyn Write,
    key: &[u8],
    timeout: Duration,
) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("phig ignored retryable exit key");
        }
        writer.write_all(key).unwrap();
        writer.flush().unwrap();
        thread::sleep(Duration::from_millis(40));
    }
}

fn assert_running_for(child: &mut Box<dyn Child + Send + Sync>, duration: Duration, context: &str) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "phig exited while {context}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_bounded(
    child: &mut Box<dyn Child + Send + Sync>,
    timeout: Duration,
    context: &str,
) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("phig timed out while {context}");
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn git(repo: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_view(repo: &std::path::Path, args: &[&str], ready: &str) -> String {
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 28,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.args(args);
    command.cwd(repo);
    command.env("TERM", "xterm-256color");
    command.env("NO_COLOR", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, reader_thread) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_marker(&output, ready, Duration::from_secs(5));
    let _ = retry_key_until_exit(&mut child, &mut writer, b"q", Duration::from_secs(8));
    drop(writer);
    drop(pair.master);
    reader_thread.join().unwrap();
    output_text(&output)
}

#[test]
fn all_daily_inspection_views_render_through_a_real_pty() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "base commit"]);
    git(repo.path(), &["checkout", "-qb", "feature"]);
    fs::write(repo.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(repo.path(), &["commit", "-qam", "feature commit"]);
    fs::write(repo.path().join("stash.txt"), "saved\n").unwrap();
    git(repo.path(), &["add", "stash.txt"]);
    git(repo.path(), &["stash", "push", "-qm", "saved work"]);
    fs::write(repo.path().join("file.txt"), "one\ntwo\nworking\n").unwrap();

    for (args, expected, ready) in [
        (
            vec!["compare", "main", "feature"],
            vec!["COMPARE", "merge-base"],
            "merge-base",
        ),
        (
            vec!["diff", "main", "feature"],
            vec!["COMPARE", "exact"],
            "exact",
        ),
        (vec!["refs"], vec!["REFS", "feature"], "branch"),
        (vec!["status"], vec!["STATUS", "file.txt"], "file.txt"),
        (vec!["tree", "HEAD"], vec!["TREE", "file.txt"], "file.txt"),
        (
            vec!["blame", "HEAD", "--", "file.txt"],
            vec!["BLAME", "one"],
            "one",
        ),
        (vec!["stash"], vec!["STASH", "stash@{0}"], "stash@{0}"),
    ] {
        let output = run_view(repo.path(), &args, ready);
        for needle in expected {
            assert!(
                output.contains(needle),
                "view {args:?} omitted {needle}: {output}"
            );
        }
        assert!(
            output.contains("\u{1b}[?25h"),
            "view {args:?} did not restore cursor"
        );
    }

    let config = repo.path().join("exact.toml");
    fs::write(&config, "version = 1\n[compare]\nmode = \"exact\"\n").unwrap();
    let output = run_view(
        repo.path(),
        &[
            "--config",
            config.to_str().unwrap(),
            "compare",
            "main",
            "feature",
        ],
        "merge-base",
    );
    assert!(
        output.contains("merge-base"),
        "explicit compare honored config: {output}"
    );
    assert!(
        !output.contains("COMPARE  exact"),
        "explicit compare became exact: {output}"
    );
}

#[test]
fn narrow_status_enter_opens_full_working_diff_and_returns() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "base"]);
    fs::write(repo.path().join("file.txt"), "base\nstaged\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    fs::write(repo.path().join("file.txt"), "base\nstaged\nunstaged\n").unwrap();

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 16,
            cols: 60,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.arg("status");
    command.cwd(repo.path());
    command.env("TERM", "xterm-256color");
    command.env("NO_COLOR", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, reader_thread) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_marker(&output, "Enter", Duration::from_secs(5));
    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "+staged", Duration::from_secs(5));
    let status = retry_key_until_exit(&mut child, &mut writer, b"q", Duration::from_secs(8));
    drop(writer);
    drop(pair.master);
    reader_thread.join().unwrap();
    let output = output_text(&output);
    assert!(status.success());
    assert!(
        output.contains("staged working"),
        "Enter did not open dominant status diff: {output}"
    );
    assert!(
        output.contains("+staged"),
        "staged working diff was not visible: {output}"
    );
    assert!(output.contains("mixed"));
    assert!(output.contains("MM"));
    assert!(output.contains("\u{1b}[?25h"));
}

#[test]
fn narrow_log_cannot_focus_an_invisible_preview() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "first commit"]);
    fs::write(repo.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(repo.path(), &["commit", "-qam", "second commit"]);
    let first_oid = Command::new("git")
        .args([
            "-C",
            repo.path().to_str().unwrap(),
            "rev-parse",
            "--short=8",
            "HEAD~1",
        ])
        .output()
        .unwrap();
    let first_oid = String::from_utf8(first_oid.stdout).unwrap();

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 16,
            cols: 60,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.cwd(repo.path());
    command.env("TERM", "xterm-256color");
    command.env("NO_COLOR", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, reader_thread) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_marker(&output, "second commit", Duration::from_secs(5));
    output.lock().unwrap().clear();
    writer.write_all(b"\tj\r").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "SHOW", Duration::from_secs(5));
    wait_for_marker(&output, first_oid.trim(), Duration::from_secs(5));
    let status = retry_key_until_exit(&mut child, &mut writer, b"q", Duration::from_secs(8));
    drop(writer);
    drop(pair.master);
    reader_thread.join().unwrap();
    assert!(status.success());
}

#[test]
fn real_pty_exercises_navigation_overlays_resize_and_cleanup() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "first commit"]);
    git(repo.path(), &["branch", "side"]);
    fs::write(repo.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(repo.path(), &["commit", "-qam", "second commit"]);
    git(repo.path(), &["checkout", "-q", "side"]);
    fs::write(repo.path().join("side.txt"), "side\n").unwrap();
    git(repo.path(), &["add", "side.txt"]);
    git(repo.path(), &["commit", "-qm", "side commit"]);
    git(repo.path(), &["checkout", "-q", "main"]);
    git(
        repo.path(),
        &["merge", "-q", "--no-ff", "side", "-m", "merge side branch"],
    );

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 28,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.cwd(repo.path());
    command.env("TERM", "xterm-256color");
    command.env("NO_COLOR", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().unwrap();
    let (output, reader_thread) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();

    let merge_oid = Command::new("git")
        .args([
            "-C",
            repo.path().to_str().unwrap(),
            "rev-parse",
            "--short=8",
            "HEAD",
        ])
        .output()
        .unwrap();
    let merge_oid = String::from_utf8(merge_oid.stdout).unwrap();
    wait_for_marker(&output, merge_oid.trim(), Duration::from_secs(5));
    writer.write_all(b"\r").unwrap(); // inspect the selected merge commit
    writer.flush().unwrap();
    wait_for_marker(&output, "SHOW", Duration::from_secs(5));
    wait_for_marker(&output, "+side", Duration::from_secs(5));
    writer.write_all(b"f").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "Changed", Duration::from_secs(3));
    writer.write_all(b"side\r").unwrap(); // filter and jump through the file index
    writer.write_all(b"]}").unwrap(); // hunk and file navigation
    writer.write_all(b"/\x1b[200~first\x1b[201~\r").unwrap(); // pasted diff search
    writer.write_all(b":help\r").unwrap(); // palette -> contextual help
    writer.flush().unwrap();
    wait_for_marker(&output, "Help", Duration::from_secs(5));
    writer.write_all(b"\x1b").unwrap(); // close help
    writer.flush().unwrap();
    pair.master
        .resize(PtySize {
            rows: 20,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let status = retry_key_until_exit(&mut child, &mut writer, b"q", Duration::from_secs(8));
    drop(writer);
    drop(pair.master);
    reader_thread.join().unwrap();
    let screen = output_text(&output);

    assert!(status.success(), "phig exited unsuccessfully: {status:?}");
    assert!(screen.contains("phig"));
    assert!(screen.contains(merge_oid.trim()));
    assert!(
        screen.contains('◆'),
        "merge topology marker was not rendered"
    );
    assert!(
        screen.contains("SHOW"),
        "Enter did not render commit detail"
    );
    assert!(screen.contains("+side"), "commit diff was not rendered");
    assert!(screen.contains("Help"), "help overlay was not rendered");
    assert!(screen.contains("\u{1b}[?25h"), "cursor was not restored");
    assert!(
        screen.contains("\u{1b}[?1049l"),
        "alternate screen was not restored"
    );
}

#[test]
fn remapped_printable_key_still_types_in_every_text_overlay() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "overlay input"]);
    let config = repo.path().join("remap.toml");
    fs::write(&config, "version = 1\n[keys]\nquit = \"x\"\nhelp = \"h\"\n").unwrap();

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 28,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.args(["--config", config.to_str().unwrap()]);
    command.cwd(repo.path());
    command.env("TERM", "xterm-256color");
    command.env("NO_COLOR", "1");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, reader_thread) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();

    wait_for_marker(&output, "a/file.txt", Duration::from_secs(5));
    writer.write_all(b"/").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "cancel", Duration::from_secs(3));
    writer.write_all(b"x").unwrap();
    writer.flush().unwrap();
    assert_running_for(
        &mut child,
        Duration::from_millis(150),
        "remapped x quit instead of typing in search",
    );
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));

    writer.write_all(b":").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "Commands", Duration::from_secs(3));
    writer.write_all(b"x").unwrap();
    writer.flush().unwrap();
    assert_running_for(
        &mut child,
        Duration::from_millis(150),
        "remapped x quit instead of typing in palette",
    );
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));

    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "SHOW", Duration::from_secs(5));
    writer.write_all(b"f").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "Changed", Duration::from_secs(3));
    writer.write_all(b"x").unwrap();
    writer.flush().unwrap();
    assert_running_for(
        &mut child,
        Duration::from_millis(150),
        "remapped x quit instead of typing in file picker",
    );
    writer.write_all(b"\x7f\x1b[200~x\x1b[201~").unwrap();
    writer.flush().unwrap();
    assert_running_for(
        &mut child,
        Duration::from_millis(150),
        "bracketed paste failed in file picker",
    );
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));

    writer.write_all(b"h").unwrap();
    writer.flush().unwrap();
    wait_for_marker(&output, "Help", Duration::from_secs(3));
    writer.write_all(b"h").unwrap(); // configured help key closes the overlay
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"?").unwrap(); // old help key is disabled by the remap
    writer.flush().unwrap();
    thread::sleep(Duration::from_millis(100));
    // From SHOW, two configured quits return to LOG and exit. If the old `?`
    // reopened Help, the first x would only close it and the process would remain.
    writer.write_all(b"xx").unwrap();
    writer.flush().unwrap();
    let status = wait_bounded(
        &mut child,
        Duration::from_secs(8),
        "waiting for remapped quits after closing help",
    );
    drop(writer);
    drop(pair.master);
    reader_thread.join().unwrap();
    assert!(status.success());
    let screen = output_text(&output);
    for marker in ["cancel", "Commands", "Changed"] {
        assert!(
            screen.contains(marker),
            "overlay omitted {marker:?}: {screen}"
        );
    }
}

#[test]
fn no_alt_screen_mode_leaves_scrollback_and_restores_cursor() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "path ancestor"]);
    fs::write(repo.path().join("other.txt"), "two\n").unwrap();
    git(repo.path(), &["add", "other.txt"]);
    git(repo.path(), &["commit", "-qm", "exact show target"]);

    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 20,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
    command.args(["--no-alt-screen", "show", "HEAD", "--", "file.txt"]);
    command.cwd(repo.path());
    command.env("TERM", "xterm-256color");
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);
    let reader = pair.master.try_clone_reader().unwrap();
    let (output, reader_thread) = read_live(reader);
    let mut writer = pair.master.take_writer().unwrap();
    wait_for_marker(&output, "exact show target", Duration::from_secs(5));
    let status = retry_key_until_exit(&mut child, &mut writer, b"q", Duration::from_secs(8));
    drop(writer);
    drop(pair.master);
    reader_thread.join().unwrap();
    let screen = output_text(&output);

    assert!(status.success());
    assert!(screen.contains("phig"));
    assert!(
        screen.contains("exact show target"),
        "show REV -- PATH selected an ancestor instead of REV"
    );
    assert!(screen.contains("\u{1b}[?25h"));
    assert!(!screen.contains("\u{1b}[?1049h"));
    assert!(!screen.contains("\u{1b}[?1049l"));
}

#[test]
fn clipboard_defaults_to_osc52_and_explicit_off_reports_disabled() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "copy commit"]);
    let disabled = repo.path().join("clipboard-off.toml");
    fs::write(&disabled, "version = 1\n[ui]\nclipboard = \"off\"\n").unwrap();

    for (extra, marker, osc52) in [
        (Vec::<String>::new(), "]52;c;", true),
        (
            vec!["--config".into(), disabled.to_string_lossy().into_owned()],
            "disabled",
            false,
        ),
    ] {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 20,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
        command.args(extra);
        command.arg("--no-alt-screen");
        command.cwd(repo.path());
        command.env("TERM", "xterm-256color");
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().unwrap();
        let (output, reader_thread) = read_live(reader);
        let mut writer = pair.master.take_writer().unwrap();
        wait_for_marker(&output, "copy commit", Duration::from_secs(5));
        writer.write_all(b"y").unwrap();
        writer.flush().unwrap();
        wait_for_marker(&output, marker, Duration::from_secs(3));
        let status = retry_key_until_exit(&mut child, &mut writer, b"q", Duration::from_secs(5));
        assert!(status.success());
        drop(writer);
        drop(pair.master);
        reader_thread.join().unwrap();
        let screen = output_text(&output);
        assert_eq!(screen.contains("]52;c;"), osc52);
    }
}

#[test]
fn external_termination_signal_restores_the_terminal() {
    let _guard = PTY_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Phig PTY"]);
    git(
        repo.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    fs::write(repo.path().join("file.txt"), "one\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-qm", "signal commit"]);

    for (signal_name, expected_code, exercise_suspend) in [
        ("-INT", 130, false),
        ("-TERM", 143, true),
        ("-HUP", 129, false),
    ] {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 20,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("phig"));
        command.cwd(repo.path());
        command.env("TERM", "xterm-256color");
        let mut child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let process_id = child.process_id().expect("PTY child has a process id");
        let reader = pair.master.try_clone_reader().unwrap();
        let (output, reader_thread) = read_live(reader);
        let send_signal = |name: &str| {
            let signal = Command::new("kill")
                .args([name, &process_id.to_string()])
                .status()
                .unwrap();
            assert!(signal.success(), "failed to send {name}");
        };

        wait_for_marker(&output, "signal commit", Duration::from_secs(5));
        if exercise_suspend {
            send_signal("-TSTP");
            wait_for_marker_count(&output, "\u{1b}[?1049l", 1, Duration::from_secs(3));
            send_signal("-CONT");
            wait_for_marker_count(&output, "\u{1b}[?1049h", 2, Duration::from_secs(3));
        }
        send_signal(signal_name);
        let status = wait_bounded(
            &mut child,
            Duration::from_secs(5),
            "waiting for signal exit",
        );
        drop(pair.master);
        reader_thread.join().unwrap();
        let screen = output_text(&output);

        assert_eq!(
            status.exit_code(),
            expected_code,
            "wrong exit for {signal_name}"
        );
        let minimum = if exercise_suspend { 2 } else { 1 };
        assert!(
            screen.matches("\u{1b}[?25h").count() >= minimum,
            "cursor was not restored for {signal_name}"
        );
        assert!(
            screen.matches("\u{1b}[?1049l").count() >= minimum,
            "alternate screen was not restored for {signal_name}"
        );
        if exercise_suspend {
            assert!(
                screen.matches("\u{1b}[?1049h").count() >= 2,
                "terminal was not reclaimed after SIGCONT"
            );
        }
    }
}
