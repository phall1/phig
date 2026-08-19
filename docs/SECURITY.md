# Security model

Phig treats repositories, Git configuration, and terminal content as untrusted
inputs. It is read-only by design, but reading a repository can still trigger
helpers or inject terminal controls if implemented carelessly.

## Guarantees

- Git is launched with argument vectors, never interpolated shell commands.
- Normal browsing does not access the network.
- Every mode disables prompts, pagers, external diff drivers, textconv,
  fsmonitor, signing helpers, credential interaction, hooks, lazy fetches, and
  optional replacements where supported unless a future trusted action opts in.
- Repository text, ANSI/OSC escapes, and bidirectional controls are escaped
  before terminal rendering; phig applies diff styles itself.
- Machine stdout contains only the documented payload.
- Captured output, queues, history pages, patches, and subprocess lifetimes are
  bounded.
- Temporary and installed files are created with restrictive permissions and
  replaced atomically.
- Release installers verify checksums before installation, while GitHub build
  attestations provide separately verifiable provenance.

## Explicitly trusted actions

Opening an editor or external command, enabling textconv/external diff, and
running an update can execute software or use the network. Phig 1.0 does not
implement the first two actions. `phig update --check` contacts GitHub only when
invoked; `phig update` may execute `brew upgrade phig` or a versioned cargo-dist
installer downloaded over constrained TLS into a private same-filesystem staging
directory. Non-Homebrew updates require an exact version-tag match, verify the
staged and replaced executable, sync where supported, and retain a rollback link
until post-install verification. The automated path trusts GitHub's TLS-delivered
installer; its embedded archive checksum is same-authority integrity, not an
independent signature. Attestation verification is a separate documented user
action. These actions are never triggered by opening a repository.

## Supported boundary

Phig inherits the security of the installed Git executable and operating system.
It does not attempt to sandbox Git object parsing. Repositories suspected of
exploiting Git itself should be inspected using an appropriately patched Git and
an external sandbox.

## Reporting

Do not open a public issue for an undisclosed vulnerability. Use GitHub private
vulnerability reporting for `phall1/phig`. Include the phig and Git versions,
platform, reproduction repository or fixture, impact, and whether terminal
restoration or command execution is involved.
