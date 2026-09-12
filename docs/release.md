# Release process

GitHub Releases are the distribution authority. cargo-dist 0.32.0 builds four
archives, per-archive and unified SHA-256 checksums, a shell installer, source
tarball, and GitHub attestations. It does not build or push a Homebrew formula:
`phall1/homebrew-tap` renders `Formula/phig.rb` itself from `tools/phig.json`,
re-resolving this repository's latest release on a fifteen-minute schedule.

## Releases default to "merge a PR"

release-please (`.github/workflows/release-please.yml`, configured in
`.github/release-please/`) turns releases into a single PR merge:

1. Conventional commits land on `main` (`feat:`, `fix:`, `perf:`, ...).
2. release-please opens (and keeps up to date) a release PR that bumps
   `Cargo.toml`, `Cargo.lock`, `tests/fixtures/version.json`, and
   `CHANGELOG.md`. Features bump the minor version, fixes bump the patch.
3. **Merge the release PR.** That is the whole release decision.
4. `.github/workflows/release-tag.yml` pushes the `vX.Y.Z` tag on the merged
   release PR using `RELEASE_PLEASE_TOKEN`.
5. The tag push triggers cargo-dist's Release workflow (archives, checksums,
   shell installer, attestations, GitHub Release) and
   `.github/workflows/publish-crates.yml` (`cargo publish --locked`).
6. The Homebrew tap re-resolves the new release within its fifteen-minute
   schedule.

Do not push a manual version tag while the automation is healthy: a release
touchpoint outside a release PR skips review, and the tag workflow may race the
crates.io publish. If a manual release is genuinely needed, follow the manual
path below and expect the two tag-push workflows to publish automatically.

There are two important constraints behind this design:

- Tags pushed with the default `GITHUB_TOKEN` do **not** trigger further
  workflow runs, so tag creation must use a real token (`RELEASE_PLEASE_TOKEN`)
  for cargo-dist and the crates.io publish to fire at all.
- `skip-github-release: true` keeps release-please from creating the tag or a
  GitHub Release itself; cargo-dist stays the single creator of the GitHub
  Release on the tag push.

## One-time repository setup

The `phall1/phig` repository must have Actions enabled. GitHub's generated
`GITHUB_TOKEN` creates releases and attestations. No tap credential is required
here: the tap reads this repository's public releases under its own token rather
than being pushed to, so there is no `HOMEBREW_TAP_TOKEN` to hold or rotate.
Private vulnerability reporting should be enabled.

Two repository secrets are required by the automated path:

- `RELEASE_PLEASE_TOKEN` — a personal access token (classic with `repo`, or
  fine-grained with `Contents: Read and write`) used by release-please to open
  release PRs and by `release-tag` to push the version tag. A PAT is required
  because tags created with `GITHUB_TOKEN` do not trigger downstream workflows.
- `CARGO_REGISTRY_TOKEN` — a crates.io API token for `publish-crates`. Prefer a
  granular token scoped to `phig-cli` with an expiry over a full-account token.

## Automated release checklist

1. Merge conventional-commit changes to `main`. Prefer the commit types
   `feat:`, `fix:`, `perf:`, and `chore:`; `docs:`/`test:`/`ci:`/`build:`
   commits are hidden from the generated changelog.
2. When a release PR titled `chore(main): release phig-cli vX.Y.Z` is open,
   merge it. Do not merge two release PRs back to back; each merge tags
   immediately.
3. Wait for the tag-push workflows. Verify the GitHub Release and the crates.io
   publish:

   ```sh
   gh release view vX.Y.Z --repo phall1/phig
   gh release download vX.Y.Z --repo phall1/phig --dir /tmp/phig-release
   (cd /tmp/phig-release && shasum -a 256 -c phig-cli-aarch64-apple-darwin.tar.xz.sha256)
   gh attestation verify /tmp/phig-release/phig-cli-aarch64-apple-darwin.tar.xz \
     --repo phall1/phig
   cargo search phig-cli --limit 1
   ```

4. Let the tap catch up (up to fifteen minutes), then `brew update && brew
   install phall1/tap/phig` and `phig update --check` in a clean home.
5. If a workflow failed, see [Failure and recovery](#failure-and-recovery);
   never push a second tag for the same version.

## Manual release (fallback)

Use this path when the automated path cannot (workflow breakage, first-time
runoff, a prerelease that must stay out of crates.io). A manual cut is a
prepared commit on `main`, then a tag; the `release-tag` and `publish-crates`
workflows still fire on the tag push, so keep `Cargo.toml` at the intended
version in the commit you tag.

### Prepare

1. Update `CHANGELOG.md`, set `Cargo.toml` to the release version, and refresh
   `Cargo.lock`.
2. Run the complete local gate:

   ```sh
   just check
   cargo deny check
   shellcheck install.sh scripts/*.sh
   cargo build --release --locked
   dist generate --check
   dist plan
   cargo package --locked
   cargo publish --dry-run --allow-dirty
   scripts/benchmark.sh /tmp/phig-benchmark 1000 --json
   ```

   On an ARM64 Mac, rehearse the real native archive and installer. The local
   manifest must be saved under `target/distrib` before the global installer is
   generated, or the embedded checksum may describe an older archive:

   ```sh
   mkdir -p target/distrib
   local_manifest="$(mktemp)"
   global_manifest="$(mktemp)"
   dist build --allow-dirty --artifacts=local --target=aarch64-apple-darwin \
     --output-format=json >"$local_manifest"
   cp "$local_manifest" target/distrib/aarch64-apple-darwin-dist-manifest.json
   dist build --allow-dirty --artifacts=global --output-format=json \
     >"$global_manifest"
   cp "$global_manifest" target/distrib/global-dist-manifest.json
   rm -f "$local_manifest" "$global_manifest"
   prefix="$(mktemp -d)"
   PHIG_CLI_UNMANAGED_INSTALL="$prefix/bin" \
     PHIG_CLI_DOWNLOAD_URL="file://$PWD/target/distrib" \
     sh target/distrib/phig-cli-installer.sh
   "$prefix/bin/phig" version --json
   ```

3. Test `install.sh`, `phig update --check`, the PTY selector, and the primary
   views on macOS and Linux CI. Review `git diff` and ensure the tree is clean.
4. Merge the release commit to `main` and wait for CI. cargo-dist PRs validate
   the release plan (`pr-run-mode = "plan"`). Release-related CI changes rehearse
   the native macOS archive and installer; tags build all four target archives.
   Complete the local rehearsal above before tagging.

The local benchmark script gates warm snapshot p95 at 500 ms by default, which
a warm shared dev machine can exceed regardless of release content. When that
happens, compare the same fixture against the previous release tag's binary
(the current release should not be slower) and re-run with CI's gates,
`--snapshot-p95-ms 1000 --first-frame-p95-ms 1500`; CI's platform gate is the
release gate.

### CI coverage and cost

Every PR and push to `main` runs format, strict Clippy, documentation tests,
shell lint, dependency policy, all-feature tests on Linux and macOS, and the
performance regression gate. Linux unit/integration tests run once, in the
platform job. Local `just check` is useful fast feedback; Actions also validates
the pinned toolchain and clean checkout on both operating systems.

Package verification, the extra release-plan check on `main`, and the real
macOS cargo-dist installer fixture run when manifests, lockfiles, toolchain,
build/release configuration, installer/update/CLI code, scripts, workflows, or
release documentation change. They also run weekly and with
`gh workflow run ci.yml`. Missing comparison history falls back to full checks.
The generated release workflow validates PR plans and builds all artifacts on
version tags. Regenerate it with `dist generate` after changing dist settings.

The local Beads pre-push hook performs issue bookkeeping; it does not run Rust
tests. Its default timeout is 300 seconds. `BEADS_HOOK_TIMEOUT=15 git push`
limits that bookkeeping wait without suppressing Actions.

### Publish

The version tag must exactly match the Cargo package version:

```sh
git tag -s v1.1.1 -m 'phig 1.1.1'
git push origin v1.1.1
```

A signed tag is preferred; an annotated tag is acceptable only when signing is
unavailable and the release record documents that exception. Tag publication
starts `.github/workflows/release.yml`. Do not manually create a competing
release.

After the workflow succeeds:

```sh
gh release view v1.1.1 --repo phall1/phig
gh release download v1.1.1 --repo phall1/phig --dir /tmp/phig-release
(cd /tmp/phig-release && shasum -a 256 -c phig-cli-aarch64-apple-darwin.tar.xz.sha256)
gh attestation verify /tmp/phig-release/phig-cli-aarch64-apple-darwin.tar.xz \
  --repo phall1/phig
```

Test both public onboarding routes in clean temporary homes:

```sh
env HOME="$(mktemp -d)" PATH="/usr/bin:/bin:/usr/sbin:/sbin" \
  /bin/sh -c 'curl --proto "=https" --tlsv1.2 -LsSf https://raw.githubusercontent.com/phall1/phig/main/install.sh | sh'
brew update
brew install phall1/tap/phig
phig version
phig update --check
```

The tap updates on its own schedule, so `brew install` serves the previous
version for up to fifteen minutes after the release publishes. Once it has run,
verify `Formula/phig.rb` in the tap points to the new release and that its CI is
healthy. The crates.io route publishes through `publish-crates` on the same tag
push; on a manual cut, confirm it in the run logs:

```sh
gh run list --workflow publish-crates --limit 1
cargo search phig-cli --limit 1
CARGO_HOME="$(mktemp -d)" cargo install phig-cli --version 1.1.1 --locked
```

The release is not complete while the README's Cargo command is unavailable.
Publishing to crates.io and pushing a tag are irreversible external actions;
each requires maintainer authority.

## Failure and recovery

Never move or replace a published version tag. cargo-dist may leave a visible
GitHub Release with incomplete assets when a tag workflow fails; do not assume it
remains a draft. Stop onboarding, mark the release incomplete, fix the workflow,
and rerun the failed jobs against the same immutable tag. If consumers could
have installed a broken artifact or the tagged source itself is wrong, preserve
the record and publish a patch version instead. A tap that has not caught up is
not a release failure and never blocks one; if it stays stale, run the tap's own
`Update packages` workflow and read its log. Do not hand-edit the formula or its
checksums.

Release assets are built from tagged source with the root `rust-toolchain.toml`
pinning Rust 1.88.0; cargo-dist and rustup honor that repository override on each
native runner. They are not claimed bit-for-bit reproducible across different
host machines. `.cargo/config.toml` pins `MACOSX_DEPLOYMENT_TARGET=12.0`, and
cargo-dist pins the Linux installer floor to glibc 2.31; both are release-plan
invariants.