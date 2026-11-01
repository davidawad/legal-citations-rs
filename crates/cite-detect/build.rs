//! Embed the git commit in `cite-detect --version` as `CITE_DETECT_GIT_SHA`.
//! An explicit `CITE_DETECT_GIT_SHA` in the build environment wins (builds from
//! a source archive have no `.git`); otherwise `git rev-parse`, else `unknown`.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?.trim().to_owned();
    (!s.is_empty()).then_some(s)
}

fn main() {
    println!("cargo:rerun-if-env-changed=CITE_DETECT_GIT_SHA");
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        // Rebuild when HEAD moves (a checkout, or a commit on the current branch).
        println!("cargo:rerun-if-changed={dir}/HEAD");
        println!("cargo:rerun-if-changed={dir}/packed-refs");
        if let Some(head_ref) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={dir}/{head_ref}");
        }
    }
    let sha = std::env::var("CITE_DETECT_GIT_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| git(&["rev-parse", "--short=12", "HEAD"]))
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=CITE_DETECT_GIT_SHA={sha}");
}
