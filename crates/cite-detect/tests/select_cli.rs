//! CLI rule selection: --profile, --include, --exclude, --jurisdiction,
//! `profiles list|show` and `rules list --select` (lcr-z18).

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

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

const BRIEF: &str = "See Ohio Rev. Code § 2907.02 and 28 U.S.C. § 1331. Id. at 5. \
See State v. Smith, 100 Ohio St.3d 1 (2003). Id. at 3.\n";

fn cited(args: &[&str]) -> Vec<String> {
    let mut a = vec!["--format", "json", "--input", "text"];
    a.extend_from_slice(args);
    let o = run(&a, BRIEF);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    v["files"][0]["document"]["citations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn jurisdiction_shorthand_lints_only_that_jurisdiction() {
    assert_eq!(
        cited(&["--jurisdiction", "us.oh"]),
        vec![
            "Ohio Rev. Code § 2907.02",
            "State v. Smith, 100 Ohio St.3d 1 (2003)",
            "Id. at 3"
        ]
    );
    assert_eq!(cited(&[]).len(), 5);
}

/// lcr-k5o: `--jurisdiction` follows the court parenthetical, not just the
/// rule (`N.E.2d` and Westlaw are not Ohio rules).
#[test]
fn jurisdiction_shorthand_follows_the_court_parenthetical() {
    let text = "Smith v. Jones, 100 N.E.2d 1 (Ohio 2010). \
Doe v. Roe, 2010 WL 123456 (Ohio Ct. App. 2010). \
Fed v. Case, 250 F.3d 1 (9th Cir. 2001).\n";
    let o = run(
        &[
            "--format",
            "json",
            "--input",
            "text",
            "--jurisdiction",
            "us.oh",
        ],
        text,
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    let cites = v["files"][0]["document"]["citations"].as_array().unwrap();
    let got: Vec<(&str, &str)> = cites
        .iter()
        .map(|c| {
            (
                c["text"].as_str().unwrap(),
                c["rule_jurisdiction"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(got[0].0, "Smith v. Jones, 100 N.E.2d 1 (Ohio 2010)");
    assert_eq!(got[1].0, "Doe v. Roe, 2010 WL 123456 (Ohio Ct. App. 2010)");
    assert!(got.iter().all(|(_, rj)| *rj != "us.oh"), "{got:?}");
    assert_eq!(cites[0]["jurisdiction"], "us.oh.sup");
    assert_eq!(cites[1]["jurisdiction"], "us.oh.app");
}

#[test]
fn include_exclude_and_profile_flags() {
    assert_eq!(
        cited(&["--include", "kind:statute"]).len(),
        3,
        "two statutes and the Id. that follows one"
    );
    assert_eq!(
        cited(&["--select", "rule:us.fed.statute.usc"]),
        vec!["28 U.S.C. § 1331", "Id. at 5"]
    );
    assert_eq!(
        cited(&[
            "--include",
            "kind:statute",
            "--exclude",
            "jurisdiction:us.fed"
        ]),
        vec!["Ohio Rev. Code § 2907.02"]
    );
    assert_eq!(cited(&["--profile", "ohio"]).len(), 3);
    assert_eq!(
        cited(&["--profile", "ohio", "--exclude", "kind:case"]).len(),
        1
    );
}

#[test]
fn policy_file_selector_combines_with_flags() {
    let dir = std::env::temp_dir().join(format!("cite-detect-sel-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("policy.json");
    std::fs::write(
        &p,
        r#"{"include": ["jurisdiction:us.fed"], "profiles": []}"#,
    )
    .unwrap();
    let o = run(
        &[
            "--policy",
            p.to_str().unwrap(),
            "--format",
            "json",
            "--input",
            "text",
            "--fail-on",
            "never",
        ],
        BRIEF,
    );
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(
        v["files"][0]["document"]["citations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    std::fs::write(&p, r#"{"profiles": ["nope"]}"#).unwrap();
    let o = run(&["--policy", p.to_str().unwrap()], BRIEF);
    assert_eq!(o.status.code(), Some(3));
    assert!(err(&o).contains("unknown profile"), "{}", err(&o));
}

#[test]
fn unknown_selectors_and_profiles_exit_3() {
    for args in [
        &["--jurisdiction", "us.zz"][..],
        &["--include", "kind:cases"],
        &["--exclude", "planet:mars"],
        &["--profile", "nope"],
        &["rules", "list", "--select", "family:nope"],
        &["profiles", "show", "nope"],
    ] {
        let o = run(args, BRIEF);
        assert_eq!(o.status.code(), Some(3), "{args:?}: {}", err(&o));
        assert!(err(&o).contains("selector"), "{args:?}: {}", err(&o));
    }
}

#[test]
fn profiles_list_and_show() {
    let o = run(&["profiles", "list"], "");
    assert_eq!(o.status.code(), Some(0));
    for name in [
        "federal-litigation",
        "ohio",
        "ptab",
        "agency-all",
        "everything",
        "uae",
        "difc-adgm",
        "gcc",
        "israel",
        "mena",
    ] {
        assert!(out(&o).contains(name), "{name}");
    }
    assert!(out(&o).contains("18 profiles"));
    let o = run(&["profiles", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 18);
    let o = run(&["profiles", "show", "uspto-prosecution"], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("exclude: rule:us.fed.uspto.ptab_exhibit"));
    assert!(out(&o).contains("note:"));
    assert!(out(&o).contains("  us.fed.uspto.mpep"));
    let o = run(&["profiles", "show", "everything"], "");
    assert!(!out(&o).contains("exclude:"));
    let o = run(&["profiles", "--json", "show", "ptab"], "");
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert_eq!(v["profile"]["name"], "ptab");
    assert!(v["rules"].as_array().unwrap().len() > 5);
}

#[test]
fn rules_list_select_filters_and_validate_counts_profiles() {
    let o = run(
        &[
            "rules",
            "list",
            "--select",
            "family:agency",
            "--exclude",
            "kind:federal_register",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("us.fed.uspto.mpep"));
    assert!(!out(&o).contains("us.fed.federal_register"));
    let listed = out(&o).lines().filter(|l| l.contains(" agency ")).count();
    assert!(out(&o).contains(&format!("{listed} rules")), "{}", out(&o));
    let o = run(&["rules", "--json", "list", "--jurisdiction", "us.oh"], "");
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .all(|r| r["jurisdiction"].as_str().unwrap().starts_with("us.oh")));
    assert!(v[0]["family"].is_string());
    let o = run(&["rules", "validate"], "");
    assert!(out(&o).contains("18 profiles, 0 violations"), "{}", out(&o));
}

#[test]
fn unrecognized_citations_warn_and_fail_only_at_warn() {
    let text = "The claim rests on 45 Haw. Trib. 112.\n";
    let o = run(&["--input", "text"], text);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        out(&o).contains("CITE011 unrecognized-citation [Warn]"),
        "{}",
        out(&o)
    );
    let o = run(&["--input", "text", "--fail-on", "warn"], text);
    assert_eq!(o.status.code(), Some(1));
    let o = run(&["--input", "text", "--exclude", "kind:unrecognized"], text);
    assert!(out(&o).contains("0 citation(s)"), "{}", out(&o));
    let o = run(&["explain", "CITE011"], "");
    assert!(out(&o).contains("\"severity\": \"warn\""));
    let o = run(&["rules", "list", "--kind", "unrecognized"], "");
    assert!(out(&o).contains("4 rules"), "{}", out(&o));
    let o = run(&["authorities", "--input", "text"], text);
    assert!(
        out(&o).contains("### Unrecognized citations"),
        "{}",
        out(&o)
    );
}

const COURTS: &str = "See Alpha Co. v. Beta Inc., 500 F. Supp. 3d 100 (E.D. Tex. 2020). \
Gamma v. Delta, 900 F.3d 1 (9th Cir. 2019); State v. Smith, 100 N.E.3d 5 (Ohio 2018).\n";

fn court_cites(args: &[&str]) -> Vec<(String, String)> {
    let mut a = vec!["--format", "json", "--input", "text"];
    a.extend_from_slice(args);
    let o = run(&a, COURTS);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    v["files"][0]["document"]["citations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["text"].as_str().unwrap().to_string(),
                c["jurisdiction"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn court_level_jurisdictions_repeat_glob_and_exclude() {
    let tex = (
        "Alpha Co. v. Beta Inc., 500 F. Supp. 3d 100 (E.D. Tex. 2020)".to_string(),
        "us.fed.dist.txed".to_string(),
    );
    let ohio = (
        "State v. Smith, 100 N.E.3d 5 (Ohio 2018)".to_string(),
        "us.oh.sup".to_string(),
    );
    assert_eq!(
        court_cites(&[
            "--jurisdiction",
            "us.fed.dist.txed",
            "--jurisdiction",
            "us.oh*"
        ]),
        vec![tex.clone(), ohio.clone()]
    );
    assert_eq!(
        court_cites(&["--include", "court:E.D. Tex."]),
        vec![tex.clone()]
    );
    assert_eq!(
        court_cites(&["--jurisdiction", "us.*", "--exclude", "court:9th Cir."]),
        vec![tex, ohio]
    );
    let o = run(
        &["--input", "text", "--jurisdiction", "us.fed.dist.zzz"],
        COURTS,
    );
    assert_eq!(o.status.code(), Some(3));
    assert!(err(&o).contains("no rule or court"), "{}", err(&o));
}

#[test]
fn courts_lists_the_court_table() {
    let o = run(&["courts", "E.D. Tex"], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("us.fed.dist.txed"), "{}", out(&o));
    let o = run(&["courts", "--json", "us.fed.app.ca9"], "");
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    assert!(v
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["court"] == "9th Cir."));
}

fn doc_cites(args: &[&str], doc: &str) -> Vec<(String, String)> {
    let mut a = vec!["--format", "json", "--input", "text"];
    a.extend_from_slice(args);
    let o = run(&a, doc);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    let v: serde_json::Value = serde_json::from_str(&out(&o)).unwrap();
    v["files"][0]["document"]["citations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["text"].as_str().unwrap().to_string(),
                c["jurisdiction"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pair(text: &str, jurisdiction: &str) -> (String, String) {
    (text.to_string(), jurisdiction.to_string())
}

const ISRAEL_AND_TEXAS: &str = "See Alpha Co. v. Beta Inc., 500 F. Supp. 3d 100 (E.D. Tex. 2020); \
Gamma v. Delta, 900 F.3d 1 (9th Cir. 2019). Compare CC (TA) 1234/05 Cohen v. Levi \
and ת\"א (מחוזי ת\"א) 5678/06 כהן נ' לוי with CA 6821/93 United Mizrahi Bank Ltd. v. \
Migdal Coop. Village.\n";

#[test]
fn an_israeli_district_court_and_the_eastern_district_of_texas() {
    let all = doc_cites(&[], ISRAEL_AND_TEXAS);
    assert!(
        all.contains(&pair("CA 6821/93", "foreign.il.sup")),
        "{all:?}"
    );
    assert!(all.iter().any(|(_, j)| j == "us.fed.app.ca9"), "{all:?}");
    assert_eq!(
        doc_cites(
            &[
                "--jurisdiction",
                "us.fed.dist.txed",
                "--jurisdiction",
                "foreign.il.dist*"
            ],
            ISRAEL_AND_TEXAS
        ),
        vec![
            pair(
                "Alpha Co. v. Beta Inc., 500 F. Supp. 3d 100 (E.D. Tex. 2020)",
                "us.fed.dist.txed"
            ),
            pair("CC (TA) 1234/05", "foreign.il.dist.tlv"),
            pair("ת\"א (מחוזי ת\"א) 5678/06", "foreign.il.dist.tlv"),
        ]
    );
}

const GULF: &str = "See [2021] QIC (F) 8; Federal Law No. 5 of 1985; \
Smith v Jones [2016] DIFC CFI 12 at [29].\n";

#[test]
fn gcc_and_uae_profiles_split_the_gulf() {
    let qicdc = pair("[2021] QIC (F) 8", "foreign.qa.qicdc");
    let difc = pair("[2016] DIFC CFI 12 at [29]", "foreign.ae.difc.cfi");
    let all = doc_cites(&[], GULF);
    assert!(all.contains(&pair("Federal Law No. 5 of 1985", "foreign.ae")));
    assert_eq!(doc_cites(&["--profile", "gcc"], GULF), vec![qicdc.clone()]);
    let uae = doc_cites(&["--profile", "uae"], GULF);
    assert!(uae.contains(&difc), "{uae:?}");
    assert!(!uae.contains(&qicdc), "{uae:?}");
    assert_eq!(
        doc_cites(&["--jurisdiction", "foreign.ae.difc.cfi"], GULF),
        vec![difc.clone()]
    );
    assert_eq!(doc_cites(&["--profile", "difc-adgm"], GULF), vec![difc]);
    let o = run(&["profiles", "show", "mena"], "");
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert!(out(&o).contains("foreign.il.dist.tlv.he"), "{}", out(&o));
}
