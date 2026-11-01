//! `xtask-rules [--check]`: regenerate `rules/case/generated/` from the
//! vendored reporters-db, or (with `--check`) fail if it is stale.
//! `xtask-rules indigo [--check]`: the same for the rules authored from the
//! Indigo Book inventory.

use std::process::ExitCode;

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");
    let indigo = std::env::args().nth(1).is_some_and(|a| a == "indigo");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let (ok, out, err) = if indigo {
        xtask_rules::indigo::cli(&root, check)
    } else {
        xtask_rules::cli(&root, check)
    };
    print!("{out}");
    eprint!("{err}");
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
