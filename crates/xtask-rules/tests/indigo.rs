//! The rules authored from the Indigo inventory are what
//! `cargo run -p xtask-rules -- indigo` generates, and the registry embeds
//! every one of them.

#![allow(clippy::unwrap_used)]

use legal_citations::registry;
use std::path::Path;
use xtask_rules::indigo::{run, AUTHORED_BY};

#[test]
fn committed_indigo_rules_are_fresh() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (n, stale) = run(&root, true).unwrap();
    assert!(
        stale.is_empty(),
        "stale (run `cargo run -p xtask-rules -- indigo`): {stale:?}"
    );
    assert_eq!(n, 163);
    let embedded = registry()
        .rules
        .iter()
        .filter(|r| r.def.note.starts_with(AUTHORED_BY))
        .count();
    assert_eq!(embedded, n);
}
