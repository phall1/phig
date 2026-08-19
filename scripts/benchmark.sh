#!/bin/sh
set -eu

repository=${1-${TMPDIR:-/tmp}/phig-benchmark}
if [ "$#" -gt 0 ]; then shift; fi
commits=${1-1000}
if [ "$#" -gt 0 ]; then shift; fi
CDPATH=''
export CDPATH
root=$(cd -- "$(dirname -- "$0")/.." && pwd)
exec python3 "$root/scripts/benchmark.py" --repository "$repository" --commits "$commits" "$@"
