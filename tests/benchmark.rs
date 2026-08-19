use std::{path::Path, process::Command};

fn run(command: &mut Command) -> std::process::Output {
    let output = command.output().expect("command starts");
    assert!(
        output.status.success(),
        "command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn git_config(repository: &Path, key: &str) -> String {
    let output =
        run(Command::new("git").args(["-C", repository.to_str().unwrap(), "config", "--get", key]));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn benchmark_fixture_identity_is_stamped_and_mismatches_are_rebuilt() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let temp = tempfile::tempdir().unwrap();
    let repository = temp.path().join("fixture");

    run(Command::new(root.join("scripts/make-benchmark-repo.sh"))
        .args([repository.to_str().unwrap(), "3"]));
    assert_eq!(git_config(&repository, "phig.benchmark.version"), "1");
    assert_eq!(git_config(&repository, "phig.benchmark.commits"), "3");
    assert_eq!(git_config(&repository, "phig.benchmark.paths"), "100");

    run(Command::new("git").args([
        "-C",
        repository.to_str().unwrap(),
        "config",
        "--unset",
        "phig.benchmark.version",
    ]));
    let output = run(Command::new("python3").args([
        root.join("scripts/benchmark.py").to_str().unwrap(),
        "--repository",
        repository.to_str().unwrap(),
        "--commits",
        "3",
        "--validate-fixture-only",
    ]));
    let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(metadata["version"], 1);
    assert_eq!(metadata["commits"], 3);
    assert_eq!(metadata["declared_commits"], 3);
    assert_eq!(metadata["paths"], 100);
    assert_eq!(git_config(&repository, "phig.benchmark.version"), "1");
}
