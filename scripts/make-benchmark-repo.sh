#!/bin/sh
set -eu

repository=${1-}
commits=${2-500}
if [ -z "$repository" ]; then
  echo "usage: $0 REPOSITORY [COMMITS]" >&2
  exit 2
fi
case "$commits" in ''|*[!0-9]*) echo "COMMITS must be an integer" >&2; exit 2 ;; esac

rm -rf "$repository"
mkdir -p "$repository"
git -C "$repository" init --quiet -b main
git -C "$repository" config user.name 'phig benchmark'
git -C "$repository" config user.email 'benchmark@example.invalid'
printf '0\n' >"$repository/history.txt"
git -C "$repository" add history.txt
git -C "$repository" commit --quiet -m 'benchmark 0'

i=1
while [ "$i" -lt "$commits" ]; do
  printf '%s\n' "$i" >>"$repository/history.txt"
  git -C "$repository" add history.txt
  git -C "$repository" commit --quiet -m "benchmark $i"
  i=$((i + 1))
done
printf '%s\n' "created $commits commits in $repository"
