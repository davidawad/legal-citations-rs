//! Report contents: header hashes, summary counts, markdown escaping and the
//! `explain` payload. These pin what consumers read from a report, not just
//! its shape.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_cite-detect"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn json(args: &[&str], stdin: &str) -> serde_json::Value {
    serde_json::from_str(&out(&run(args, stdin))).unwrap()
}

fn tmp(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cite-detect-report-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

/// Reference FNV-1a 64, the documented hash for reproducible reports.
fn fnv1a(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[test]
fn header_policy_hash_is_fnv1a_of_the_effective_policy() {
    let v = json(&["--format", "json"], "");
    let policy = serde_json::to_string(&legal_citations::Policy::default()).unwrap();
    assert_eq!(v["header"]["policy_hash"], fnv1a(&policy));
    let changed = legal_citations::Policy {
        min_confidence: 0.75,
        ..Default::default()
    };
    let v = json(&["--format", "json", "--min-confidence", "0.75"], "");
    assert_eq!(
        v["header"]["policy_hash"],
        fnv1a(&serde_json::to_string(&changed).unwrap())
    );
}

#[test]
fn summary_counts_files_links_kinds_rules_and_suppressions() {
    let linked = tmp(
        "linked.md",
        "[28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331) and 42 U.S.C. § 1983.\n",
    );
    let suppressed = tmp(
        "suppressed.md",
        "<!-- cite-detect: ignore CITE001 reason=\"quoted\" -->\nFed. R. Evid. 501\n",
    );
    let v = json(
        &[
            "--require-links",
            "--format",
            "json",
            linked.to_str().unwrap(),
            suppressed.to_str().unwrap(),
        ],
        "",
    );
    let s = &v["summary"];
    assert_eq!(s["files"], 2);
    assert_eq!(s["citations"], 3);
    assert_eq!(s["linked"], 1);
    assert_eq!(s["by_kind"]["statute"], 2);
    assert_eq!(s["by_kind"]["court_rule"], 1);
    assert_eq!(s["by_rule"]["us.fed.statute.usc"], 2);
    assert_eq!(s["by_rule"]["us.fed.rules.evidence"], 1);
    assert_eq!(s["by_code"]["CITE001"], 1);
    assert_eq!(s["suppressed"], 1);
}

#[test]
fn md_report_escapes_pipes_and_newlines() {
    let file = tmp("a|b.md", "42 U.S.C.\n§ 1983\n");
    let o = run(
        &[
            "--require-links",
            "--format",
            "md-report",
            file.to_str().unwrap(),
        ],
        "",
    );
    let s = out(&o);
    assert!(s.contains("a\\|b.md\n"), "{s}");
    assert!(
        s.contains("| 1 | CITE001 unlinked | Error | 42 U.S.C. § 1983 |"),
        "{s}"
    );
}

#[test]
fn explain_rule_summary_is_for_that_rule() {
    let v = json(&["explain", "us.fed.uspto.mpep"], "");
    assert_eq!(v["summary"]["id"], "us.fed.uspto.mpep");
    assert_eq!(v["rule"]["id"], "us.fed.uspto.mpep");
}
