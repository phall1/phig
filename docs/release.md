# Release process

GitHub Releases are the distribution authority. cargo-dist 0.32.0 builds four
archives, per-archive and unified SHA-256 checksums, a shell installer, source
tarball, Homebrew formula, and GitHub attestations. Stable releases publish the
formula to `phall1/homebrew-tap`.

## One-time repository setup

The `phall1/phig` repository must have Actions enabled and a
`HOMEBREW_TAP_TOKEN` Actions secret. The token needs contents write access only
to `phall1/homebrew-tap`. GitHub's generated `GITHUB_TOKEN` creates releases and
attestations. Private vulnerability reporting should be enabled.

The release workflow intentionally has no crates.io credential. Publishing and
clean-installing `phig-cli` is a separate, explicit, required final release step.

## Prepare

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
4. Merge the release commit to `main` and wait for CI. The cargo-dist pull-request
   job uses `pr-run-mode = "upload"`, so all four native archives and global
   installers must build and upload successfully before an irreversible tag.

## Publish

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

Verify `Formula/phig.rb` in the tap points to the new release and that its CI is
healthy. Then publish and verify the required crates.io route:

```sh
cargo publish --locked
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
the record and publish a patch version instead. A Homebrew publication failure
can be rerun after correcting `HOMEBREW_TAP_TOKEN`; do not hand-edit checksums.

Release assets are built from tagged source with the root `rust-toolchain.toml`
pinning Rust 1.88.0; cargo-dist and rustup honor that repository override on each
native runner. They are not claimed bit-for-bit reproducible across different
host machines. `.cargo/config.toml` pins `MACOSX_DEPLOYMENT_TARGET=12.0`, and
cargo-dist pins the Linux installer floor to glibc 2.31; both are release-plan
invariants.
