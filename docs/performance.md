# Performance

Phig keeps Git work off the terminal thread, bounds queues and captured output,
and loads history incrementally. Performance claims come from the committed
fixture and standard-library harness rather than a developer's changing checkout.

## Release benchmark

```sh
scripts/benchmark.sh /tmp/phig-benchmark 1000 --json
```

The fixture contains 1,000 commits distributed across 100 paths. The Python
standard-library harness builds the locked release binary, warms the deterministic
`snapshot log` path, and records:

- 20 warm snapshot samples with p50/p95;
- 10 real PTY launches from process start to the first frame containing the
  newest commit, with p50/p95;
- release binary bytes/MiB;
- per-process peak RSS for each PTY-launched phig via `wait4`;
- source and fixture commits, source dirty state, platform, architecture, Git
  and Python versions, and sample counts.

The command fails if warm snapshot p95 exceeds 500 ms, PTY first-useful-frame p95
exceeds 1,000 ms, or the release binary exceeds 15 MiB. CI runs five samples of
each timing path on macOS and Linux; `just release-check` runs the full 20/10 gate.
The PTY metric is a real rendered history frame, not terminal setup alone.

The harness captures each PTY phig process with `wait4`, normalizes Darwin bytes
and Linux KiB to MiB, and reports the largest sample. It is still reported rather
than gated because allocator and OS accounting differ across platforms.

## 1.0.0 release-candidate measurement

This result is from the clean release candidate at commit
`ecaf54b1673d997fc5034698b2586b7bcc123430`, not from a published tag. It was
measured on 2026-08-19 with Apple M4 Pro/arm64, macOS 26.5.1, Git 2.55.0, Python
3.14.7, and fixture commit `7b46e04b7727121e1369ba05c6f223822c4d6ab5`:

```text
warm snapshot, 20 samples:       p50 133.609 ms  p95 139.570 ms
PTY first useful frame, 10:      p50 162.799 ms  p95 166.430 ms
release binary:                  2.316 MiB (2,428,960 bytes)
representative phig peak RSS:    9.125 MiB
```

The measured release gates passed. Replace or supplement this record with the
exact tag commit after publication; do not silently relabel candidate evidence
as tagged-release evidence.

## Targets not yet claimed

The 150 ms first-useful-frame architectural target, cached key-to-paint latency,
idle CPU, cancellation latency, and a 100,000-commit/10,000-path stress fixture
remain unmeasured optimization goals. They are explicitly not 1.0 release claims
or blockers. Any future claim must record the fixture, exact commit, platform,
cache state, sample count, and measurement method together.
