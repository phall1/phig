# Performance

Phig keeps Git work off the terminal thread, bounds queues and captured output,
and loads history incrementally. Performance claims come from the committed
fixture and standard-library harness rather than a developer's changing checkout.

## Release benchmark

### Cached interaction probe

```sh
MACOSX_DEPLOYMENT_TARGET=12.0 cargo test --release --lib interaction_benchmark -- --ignored --nocapture
```

This opt-in probe builds 50,000 commits (25,000 independent tips followed by
their roots) and 50,000 changed paths. It reports 40-sample p50/p95 times for a
fresh history graph plus frame, cached navigation plus frame, uncached fuzzy
ranking, and cached file-picker movement plus frame. Frames use Ratatui's
120×28 test backend. The first optimized test build can take several minutes;
normal `just check` skips this probe.

The fresh/cached comparison measures this implementation with and without cache
reuse. It is a rendering/ranking CPU measurement, not end-to-end terminal input
latency or a comparison against an older release. Graphs retain a computed
prefix for the terminal session and clear on history replacement, scope changes,
or lane-width changes. File matches are retained only while their overlay is
open and refresh when the active patch or query changes. Borrowed file metadata
is compared before reuse so direct mutations through the public `App` fields
cannot leave stale paths or jump anchors; unchanged entries avoid allocation
and fuzzy scoring.

On 2026-09-07, the final Rust 1.88.0 optimized build on the Apple Silicon macOS
development host, including public-mutation cache validation, produced:

| Probe | p50 | p95 |
| --- | ---: | ---: |
| 50k commits, fresh graph + frame | 12.415 ms | 12.922 ms |
| 50k commits, cached navigation + frame | 0.157 ms | 0.165 ms |
| 50k files, uncached `srcrs` ranking | 18.460 ms | 25.134 ms |
| 50k files, cached movement + frame | 0.641 ms | 0.671 ms |

These are not portable timing gates or claimed speedups over the previous
release. Both cached p95 values were below the 16 ms cached-interaction target.
Typing a new query still pays the ranking cost; arrow movement and redraw reuse
it. An earlier run under heavy host contention was much slower; these final
numbers supersede that run.

### Startup and machine snapshots

Functional PTY readiness checks keep five-second defaults in CI. If the host is
busy enough that the installed release also misses that deadline, an explicit
local override avoids mistaking host contention for an application regression:

```sh
PHIG_TEST_READINESS_MULTIPLIER=3 NEXTEST_TEST_THREADS=2 MACOSX_DEPLOYMENT_TARGET=12.0 just check
```

Only readiness waits are extended; cancellation/cleanup assertions and the
release performance gates below retain their normal bounds. On assertion
failure, PTY guards kill the test process group and reap its direct child.

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

The local release command fails if warm snapshot p95 exceeds 500 ms, PTY
first-useful-frame p95 exceeds 1,000 ms, or the release binary exceeds 15 MiB.
Shared CI runners use a 1,000/1,500 ms regression envelope with five samples to
absorb noisy virtualization and cold ephemeral disks; `just release-check` runs
the stricter full 20/10 gate. The PTY metric is a real rendered history frame,
not terminal setup alone.

The harness captures each PTY phig process with `wait4`, normalizes Darwin bytes
and Linux KiB to MiB, and reports the largest sample. It is still reported rather
than gated because allocator and OS accounting differ across platforms.

## 1.1.0 release-candidate measurement

This result compares the clean `c997d17` candidate with the installed 1.0.0
release on the same fixture and host, using 30 warm snapshots and 20 real-PTY
samples for each binary:

| Metric | 1.0.0 | 1.1.0 | Change |
| --- | ---: | ---: | ---: |
| warm snapshot p50 | 210.139 ms | 240.276 ms | +14.3% |
| warm snapshot p95 | 258.415 ms | 281.480 ms | +8.9% |
| first useful frame p50 | 231.043 ms | 223.231 ms | -3.4% |
| first useful frame p95 | 296.167 ms | 317.436 ms | +7.2% |
| release binary | 2.153 MiB | 2.332 MiB | +8.3% |
| representative peak RSS | 8.156 MiB | 8.172 MiB | +0.2% |

The fixture was `90ec4a2d04ceddab52d3a7a8e032d714a54a0d63` with
1,000 commits and 100 paths on Apple Silicon macOS 26.5.1 using Git 2.55.0 and
Python 3.14.7. Both binaries were measured consecutively under the same elevated
host load; the comparison remains within the 15% latency and 10% binary-size
budgets, and the candidate remains comfortably inside every absolute release
gate. Earlier isolated candidate runs measured 129–139 ms snapshot p95 and
154–155 ms PTY p95, illustrating why the release decision uses a paired run
rather than comparing unrelated host conditions.

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
