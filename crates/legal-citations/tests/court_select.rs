//! Court-level selection: one document, cites to several courts, a custom
//! selection of two of them (David's "a random Israeli district court and
//! the Eastern District of Texas in one doc").

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::registry::{registry, rule_files, validate, Registry};
use legal_citations::select::Pattern;
use legal_citations::{detect_with, Policy, Selector};

/// A test-only rule for a Tel Aviv District Court civil case number. Not in
/// the embedded registry.
const TLV_RULE: &str = r#"{
  "id": "foreign.il.dist.tlv.case_test",
  "kind": "foreign",
  "jurisdiction": "foreign.il.dist.tlv",
  "pattern": "\\bCC\\s+\\(TA\\)\\s+(?P<number>[0-9]{1,5}-[0-9]{2}-[0-9]{2})\\b",
  "normalize": "CC (TA) {number}",
  "authority_key": "foreign.il.dist.tlv/{number}",
  "priority": 60,
  "provenance": {
    "encodes": "test fixture",
    "opened": false,
    "source_url": "",
    "how_to_verify": "test only",
    "derived_from": [],
    "third_party": []
  },
  "examples": {
    "positive": [
      {"text": "See CC (TA) 1234-05-19.", "spans": [[4, 22]]},
      {"text": "CC (TA) 1-01-20", "spans": [[0, 15]]},
      {"text": "and CC (TA) 99-12-18 too", "spans": [[4, 20]]}
    ],
    "negative": ["CC (TA)", "CC 1234-05-19", "CC (TA) 12"]
  },
  "measured": {"precision": "unmeasured"},
  "note": ""
}"#;

const DOC: &str = "See Alpha Co. v. Beta Inc., 500 F. Supp. 3d 100 (E.D. Tex. 2020). \
Gamma v. Delta, 900 F.3d 1 (9th Cir. 2019). \
State v. Smith, 100 N.E.3d 5 (Ohio 2018). \
Plonit v. Ploni, CC (TA) 1234-05-19. \
Alpha Co., 500 F. Supp. 3d at 104.";

fn with_tlv() -> Registry {
    let mut files = rule_files().to_vec();
    files.push(("rules/foreign/foreign.il.dist.tlv.case_test.json", TLV_RULE));
    Registry::from_sources(&files)
}

fn picked(reg: &Registry, sel: &Selector) -> Vec<(String, String)> {
    let s = reg.select(sel).unwrap();
    detect_with(&s, DOC)
        .into_iter()
        .map(|c| (c.text, c.jurisdiction))
        .collect()
}

fn include(pats: &[&str]) -> Selector {
    Selector {
        include: pats.iter().map(|s| s.to_string()).collect(),
        ..Selector::default()
    }
}

#[test]
fn the_test_rule_keeps_the_contract() {
    let reg = with_tlv();
    let v: Vec<_> = validate(&reg)
        .into_iter()
        .filter(|v| v.rule.starts_with("foreign.il.dist"))
        .collect();
    assert!(v.is_empty(), "{v:#?}");
}

#[test]
fn a_district_court_and_an_israeli_court_in_one_document() {
    let reg = with_tlv();
    let all = picked(&reg, &Selector::default());
    let jur = |t: &str| {
        all.iter()
            .find(|(text, _)| text.contains(t))
            .map(|(_, j)| j.as_str())
            .unwrap()
    };
    assert_eq!(jur("(E.D. Tex. 2020)"), "us.fed.dist.txed");
    assert_eq!(jur("(9th Cir. 2019)"), "us.fed.app.ca9");
    assert_eq!(jur("(Ohio 2018)"), "us.oh.sup");
    assert_eq!(jur("CC (TA)"), "foreign.il.dist.tlv");

    // `--jurisdiction us.fed.dist.txed --jurisdiction 'foreign.il.dist*'`
    let want = vec![
        (
            "Alpha Co. v. Beta Inc., 500 F. Supp. 3d 100 (E.D. Tex. 2020)".to_string(),
            "us.fed.dist.txed".to_string(),
        ),
        (
            "CC (TA) 1234-05-19".to_string(),
            "foreign.il.dist.tlv".to_string(),
        ),
        (
            "Alpha Co., 500 F. Supp. 3d at 104".to_string(),
            "us.fed.dist.txed".to_string(),
        ),
    ];
    let got = picked(
        &reg,
        &include(&[
            "jurisdiction:us.fed.dist.txed",
            "jurisdiction:foreign.il.dist*",
        ]),
    );
    assert_eq!(got, want);
    // The `court:` alias, by id or as cited, selects the same.
    let got = picked(
        &reg,
        &include(&["court:E.D. Tex.", "court:foreign.il.dist*"]),
    );
    assert_eq!(got, want);
    // A policy file says the same thing.
    let policy: Policy = serde_json::from_str(
        r#"{"include": ["jurisdiction:us.fed.dist.txed", "jurisdiction:foreign.il.dist*"]}"#,
    )
    .unwrap();
    assert_eq!(picked(&reg, &policy.selector()), want);
}

#[test]
fn prefixes_still_select_the_level_and_exclude_wins() {
    let reg = registry();
    let texts = |sel: Selector| -> Vec<String> {
        let s = reg.select(&sel).unwrap();
        detect_with(&s, DOC).into_iter().map(|c| c.text).collect()
    };
    let dist = texts(include(&["jurisdiction:us.fed.dist"]));
    assert!(dist.iter().any(|t| t.contains("E.D. Tex.")), "{dist:?}");
    assert!(!dist.iter().any(|t| t.contains("9th Cir.")), "{dist:?}");
    let fed = texts(include(&["jurisdiction:us.fed"]));
    assert!(fed.iter().any(|t| t.contains("9th Cir.")), "{fed:?}");
    assert!(!fed.iter().any(|t| t.contains("(Ohio")), "{fed:?}");
    let ohio = texts(include(&["jurisdiction:us.oh"]));
    assert!(ohio.iter().any(|t| t.contains("(Ohio 2018)")), "{ohio:?}");
    assert_eq!(ohio.len(), 1, "{ohio:?}");
    let glob = texts(include(&["jurisdiction:us.fed.app.ca*"]));
    assert!(glob.iter().any(|t| t.contains("9th Cir.")), "{glob:?}");
    assert!(!glob.iter().any(|t| t.contains("E.D. Tex.")), "{glob:?}");
    // Exclude wins over a broader include.
    let no_tex = texts(Selector {
        include: vec!["jurisdiction:us.fed".into()],
        exclude: vec!["court:E.D. Tex.".into()],
        ..Selector::default()
    });
    assert!(no_tex.iter().any(|t| t.contains("9th Cir.")), "{no_tex:?}");
    assert!(!no_tex.iter().any(|t| t.contains("Tex.")), "{no_tex:?}");
}

#[test]
fn court_selectors_are_checked() {
    let reg = registry();
    // A court id no rule uses is fine: citations carry it.
    assert!(reg
        .select(&include(&["jurisdiction:us.fed.dist.nysd"]))
        .is_ok());
    // A typo is not.
    let e = reg
        .select(&include(&["jurisdiction:us.fed.dist.zzz"]))
        .unwrap_err();
    assert!(e.reason.contains("no rule or court"), "{e}");
    let e = Pattern::parse("court:Nowhere Ct.").unwrap_err();
    assert!(e.reason.contains("unknown court"), "{e}");
    let p = Pattern::parse("court:S.D.N.Y.").unwrap();
    assert_eq!(
        (p.namespace, p.glob.as_str()),
        ("jurisdiction", "us.fed.dist.nysd")
    );
    let p = Pattern::parse("court:us.tx*").unwrap();
    assert_eq!((p.namespace, p.glob.as_str()), ("jurisdiction", "us.tx*"));
}
