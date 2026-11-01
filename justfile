set shell := ["bash", "-cu"]

artifacts := env_var_or_default("ARTIFACTS", "target/artifacts")

default:
    just --list

# Everything a change must pass (heavy: run in a box on carbon).
check: fmt-check clippy test doc deny schemas-check

test:
    cargo nextest run --workspace --all-features
    cargo test --workspace --all-features --doc

clippy:
    cargo clippy --workspace --all-targets --all-features -- -D warnings

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps

deny:
    cargo deny check

# Regenerate the committed JSON Schemas from the code.
schemas:
    mkdir -p schema; for s in report finding citation rule policy authorities bundle vectors; do cargo run -q -p cite-detect -- schema $s > schema/$s.schema.json; done

schemas-check: schemas
    git diff --exit-code -- schema

# The rule contract only (fast feedback while editing rules/).
rules:
    cargo run -q -p cite-detect -- rules validate

# Release artifact for other languages (rule data is MIT, rules/LICENSE): every rule in
# dist/legal-citation-rules.json, test vectors from the rule examples, schemas, LICENSE, NOTICE.
dist:
    rm -rf dist && mkdir -p dist
    cargo run -q -p cite-detect -- rules validate > /dev/null
    cargo run -q -p cite-detect -- rules export > dist/legal-citation-rules.json
    cargo run -q -p cite-detect -- rules vectors > dist/test-vectors.json
    cp schema/bundle.schema.json dist/legal-citation-rules.schema.json
    cp schema/vectors.schema.json dist/test-vectors.schema.json
    cp schema/rule.schema.json dist/rule.schema.json
    cp rules/LICENSE dist/LICENSE && cp rules/NOTICE dist/NOTICE

cov:
    mkdir -p {{artifacts}}
    cargo llvm-cov nextest --workspace --all-features --fail-under-lines 97 --lcov --output-path {{artifacts}}/coverage.lcov

# Memory-capped, one job, one shard per file with incremental notes (dot-1ij4xz: mutation runs must not exhaust the box).
mutants:
    ARTIFACTS={{artifacts}} NOTES={{artifacts}}/notes bash scripts/mutants-shards.sh

fuzz seconds="300":
    mkdir -p {{artifacts}}/fuzz
    cd fuzz && for t in detect detect_markdown policy_parse; do cargo +nightly fuzz run $t -- -max_total_time={{seconds}} -artifact_prefix={{artifacts}}/fuzz/; done

gitleaks:
    gitleaks detect --no-banner --redact

ci: check cov mutants fuzz gitleaks

# Indigo Book format coverage: regenerate docs/measurements/indigo-coverage.{md,json}
# and fail if detected counts drop below docs/measurements/indigo-baseline.json.
indigo-coverage:
    cargo run -q -p cite-detect -- coverage corpus/formats/indigo.jsonl > docs/measurements/indigo-coverage.md
    cargo run -q -p cite-detect -- coverage corpus/formats/indigo.jsonl --format json --baseline docs/measurements/indigo-baseline.json > docs/measurements/indigo-coverage.json

# Regenerate the 163 rules authored from the Indigo inventory (`--check` fails if stale).
indigo-rules *args:
    cargo run -q -p xtask-rules -- indigo {{args}}

# Refuse to land CANDIDATE unless its tree has a PASS gate record (scripts/box-verify.sh writes them).
land-check candidate="HEAD" base="":
    sh scripts/land-check.sh {{candidate}} {{base}}

# Needs a clean tree on main and a `## [VERSION]` section in CHANGELOG.md. It does not push;
# the land step pushes the tag.
# Run the registry gate (`just check`) and create the annotated tag vVERSION.
release VERSION:
    #!/usr/bin/env bash
    set -euo pipefail
    test -z "$(git status --porcelain)" || { echo "release: working tree is not clean" >&2; exit 1; }
    branch="$(git symbolic-ref --short -q HEAD || true)"
    test "$branch" = main || { echo "release: on '${branch:-detached HEAD}', not main" >&2; exit 1; }
    grep -qF '## [{{VERSION}}]' CHANGELOG.md || { echo "release: CHANGELOG.md has no '## [{{VERSION}}]' section" >&2; exit 1; }
    ! git rev-parse -q --verify "refs/tags/v{{VERSION}}" >/dev/null || { echo "release: tag v{{VERSION}} exists" >&2; exit 1; }
    just check
    test -z "$(git status --porcelain)" || { echo "release: just check changed the tree" >&2; exit 1; }
    git tag -a "v{{VERSION}}" -m "legal-citations-rs {{VERSION}}"
    echo "tagged v{{VERSION}} at $(git rev-parse --short HEAD); push with: git push origin v{{VERSION}}"
