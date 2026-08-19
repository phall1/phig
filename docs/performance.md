# Performance

Phig keeps all Git work off the terminal thread, bounds queues and captured
output, and loads history incrementally. Performance regressions should be
measured against a generated repository rather than a developer's changing
checkout.

## Reproduce

```sh
scripts/make-benchmark-repo.sh /tmp/phig-benchmark 500
scripts/benchmark.sh /tmp/phig-benchmark 500
```

`benchmark.sh` builds the locked release profile, warms the deterministic log
snapshot, runs 20 samples with `hyperfine` when available (or one portable
`time -p` sample), and reports stripped binary size. The fixture contains one
line-appending commit per revision and no remotes.

For interactive changes, also use the PTY tests and record cold/warm
startup-to-first-frame, key-to-paint, idle CPU, resident memory, and cancellation
latency described in `docs/PRODUCT.md`. A machine snapshot benchmark does not
substitute for those UI measurements.

## 1.0.0 pre-release candidate smoke

This is a pre-release baseline, not a result measured from the eventual
`v1.0.0` tag. It was measured from the release working tree based on commit
`169ed31fc2bd` on 2026-08-19, using an Apple M4 Pro, macOS/Darwin 25.5.0, Git
2.55.0, Rust 1.88, a 500-commit generated fixture, and a warm filesystem cache:

```text
phig snapshot log: real 0.10 s, user 0.02 s, sys 0.01 s (one time -p sample)
stripped release binary: 2.30 MiB (2,412,256 bytes)
```

This is a reproducible candidate smoke measurement, not a statistically valid
p95 claim or a tagged-release benchmark. Replace or supplement it with the exact
tag commit after publication. CI guards correctness on macOS and Linux;
performance claims should be updated only with the fixture, command, sample
count, cache state, tested commit, and hardware recorded together.
