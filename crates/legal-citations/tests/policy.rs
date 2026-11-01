//! L5/L6: every finding code, severity overrides, suppressions, fixes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::document::{Binding, Via};
use legal_citations::policy::{code_info, SchemeRule, CODES};
use legal_citations::{
    detect_markdown, evaluate, lint_markdown, registry, Kind, Mode, Policy, Severity,
};

fn codes(md: &str, policy: &Policy) -> Vec<&'static str> {
    let (_, findings) = lint_markdown(md, policy);
    findings
        .iter()
        .filter(|f| !f.suppressed)
        .map(|f| f.code)
        .collect()
}

#[test]
fn compliant_document_has_no_blocking_findings() {
    let md = "Under [28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331) and \
              [*Brown v. Board of Education*, 347 U.S. 483 (1954)](https://www.courtlistener.com/c/U.S./347/483/), \
              the [Fed. R. Civ. P. 12(b)(6)](https://www.law.cornell.edu/rules/frcp/rule_12) motion fails.";
    let (doc, findings) = lint_markdown(md, &Policy::default());
    assert_eq!(doc.citations.len(), 3);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn unlinked_citation_is_cite001_with_a_fix() {
    let (_, findings) = lint_markdown(
        "See Smith v. Jones, 100 F. Supp. 2d 200 (S.D.N.Y. 2000).",
        &Policy::default(),
    );
    assert_eq!(findings.len(), 1);
    let f = &findings[0];
    assert_eq!(f.code, "CITE001");
    assert_eq!(f.severity, Severity::Error);
    assert_eq!(f.kind, Some(Kind::Case));
    assert_eq!(f.rule_id.as_deref(), Some("us.case.federal_supplement"));
    let fix = f.fix.as_ref().expect("fix");
    assert_eq!(
        fix.url,
        "https://www.courtlistener.com/c/F.%20Supp.%202d/100/200/"
    );
    assert!(fix
        .replacement
        .starts_with("[Smith v. Jones, 100 F. Supp. 2d 200 (S.D.N.Y. 2000)]("));
    assert!(!fix.safe, "rule text not opened: fix is a suggestion only");
}

#[test]
fn record_cites_are_exempt_by_default() {
    assert!(codes("See ER-139 and 15 TTABVUE 4.", &Policy::default()).is_empty());
    let strict = Policy {
        require_link: Kind::ALL.to_vec(),
        ..Policy::default()
    };
    assert_eq!(codes("See ER-139.", &strict), vec!["CITE001"]);
}

#[test]
fn bad_url_host_and_incongruence() {
    let p = Policy::default();
    // CommonMark: a destination with spaces needs <...>; without it there is no link.
    assert_eq!(codes("[28 U.S.C. § 1331](not a url)", &p), vec!["CITE001"]);
    assert_eq!(
        codes("[28 U.S.C. § 1331](<not a url>)", &p),
        vec!["CITE002"]
    );
    assert_eq!(
        codes("[28 U.S.C. § 1331](ftp://example.com/x)", &p),
        vec!["CITE002"]
    );
    assert_eq!(
        codes("[28 U.S.C. § 1331](https://blog.example.com/1331)", &p),
        vec!["CITE003"]
    );
    assert_eq!(
        codes(
            "[28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1332)",
            &p
        ),
        vec!["CITE004"]
    );
    assert_eq!(
        codes(
            "[28 U.S.C. § 1331](https://www.govinfo.gov/app/details/USCODE)",
            &p
        ),
        vec!["CITE005"]
    );
    assert_eq!(
        codes(
            "[45 C.F.R. § 164.502](https://www.ecfr.gov/current/title-45/section-164.502)",
            &p
        ),
        Vec::<&str>::new()
    );
    assert_eq!(
        codes(
            "[347 U.S. 483](https://www.courtlistener.com/c/U.S./347/484/)",
            &p
        ),
        vec!["CITE004"]
    );
    let lax = Policy {
        check_congruence: false,
        ..Policy::default()
    };
    assert!(codes(
        "[28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1332)",
        &lax
    )
    .is_empty());
}

#[test]
fn subdomains_of_allowed_hosts_are_allowed() {
    let p = Policy::default();
    let (_, f) = lint_markdown("[Fed. R. Evid. 501](https://supreme.justia.com/x)", &p);
    assert!(f.iter().all(|f| f.code != "CITE003"), "{f:#?}");
}

#[test]
fn accepted_schemes_report_info_only() {
    let mut p = Policy::default();
    assert_eq!(
        codes("[28 U.S.C. § 1331](toa:usc1331)", &p),
        vec!["CITE002"]
    );
    p.link_schemes.insert("toa".into(), SchemeRule::Accept);
    let (_, f) = lint_markdown("[28 U.S.C. § 1331](toa:usc1331)", &p);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].code, "CITE006");
    assert_eq!(f[0].severity, Severity::Info);
}

#[test]
fn a_clean_binding_wins_over_a_bad_one() {
    let md = "[28 U.S.C. § 1331](https://blog.example.com/x)[^1]\n\n[^1]: <https://www.law.cornell.edu/uscode/text/28/1331>\n";
    // Direct links take precedence over footnote links, so the bad direct link is reported.
    assert_eq!(codes(md, &Policy::default()), vec!["CITE003"]);
}

#[test]
fn severity_overrides_and_kind_filter() {
    let mut p = Policy::default();
    p.severity.insert("CITE001".into(), Severity::Warn);
    let (_, f) = lint_markdown("42 U.S.C. § 1983", &p);
    assert_eq!(f[0].severity, Severity::Warn);
    p.kinds = vec![Kind::Case];
    assert!(codes("42 U.S.C. § 1983", &p).is_empty());
}

#[test]
fn low_confidence_matches_are_cues_not_errors() {
    let p = Policy {
        min_confidence: 0.95,
        ..Policy::default()
    };
    let (_, f) = lint_markdown("42 U.S.C. § 1983", &p);
    assert_eq!(f[0].code, "CITE020");
    assert_eq!(f[0].severity, Severity::Info);
    assert!(!code_info("CITE020").unwrap().deterministic);
}

#[test]
fn detect_mode_is_an_inventory() {
    let doc = detect_markdown(
        "42 U.S.C. § 1983 and [5 U.S.C. § 552](https://www.law.cornell.edu/uscode/text/5/552)",
    );
    let f = evaluate(registry(), &doc, &Policy::default(), Mode::Detect);
    assert_eq!(
        f.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec!["CITE010", "CITE010"]
    );
}

#[test]
fn suppressions_need_reasons_and_are_counted() {
    let md = "<!-- cite-detect: ignore CITE001 reason=\"quoted from the order\" -->\n42 U.S.C. § 1983\n\n5 U.S.C. § 552\n";
    let (_, f) = lint_markdown(md, &Policy::default());
    assert_eq!(f.len(), 2);
    assert!(f[0].suppressed);
    assert!(!f[1].suppressed);
    let md = "<!-- cite-detect: ignore-file CITE001 -->\n42 U.S.C. § 1983\n";
    let (_, f) = lint_markdown(md, &Policy::default());
    let got: Vec<(&str, bool)> = f.iter().map(|f| (f.code, f.suppressed)).collect();
    assert_eq!(got, vec![("CITE009", false), ("CITE001", false)]);
    let md = "<!-- cite-detect: ignore-file ALL reason=\"draft\" -->\n42 U.S.C. § 1983\n";
    let (_, f) = lint_markdown(md, &Policy::default());
    assert!(f.iter().all(|f| f.suppressed));
}

#[test]
fn every_code_is_documented() {
    for c in CODES {
        assert!(c.code.starts_with("CITE") && !c.help.is_empty() && !c.name.is_empty());
        assert_eq!(code_info(c.code).unwrap().code, c.code);
    }
    assert!(code_info("NOPE").is_none());
    assert_eq!(Severity::parse("warn"), Some(Severity::Warn));
    assert_eq!(Severity::parse("info"), Some(Severity::Info));
    assert_eq!(Severity::parse("error"), Some(Severity::Error));
    assert_eq!(Severity::parse("x"), None);
    assert!(Severity::Error > Severity::Warn);
}

#[test]
fn policy_json_round_trips_and_rejects_unknown_fields() {
    let p = Policy::default();
    let json = serde_json::to_string(&p).unwrap();
    let back: Policy = serde_json::from_str(&json).unwrap();
    assert_eq!(back, p);
    let partial: Policy =
        serde_json::from_str(r#"{"require_link": ["case"], "link_schemes": {"toa": "accept"}}"#)
            .unwrap();
    assert_eq!(partial.require_link, vec![Kind::Case]);
    assert_eq!(partial.allowed_hosts, Policy::default().allowed_hosts);
    assert!(serde_json::from_str::<Policy>(r#"{"bogus": 1}"#).is_err());
}

#[test]
fn same_line_suppression_covers_only_what_follows_it() {
    let md = "Smith v. Jones, 100 F. Supp. 2d 200 (S.D.N.Y. 2000). \
              <!-- cite-detect: ignore CITE001 reason=\"quoted\" --> See 42 U.S.C. § 1983.\n";
    let (_, f) = lint_markdown(md, &Policy::default());
    let got: Vec<(&str, bool)> = f.iter().map(|f| (f.text.as_str(), f.suppressed)).collect();
    assert_eq!(
        got,
        vec![
            ("Smith v. Jones, 100 F. Supp. 2d 200 (S.D.N.Y. 2000)", false),
            ("42 U.S.C. § 1983", true),
        ]
    );
}

#[test]
fn confidence_equal_to_the_threshold_is_not_low() {
    let doc = detect_markdown("42 U.S.C. § 1983");
    let p = Policy {
        min_confidence: doc.citations[0].confidence,
        ..Policy::default()
    };
    let f = evaluate(registry(), &doc, &p, Mode::Lint);
    assert_eq!(
        f.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec!["CITE001"]
    );
}

#[test]
fn direct_bindings_are_checked_before_footnote_bindings() {
    let md = "[28 U.S.C. § 1331](https://blog.example.com/x)\n\n<https://www.law.cornell.edu/uscode/text/28/1331>\n";
    let mut doc = detect_markdown(md);
    assert_eq!(doc.links.len(), 2);
    // detect_markdown only falls back to footnotes without a direct link; a
    // consumer-built document may carry both.
    doc.bindings[0].push(Binding {
        link: 1,
        via: Via::Footnote,
    });
    let f = evaluate(registry(), &doc, &Policy::default(), Mode::Lint);
    assert_eq!(
        f.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec!["CITE003"]
    );
}

#[test]
fn the_first_clean_binding_ends_the_check() {
    let md = "28 U.S.C. § 1331[^1]\n\n[^1]: <https://www.justia.com/x> and <https://blog.example.com/x>\n";
    let doc = detect_markdown(md);
    assert_eq!(doc.bindings[0].len(), 2);
    let f = evaluate(registry(), &doc, &Policy::default(), Mode::Lint);
    // justia.com is allowed but not decodable: an info finding only, so the
    // second (disallowed) link is never checked.
    assert_eq!(
        f.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec!["CITE005"]
    );
}
