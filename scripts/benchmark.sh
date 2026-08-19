#!/bin/sh
set -eu

repository=${1-${TMPDIR:-/tmp}/phig-benchmark}
commits=${2-500}
CDPATH=
export CDPATH
root=$(cd -- "$(dirname -- "$0")/.." && pwd)

if [ ! -d "$repository/.git" ]; then
  "$root/scripts/make-benchmark-repo.sh" "$repository" "$commits"
fi
cargo build --manifest-path "$root/Cargo.toml" --release --locked --quiet
binary=$root/target/release/phig

# Warm Git's filesystem cache before measuring the deterministic machine path.
"$binary" --repo "$repository" snapshot log >/dev/null
if command -v hyperfine >/dev/null 2>&1; then
  hyperfine --warmup 3 --runs 20 \
    "'$binary' --repo '$repository' snapshot log >/dev/null"
else
  echo 'hyperfine not found; reporting one portable wall-clock sample' >&2
  command time -p "$binary" --repo "$repository" snapshot log >/dev/null
fi
wc -c "$binary" | awk '{ printf "release binary: %.2f MiB (%s bytes)\n", $1 / 1048576, $1 }'
