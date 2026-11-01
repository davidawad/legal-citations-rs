//! `cite-detect authorities`: json, jsonl and md tables (lcr-bq9).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write as _;
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

const BRIEF: &str = "# Brief\n\nSee [Roe v. Wade, 410 U.S. 113, 115](https://www.courtlistener.com/c/U.S./410/113/), \
93 S. Ct. 705 (1973). Id. at 120.\n\nJurisdiction: 28 U.S.C. § 1332(a). Also Ohio Rev. Code § 2907.02 \
and Smith v. Jones, 100 F. Supp. 2d 1 (S.D.N.Y. 2000), aff'd, 250 F.3d 9 (2d Cir. 2001).\n\n\
Later: 410 U.S. at 116.\n";

#[test]
fn json_groups_by_authority_with_every_occurrence() {
    let o = run(&["authorities", "--format", "json"], BRIEF);
    assert_eq!(o.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["header"]["schema"], "cite-detect.authorities/1");
    let file = &v["files"][0];
    assert_eq!(file["file"], "-");
    let auth = file["authorities"].as_array().unwrap();
    let roe = auth
        .iter()
        .find(|a| a["key"] == "case/410/u.s./113")
        .unwrap();
    assert_eq!(roe["short_forms"], 2);
    assert_eq!(roe["occurrences"].as_array().unwrap().len(), 4);
    assert_eq!(roe["occurrences"][0]["span"]["line"], 3);
    assert_eq!(roe["occurrences"][3]["span"]["line"], 7);
    assert_eq!(roe["details"]["type"], "case");
    assert_eq!(roe["details"]["case_name"], "Roe v. Wade");
    assert_eq!(roe["pages_cited"], serde_json::json!(["115", "120", "116"]));
    let smith = auth
        .iter()
        .find(|a| a["key"] == "case/100/f.supp.2d/1")
        .unwrap();
    assert_eq!(smith["subsequent_history"][0]["phrase"], "aff'd");
    assert!(auth.iter().all(|a| a["key"] != "case/250/f.3d/9"));
}

#[test]
fn jsonl_has_header_records_and_summary() {
    let o = run(
        &["authorities", "--format", "jsonl", "--input", "md"],
        BRIEF,
    );
    let lines: Vec<serde_json::Value> = out(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "header");
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "summary");
    assert_eq!(last["authorities"], 4);
    assert_eq!(last["by_kind"]["case"], 2);
    assert_eq!(last["by_kind"]["statute"], 2);
    assert!(lines[1..lines.len() - 1]
        .iter()
        .all(|l| l["type"] == "authority" && l["file"] == "-"));
}

#[test]
fn md_is_a_table_per_kind_and_honors_selection() {
    let o = run(&["authorities"], BRIEF);
    let md = out(&o);
    assert!(md.starts_with("# Table of authorities"));
    assert!(md.contains("### Cases"));
    assert!(md.contains("### Statutes"));
    assert!(md.contains(
        "| [Roe v. Wade, 410 U.S. 113, 115, 93 S. Ct. 705 (1973)](https://www.courtlistener.com/c/U.S./410/113/) | 3 | 2 | 115, 120, 116 | 3, 7 |"
    ), "{md}");
    assert!(md.contains("(S.D.N.Y. 2000), aff'd, 250 F.3d 9 (2d Cir. 2001)"));
    assert!(
        md.contains("| Ohio Rev. Code § 2907.02 | 1 | 0 |  | 5 |"),
        "{md}"
    );
    let o = run(&["authorities", "--jurisdiction", "us.oh"], BRIEF);
    let md = out(&o);
    assert!(!md.contains("### Cases"), "{md}");
    assert!(md.contains("Ohio Rev. Code"));
    let o = run(&["authorities", "--profile", "nope"], BRIEF);
    assert_eq!(o.status.code(), Some(3));
    let o = run(&["authorities"], "No citations here.\n");
    assert!(out(&o).contains("No citations."));
    let o = run(&["authorities", "/no/such/file.md"], "");
    assert_eq!(o.status.code(), Some(4));
}

#[test]
fn citation_records_gain_details_and_urls_only() {
    let o = run(&["--format", "jsonl"], "See 28 U.S.C. § 1331.\n");
    let cite: serde_json::Value = out(&o)
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["type"] == "citation")
        .unwrap();
    assert_eq!(cite["details"]["type"], "code");
    assert_eq!(cite["details"]["sections"][0], "1331");
    assert_eq!(
        cite["urls"][0],
        "https://www.law.cornell.edu/uscode/text/28/1331"
    );
    for old in [
        "kind",
        "rule_id",
        "span",
        "core_span",
        "full_span",
        "text",
        "fields",
        "authority",
        "normalized",
        "confidence",
    ] {
        assert!(cite.get(old).is_some(), "{old} kept");
    }
}

#[test]
fn md_has_a_section_for_every_kind_present() {
    let text = "U.S. Const. art. I, § 8, cl. 3. Pub. L. No. 111-148. 40 C.F.R. § 52.21. \
86 Fed. Reg. 1234. Fed. R. Civ. P. 12. MPEP § 2141. Rev. Rul. 2020-1. ER-139. \
Restatement (Second) of Torts § 402A. 1155 U.N.T.S. 331. [2019] UKSC 12. No. 21-1234.\n";
    let o = run(&["authorities", "--input", "text"], text);
    let md = out(&o);
    for h in [
        "Constitutional provisions",
        "Session laws",
        "Regulations",
        "Federal Register",
        "Court rules",
        "Agency manuals",
        "Agency decisions",
        "Record",
        "Secondary sources",
        "Treaties",
        "Foreign authorities",
        "Dockets",
    ] {
        assert!(md.contains(&format!("### {h}\n")), "{h}: {md}");
    }
}
