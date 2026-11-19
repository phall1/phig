#!/bin/sh
set -eu

repository=${1-}
commits=${2-1000}
if [ -z "$repository" ]; then
  echo "usage: $0 REPOSITORY [COMMITS]" >&2
  exit 2
fi
case "$commits" in ''|*[!0-9]*) echo "COMMITS must be an integer" >&2; exit 2 ;; esac
if [ "$commits" -lt 1 ]; then
  echo "COMMITS must be positive" >&2
  exit 2
fi

rm -rf "$repository"
mkdir -p "$repository"
git -C "$repository" init --quiet -b main
git -C "$repository" config user.name 'phig benchmark'
git -C "$repository" config user.email 'benchmark@example.invalid'
# Keep fixture creation deterministic: automatic background packing can race
# the tight commit loop on shared CI runners.
git -C "$repository" config gc.auto 0
git -C "$repository" config maintenance.auto false
# Stamp the private fixture identity so benchmark reuse cannot silently accept
# an unrelated repository that merely has the same number of commits.
git -C "$repository" config phig.benchmark.version 2
git -C "$repository" config phig.benchmark.commits "$commits"
git -C "$repository" config phig.benchmark.paths 100
# One import process, rather than staging and committing through 2,000 forks.
# Fixed timestamps make identical fixture requests produce identical history.
awk -v commits="$commits" '
  function file(path, content) {
    printf "M 100644 inline files/path-%03d.txt\ndata %d\n%s\n", path, length(content), content
  }
  BEGIN {
    print "feature done"
    for (i = 0; i < commits; i++) {
      printf "commit refs/heads/main\ncommitter phig benchmark <benchmark@example.invalid> %.0f +0000\n", 1700000000 + i
      message = "benchmark " i
      printf "data %d\n%s\n", length(message), message
      if (i == 0) {
        for (p = 0; p < 100; p++) {
          contents[p] = "0\n"
          file(p, contents[p])
        }
      } else {
        p = i % 100
        contents[p] = contents[p] i "\n"
        file(p, contents[p])
      }
      print ""
    }
    print "done"
  }
' | git -C "$repository" fast-import --quiet
git -C "$repository" reset --quiet --mixed HEAD
git -C "$repository" checkout-index --all
printf '%s\n' "created $commits commits across 100 paths in $repository" >&2
