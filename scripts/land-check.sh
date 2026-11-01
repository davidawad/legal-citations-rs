#!/bin/sh
# Refuse to land a commit whose tree has not passed the gate.
# Usage: scripts/land-check.sh [CANDIDATE [BASE]]
#   CANDIDATE  the commit to fast-forward main to (default HEAD)
#   BASE       the ref CANDIDATE must fast-forward (default origin/main when
#              it exists; '-' skips the check)
# Gate records are one-line JSON objects written by scripts/box-verify.sh:
#   {"commit":"..","tree":"..","result":"PASS","mode":"quick",...}
# They are read from the git notes on refs/notes/gate (any commit: a rebased
# commit with the same tree keeps its PASS) and from the record files in
# $GATE_RECORDS (colon-separated; unset means $ARTIFACTS/gate.json and
# target/artifacts/gate.json, empty means notes only).
# LAND_MODE=full accepts only records of a full gate run.
# Exit 0: allowed. 1: refused. 2: usage or git error.
set -u

refuse() {
  printf 'land-check: REFUSED: %s\n' "$1" >&2
  exit 1
}

candidate=${1:-HEAD}
if [ $# -gt 2 ]; then
  printf 'usage: %s [CANDIDATE [BASE]]\n' "$0" >&2
  exit 2
fi
if ! commit=$(git rev-parse -q --verify "$candidate^{commit}"); then
  printf 'land-check: %s is not a commit\n' "$candidate" >&2
  exit 2
fi
tree=$(git rev-parse "$commit^{tree}") || exit 2

if [ $# -ge 2 ]; then
  base=$2
elif git rev-parse -q --verify origin/main >/dev/null; then
  base=origin/main
else
  base=-
fi
if [ "$base" != - ] && ! git merge-base --is-ancestor "$base" "$commit"; then
  refuse "$candidate ($commit) is not a fast-forward of $base"
fi

records() {
  if git rev-parse -q --verify refs/notes/gate >/dev/null; then
    git notes --ref=gate list | while read -r blob _; do
      git cat-file blob "$blob"
      echo
    done
  fi
  old_ifs=$IFS
  IFS=:
  for f in ${GATE_RECORDS-${ARTIFACTS:-/work/artifacts}/gate.json:target/artifacts/gate.json}; do
    if [ -f "$f" ]; then
      cat "$f"
      echo
    fi
  done
  IFS=$old_ifs
}

pass=$(records | grep -F "\"tree\":\"$tree\"" | grep -F '"result":"PASS"' |
  grep -F "\"mode\":\"${LAND_MODE:-}" | head -n 1)
if [ -z "$pass" ]; then
  refuse "no PASS gate record for tree $tree of $candidate ($commit); run scripts/box-verify.sh on it"
fi
printf 'land-check: OK %s (tree %s): %s\n' "$commit" "$tree" "$pass"
