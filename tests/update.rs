#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn executable(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn version_script(version: &str) -> String {
    format!(
        "#!/bin/sh\nprintf '%s\\n' '{{\"protocol\":\"phig/1\",\"kind\":\"version\",\"payload\":{{\"version\":\"{version}\",\"gitMinimum\":\"2.45.1\"}}}}'\n"
    )
}

fn fake_path(directory: &TempDir) -> String {
    format!(
        "{}:{}",
        directory.path().display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

fn fake_tools(directory: &TempDir, tag: &str) -> PathBuf {
    let log = directory.path().join("network.log");
    executable(
        &directory.path().join("curl"),
        &format!(
            r#"#!/bin/sh
set -eu
output=
previous=
last=
for argument do
  last=$argument
  printf '%s\n' "$argument" >>"$PHIG_TEST_NETWORK_LOG"
  if [ "$previous" = output ]; then output=$argument; fi
  previous=
  if [ "$argument" = --output ]; then previous=output; fi
done
if [ -n "$output" ]; then
  cat >"$output" <<'INSTALLER'
#!/bin/sh
set -eu
if [ "${{PHIG_TEST_INSTALL_FAIL-}}" = 1 ]; then exit 42; fi
: "${{PHIG_CLI_UNMANAGED_INSTALL:?missing flat install destination}}"
mkdir -p "$PHIG_CLI_UNMANAGED_INSTALL"
version=${{PHIG_TEST_CANDIDATE_VERSION-{version}}}
cat >"$PHIG_CLI_UNMANAGED_INSTALL/phig" <<EOF
#!/bin/sh
printf '%s\\n' '{{"protocol":"phig/1","kind":"version","payload":{{"version":"'$version'","gitMinimum":"2.45.1"}}}}'
EOF
chmod 0777 "$PHIG_CLI_UNMANAGED_INSTALL/phig"
INSTALLER
else
  printf '%s\n' '{{"tag_name":"{tag}"}}'
fi
"#,
            version = tag.strip_prefix('v').unwrap_or(tag)
        ),
    );
    executable(
        &directory.path().join("brew"),
        r#"#!/bin/sh
set -eu
if [ "$1" = --prefix ]; then
  [ "${2-}" = phig ] || exit 2
  printf '%s\n' "$PHIG_TEST_BREW_PREFIX"
  exit 0
fi
printf '%s\n' "$*" >>"$PHIG_TEST_BREW_LOG"
[ "$1" = upgrade ] && [ "${2-}" = phig ]
"#,
    );
    log
}

fn update_command(fixture: &TempDir, destination: &Path, tag: &str) -> (Command, PathBuf) {
    let log = fake_tools(fixture, tag);
    let mut command = Command::cargo_bin("phig").unwrap();
    command
        .env("PATH", fake_path(fixture))
        .env("PHIG_TEST_RELEASE_API", "https://example.invalid/latest")
        .env(
            "PHIG_TEST_INSTALLER_URL",
            "https://example.invalid/releases/download/{tag}/phig-cli-installer.sh",
        )
        .env("PHIG_TEST_NETWORK_LOG", &log)
        .env("PHIG_TEST_CURRENT_EXE", destination);
    (command, log)
}

fn assert_no_staging(parent: &Path) {
    let leftovers = fs::read_dir(parent)
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".phig-update-")
        })
        .collect::<Vec<_>>();
    assert!(leftovers.is_empty(), "staging leftovers: {leftovers:?}");
}

#[test]
fn update_check_reports_current_and_available_releases() {
    let fixture = TempDir::new().unwrap();
    let destination = fixture.path().join("bin/phig");
    executable(&destination, &version_script("1.0.0"));

    let (mut current, _) = update_command(&fixture, &destination, "v1.0.0");
    current
        .args(["update", "--check"])
        .assert()
        .success()
        .stdout("phig 1.0.0 is current\n");

    let (mut available, _) = update_command(&fixture, &destination, "v1.4.2");
    available
        .args(["update", "--check"])
        .assert()
        .success()
        .stdout("phig 1.4.2 is available (current 1.0.0); run `phig update`\n");
}

#[test]
fn release_install_stages_verifies_and_atomically_replaces() {
    let fixture = TempDir::new().unwrap();
    let destination = fixture.path().join("bin/phig");
    executable(&destination, &version_script("1.0.0"));
    let (mut command, log) = update_command(&fixture, &destination, "v1.1.0");
    command
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "updated phig 1.0.0 to 1.1.0 with release installer",
        ));

    let output = std::process::Command::new(&destination)
        .args(["version", "--json"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"version\":\"1.1.0\""));
    let mode = fs::metadata(&destination).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "candidate permissions were not hardened");
    assert!(
        fs::read_to_string(log)
            .unwrap()
            .contains("/releases/download/v1.1.0/phig-cli-installer.sh")
    );
    assert_no_staging(destination.parent().unwrap());
}

#[test]
fn candidate_mismatch_installer_failure_and_post_verify_all_preserve_old_binary() {
    for failure in ["mismatch", "installer", "post"] {
        let fixture = TempDir::new().unwrap();
        let destination = fixture.path().join("bin/phig");
        let original = version_script("1.0.0");
        executable(&destination, &original);
        let (mut command, _) = update_command(&fixture, &destination, "v1.1.0");
        match failure {
            "mismatch" => {
                command.env("PHIG_TEST_CANDIDATE_VERSION", "1.0.9");
            }
            "installer" => {
                command.env("PHIG_TEST_INSTALL_FAIL", "1");
            }
            "post" => {
                command.env("PHIG_TEST_POST_INSTALL_FAIL", "1");
            }
            _ => unreachable!(),
        }
        command.arg("update").assert().code(6).stdout("");
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            original,
            "{failure}"
        );
        assert_no_staging(destination.parent().unwrap());
    }
}

#[test]
fn rollback_failure_preserves_manual_recovery_backup() {
    let fixture = TempDir::new().unwrap();
    let destination = fixture.path().join("bin/phig");
    let original = version_script("1.0.0");
    executable(&destination, &original);
    let (mut command, _) = update_command(&fixture, &destination, "v1.1.0");
    command
        .env("PHIG_TEST_POST_INSTALL_FAIL", "1")
        .env("PHIG_TEST_ROLLBACK_FAIL", "1")
        .arg("update")
        .assert()
        .code(6)
        .stdout("")
        .stderr(predicate::str::contains("rollback is preserved at"));

    let recovery = fs::read_dir(destination.parent().unwrap())
        .unwrap()
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".phig-update-")
        })
        .expect("manual recovery directory");
    assert!(recovery.path().join("ROLLBACK_REQUIRED").is_file());
    assert_eq!(
        fs::read_to_string(recovery.path().join("previous-phig")).unwrap(),
        original
    );
}

#[test]
fn no_op_and_unwritable_destination_do_not_damage_installation() {
    let fixture = TempDir::new().unwrap();
    let parent = fixture.path().join("bin");
    let destination = parent.join("phig");
    let original = version_script("1.0.0");
    executable(&destination, &original);

    let (mut no_op, log) = update_command(&fixture, &destination, "v1.0.0");
    no_op.arg("update").assert().success();
    let network = fs::read_to_string(log).unwrap();
    assert!(!network.contains("phig-cli-installer.sh"));
    assert_eq!(fs::read_to_string(&destination).unwrap(), original);

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    let (mut denied, _) = update_command(&fixture, &destination, "v1.1.0");
    denied.arg("update").assert().code(6).stdout("");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(fs::read_to_string(&destination).unwrap(), original);
}

#[test]
fn homebrew_ownership_uses_canonical_formula_prefix_and_rejects_false_positives() {
    let fixture = TempDir::new().unwrap();
    let cellar = fixture.path().join("Cellar/phig/1.0.0/bin/phig");
    executable(&cellar, &version_script("1.0.0"));
    let formula = fixture.path().join("opt/phig/bin");
    fs::create_dir_all(&formula).unwrap();
    std::os::unix::fs::symlink(&cellar, formula.join("phig")).unwrap();
    let brew_log = fixture.path().join("brew.log");
    let (mut brew, _) = update_command(&fixture, &cellar, "v2.0.0");
    brew.env("PHIG_TEST_BREW_PREFIX", fixture.path().join("opt/phig"))
        .env("PHIG_TEST_BREW_LOG", &brew_log)
        .env("PHIG_TEST_INSTALLED_VERSION", "2.0.0")
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains("with Homebrew"));
    assert_eq!(
        fs::read_to_string(&brew_log).unwrap().trim(),
        "upgrade phig"
    );

    let false_positive = fixture.path().join("Cellar/not-phig/bin/phig");
    executable(&false_positive, &version_script("1.0.0"));
    fs::write(&brew_log, "").unwrap();
    let (mut release, _) = update_command(&fixture, &false_positive, "v1.1.0");
    release
        .env("PHIG_TEST_BREW_PREFIX", fixture.path().join("opt/phig"))
        .env("PHIG_TEST_BREW_LOG", &brew_log)
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains("release installer"));
    assert_eq!(fs::read_to_string(brew_log).unwrap(), "");
}

#[test]
fn exact_release_tag_is_retained_and_noncanonical_tags_are_rejected() {
    let fixture = TempDir::new().unwrap();
    let destination = fixture.path().join("bin/phig");
    executable(&destination, &version_script("1.0.0"));
    let (mut exact, log) = update_command(&fixture, &destination, "v1.1.0+build.7");
    exact.arg("update").assert().success();
    assert!(
        fs::read_to_string(log)
            .unwrap()
            .contains("/v1.1.0+build.7/phig-cli-installer.sh")
    );

    for invalid in ["1.2.0", "release-v1.2.0", "v01.2.0"] {
        let fixture = TempDir::new().unwrap();
        let destination = fixture.path().join("bin/phig");
        executable(&destination, &version_script("1.0.0"));
        let (mut command, _) = update_command(&fixture, &destination, invalid);
        command.arg("update").assert().code(6).stdout("");
        assert!(fs::read_to_string(&destination).unwrap().contains("1.0.0"));
    }
}

#[test]
fn interrupted_update_leaves_old_binary_and_only_private_staging() {
    let fixture = TempDir::new().unwrap();
    let destination = fixture.path().join("bin/phig");
    let original = version_script("1.0.0");
    executable(&destination, &original);
    let child_pid = fixture.path().join("installer.pid");
    let curl = fixture.path().join("curl");
    executable(
        &curl,
        r#"#!/bin/sh
set -eu
output=
previous=
for argument do
  if [ "$previous" = output ]; then output=$argument; fi
  previous=
  if [ "$argument" = --output ]; then previous=output; fi
done
if [ -n "$output" ]; then
  cat >"$output" <<'INSTALLER'
#!/bin/sh
printf '%s\n' "$$" >"$PHIG_TEST_CHILD_PID"
exec sleep 30
INSTALLER
else
  printf '%s\n' '{"tag_name":"v1.1.0"}'
fi
"#,
    );
    let mut process = std::process::Command::new(assert_cmd::cargo::cargo_bin("phig"))
        .env("PATH", fake_path(&fixture))
        .env("PHIG_TEST_RELEASE_API", "https://example.invalid/latest")
        .env(
            "PHIG_TEST_INSTALLER_URL",
            "https://example.invalid/releases/download/{tag}/phig-cli-installer.sh",
        )
        .env("PHIG_TEST_CURRENT_EXE", &destination)
        .env("PHIG_TEST_CHILD_PID", &child_pid)
        .arg("update")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !child_pid.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    assert!(child_pid.exists(), "installer did not start");
    let installer_pid = fs::read_to_string(&child_pid).unwrap();
    std::process::Command::new("/bin/kill")
        .args(["-TERM", &process.id().to_string()])
        .status()
        .unwrap();
    std::process::Command::new("/bin/kill")
        .args(["-TERM", installer_pid.trim()])
        .status()
        .unwrap();
    let _ = process.wait();

    assert_eq!(fs::read_to_string(&destination).unwrap(), original);
    let stages = fs::read_dir(destination.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".phig-update-")
        })
        .collect::<Vec<_>>();
    assert_eq!(stages.len(), 1);
    assert_eq!(
        fs::metadata(stages[0].path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn network_failure_uses_stable_exit_six() {
    let fixture = TempDir::new().unwrap();
    let destination = fixture.path().join("bin/phig");
    executable(&destination, &version_script("1.0.0"));
    executable(&fixture.path().join("curl"), "#!/bin/sh\nexit 7\n");
    Command::cargo_bin("phig")
        .unwrap()
        .env("PATH", fake_path(&fixture))
        .env("PHIG_TEST_RELEASE_API", "https://example.invalid/latest")
        .env("PHIG_TEST_CURRENT_EXE", &destination)
        .args(["update", "--check"])
        .assert()
        .code(6)
        .stdout("")
        .stderr(predicate::str::contains("update unavailable"));
}
