#!/usr/bin/env bash
# cargo-mutants, one shard per source file, one job, memory-capped (dot-1ij4xz).
# A whole-workspace run was OOM-killed (exit 137) with nothing saved, so each
# shard appends its outcome to $NOTES/mutants-<file>.txt as soon as it ends:
# a kill loses at most the shard in flight.
# Usage: scripts/mutants-shards.sh [FILE...]   (default: every crates/*/src/*.rs)
# Env: NOTES, ARTIFACTS, MUTANTS_VMEM_KB (8000000), MUTANTS_TIMEOUT (120),
#      MUTANTS_BUDGET_SECS (10800: stop starting new shards after 3 hours).
set -uo pipefail
NOTES="${NOTES:-/work/artifacts/notes}"
ARTIFACTS="${ARTIFACTS:-/work/artifacts}"
VMEM="${MUTANTS_VMEM_KB:-8000000}"
TIMEOUT="${MUTANTS_TIMEOUT:-120}"
BUDGET="${MUTANTS_BUDGET_SECS:-10800}"
cd "$(dirname "$0")/.." || exit 2
mkdir -p "$NOTES" "$ARTIFACTS/mutants"
if [ $# -gt 0 ]; then files=("$@"); else files=(crates/*/src/*.rs); fi
start=$SECONDS
status=0
baseline=run # later shards reuse the first passing baseline
for f in "${files[@]}"; do
  key="$(echo "$f" | sed 's#^crates/##; s#/src/#-#; s#\.rs$##')"
  out="$NOTES/mutants-$key.txt"
  if [ $((SECONDS - start)) -ge "$BUDGET" ]; then
    echo "SKIPPED (budget ${BUDGET}s exhausted)" >>"$out"
    status=1
    continue
  fi
  t0=$SECONDS
  echo "== $f $(date -u +%FT%TZ)" >>"$out"
  (
    ulimit -v "$VMEM"
    cargo mutants --workspace --file "$f" --jobs 1 --timeout "$TIMEOUT" \
      --test-tool nextest --baseline "$baseline" --output "$ARTIFACTS/mutants/$key"
  ) >>"$out" 2>&1
  rc=$?
  [ "$rc" = 0 ] || [ "$rc" = 2 ] || [ "$rc" = 3 ] && baseline=skip
  dir="$ARTIFACTS/mutants/$key/mutants.out"
  {
    echo "exit $rc after $((SECONDS - t0))s"
    for o in caught missed timeout unviable; do
      echo "$o $(wc -l <"$dir/$o.txt" 2>/dev/null || echo '?')"
    done
    echo "-- missed:"
    cat "$dir/missed.txt" 2>/dev/null
    echo "-- timeout:"
    cat "$dir/timeout.txt" 2>/dev/null
  } >>"$out"
  # cargo-mutants exits 2 for missed mutants, 3 for timeouts: findings, not crashes.
  case $rc in 0 | 2 | 3) ;; *) status=1 ;; esac
done
exit $status
