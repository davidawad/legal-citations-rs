//! Embeds every `rules/**/*.json` (the repository's top-level, MIT-licensed
//! rule data) and `profiles/*.json` file into the binary, sorted by path, so
//! the registry has no runtime file dependency. Validation is NOT done here:
//! it lives in `registry::validate` where it is tested, covered and callable
//! from `cite-detect rules validate`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
    Ok(())
}

/// Write `OUT_DIR/<out>` declaring `static <name>` over every JSON file in
/// `base/dir`, keyed by its path relative to `base` (`rules/case/x.json`).
fn embed(base: &Path, dir: &str, name: &str, out: &str) -> std::io::Result<()> {
    let root = base.join(dir);
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    collect(&root, &mut files)?;
    files.sort();
    let mut src = format!("/// `(relative path, file contents)` of every embedded `{dir}/` file.\npub(crate) static {name}: &[(&str, &str)] = &[\n");
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
        let rel = file.strip_prefix(base).map_err(std::io::Error::other)?;
        let _ = writeln!(
            src,
            "    ({:?}, include_str!({:?})),",
            rel.to_string_lossy().replace('\\', "/"),
            file.to_string_lossy()
        );
    }
    src.push_str("];\n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").map_err(std::io::Error::other)?);
    std::fs::write(out_dir.join(out), src)
}

fn main() -> std::io::Result<()> {
    let manifest =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(std::io::Error::other)?);
    let workspace = manifest.join("../..");
    embed(&workspace, "rules", "RULE_FILES", "rule_files.rs")?;
    embed(&manifest, "profiles", "PROFILE_FILES", "profile_files.rs")
}
