#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use portable_pty::{Child, CommandBuilder, ExitStatus, PtySize, native_pty_system};

static PTY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

fn run_view(repo: &std::path::Path, args: &[&str]) -> String {
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
    let mut reader = pair.master.try_clone_reader().unwrap();
    let reader_thread = thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        output
    });
    let mut writer = pair.master.take_writer().unwrap();
    thread::sleep(Duration::from_millis(800));
    writer.write_all(b"qq").unwrap();
    writer.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            panic!("phig view {args:?} did not exit");
        }
        thread::sleep(Duration::from_millis(25));
    }
    drop(writer);
    drop(pair.master);
    String::from_utf8_lossy(&reader_thread.join().unwrap()).into_owned()
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

    for (args, expected) in [
        (
            vec!["compare", "main", "feature"],
            vec!["COMPARE", "merge-base"],
        ),
        (vec!["diff", "main", "feature"], vec!["COMPARE", "exact"]),
        (vec!["refs"], vec!["REFS", "feature"]),
        (vec!["status"], vec!["STATUS", "file.txt"]),
        (vec!["tree", "HEAD"], vec!["TREE", "file.txt"]),
        (
            vec!["blame", "HEAD", "--", "file.txt"],
            vec!["BLAME", "Phig PTY"],
        ),
        (vec!["stash"], vec!["STASH", "saved work"]),
    ] {
        let output = run_view(repo.path(), &args);
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
    let mut reader = pair.master.try_clone_reader().unwrap();
    let reader_thread = thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        output
    });
    let mut writer = pair.master.take_writer().unwrap();
    thread::sleep(Duration::from_millis(800));
    writer.write_all(b"\r").unwrap();
    thread::sleep(Duration::from_millis(150));
    writer.write_all(b"qqq").unwrap();
    writer.flush().unwrap();
    let status = wait_bounded(
        &mut child,
        Duration::from_secs(8),
        "waiting for narrow status flow",
    );
    drop(writer);
    drop(pair.master);
    let output = String::from_utf8_lossy(&reader_thread.join().unwrap()).into_owned();
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

    let mut reader = pair.master.try_clone_reader().unwrap();
    let reader_thread = thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        output
    });
    let mut writer = pair.master.take_writer().unwrap();

    thread::sleep(Duration::from_millis(500));
    writer.write_all(b"j").unwrap();
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"\r").unwrap(); // inspect
    thread::sleep(Duration::from_millis(300));
    writer.write_all(b"]}").unwrap(); // hunk and file navigation
    thread::sleep(Duration::from_millis(75));
    writer.write_all(b"/\x1b[200~first\x1b[201~\r").unwrap(); // pasted diff search
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b":help\r").unwrap(); // palette -> contextual help
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"\x1b").unwrap(); // close help
    thread::sleep(Duration::from_millis(75));
    pair.master
        .resize(PtySize {
            rows: 20,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    writer.write_all(b"q").unwrap(); // detail -> log
    thread::sleep(Duration::from_millis(100));
    writer.write_all(b"q").unwrap(); // exit
    writer.flush().unwrap();

    let status = wait_bounded(&mut child, Duration::from_secs(8), "waiting for q exit");
    drop(writer);
    drop(pair.master);
    let output = reader_thread.join().unwrap();
    let screen = String::from_utf8_lossy(&output);

    assert!(status.success(), "phig exited unsuccessfully: {status:?}");
    assert!(screen.contains("phig"));
    assert!(screen.contains("merge side branch") || screen.contains("second commit"));
    assert!(
        screen.contains('◆'),
        "merge topology marker was not rendered"
    );
    assert!(
        screen.contains("SHOW"),
        "Enter did not render commit detail"
    );
    assert!(
        screen.contains("+one") || screen.contains("+two"),
        "commit diff was not rendered"
    );
    assert!(
        screen.contains("Help") && screen.contains("keys"),
        "help overlay was not rendered"
    );
    assert!(screen.contains("\u{1b}[?25h"), "cursor was not restored");
    assert!(
        screen.contains("\u{1b}[?1049l"),
        "alternate screen was not restored"
    );
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
    let mut reader = pair.master.try_clone_reader().unwrap();
    let reader_thread = thread::spawn(move || {
        let mut output = Vec::new();
        reader.read_to_end(&mut output).unwrap();
        output
    });
    let mut writer = pair.master.take_writer().unwrap();
    thread::sleep(Duration::from_millis(500));
    writer.write_all(b"q").unwrap();
    thread::sleep(Duration::from_millis(75));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    let status = wait_bounded(
        &mut child,
        Duration::from_secs(8),
        "waiting for no-alt-screen exit",
    );
    drop(writer);
    drop(pair.master);
    let output = reader_thread.join().unwrap();
    let screen = String::from_utf8_lossy(&output);

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
        let mut reader = pair.master.try_clone_reader().unwrap();
        let reader_thread = thread::spawn(move || {
            let mut output = Vec::new();
            reader.read_to_end(&mut output).unwrap();
            output
        });
        let send_signal = |name: &str| {
            let signal = Command::new("kill")
                .args([name, &process_id.to_string()])
                .status()
                .unwrap();
            assert!(signal.success(), "failed to send {name}");
        };

        thread::sleep(Duration::from_millis(500));
        if exercise_suspend {
            send_signal("-TSTP");
            thread::sleep(Duration::from_millis(150));
            send_signal("-CONT");
            thread::sleep(Duration::from_millis(200));
        }
        send_signal(signal_name);
        let status = wait_bounded(
            &mut child,
            Duration::from_secs(5),
            "waiting for signal exit",
        );
        drop(pair.master);
        let output = reader_thread.join().unwrap();
        let screen = String::from_utf8_lossy(&output);

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
