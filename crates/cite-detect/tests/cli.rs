//! End-to-end CLI behavior: formats, exit codes, subcommands.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cite-detect"))
}

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = bin()
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

fn tmp(name: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cite-detect-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

const VIOLATING: &str =
    "Jurisdiction lies under 28 U.S.C. § 1331. See Smith v. Jones, 123 F.3d 456 (9th Cir. 2001).\n";
const COMPLIANT: &str = "Jurisdiction lies under [28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331).\n";

#[test]
fn detect_mode_is_inventory_and_passes() {
    let o = run(&[], VIOLATING);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("CITE010"));
    assert!(out(&o).contains("2 citation(s), 0 linked"));
}

#[test]
fn require_links_fails_a_violating_document() {
    let o = run(&["--require-links"], VIOLATING);
    assert_eq!(o.status.code(), Some(1));
    let s = out(&o);
    assert!(s.contains("-:1:25: CITE001 unlinked [Error]"), "{s}");
    assert!(s.contains("fix: https://www.law.cornell.edu/uscode/text/28/1331"));
    assert!(s.trim_end().ends_with("FAIL"));
}

#[test]
fn require_links_passes_a_compliant_document() {
    let o = run(&["--require-links"], COMPLIANT);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).trim_end().ends_with("PASS"));
}

#[test]
fn fail_on_controls_the_exit_code() {
    assert_eq!(
        run(&["--require-links", "--fail-on", "never"], VIOLATING)
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        run(&["--fail-on", "info"], VIOLATING).status.code(),
        Some(1)
    );
    let warn_only = "[28 U.S.C. § 1331](https://blog.example.com/x)";
    assert_eq!(run(&["--require-links"], warn_only).status.code(), Some(0));
    assert_eq!(
        run(&["--require-links", "--fail-on", "warn"], warn_only)
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        run(
            &[
                "--require-links",
                "--allow-host",
                "example.com",
                "--fail-on",
                "warn"
            ],
            warn_only
        )
        .status
        .code(),
        Some(0)
    );
}

#[test]
fn jsonl_has_header_records_and_summary() {
    let o = run(&["--require-links", "--format", "jsonl"], VIOLATING);
    let lines: Vec<serde_json::Value> = out(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "header");
    assert_eq!(lines[0]["schema"], "cite-detect.report/1");
    assert_eq!(lines[0]["mode"], "lint");
    assert_eq!(lines[1]["type"], "citation");
    assert_eq!(lines[1]["linked"], false);
    assert_eq!(lines[1]["span"]["start"], 24);
    let last = lines.last().unwrap();
    assert_eq!(last["type"], "summary");
    assert_eq!(last["passed"], false);
    assert_eq!(last["by_code"]["CITE001"], 2);
    assert!(lines
        .iter()
        .any(|l| l["type"] == "finding" && l["code"] == "CITE001"));
}

#[test]
fn json_and_md_report_render() {
    let o = run(&["--require-links", "--format", "json"], VIOLATING);
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["files"][0]["file"], "-");
    assert_eq!(
        v["files"][0]["document"]["citations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(v["summary"]["citations"], 2);
    let o = run(
        &["--require-links", "--format", "md-report"],
        "a | b 42 U.S.C. § 1983",
    );
    let s = out(&o);
    assert!(s.starts_with("# cite-detect report"));
    assert!(s.contains("| 1 | CITE001 unlinked | Error |"), "{s}");
    assert!(s.contains("**FAIL**"));
}

#[test]
fn files_kinds_input_and_policy() {
    let md = tmp("brief.md", "[x](https://www.law.cornell.edu/uscode/text/28/1331) 42 U.S.C. § 1983 and Fed. R. Evid. 501\n");
    let txt = tmp(
        "brief.txt",
        "[28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331)\n",
    );
    let o = run(
        &[
            "--require-links",
            "--format",
            "jsonl",
            "--kinds",
            "court_rule",
            md.to_str().unwrap(),
        ],
        "",
    );
    let records: Vec<serde_json::Value> = out(&o)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let findings: Vec<&serde_json::Value> =
        records.iter().filter(|r| r["type"] == "finding").collect();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["kind"], "court_rule");
    assert_eq!(findings[0]["rule_id"], "us.fed.rules.evidence");
    // .txt is plain text: the bracketed link is not a link.
    let o = run(&["--require-links", txt.to_str().unwrap()], "");
    assert_eq!(o.status.code(), Some(1));
    let o = run(
        &["--require-links", "--input", "md", txt.to_str().unwrap()],
        "",
    );
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let o = run(
        &["--input", "text", "-"],
        "[28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331)",
    );
    assert!(out(&o).contains("0 linked"));
    let policy = tmp("policy.json", r#"{"link_schemes": {"toa": "accept"}}"#);
    let o = run(
        &["--policy", policy.to_str().unwrap()],
        "[28 U.S.C. § 1331](toa:usc)",
    );
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    let o = run(
        &["--require-links", "--accept-scheme", "TOA"],
        "[28 U.S.C. § 1331](toa:usc)",
    );
    assert_eq!(o.status.code(), Some(0));
    let o = run(
        &[
            "--require-links",
            "--min-confidence",
            "0.99",
            "--fail-on",
            "info",
        ],
        "42 U.S.C. § 1983",
    );
    assert!(out(&o).contains("CITE020"));
}

#[test]
fn exit_codes_for_bad_usage_config_and_input() {
    assert_eq!(run(&["--kinds", "nope"], "").status.code(), Some(2));
    assert_eq!(run(&["--min-confidence", "2"], "").status.code(), Some(2));
    assert_eq!(run(&["--bogus-flag"], "").status.code(), Some(2));
    assert_eq!(
        run(&["--policy", "/nonexistent/policy.json"], "")
            .status
            .code(),
        Some(3)
    );
    let bad = tmp("bad-policy.json", "{\"bogus\": 1}");
    assert_eq!(
        run(&["--policy", bad.to_str().unwrap()], "").status.code(),
        Some(3)
    );
    let o = run(&["/nonexistent/file.md"], "");
    assert_eq!(o.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&o.stderr).contains("/nonexistent/file.md"));
    assert_eq!(run(&["--help"], "").status.code(), Some(0));
    assert_eq!(run(&["--version"], "").status.code(), Some(0));
}

#[test]
fn version_names_package_version_and_git_sha() {
    let o = run(&["--version"], "");
    assert_eq!(o.status.code(), Some(0));
    let sha = env!("CITE_DETECT_GIT_SHA");
    assert_eq!(
        out(&o),
        format!("cite-detect {} ({sha})\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(
        sha == "unknown" || (sha.len() >= 7 && sha.chars().all(|c| c.is_ascii_hexdigit())),
        "{sha}"
    );
}

#[test]
fn rules_subcommands() {
    let o = run(&["rules"], "");
    assert!(out(&o).contains("us.fed.statute.usc"));
    assert!(out(&o).contains("unverified"));
    assert!(out(&o).contains("retired"));
    let o = run(&["rules", "--json", "list", "--kind", "court_rule"], "");
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["kind"] == "court_rule"));
    assert_eq!(
        run(&["rules", "list", "--kind", "nope"], "").status.code(),
        Some(2)
    );
    let o = run(&["rules", "show", "us.fed.statute.usc"], "");
    assert!(out(&o).contains("\"authority_key\": \"us.usc/{title}/{section}\""));
    assert_eq!(run(&["rules", "show", "nope"], "").status.code(), Some(2));
    let o = run(&["rules", "validate"], "");
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert!(out(&o).contains("0 violations, 0 portability warnings"));
    let o = run(&["rules", "--json", "validate"], "");
    assert_eq!(out(&o).trim(), "[]");
}

#[test]
fn rules_export_and_vectors_for_other_languages() {
    let o = run(&["rules", "export"], "");
    assert_eq!(o.status.code(), Some(0));
    let b: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(b["format"], "legal-citation-rules");
    assert_eq!(b["license"], "MIT");
    let usc = b["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"]["id"] == "us.fed.statute.usc")
        .unwrap();
    assert_eq!(usc["path"], "rules/statute/us.fed.statute.usc.json");
    assert!(usc["rule"]["pattern"]
        .as_str()
        .unwrap()
        .contains("(?P<title>"));
    assert!(usc["js"]["pattern"].as_str().unwrap().contains("(?<title>"));
    assert_eq!(usc["js"]["flags"], "u");

    let o = run(&["rules", "vectors"], "");
    assert_eq!(o.status.code(), Some(0));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["format"], "legal-citation-rules/test-vectors");
    assert_eq!(v["rules_hash"], b["rules_hash"]);
    let vectors = v["vectors"].as_array().unwrap();
    assert!(vectors.iter().any(|x| x["polarity"] == "positive"));
    assert!(vectors
        .iter()
        .filter(|x| x["polarity"] == "negative")
        .all(|x| x["spans"].as_array().unwrap().is_empty()));
}

#[test]
fn explain_codes_and_rules() {
    let o = run(&["explain", "CITE004"], "");
    assert!(out(&o).contains("incongruent"));
    let o = run(&["explain", "us.fed.uspto.mpep"], "");
    assert!(out(&o).contains("Manual of Patent Examining Procedure"));
    let o = run(&["explain", "nothing"], "");
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("CITE001"));
}

#[test]
fn inspect_every_stage() {
    let md = "*See* [28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331) and 42 U.S.C. § 1983.";
    let o = run(&["inspect", "-", "--stage", "mask"], md);
    assert!(!out(&o).contains('*') && !out(&o).contains("https"));
    assert_eq!(out(&o).len(), md.len());
    let o = run(&["inspect", "-", "--stage", "recognize"], md);
    assert!(out(&o).contains("us.fed.statute.usc"));
    let o = run(&["inspect", "-", "--stage", "document"], md);
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["links"].as_array().unwrap().len(), 1);
    let o = run(&["inspect", "-", "--stage", "findings"], md);
    assert!(out(&o).contains("CITE001"));
    let o = run(&["inspect", "-", "--input", "text", "--stage", "mask"], md);
    assert_eq!(out(&o), md);
    let o = run(
        &["inspect", "-", "--input", "text", "--stage", "recognize"],
        md,
    );
    assert!(out(&o).contains("us.fed.statute.usc"));
}

#[test]
fn schemas_are_emitted_and_match_committed_copies() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema");
    for (which, file) in [
        ("report", "report.schema.json"),
        ("finding", "finding.schema.json"),
        ("citation", "citation.schema.json"),
        ("rule", "rule.schema.json"),
        ("policy", "policy.schema.json"),
        ("authorities", "authorities.schema.json"),
        ("bundle", "bundle.schema.json"),
        ("vectors", "vectors.schema.json"),
    ] {
        let o = run(&["schema", which], "");
        assert_eq!(o.status.code(), Some(0));
        let generated = out(&o);
        let committed = std::fs::read_to_string(root.join(file)).unwrap_or_default();
        assert_eq!(
            generated, committed,
            "schema/{file} is stale: run `cite-detect schema {which} > schema/{file}`"
        );
    }
}

#[test]
fn fix_plans_and_applies() {
    let md = tmp(
        "fix.md",
        "See 42 U.S.C. § 1983 and Smith v. Jones, 123 F.3d 456 (9th Cir. 2001).\n",
    );
    let o = run(&["fix", md.to_str().unwrap()], "");
    assert_eq!(out(&o).trim(), "[]", "no opened rules yet: nothing is safe");
    let o = run(&["fix", "--include-unverified", md.to_str().unwrap()], "");
    let plan: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(plan.as_array().unwrap().len(), 2);
    let o = run(
        &[
            "fix",
            "--apply",
            "--include-unverified",
            md.to_str().unwrap(),
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    let fixed = std::fs::read_to_string(&md).unwrap();
    assert_eq!(
        fixed,
        "See [42 U.S.C. § 1983](https://www.law.cornell.edu/uscode/text/42/1983) and [Smith v. Jones, 123 F.3d 456 (9th Cir. 2001)](https://www.courtlistener.com/c/F.3d/123/456/).\n"
    );
    let o = run(&["--require-links", md.to_str().unwrap()], "");
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
    assert_eq!(run(&["fix", "--apply", "-"], "").status.code(), Some(2));
}

#[test]
fn coverage_reports_and_gates_on_a_baseline() {
    let inv = tmp(
        "inv.jsonl",
        r#"{"section":"T1 A","jurisdiction":"us.fed","kind":"case","form":"f","examples":["Smith v. Jones, 501 F.3d 12 (1st Cir. 2007)"]}
{"section":"T1 B","jurisdiction":"us.fed","kind":"statute","form":"f","examples":["Nothing 12 Here"]}
"#,
    );
    let inv = inv.to_str().unwrap();
    let o = run(&["coverage", inv], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(
        out(&o).contains("| formats | 2 | 1 | 1 | 50.0% |"),
        "{}",
        out(&o)
    );
    let o = run(&["coverage", inv, "--format", "json"], "");
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["formats"]["detected"], 1);
    // Write a baseline, pass against it, fail against a higher one.
    let base = tmp("base.json", "");
    let b = base.to_str().unwrap();
    let o = run(&["coverage", inv, "--baseline", b, "--update-baseline"], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(std::fs::read_to_string(&base)
        .unwrap()
        .contains("\"formats_detected\": 1"));
    assert_eq!(
        run(&["coverage", inv, "--baseline", b], "").status.code(),
        Some(0)
    );
    std::fs::write(&base, r#"{"formats_detected": 2, "examples_detected": 2}"#).unwrap();
    let o = run(&["coverage", inv, "--baseline", b], "");
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("detected formats dropped"));
    // Bad baseline, bad inventory, missing file.
    std::fs::write(&base, "{").unwrap();
    assert_eq!(
        run(&["coverage", inv, "--baseline", b], "").status.code(),
        Some(3)
    );
    let missing = format!("{b}.missing");
    assert_eq!(
        run(&["coverage", inv, "--baseline", &missing], "")
            .status
            .code(),
        Some(3)
    );
    let bad = tmp("bad.jsonl", "{");
    assert_eq!(
        run(&["coverage", bad.to_str().unwrap()], "").status.code(),
        Some(4)
    );
    assert_eq!(
        run(&["coverage", "/nonexistent/x.jsonl"], "").status.code(),
        Some(4)
    );
    let o = run(&["coverage", "-"], "");
    assert_eq!(o.status.code(), Some(0));
    let dir = std::env::temp_dir().join("cite-detect-cov-no-such-dir/b.json");
    let o = run(
        &[
            "coverage",
            inv,
            "--baseline",
            dir.to_str().unwrap(),
            "--update-baseline",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(5));
}

/// README's coverage table is `cite-detect rules table`, byte for byte.
#[test]
fn readme_rules_table_is_fresh() {
    let o = run(&["rules", "table"], "");
    assert_eq!(o.status.code(), Some(0));
    let table = out(&o);
    let readme =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md")).unwrap();
    let start = readme.find(legal_citations::coverage::TABLE_START).unwrap();
    let end = readme.find(legal_citations::coverage::TABLE_END).unwrap()
        + legal_citations::coverage::TABLE_END.len();
    assert_eq!(
        &readme[start..end],
        table.trim_end(),
        "README table is stale: run `cite-detect rules table` and paste it"
    );
}
