#![cfg(unix)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

use tempfile::TempDir;

fn executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture() -> (TempDir, String) {
    let fixture = TempDir::new().unwrap();
    let curl_log = fixture.path().join("curl.log");
    executable(
        &fixture.path().join("curl"),
        r#"#!/bin/sh
set -eu
output=
previous=
for argument do
  printf '%s\n' "$argument" >>"$PHIG_TEST_CURL_LOG"
  if [ "$previous" = output ]; then output=$argument; fi
  previous=
  if [ "$argument" = --output ]; then previous=output; fi
done
cat >"$output" <<'INSTALLER'
#!/bin/sh
set -eu
if [ -n "${PHIG_CLI_UNMANAGED_INSTALL-}" ]; then
  install_dir=$PHIG_CLI_UNMANAGED_INSTALL
else
  install_dir=${CARGO_HOME:-$HOME/.cargo}/bin
fi
mkdir -p "$install_dir"
printf '#!/bin/sh\necho phig 1.0.0\n' >"$install_dir/phig"
chmod +x "$install_dir/phig"
printf '%s\n' "$install_dir" >"$PHIG_TEST_INSTALL_LOG"
INSTALLER
"#,
    );
    (fixture, curl_log.to_string_lossy().into_owned())
}

fn install_command(fixture: &TempDir, curl_log: &str) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh"))
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("PHIG_TEST_CURL_LOG", curl_log)
        .env("PHIG_TEST_INSTALL_LOG", fixture.path().join("install.log"))
        .env("HOME", fixture.path().join("home"))
        .env("CARGO_HOME", fixture.path().join("cargo"));
    command
}

#[test]
fn installer_supports_latest_versioned_and_prefix_installs() {
    let (fixture, curl_log) = fixture();
    let prefix = fixture.path().join("prefix with spaces");
    let output = install_command(&fixture, &curl_log)
        .args(["--prefix", prefix.to_str().unwrap(), "--yes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(prefix.join("bin/phig").exists());
    let curl = fs::read_to_string(&curl_log).unwrap();
    assert!(curl.contains("--proto\n=https"));
    assert!(curl.contains("--tlsv1.2"));
    assert!(curl.contains("releases/latest/download/phig-cli-installer.sh"));

    fs::write(&curl_log, "").unwrap();
    let output = install_command(&fixture, &curl_log)
        .env("PHIG_VERSION", "v1.0.0")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(fixture.path().join("cargo/bin/phig").is_file());
    assert!(
        !fixture.path().join("home/.cargo/bin/phig").exists(),
        "installer test escaped its explicit CARGO_HOME sandbox"
    );
    assert!(
        fs::read_to_string(curl_log)
            .unwrap()
            .contains("releases/download/v1.0.0/phig-cli-installer.sh")
    );
}

#[test]
fn installer_help_and_invalid_versions_do_not_use_network() {
    let (fixture, curl_log) = fixture();
    let output = install_command(&fixture, &curl_log)
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("--prefix DIR"));
    assert!(!Path::new(&curl_log).exists());

    for invalid in [
        "1.2.3;echo-pwned",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.3-alpha..1",
        "1.2.3-01",
        "1.2.3+build..1",
        "1.2.3+one+two",
    ] {
        let output = install_command(&fixture, &curl_log)
            .env("PHIG_VERSION", invalid)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "accepted {invalid}");
        assert!(!Path::new(&curl_log).exists());
    }

    let output = install_command(&fixture, &curl_log)
        .env("PHIG_VERSION", "1.2.3-rc.1+build.7")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        fs::read_to_string(curl_log)
            .unwrap()
            .contains("releases/download/v1.2.3-rc.1+build.7/phig-cli-installer.sh")
    );
}

#[test]
fn real_generated_installer_uses_flat_unmanaged_layout_when_available() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let installer = root.join("target/distrib/phig-cli-installer.sh");
    let archive = root.join("target/distrib/phig-cli-aarch64-apple-darwin.tar.xz");
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return;
    }
    if !installer.exists() || !archive.exists() {
        assert!(
            std::env::var_os("PHIG_REQUIRE_DIST_ARTIFACTS").is_none(),
            "required cargo-dist installer/archive were not generated"
        );
        return;
    }
    let generated = fs::read_to_string(&installer).unwrap();
    assert!(generated.contains("PHIG_CLI_UNMANAGED_INSTALL"));
    assert!(generated.contains("check_glibc \"2\" \"31\""));
    let fixture = TempDir::new().unwrap();
    let install_dir = fixture.path().join("prefix/bin");
    let output = Command::new("/bin/sh")
        .arg(installer)
        .env("PHIG_CLI_UNMANAGED_INSTALL", &install_dir)
        .env(
            "PHIG_CLI_DOWNLOAD_URL",
            format!("file://{}/target/distrib", root.display()),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(install_dir.join("phig").is_file());
    assert!(
        !install_dir.join("bin/phig").exists(),
        "installer used hierarchical layout"
    );
}

#[test]
fn wrapper_drives_real_generated_installer_when_available() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let installer = root.join("target/distrib/phig-cli-installer.sh");
    let archive = root.join("target/distrib/phig-cli-aarch64-apple-darwin.tar.xz");
    if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        return;
    }
    if !installer.exists() || !archive.exists() {
        assert!(
            std::env::var_os("PHIG_REQUIRE_DIST_ARTIFACTS").is_none(),
            "required cargo-dist installer/archive were not generated"
        );
        return;
    }
    let fixture = TempDir::new().unwrap();
    executable(
        &fixture.path().join("curl"),
        "#!/bin/sh\nset -eu\nout=\nprev=\nis_file=\nfor arg do case \"$arg\" in file://*) is_file=1 ;; esac; [ \"$prev\" = out ] && out=$arg; prev=; [ \"$arg\" = --output ] && prev=out; done\n[ -n \"$is_file\" ] && exec /usr/bin/curl \"$@\"\ncp \"$PHIG_TEST_REAL_INSTALLER\" \"$out\"\n",
    );
    let prefix = fixture.path().join("prefix");
    let output = Command::new("/bin/sh")
        .arg(root.join("install.sh"))
        .args(["--prefix", prefix.to_str().unwrap(), "--yes"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fixture.path().display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("PHIG_TEST_REAL_INSTALLER", installer)
        .env(
            "PHIG_CLI_DOWNLOAD_URL",
            format!("file://{}/target/distrib", root.display()),
        )
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(prefix.join("bin/phig").is_file());
    assert!(!prefix.join("bin/bin/phig").exists());
}

#[test]
fn installer_stops_on_download_failure_without_running_content() {
    let fixture = TempDir::new().unwrap();
    executable(&fixture.path().join("curl"), "#!/bin/sh\nexit 28\n");
    let output = install_command(&fixture, fixture.path().join("curl.log").to_str().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!fixture.path().join("install.log").exists());
}
