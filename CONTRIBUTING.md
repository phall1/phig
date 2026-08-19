# Contributing to phig

Thank you for helping make Git inspection faster and clearer.

## Before changing code

1. Search existing issues and open a focused issue for behavior or product
   changes.
2. Read `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`, and `docs/UX.md`.
3. Preserve the version-1 read-only boundary. Mutation and embedded-agent
   proposals require an approved design before implementation.

Small bug fixes and documentation corrections can go directly to a pull
request. Security vulnerabilities must use
[private vulnerability reporting](https://github.com/phall1/phig/security/advisories/new).

## Development

Install Rust 1.88, Git 2.45.1, `just`, cargo-deny, and ShellCheck. Then:

```sh
just check
cargo deny check
shellcheck install.sh scripts/*.sh
cargo build --release --locked
```

Changes to parsers or reducers need narrow unit tests. User-visible behavior also
needs an integration or PTY/UI-path test. Machine stdout must remain byte-clean,
and protocol changes must update the schema and golden fixtures. Avoid adding a
dependency when the standard library or an existing crate is sufficient.

Use `scripts/make-benchmark-repo.sh` and `scripts/benchmark.sh` for performance
work. Release configuration changes must pass `dist generate --check` and
`dist plan` with cargo-dist 0.32.0.

## Pull requests

Keep commits reviewable and use conventional summaries such as
`fix(tui): restore terminal after suspend`. A pull request should explain the
user problem, scope, tests, and any remaining risk. Do not include generated
editor state or unrelated formatting.

By contributing, you agree that your contribution is licensed under the
project's MIT OR Apache-2.0 terms and that you will follow the code of conduct.
