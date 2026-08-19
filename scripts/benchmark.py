#!/usr/bin/env python3
"""Reproducible phig release benchmark using only the Python standard library."""

from __future__ import annotations

import argparse
import json
import math
import os
import platform
import pty
import resource
import select
import signal
import statistics
import struct
import subprocess
import sys
import termios
import time
from pathlib import Path


def command_output(argv: list[str], cwd: Path | None = None) -> str:
    return subprocess.check_output(argv, cwd=cwd, text=True).strip()


def percentile(values: list[float], quantile: float) -> float:
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * quantile) - 1)]


def timed_command(argv: list[str], cwd: Path | None = None) -> float:
    started = time.perf_counter_ns()
    subprocess.run(argv, cwd=cwd, check=True, stdout=subprocess.DEVNULL)
    return (time.perf_counter_ns() - started) / 1_000_000


def ensure_fixture(root: Path, repository: Path, commits: int) -> None:
    try:
        probe = subprocess.run(
            ["git", "-C", str(repository), "rev-list", "--count", "HEAD"],
            check=True,
            capture_output=True,
            text=True,
        )
        current = int(probe.stdout.strip())
    except (subprocess.CalledProcessError, FileNotFoundError, ValueError):
        current = -1
    if current != commits:
        subprocess.run(
            [str(root / "scripts/make-benchmark-repo.sh"), str(repository), str(commits)],
            check=True,
        )


def rss_mib(value: float) -> float:
    # Darwin reports bytes; Linux and the BSDs used by CI report KiB.
    return value / (1024 * 1024) if sys.platform == "darwin" else value / 1024


def first_useful_frame(
    binary: Path, repository: Path, marker: bytes, timeout: float
) -> tuple[float, float]:
    master, slave = pty.openpty()
    try:
        # A stable 100x28 viewport exercises the normal stacked layout.
        import fcntl

        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 28, 100, 0, 0))
        env = os.environ.copy()
        env.update({"TERM": "xterm-256color", "NO_COLOR": "1"})
        started = time.perf_counter_ns()
        process = subprocess.Popen(
            [str(binary), "--repo", str(repository), "--no-config", "--no-alt-screen"],
            stdin=slave,
            stdout=slave,
            stderr=slave,
            env=env,
            start_new_session=True,
            close_fds=True,
        )
        os.close(slave)
        slave = -1
        output = bytearray()
        child_usage: resource.struct_rusage | None = None

        def reap(options: int) -> resource.struct_rusage | None:
            nonlocal child_usage
            waited, status, usage = os.wait4(process.pid, options)
            if waited == process.pid:
                process.returncode = os.waitstatus_to_exitcode(status)
                child_usage = usage
                return usage
            return None

        deadline = time.monotonic() + timeout
        observed_ms: float | None = None
        while time.monotonic() < deadline:
            ready, _, _ = select.select([master], [], [], 0.05)
            if ready:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    chunk = b""
                if chunk:
                    output.extend(chunk)
                    if marker in output:
                        observed_ms = (time.perf_counter_ns() - started) / 1_000_000
                        break
            if reap(os.WNOHANG) is not None:
                break
        if observed_ms is None:
            if child_usage is None:
                os.killpg(process.pid, signal.SIGKILL)
                reap(0)
            raise RuntimeError(f"first useful frame omitted {marker!r}: {bytes(output[-400:])!r}")

        # q is idempotent at the root; retry it until exit under a bounded deadline.
        exit_deadline = time.monotonic() + 3
        while child_usage is None and time.monotonic() < exit_deadline:
            try:
                os.write(master, b"q")
            except OSError:
                pass
            select.select([], [], [], 0.04)
            reap(os.WNOHANG)
        if child_usage is None:
            os.killpg(process.pid, signal.SIGKILL)
            reap(0)
            raise RuntimeError("phig did not exit after first-frame measurement")
        if process.returncode != 0:
            raise RuntimeError(f"phig exited {process.returncode} after first useful frame")
        return observed_ms, rss_mib(float(child_usage.ru_maxrss))
    finally:
        if slave >= 0:
            os.close(slave)
        os.close(master)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--repository", type=Path, default=Path(os.environ.get("TMPDIR", "/tmp")) / "phig-benchmark")
    parser.add_argument("--commits", type=int, default=1000)
    parser.add_argument("--snapshot-samples", type=int, default=20)
    parser.add_argument("--pty-samples", type=int, default=10)
    parser.add_argument("--snapshot-p95-ms", type=float, default=500)
    parser.add_argument("--first-frame-p95-ms", type=float, default=1000)
    parser.add_argument("--binary-max-mib", type=float, default=15)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    if min(args.commits, args.snapshot_samples, args.pty_samples) < 1:
        parser.error("commit and sample counts must be positive")

    root = Path(__file__).resolve().parent.parent
    ensure_fixture(root, args.repository, args.commits)
    if not args.skip_build:
        subprocess.run(["cargo", "build", "--release", "--locked", "--quiet"], cwd=root, check=True)
    binary = root / "target/release/phig"
    if not binary.is_file():
        raise RuntimeError(f"release binary is missing: {binary}")

    snapshot = [str(binary), "--repo", str(args.repository), "snapshot", "log"]
    for _ in range(3):
        subprocess.run(snapshot, check=True, stdout=subprocess.DEVNULL)
    snapshot_ms = [timed_command(snapshot) for _ in range(args.snapshot_samples)]
    marker = f"benchmark {args.commits - 1}".encode()
    frame_samples = [
        first_useful_frame(binary, args.repository, marker, timeout=10)
        for _ in range(args.pty_samples)
    ]
    frame_ms = [sample[0] for sample in frame_samples]
    frame_rss_mib = [sample[1] for sample in frame_samples]
    size_mib = binary.stat().st_size / (1024 * 1024)
    result = {
        "source_commit": command_output(["git", "rev-parse", "HEAD"], root),
        "source_dirty": bool(command_output(["git", "status", "--porcelain"], root)),
        "fixture_commit": command_output(["git", "-C", str(args.repository), "rev-parse", "HEAD"]),
        "fixture_commits": args.commits,
        "fixture_paths": 100,
        "platform": platform.platform(),
        "machine": platform.machine(),
        "python": platform.python_version(),
        "git": command_output(["git", "--version"]),
        "samples": {"snapshot": args.snapshot_samples, "first_useful_frame": args.pty_samples},
        "warm_snapshot_ms": {
            "p50": round(statistics.median(snapshot_ms), 3),
            "p95": round(percentile(snapshot_ms, 0.95), 3),
        },
        "pty_first_useful_frame_ms": {
            "p50": round(statistics.median(frame_ms), 3),
            "p95": round(percentile(frame_ms, 0.95), 3),
        },
        "release_binary": {"bytes": binary.stat().st_size, "mib": round(size_mib, 3)},
        "representative_phig_peak_rss_mib": round(max(frame_rss_mib), 3),
        "gates": {
            "warm_snapshot_p95_ms": args.snapshot_p95_ms,
            "first_useful_frame_p95_ms": args.first_frame_p95_ms,
            "binary_max_mib": args.binary_max_mib,
        },
    }
    failures = []
    if result["warm_snapshot_ms"]["p95"] > args.snapshot_p95_ms:
        failures.append("warm snapshot p95")
    if result["pty_first_useful_frame_ms"]["p95"] > args.first_frame_p95_ms:
        failures.append("PTY first useful frame p95")
    if size_mib > args.binary_max_mib:
        failures.append("release binary size")
    result["passed"] = not failures
    result["failures"] = failures
    print(json.dumps(result, sort_keys=True) if args.json else json.dumps(result, indent=2, sort_keys=True))
    return 0 if not failures else 1


if __name__ == "__main__":
    raise SystemExit(main())
