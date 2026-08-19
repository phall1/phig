use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn help_and_version_are_clean_cli_surfaces() {
    cargo_bin_cmd!("phig")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "terminal Git history and diff browser",
        ))
        .stdout(predicate::str::contains("log"))
        .stdout(predicate::str::contains("show"))
        .stdout(predicate::str::contains("compare"))
        .stdout(predicate::str::contains("refs"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("blame"))
        .stdout(predicate::str::contains("stash"));

    cargo_bin_cmd!("phig")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("phig "));
}

#[test]
fn non_repository_uses_documented_exit_and_stderr() {
    let directory = tempfile::tempdir().unwrap();
    cargo_bin_cmd!("phig")
        .args(["--repo", directory.path().to_str().unwrap()])
        .assert()
        .code(3)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("is not a Git repository"));
}

#[test]
fn paths_require_the_explicit_separator() {
    cargo_bin_cmd!("phig")
        .args(["show", "HEAD", "src/lib.rs"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unexpected argument"));
}
