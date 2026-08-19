//! Explicit, installer-aware release updates.

use std::{
    cmp::Ordering,
    env, fs,
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::Value;
use thiserror::Error;

const RELEASE_API: &str = "https://api.github.com/repos/phall1/phig/releases/latest";
const INSTALLER_TEMPLATE: &str =
    "https://github.com/phall1/phig/releases/download/{tag}/phig-cli-installer.sh";
const STAGING_PREFIX: &str = ".phig-update-";
const STALE_STAGING_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("curl is required for release checks and updates")]
    CurlMissing,
    #[error("release request failed: {0}")]
    Request(String),
    #[error("GitHub returned an invalid latest-release response")]
    InvalidResponse,
    #[error("release tag `{0}` must be exactly `v<semver>`")]
    InvalidTag(String),
    #[error("release tag `{0}` is not a supported semantic version")]
    InvalidVersion(String),
    #[error("cannot locate the current phig executable: {0}")]
    CurrentExecutable(io::Error),
    #[error("cannot prepare the updater: {0}")]
    Prepare(io::Error),
    #[error("Homebrew update failed: {0}")]
    BrewFailed(String),
    #[error("release installer failed: {0}")]
    InstallerFailed(String),
    #[error("installed release verification failed: {0}")]
    Verification(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateResult {
    Current {
        current: String,
    },
    Available {
        current: String,
        latest: String,
    },
    Updated {
        previous: String,
        installed: String,
        method: &'static str,
    },
}

#[derive(Debug, Clone)]
struct Release {
    tag: String,
    version: Version,
}

impl Release {
    fn from_tag(tag: &str) -> Result<Self, UpdateError> {
        let raw = tag
            .strip_prefix('v')
            .ok_or_else(|| UpdateError::InvalidTag(tag.to_owned()))?;
        let version = Version::parse(raw)?;
        if tag != format!("v{}", version.normalized()) {
            return Err(UpdateError::InvalidTag(tag.to_owned()));
        }
        Ok(Self {
            tag: tag.to_owned(),
            version,
        })
    }
}

#[derive(Debug, Clone)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
    build: Option<String>,
}

impl Version {
    fn parse(input: &str) -> Result<Self, UpdateError> {
        let (core_and_pre, build) = input
            .split_once('+')
            .map_or((input, None), |(core, build)| (core, Some(build)));
        let (core, prerelease) = core_and_pre
            .split_once('-')
            .map_or((core_and_pre, None), |(core, pre)| (core, Some(pre)));
        let mut parts = core.split('.');
        let major = parse_number(parts.next(), input)?;
        let minor = parse_number(parts.next(), input)?;
        let patch = parse_number(parts.next(), input)?;
        if parts.next().is_some()
            || prerelease.is_some_and(|value| !valid_identifiers(value, true))
            || build.is_some_and(|value| !valid_identifiers(value, false))
        {
            return Err(UpdateError::InvalidVersion(input.to_owned()));
        }
        Ok(Self {
            major,
            minor,
            patch,
            prerelease: prerelease.map(str::to_owned),
            build: build.map(str::to_owned),
        })
    }

    fn normalized(&self) -> String {
        let core = format!("{}.{}.{}", self.major, self.minor, self.patch);
        let version = self
            .prerelease
            .as_ref()
            .map_or(core.clone(), |pre| format!("{core}-{pre}"));
        self.build
            .as_ref()
            .map_or(version.clone(), |build| format!("{version}+{build}"))
    }
}

fn valid_identifiers(value: &str, prerelease: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                && (!prerelease
                    || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                    || identifier.len() == 1
                    || !identifier.starts_with('0'))
        })
}

fn parse_number(value: Option<&str>, original: &str) -> Result<u64, UpdateError> {
    let value = value.filter(|part| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part.len() == 1 || !part.starts_with('0'))
    });
    value
        .and_then(|part| part.parse().ok())
        .ok_or_else(|| UpdateError::InvalidVersion(original.to_owned()))
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major
            && self.minor == other.minor
            && self.patch == other.patch
            && self.prerelease == other.prerelease
    }
}
impl Eq for Version {}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => compare_prerelease(left, right),
            })
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn compare_prerelease(left: &str, right: &str) -> Ordering {
    for (left, right) in left.split('.').zip(right.split('.')) {
        let left_numeric = left.bytes().all(|byte| byte.is_ascii_digit());
        let right_numeric = right.bytes().all(|byte| byte.is_ascii_digit());
        let ordering = match (left_numeric, right_numeric) {
            (true, true) => left.len().cmp(&right.len()).then_with(|| left.cmp(right)),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.split('.').count().cmp(&right.split('.').count())
}

pub fn run(check_only: bool) -> Result<UpdateResult, UpdateError> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let release = latest_release()?;
    if release.version <= current {
        return Ok(UpdateResult::Current {
            current: current.normalized(),
        });
    }
    if check_only {
        return Ok(UpdateResult::Available {
            current: current.normalized(),
            latest: release.version.normalized(),
        });
    }

    let executable = current_executable()?;
    if let Some(entrypoint) = homebrew_entrypoint(&executable)? {
        run_brew_upgrade()?;
        let installed = installed_version(&entrypoint)?;
        if installed != release.version.normalized() {
            return Err(UpdateError::BrewFailed(format!(
                "brew completed but phig {installed} is installed; expected {}",
                release.version.normalized()
            )));
        }
        return Ok(UpdateResult::Updated {
            previous: current.normalized(),
            installed,
            method: "Homebrew",
        });
    }

    install_release(&release, &executable)?;
    Ok(UpdateResult::Updated {
        previous: current.normalized(),
        installed: release.version.normalized(),
        method: "release installer",
    })
}

fn current_executable() -> Result<PathBuf, UpdateError> {
    if let Some(path) = test_override("PHIG_TEST_CURRENT_EXE") {
        return Ok(PathBuf::from(path));
    }
    env::current_exe().map_err(UpdateError::CurrentExecutable)
}

fn run_brew_upgrade() -> Result<(), UpdateError> {
    let status = Command::new("brew")
        .args(["upgrade", "phig"])
        .status()
        .map_err(|error| UpdateError::BrewFailed(error.to_string()))?;
    if !status.success() {
        return Err(UpdateError::BrewFailed(status_label(status.code())));
    }
    Ok(())
}

fn homebrew_entrypoint(executable: &Path) -> Result<Option<PathBuf>, UpdateError> {
    let output = match Command::new("brew").args(["--prefix", "phig"]).output() {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(UpdateError::Prepare(error)),
    };
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Ok(None);
    }
    let prefix = String::from_utf8_lossy(&output.stdout);
    let prefix = prefix.trim();
    if prefix.is_empty() || prefix.lines().count() != 1 {
        return Ok(None);
    }
    let entrypoint = PathBuf::from(prefix).join("bin/phig");
    let current = match fs::canonicalize(executable) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let candidate = match fs::canonicalize(&entrypoint) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    Ok((candidate == current).then_some(entrypoint))
}

fn installed_version(executable: &Path) -> Result<String, UpdateError> {
    if let Some(version) = test_override("PHIG_TEST_INSTALLED_VERSION") {
        Version::parse(&version)?;
        return Ok(version);
    }
    let output = Command::new(executable)
        .args(["version", "--json"])
        .output()
        .map_err(UpdateError::Prepare)?;
    if !output.status.success() || output.stdout.len() > 64 * 1024 {
        return Err(UpdateError::Verification(
            "candidate `version --json` failed or exceeded 64 KiB".into(),
        ));
    }
    let response: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| UpdateError::Verification("candidate emitted invalid JSON".into()))?;
    if response.get("protocol").and_then(Value::as_str) != Some("phig/1")
        || response.get("kind").and_then(Value::as_str) != Some("version")
    {
        return Err(UpdateError::Verification(
            "candidate emitted an invalid version envelope".into(),
        ));
    }
    response
        .get("payload")
        .and_then(|payload| payload.get("version"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| UpdateError::Verification("candidate omitted its version".into()))
}

fn latest_release() -> Result<Release, UpdateError> {
    let url = test_override("PHIG_TEST_RELEASE_API").unwrap_or_else(|| RELEASE_API.to_owned());
    let output = curl_output(&[
        "--proto",
        "=https",
        "--tlsv1.2",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--connect-timeout",
        "10",
        "--max-time",
        "30",
        "--max-filesize",
        "1048576",
        "--header",
        "Accept: application/vnd.github+json",
        "--header",
        "X-GitHub-Api-Version: 2022-11-28",
        &url,
    ])?;
    let response: Value =
        serde_json::from_slice(&output.stdout).map_err(|_| UpdateError::InvalidResponse)?;
    let tag = response
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or(UpdateError::InvalidResponse)?;
    Release::from_tag(tag)
}

fn install_release(release: &Release, executable: &Path) -> Result<(), UpdateError> {
    let parent = executable.parent().ok_or_else(|| {
        UpdateError::Prepare(io::Error::other("executable has no parent directory"))
    })?;
    scavenge_stale_staging(parent, executable)?;
    let mut staging = StagingInstall::new(parent)?;
    download_installer(release, &staging.installer)?;

    let status = Command::new("/bin/sh")
        .arg(&staging.installer)
        .env("PHIG_CLI_UNMANAGED_INSTALL", &staging.directory)
        .env("PHIG_CLI_NO_MODIFY_PATH", "1")
        .status()
        .map_err(UpdateError::Prepare)?;
    if !status.success() {
        return Err(UpdateError::InstallerFailed(status_label(status.code())));
    }

    verify_candidate(&staging.candidate, &release.version.normalized())?;
    harden_candidate_permissions(&staging.candidate)?;
    sync_file(&staging.candidate)?;
    sync_directory(&staging.directory)?;

    fs::hard_link(executable, &staging.backup).map_err(UpdateError::Prepare)?;
    sync_file(&staging.backup)?;
    fs::rename(&staging.candidate, executable).map_err(UpdateError::Prepare)?;

    let post_install = (|| {
        sync_directory(parent)?;
        if test_override("PHIG_TEST_POST_INSTALL_FAIL").is_some() {
            return Err(UpdateError::Verification(
                "injected post-install verification failure".into(),
            ));
        }
        verify_candidate(executable, &release.version.normalized())?;
        sync_directory(parent)
    })();
    if let Err(error) = post_install {
        let rollback = if test_override("PHIG_TEST_ROLLBACK_FAIL").is_some() {
            Err(io::Error::other("injected rollback failure"))
        } else {
            fs::rename(&staging.backup, executable)
        };
        if let Err(rollback) = rollback {
            let recovery = staging.preserve_for_manual_recovery()?;
            return Err(UpdateError::Prepare(io::Error::other(format!(
                "post-install verification failed ({error}); rollback is preserved at {} but could not be restored: {rollback}",
                recovery.display()
            ))));
        }
        sync_directory(parent)?;
        return Err(error);
    }

    fs::remove_file(&staging.backup).map_err(UpdateError::Prepare)?;
    sync_directory(parent)?;
    Ok(())
}

fn download_installer(release: &Release, path: &Path) -> Result<(), UpdateError> {
    let template =
        test_override("PHIG_TEST_INSTALLER_URL").unwrap_or_else(|| INSTALLER_TEMPLATE.to_owned());
    let url = template.replace("{tag}", &release.tag);
    let output_path = path.to_string_lossy().into_owned();
    curl_output(&[
        "--proto",
        "=https",
        "--tlsv1.2",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--connect-timeout",
        "10",
        "--max-time",
        "120",
        "--max-filesize",
        "4194304",
        "--output",
        &output_path,
        &url,
    ])?;
    Ok(())
}

fn verify_candidate(path: &Path, expected: &str) -> Result<(), UpdateError> {
    let metadata = fs::symlink_metadata(path).map_err(UpdateError::Prepare)?;
    if !metadata.file_type().is_file() {
        return Err(UpdateError::Verification(
            "installer did not produce a regular `phig` binary".into(),
        ));
    }
    ensure_executable(&metadata)?;
    let observed = installed_version(path)?;
    if observed != expected {
        return Err(UpdateError::Verification(format!(
            "candidate is phig {observed}; expected exactly {expected}"
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_executable(metadata: &fs::Metadata) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    if metadata.permissions().mode() & 0o111 == 0 {
        return Err(UpdateError::Verification(
            "candidate is not executable".into(),
        ));
    }
    Ok(())
}
#[cfg(not(unix))]
fn ensure_executable(_metadata: &fs::Metadata) -> Result<(), UpdateError> {
    Ok(())
}

#[cfg(unix)]
fn harden_candidate_permissions(path: &Path) -> Result<(), UpdateError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::metadata(path).map_err(UpdateError::Prepare)?;
    let mode = metadata.permissions().mode() & 0o777 & !0o022;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(UpdateError::Prepare)
}
#[cfg(not(unix))]
fn harden_candidate_permissions(_path: &Path) -> Result<(), UpdateError> {
    Ok(())
}

fn sync_file(path: &Path) -> Result<(), UpdateError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(UpdateError::Prepare)
}

fn sync_directory(path: &Path) -> Result<(), UpdateError> {
    match File::open(path).and_then(|file| file.sync_all()) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(UpdateError::Prepare(error)),
    }
}

fn scavenge_stale_staging(parent: &Path, executable: &Path) -> Result<(), UpdateError> {
    let now = SystemTime::now();
    let executable_metadata = fs::metadata(executable).map_err(UpdateError::Prepare)?;
    for entry in fs::read_dir(parent).map_err(UpdateError::Prepare)? {
        let entry = entry.map_err(UpdateError::Prepare)?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        let metadata = match entry.path().symlink_metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if !safe_owned_private_directory(&metadata, &executable_metadata)
            || entry.path().join("ROLLBACK_REQUIRED").exists()
        {
            continue;
        }
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= STALE_STAGING_AGE);
        if stale {
            fs::remove_dir_all(entry.path()).map_err(UpdateError::Prepare)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn safe_owned_private_directory(candidate: &fs::Metadata, executable: &fs::Metadata) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    candidate.file_type().is_dir()
        && candidate.uid() == executable.uid()
        && candidate.permissions().mode() & 0o077 == 0
}
#[cfg(not(unix))]
fn safe_owned_private_directory(candidate: &fs::Metadata, _executable: &fs::Metadata) -> bool {
    candidate.file_type().is_dir()
}

fn curl_output(args: &[&str]) -> Result<Output, UpdateError> {
    let output = Command::new("curl").args(args).output().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            UpdateError::CurlMissing
        } else {
            UpdateError::Prepare(error)
        }
    })?;
    if !output.status.success() {
        return Err(UpdateError::Request(output_error(&output)));
    }
    Ok(output)
}

fn output_error(output: &Output) -> String {
    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if message.is_empty() {
        status_label(output.status.code())
    } else {
        message
    }
}

fn status_label(code: Option<i32>) -> String {
    code.map_or_else(|| "terminated by signal".into(), |code| code.to_string())
}

fn test_override(name: &str) -> Option<String> {
    if cfg!(debug_assertions) {
        env::var(name).ok().filter(|value| !value.is_empty())
    } else {
        None
    }
}

struct StagingInstall {
    directory: PathBuf,
    installer: PathBuf,
    candidate: PathBuf,
    backup: PathBuf,
    cleanup_on_drop: bool,
}

impl StagingInstall {
    fn new(parent: &Path) -> Result<Self, UpdateError> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let directory = parent.join(format!("{STAGING_PREFIX}{}-{nonce}", std::process::id()));
        create_private_directory(&directory).map_err(UpdateError::Prepare)?;
        let installer = directory.join("installer.sh");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&installer)
            .map_err(UpdateError::Prepare)?;
        restrict_file(&file).map_err(UpdateError::Prepare)?;
        drop(file);
        Ok(Self {
            candidate: directory.join("phig"),
            backup: directory.join("previous-phig"),
            directory,
            installer,
            cleanup_on_drop: true,
        })
    }

    fn preserve_for_manual_recovery(&mut self) -> Result<PathBuf, UpdateError> {
        self.cleanup_on_drop = false;
        let marker = self.directory.join("ROLLBACK_REQUIRED");
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .map_err(UpdateError::Prepare)?;
        use std::io::Write as _;
        file.write_all(b"The previous verified phig is preserved as previous-phig.\n")
            .map_err(UpdateError::Prepare)?;
        restrict_file(&file).map_err(UpdateError::Prepare)?;
        file.sync_all().map_err(UpdateError::Prepare)?;
        sync_directory(&self.directory)?;
        Ok(self.backup.clone())
    }
}

impl Drop for StagingInstall {
    fn drop(&mut self) {
        if self.cleanup_on_drop {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }
}

#[cfg(unix)]
fn create_private_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(0o700).create(path)
}
#[cfg(not(unix))]
fn create_private_directory(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}
#[cfg(unix)]
fn restrict_file(file: &fs::File) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(0o600))
}
#[cfg(not(unix))]
fn restrict_file(_file: &fs::File) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_tags_are_exact_and_versions_order_correctly() {
        assert_eq!(
            Release::from_tag("v1.2.3-rc.1+build.7").unwrap().tag,
            "v1.2.3-rc.1+build.7"
        );
        for invalid in ["1.2.3", "release-v1.2.3", "v01.2.3", "vv1.2.3"] {
            assert!(Release::from_tag(invalid).is_err(), "accepted {invalid}");
        }
        assert!(Version::parse("1.2.0").unwrap() > Version::parse("1.1.99").unwrap());
        assert!(Version::parse("1.0.0").unwrap() > Version::parse("1.0.0-rc.2").unwrap());
        assert!(Version::parse("1.0.0-rc.10").unwrap() > Version::parse("1.0.0-rc.2").unwrap());
        assert!(
            Version::parse("1.0.0-999999999999999999999999999999").unwrap()
                > Version::parse("1.0.0-100000000000000000000000000000").unwrap()
        );
    }

    #[test]
    fn versions_reject_injection_and_ambiguous_numbers() {
        for value in [
            "1.0",
            "01.0.0",
            "1.0.0-alpha..1",
            "1.0.0-01",
            "1.0.0+build..1",
            "1.0.0/../../x",
            "1.0.0;echo",
        ] {
            assert!(Version::parse(value).is_err(), "accepted {value}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn staging_directory_is_created_atomically_private() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let staging = StagingInstall::new(parent.path()).unwrap();
        let mode = fs::metadata(&staging.directory)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn scavenger_removes_only_owned_private_stale_staging() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        let executable = parent.path().join("phig");
        fs::write(&executable, b"old").unwrap();

        let stale = parent.path().join(".phig-update-interrupted");
        let recent = parent.path().join(".phig-update-active");
        let public = parent.path().join(".phig-update-not-private");
        let recovery = parent.path().join(".phig-update-manual-recovery");
        for path in [&stale, &recent, &public, &recovery] {
            fs::create_dir(path).unwrap();
        }
        fs::set_permissions(&stale, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&recent, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(&recovery, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(recovery.join("ROLLBACK_REQUIRED"), b"preserve").unwrap();
        let old = SystemTime::now() - STALE_STAGING_AGE - Duration::from_secs(60);
        File::open(&stale)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
        File::open(&public)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
        File::open(&recovery)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();

        scavenge_stale_staging(parent.path(), &executable).unwrap();
        assert!(!stale.exists());
        assert!(recent.exists());
        assert!(public.exists());
        assert!(recovery.exists());
    }
}
