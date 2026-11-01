//! The catch-all `unrecognized` layer (lcr-xvl): what it finds, where it
//! never fires, CITE011, and the `reject` rule field it relies on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::policy::code_info;
use legal_citations::recognize::rule_matches;
use legal_citations::registry::{registry, validate, Registry};
use legal_citations::{detect, detect_with, lint_markdown, Kind, Policy, Selector, Severity};

fn unrecognized(text: &str) -> Vec<String> {
    detect(text)
        .into_iter()
        .filter(|c| c.kind == Kind::Unrecognized)
        .map(|c| c.text)
        .collect()
}

/// Ordinary prose, numbers, dates, addresses and figures: no citation at all.
const CLEAN_PROSE: &[&str] = &[
    "The parties met 3 times in 2019 and signed 2 agreements worth $1,983.",
    "Chapter 11 of the novel runs from page 483 to page 495.",
    "Please bring a valid ID; the hearing starts at 9 a.m. in Room 12.",
    "The meeting moved from 5 Jan. 2020 to 12 Feb. 2020 at the annex.",
    "See 3 Fig. 4 and Vol. 2 No. 7 of the newsletter for the chart.",
    "Send it to 100 N. Main St., Suite 4, or to 1600 Pennsylvania Ave. 20500.",
    "Our team ranked No. 3; the NBA Finals No. 1 seed won again.",
    "Under section 5 of the lease, rent rises 3 percent each year.",
    "Version 2 Rev. 3 of the manual added 14 pages and 2 tables.",
    "Mr. Smith paid 20 Dr. Jones 40 dollars for 3 visits in 2021.",
    "The COVID-19 rules changed in 2020; ISO 9001 audits resumed in 2022.",
    "Dear Ms. Lee, the 2 Co. 3 typo in the draft is fixed.",
];

#[test]
fn clean_prose_produces_no_citation_and_no_finding_above_info() {
    for p in CLEAN_PROSE {
        let cites = detect(p);
        assert!(
            cites.is_empty(),
            "{p:?} -> {:?}",
            cites
                .iter()
                .map(|c| (&c.rule_id, &c.text))
                .collect::<Vec<_>>()
        );
        let (_, findings) = lint_markdown(p, &Policy::default());
        assert!(
            findings.iter().all(|f| f.severity == Severity::Info),
            "{p:?}"
        );
    }
}

#[test]
fn no_negative_example_of_any_rule_is_unrecognized() {
    let mut hits = Vec::new();
    for rule in &registry().rules {
        for neg in &rule.def.examples.negative {
            for t in unrecognized(neg) {
                hits.push(format!("{} negative {neg:?}: {t:?}", rule.def.id));
            }
        }
    }
    assert!(hits.is_empty(), "{hits:#?}");
}

#[test]
fn never_fires_inside_a_span_a_real_rule_matched() {
    let reg = registry();
    for (_, rule) in reg.active().filter(|(_, r)| !r.def.kind.is_catch_all()) {
        for pos in &rule.def.examples.positive {
            let cites = detect(&pos.text);
            let real: Vec<_> = cites
                .iter()
                .filter(|c| c.kind != Kind::Unrecognized)
                .collect();
            for u in cites.iter().filter(|c| c.kind == Kind::Unrecognized) {
                for want in &pos.spans {
                    assert!(
                        u.span.end <= want[0] || want[1] <= u.span.start,
                        "{}: {:?} overlaps {want:?} in {:?}",
                        rule.def.id,
                        u.text,
                        pos.text
                    );
                }
                for r in &real {
                    assert!(
                        u.span.end <= r.full_span.start || r.full_span.end <= u.span.start,
                        "{:?} inside {:?}",
                        u.text,
                        r.text
                    );
                }
            }
        }
    }
}

#[test]
fn finds_each_shape_with_low_confidence() {
    let text = "See Doe v. Roe, 45 Haw. Trib. 112 (2010); Guam Code Ann. tit. 7, § 25104; \
Index No. 654321/2019; FTC File No. 123-4567; WXYZ-HQ-2021-0317.";
    let cites = detect(text);
    let got: Vec<(&str, &str)> = cites
        .iter()
        .map(|c| (c.rule_id.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("unrecognized.reporter", "45 Haw. Trib. 112"),
            ("unrecognized.section", "Guam Code Ann. tit. 7, § 25104"),
            ("unrecognized.docket", "Index No. 654321/2019"),
            ("unrecognized.agency_document", "FTC File No. 123-4567"),
            ("unrecognized.agency_document", "WXYZ-HQ-2021-0317"),
        ]
    );
    for c in &cites {
        assert!(c.confidence < 0.5, "{} {}", c.text, c.confidence);
        assert_eq!(c.authority, None);
        assert!(!c.verified_rule);
    }
    assert_eq!(cites[0].fields["reporter"], "Haw. Trib.");
    assert_eq!(cites[0].normalized, "45 Haw. Trib. 112");
    // A known shape is claimed by its real rule, never by the catch-all.
    assert!(unrecognized("See 28 U.S.C. § 1331; No. 21-1234; 410 U.S. 113 (1973).").is_empty());
    // A leading signal word is skipped, not swallowed or fatal.
    assert_eq!(
        unrecognized("Cf. Kosrae Code Ann. tit. 33, § 4061."),
        vec!["Kosrae Code Ann. tit. 33, § 4061"]
    );
}

#[test]
fn cite011_is_a_deterministic_warning_and_never_a_link_demand() {
    let info = code_info("CITE011").unwrap();
    assert_eq!(info.name, "unrecognized-citation");
    assert_eq!(info.severity, Severity::Warn);
    assert!(info.deterministic);
    let md = "The claim rests on 45 Haw. Trib. 112 and 28 U.S.C. § 1331.";
    let (_, findings) = lint_markdown(md, &Policy::default());
    let codes: Vec<&str> = findings.iter().map(|f| f.code).collect();
    assert_eq!(codes, vec!["CITE011", "CITE001"]);
    assert!(findings[0].message.contains("45 Haw. Trib. 112"));
    assert_eq!(findings[0].kind, Some(Kind::Unrecognized));
    assert!(!Policy::default().require_link.contains(&Kind::Unrecognized));
    // Even a policy that demands links on every kind only warns.
    let strict = Policy {
        require_link: Kind::ALL.to_vec(),
        ..Policy::default()
    };
    let (_, findings) = lint_markdown("Cited: 45 Haw. Trib. 112.", &strict);
    let codes: Vec<&str> = findings.iter().map(|f| f.code).collect();
    assert_eq!(codes, vec!["CITE011"]);
    // Inventory mode reports it the same way.
    let doc = legal_citations::detect_markdown("Cited: 45 Haw. Trib. 112.");
    let f = legal_citations::evaluate(
        registry(),
        &doc,
        &Policy::default(),
        legal_citations::Mode::Detect,
    );
    assert_eq!(f[0].code, "CITE011");
}

#[test]
fn merging_keeps_short_form_and_parallel_indices_right() {
    let text = "See 12 Wyo. Comp. 3d 456. Smith v. Jones, 1 F.3d 1, 2 S. Ct. 3 (1990). Id. at 3.";
    let c = detect(text);
    let kinds: Vec<Kind> = c.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![Kind::Unrecognized, Kind::Case, Kind::Case, Kind::Id]
    );
    assert_eq!(c[3].antecedent, Some(2));
    assert_eq!(c[2].details.parallel_of(), Some(1));
    let auth = legal_citations::authorities(&c);
    assert_eq!(auth.len(), 2);
    assert_eq!(auth[0].key, "case/1/f.3d/1");
    assert_eq!(auth[1].kind, Kind::Unrecognized);
}

#[test]
fn selection_can_drop_or_keep_the_layer() {
    let text = "Doe v. Roe, 45 Haw. Trib. 112 (2010), and 28 U.S.C. § 1331.";
    let only = registry()
        .select(&Selector {
            include: vec!["kind:unrecognized".into()],
            ..Selector::default()
        })
        .unwrap();
    assert_eq!(detect_with(&only, text).len(), 1);
    let none = registry()
        .select(&Selector {
            exclude: vec!["family:unrecognized".into()],
            ..Selector::default()
        })
        .unwrap();
    assert!(detect_with(&none, text)
        .iter()
        .all(|c| c.kind != Kind::Unrecognized));
}

const REJECTING: &str = r#"{
  "id": "test.unrecognized.x", "kind": "unrecognized", "jurisdiction": "unknown",
  "pattern": "\\b[A-Z][a-z]+ [0-9]+",
  "reject": ["^(?:See|Cf) "],
  "provenance": {"encodes": "t", "opened": false, "how_to_verify": "t"},
  "examples": {
    "positive": [
      {"text": "See Widget 12.", "spans": [[4, 13]]},
      {"text": "Widget 1 and Gadget 2", "spans": [[0, 8], [13, 21]]},
      {"text": "Cf Gizmo 3", "spans": [[3, 10]]}
    ],
    "negative": ["see widget 12", "nothing here", "See 12"]
  }
}"#;

#[test]
fn reject_resumes_after_the_rejected_start() {
    let reg = Registry::from_sources(&[("rules/unrecognized/x.json", REJECTING)]);
    assert!(reg.load_errors.is_empty(), "{:?}", reg.load_errors);
    let v = validate(&reg);
    assert!(v.is_empty(), "{v:#?}");
    let rule = &reg.rules[0];
    let got: Vec<&str> = rule_matches(rule, "See Widget 12 é Cf Gizmo 3")
        .iter()
        .map(|c| c.get(0).unwrap().as_str())
        .collect();
    assert_eq!(got, vec!["Widget 12", "Gizmo 3"]);
    assert!(rule_matches(rule, "").is_empty());
    let cites = detect_with(&reg, "See Widget 12.");
    assert_eq!(cites[0].kind, Kind::Unrecognized);
    // A reject pattern that does not compile is a load error.
    let broken = REJECTING.replace("^(?:See|Cf) ", "(unclosed");
    let reg = Registry::from_sources(&[("rules/unrecognized/x.json", &broken)]);
    assert_eq!(reg.load_errors[0].check, "compile-reject");
}

const PRECEDED: &str = r#"{
  "id": "test.court_rule.lr", "kind": "court_rule", "jurisdiction": "unknown",
  "pattern": "\\bL\\.R\\. [0-9]+",
  "reject_before": ["[A-Za-z]\\.$"],
  "provenance": {"encodes": "t", "opened": false, "how_to_verify": "t"},
  "examples": {
    "positive": [
      {"text": "See L.R. 7.", "spans": [[4, 10]]},
      {"text": "L.R. 1 and L.R. 2", "spans": [[0, 6], [11, 17]]},
      {"text": "Under é L.R. 3", "spans": [[9, 15]]}
    ],
    "negative": ["12 A.L.R. 345", "nothing here", "LR 7"]
  }
}"#;

#[test]
fn reject_before_vetoes_by_the_preceding_text() {
    let reg = Registry::from_sources(&[("rules/court_rule/x.json", PRECEDED)]);
    assert!(reg.load_errors.is_empty(), "{:?}", reg.load_errors);
    let v = validate(&reg);
    assert!(v.is_empty(), "{v:#?}");
    let rule = &reg.rules[0];
    // The window starts on a char boundary even when 24 bytes back is not.
    let long = format!("{}A.L.R. 1, L.R. 2", "é".repeat(20));
    let got: Vec<&str> = rule_matches(rule, &long)
        .iter()
        .map(|c| c.get(0).unwrap().as_str())
        .collect();
    assert_eq!(got, vec!["L.R. 2"]);
    let broken = PRECEDED.replace("[A-Za-z]\\\\.$", "(unclosed");
    let reg = Registry::from_sources(&[("rules/court_rule/x.json", &broken)]);
    assert_eq!(reg.load_errors[0].check, "compile-reject");
}

/// Hand-written rules no longer split reporters they used to (lcr-g): each
/// text is one case citation covering all of the cite.
#[test]
fn hand_written_rules_do_not_split_reporters() {
    for (text, cite) in [
        ("See 12 A.L.R.3d 345 (1967).", "12 A.L.R.3d 345"),
        ("Smith, 12 A.L.R.3d at 350.", "12 A.L.R.3d at 350"),
        (
            "Doe v. Roe, 12 Fed. Appx 345 (9th Cir. 2001).",
            "12 Fed. Appx 345",
        ),
        (
            "Doe v. Roe, 12 Fed Appx 345 (9th Cir. 2001).",
            "12 Fed Appx 345",
        ),
        (
            "Doe v. Roe, 3 Hall's Sup. Ct. R. 45 (1830).",
            "3 Hall's Sup. Ct. R. 45",
        ),
    ] {
        let cites: Vec<_> = detect(text)
            .into_iter()
            .filter(|c| c.kind != Kind::Unrecognized)
            .collect();
        assert_eq!(cites.len(), 1, "{text}: {cites:#?}");
        assert!(
            matches!(cites[0].kind, Kind::Case | Kind::CaseShort),
            "{text}"
        );
        let core = &text[cites[0].core_span.start..cites[0].core_span.end];
        assert_eq!(core, cite, "{text}");
    }
    // The rules still fire on their own forms.
    let kinds = |t: &str| detect(t).iter().map(|c| c.kind).collect::<Vec<_>>();
    assert_eq!(kinds("See L.R. 7.1(a)."), vec![Kind::CourtRule]);
    assert_eq!(kinds("See Sup. Ct. R. 10."), vec![Kind::CourtRule]);
    assert_eq!(kinds("See Appx 345."), vec![Kind::Record]);
}
