//! `scripts/land-check.sh` against a scratch repository: it allows a
//! candidate only when its tree has a PASS gate record (in a record file or
//! a `refs/notes/gate` note) and it fast-forwards the base.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/land-check.sh")
}

struct Repo(PathBuf);

impl Repo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("land-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let repo = Self(dir);
        repo.git(&["init", "-q", "-b", "main"]);
        repo.commit("a", "1");
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@localhost"])
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn commit(&self, file: &str, body: &str) -> String {
        std::fs::write(self.0.join(file), body).unwrap();
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", file]);
        self.git(&["rev-parse", "HEAD"])
    }

    fn record(&self, rev: &str, result: &str, mode: &str) -> String {
        format!(
            r#"{{"commit":"{}","tree":"{}","result":"{result}","mode":"{mode}","clean":true,"date":"2026-10-23T00:00:00Z"}}"#,
            self.git(&["rev-parse", rev]),
            self.git(&["rev-parse", &format!("{rev}^{{tree}}")]),
        )
    }

    /// Exit code and stderr of `land-check.sh args`, reading only
    /// `records` (a file in the repo; empty: notes only).
    fn check(&self, args: &[&str], records: &str, mode: &str) -> (i32, String) {
        let out = Command::new("sh")
            .arg(script())
            .args(args)
            .env("GATE_RECORDS", records)
            .env("LAND_MODE", mode)
            .current_dir(&self.0)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned()
            + &String::from_utf8_lossy(&out.stderr);
        (out.status.code().unwrap(), text)
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn refuses_without_a_pass_record_for_the_tree() {
    let repo = Repo::new("records");
    let gate = repo.0.join("gate.json").display().to_string();
    let (code, err) = repo.check(&["HEAD", "-"], &gate, "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("no PASS gate record"), "{err}");

    std::fs::write(&gate, repo.record("HEAD", "FAIL", "quick")).unwrap();
    assert_eq!(repo.check(&["HEAD", "-"], &gate, "").0, 1);

    std::fs::write(&gate, repo.record("HEAD", "PASS", "quick")).unwrap();
    let (code, out) = repo.check(&["HEAD", "-"], &gate, "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("land-check: OK"), "{out}");
    // A full run is required: the quick record does not do.
    assert_eq!(repo.check(&["HEAD", "-"], &gate, "full").0, 1);

    // A new tree needs a new record; the same tree under a new commit
    // (reworded, rebased) keeps its PASS.
    repo.commit("b", "2");
    assert_eq!(repo.check(&["HEAD", "-"], &gate, "").0, 1);
    std::fs::write(&gate, repo.record("HEAD", "PASS", "full")).unwrap();
    repo.git(&["commit", "-q", "--amend", "-m", "reworded"]);
    assert_eq!(repo.check(&["HEAD", "-"], &gate, "full").0, 0);
    // The record file is ignored when GATE_RECORDS is empty.
    assert_eq!(repo.check(&["HEAD", "-"], "", "").0, 1);
}

#[test]
fn reads_records_from_gate_notes() {
    let repo = Repo::new("notes");
    let rec = repo.record("HEAD", "PASS", "quick");
    repo.git(&["notes", "--ref=gate", "append", "-m", "unrelated", "HEAD"]);
    repo.git(&["notes", "--ref=gate", "append", "-m", &rec, "HEAD"]);
    let (code, out) = repo.check(&[], "", "");
    assert_eq!(code, 0, "{out}");
    assert!(out.contains(&rec), "{out}");
}

#[test]
fn refuses_a_candidate_that_does_not_fast_forward_the_base() {
    let repo = Repo::new("base");
    let first = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "side"]);
    let side = repo.commit("side", "x");
    repo.git(&["checkout", "-q", "main"]);
    repo.commit("main", "y");
    let gate = repo.0.join("gate.json").display().to_string();
    std::fs::write(&gate, repo.record(&side, "PASS", "quick")).unwrap();
    let (code, err) = repo.check(&[&side, "main"], &gate, "");
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("not a fast-forward of main"), "{err}");
    assert_eq!(repo.check(&[&side, &first], &gate, "").0, 0);
    // No origin/main here: the default base check is skipped.
    assert_eq!(repo.check(&[&side], &gate, "").0, 0);
}

#[test]
fn usage_errors_exit_2() {
    let repo = Repo::new("usage");
    assert_eq!(repo.check(&["no-such-rev"], "", "").0, 2);
    assert_eq!(repo.check(&["HEAD", "-", "extra"], "", "").0, 2);
}
