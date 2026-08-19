# Installation and updates

## Requirements

Phig 1.x supports macOS 12 or newer and glibc-based Linux/WSL on ARM64 or
x86-64. Release builds inherit `MACOSX_DEPLOYMENT_TARGET=12.0`; generated Linux
installers require glibc 2.31 or newer. musl-native and native Windows terminals
are not supported. Git 2.45.1 or newer is required.

## Homebrew

```sh
brew install phall1/tap/phig
phig version
```

Homebrew owns this installation. Upgrade or remove it with:

```sh
brew upgrade phig
brew uninstall phig
```

`phig update` asks `brew --prefix phig`, canonicalizes that formula's
`bin/phig`, and delegates to `brew upgrade phig` only when it is exactly the
running executable.

## Shell installer

The short bootstrap command downloads the checked-in bootstrap over TLS:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://raw.githubusercontent.com/phall1/phig/main/install.sh | sh
```

The bootstrap downloads `phig-cli-installer.sh` from the latest GitHub Release.
That versioned cargo-dist installer selects the platform archive and verifies
its embedded SHA-256 checksum before replacing `phig`. The bootstrap trusts the
installer delivered by GitHub over TLS; because the embedded checksum comes from
the same release authority, it is an integrity check rather than an independent
signature. GitHub attestations can be verified separately as described below.
The installer does not use sudo. By default it installs to Cargo's binary
directory, normally `~/.cargo/bin`. Ensure that directory is on `PATH`.

For an auditable two-step install or a custom prefix:

```sh
curl --proto '=https' --tlsv1.2 -LsSf \
  https://raw.githubusercontent.com/phall1/phig/main/install.sh -o phig-install.sh
less phig-install.sh
PHIG_VERSION=1.0.0 sh phig-install.sh --prefix "$HOME/.local" --yes
"$HOME/.local/bin/phig" version
rm phig-install.sh
```

`install.sh --help` is side-effect free. Rerunning it updates an existing release
installation safely. `PHIG_VERSION` accepts a release such as `1.0.0`; omit it
to install the latest stable release.

To uninstall a shell installation, locate it first and remove only that binary:

```sh
command -v phig
rm "$HOME/.cargo/bin/phig"       # default installer location
# or: rm "$HOME/.local/bin/phig" # example custom prefix
```

## Cargo

After the release has been published and verified on crates.io, install with
Rust 1.88 or newer:

```sh
cargo install phig-cli --locked
```

Publishing and clean-installing the crate is a required final release step, not
an automatic part of the GitHub cargo-dist workflow.

Update with `cargo install phig-cli --locked --force`; uninstall with
`cargo uninstall phig-cli`. A Cargo installation cannot be distinguished
reliably from a shell installation, so `phig update` uses the release installer
and keeps the current binary directory. Use Cargo directly if Cargo should keep
ownership of the installation.

## Explicit updates

Normal browsing is offline. Update checks happen only when requested:

```sh
phig update --check
phig update
```

`--check` queries GitHub's latest stable `v<semver>` tag and changes nothing.
`update` uses Homebrew only for the canonical formula binary. Other installs use
a mode-0700 staging directory beside the destination (therefore on the same
filesystem), ask cargo-dist for a flat unmanaged install, verify the staged
`phig version --json` exactly matches the selected tag, fsync where supported,
and atomically rename it over the current executable. A hard-linked private
backup permits rollback if post-install verification fails. Failed checks,
downloads, package managers, installers, verification, and replacement return
exit code 6 without reporting success.

Normal exits remove staging immediately. A hard interruption can leave only a
private `.phig-update-*` directory and cannot replace the old binary before the
candidate has verified. Later updates remove owned, mode-0700 staging older than
24 hours; recent or non-private lookalikes are never scavenged. If automatic
rollback itself fails, phig reports the preserved `previous-phig` path and adds
a `ROLLBACK_REQUIRED` marker; marked recovery directories are never scavenged
automatically. Phig never updates itself in the background.

## Independent verification

Every release contains per-archive SHA-256 files, a unified `sha256.sum`, and
GitHub artifact attestations. After downloading an archive and its checksum:

```sh
sha256sum --check phig-cli-x86_64-unknown-linux-gnu.tar.xz.sha256
# macOS:
shasum -a 256 -c phig-cli-aarch64-apple-darwin.tar.xz.sha256
```

With the GitHub CLI, verify build provenance before extracting:

```sh
gh attestation verify phig-cli-aarch64-apple-darwin.tar.xz \
  --repo phall1/phig
```

Checksums protect download integrity; attestations tie an artifact to the public
GitHub Actions build identity. Review the tag, release notes, and workflow when
your threat model requires source-to-binary confidence.

## Completions and manuals

Generate completions directly from the installed version:

```sh
phig completions bash >~/.local/share/bash-completion/completions/phig
phig completions zsh >~/.zfunc/_phig
phig completions fish >~/.config/fish/completions/phig.fish
```

Print the root manual with `phig manpage`, or write the complete root and
subcommand set with `phig manpage --output-dir ~/.local/share/man/man1`.
