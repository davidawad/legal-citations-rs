#!/usr/bin/env bash
# Verify the workspace inside a box (carbon refuses heavy cargo).
# Usage: scripts/box-verify.sh [quick|full]
#   quick: fmt (applied), schemas (regenerated), clippy, tests, doc
#   full:  quick + deny + coverage + mutants (sharded per file) + fuzz
#          (FUZZ_SECONDS per target, default 120)
# Logs go to $NOTES (default /work/artifacts/notes) so the box returns them.
# The result and the tested tree go to $ARTIFACTS/gate.json (and, in CI, a
# git note on refs/notes/gate); scripts/land-check.sh reads them.
set -uo pipefail
MODE="${1:-quick}"
NOTES="${NOTES:-/work/artifacts/notes}"
mkdir -p "$NOTES"
cd "$(dirname "$0")/.." || exit 2
export CARGO_TERM_COLOR=never ARTIFACTS="${ARTIFACTS:-/work/artifacts}"
status=0
step() {
  local name="$1"; shift
  echo "== $name: $*" | tee -a "$NOTES/summary.log"
  if "$@" >"$NOTES/$name.log" 2>&1; then
    echo "   PASS $name" | tee -a "$NOTES/summary.log"
  else
    echo "   FAIL $name (exit $?) -> $NOTES/$name.log" | tee -a "$NOTES/summary.log"
    tail -n 60 "$NOTES/$name.log" | sed 's/^/      /' >>"$NOTES/summary.log"
    status=1
  fi
}
# The tree the steps tested: the working tree (after fmt and schemas), as git
# would commit it, read through a scratch index so the real one is untouched.
worktree_tree() {
  local idx
  idx="$(mktemp -u)"
  GIT_INDEX_FILE="$idx" git read-tree HEAD && GIT_INDEX_FILE="$idx" git add -A &&
    GIT_INDEX_FILE="$idx" git write-tree
  rm -f "$idx"
}
# Gate record: $ARTIFACTS/gate.json and, in CI (or with GATE_NOTES=1) when the
# tested tree is HEAD's, a note on refs/notes/gate (pushed to $GATE_NOTES_REMOTE
# when set). scripts/land-check.sh reads both.
write_gate_record() {
  local result="$1" head tree clean=false rec
  head="$(git rev-parse HEAD)" || return 1
  tree="$(worktree_tree)" || return 1
  [ "$tree" = "$(git rev-parse 'HEAD^{tree}')" ] && clean=true
  rec="$(printf '{"commit":"%s","tree":"%s","result":"%s","mode":"%s","clean":%s,"date":"%s"}' \
    "$head" "$tree" "$result" "$MODE" "$clean" "$(date -u +%Y-%m-%dT%H:%M:%SZ)")"
  mkdir -p "$ARTIFACTS"
  printf '%s\n' "$rec" >"$ARTIFACTS/gate.json"
  echo "gate record: $rec" | tee -a "$NOTES/summary.log"
  if [ "$clean" = true ] && { [ -n "${CI:-}" ] || [ "${GATE_NOTES:-0}" = 1 ]; }; then
    git -c user.name=gate -c user.email=gate@localhost notes --ref=gate append -m "$rec" HEAD || return 1
    if [ -n "${GATE_NOTES_REMOTE:-}" ]; then
      git push "$GATE_NOTES_REMOTE" refs/notes/gate ||
        echo "   WARN gate note not pushed to $GATE_NOTES_REMOTE" | tee -a "$NOTES/summary.log"
    fi
  fi
}
have() { command -v "$1" >/dev/null 2>&1 || cargo "$1" --version >/dev/null 2>&1; }
ensure() { have "$1" || cargo install --locked "$2" >>"$NOTES/install.log" 2>&1; }

: >"$NOTES/summary.log"
rustc --version >>"$NOTES/summary.log"
step fmt cargo fmt --all
step build cargo build --workspace --all-targets --all-features
# shellcheck disable=SC2317,SC2329  # invoked via step
gen_schemas() {
  mkdir -p schema
  local s
  for s in report finding citation rule policy authorities bundle vectors; do
    cargo run -q -p cite-detect -- schema "$s" >"schema/$s.schema.json" || return 1
  done
}
# shellcheck disable=SC2317,SC2329  # invoked via step
run_fuzz() {
  mkdir -p "$ARTIFACTS/fuzz"
  rustup toolchain install nightly >/dev/null 2>&1
  cargo +nightly install cargo-fuzz --locked >/dev/null 2>&1
  (cd fuzz && for t in detect detect_markdown policy_parse; do cargo +nightly fuzz run "$t" -- -max_total_time="${FUZZ_SECONDS:-120}" -artifact_prefix="$ARTIFACTS/fuzz/" || exit 1; done)
}
step schemas gen_schemas
tested_tree="$(worktree_tree 2>/dev/null)"
step rules cargo run -q -p cite-detect -- rules validate
step clippy cargo clippy --workspace --all-targets --all-features -- -D warnings
ensure nextest cargo-nextest
if have nextest; then
  step test cargo nextest run --workspace --all-features --no-fail-fast
else
  step test cargo test --workspace --all-features --no-fail-fast
fi
step doctest cargo test --workspace --all-features --doc
step doc env RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
if [ "$MODE" = full ]; then
  ensure deny cargo-deny && step deny cargo deny check
  ensure llvm-cov cargo-llvm-cov && step cov cargo llvm-cov nextest --workspace --all-features --fail-under-lines 97 --summary-only
  ensure mutants cargo-mutants && step mutants env NOTES="$NOTES" bash scripts/mutants-shards.sh
  step fuzz run_fuzz
fi
if [ -n "$tested_tree" ] && [ "$(worktree_tree)" != "$tested_tree" ]; then
  echo "   FAIL gate: the steps changed the tree they tested" | tee -a "$NOTES/summary.log"
  status=1
fi
result=$([ $status = 0 ] && echo PASS || echo FAIL)
if [ -n "$tested_tree" ]; then
  write_gate_record "$result" || { echo "   FAIL gate record" | tee -a "$NOTES/summary.log"; status=1; result=FAIL; }
else
  echo "   no gate record: not a git work tree" | tee -a "$NOTES/summary.log"
fi
echo "overall: $result" | tee -a "$NOTES/summary.log"
exit $status
