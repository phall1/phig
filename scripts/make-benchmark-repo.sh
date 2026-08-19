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
mkdir -p "$repository/files"
git -C "$repository" init --quiet -b main
git -C "$repository" config user.name 'phig benchmark'
git -C "$repository" config user.email 'benchmark@example.invalid'
path=0
while [ "$path" -lt 100 ]; do
  printf '0\n' >"$repository/files/path-$(printf '%03d' "$path").txt"
  path=$((path + 1))
done
git -C "$repository" add files
git -C "$repository" commit --quiet -m 'benchmark 0'

i=1
while [ "$i" -lt "$commits" ]; do
  path=$((i % 100))
  printf '%s\n' "$i" >>"$repository/files/path-$(printf '%03d' "$path").txt"
  git -C "$repository" add files
  git -C "$repository" commit --quiet -m "benchmark $i"
  i=$((i + 1))
done
printf '%s\n' "created $commits commits across 100 paths in $repository"
