//! L2/L3 assembly details: date-parenthetical confidence terms,
//! explanatory-parenthetical spans and short-case antecedents.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::citation::Term;
use legal_citations::{detect, Kind};

fn add_term(terms: &[Term], want: &str) -> Option<f64> {
    terms.iter().find_map(|t| match t {
        Term::Add { label, value } if label == want => Some(*value),
        _ => None,
    })
}

/// This year, by the same average-Gregorian-year arithmetic the library uses.
fn this_year() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    1970 + u32::try_from(secs / 31_556_952).unwrap()
}

#[test]
fn past_and_current_years_are_plausible_dates() {
    for year in [1954, 2001, this_year()] {
        let text = format!("Smith v. Jones, 123 F.3d 456 (9th Cir. {year})");
        let c = &detect(&text)[0];
        assert_eq!(c.fields["year"], year.to_string());
        assert_eq!(
            add_term(&c.confidence_terms, "date parenthetical"),
            Some(0.05),
            "{text}"
        );
        assert_eq!(
            add_term(&c.confidence_terms, "year in the future"),
            None,
            "{text}"
        );
    }
}

#[test]
fn a_future_year_subtracts_confidence() {
    let past = &detect("Smith v. Jones, 123 F.3d 456 (9th Cir. 2001)")[0];
    let future = &detect("Smith v. Jones, 123 F.3d 456 (9th Cir. 2099)")[0];
    assert_eq!(
        add_term(&future.confidence_terms, "year in the future"),
        Some(-0.3)
    );
    assert!(future.confidence < past.confidence);
}

#[test]
fn explanatory_parenthetical_span_starts_at_its_paren() {
    for text in [
        "Smith v. Jones, 123 F.3d 456 (2d Cir. 2001)(holding x).",
        "Smith v. Jones, 123 F.3d 456 (2d Cir. 2001)  (holding x).",
    ] {
        let c = &detect(text)[0];
        let p = c.parenthetical.unwrap();
        assert_eq!(&text[p.start..p.end], "(holding x)", "{text}");
        assert_eq!(c.full_span.end, p.end);
    }
}

#[test]
fn short_case_resolves_to_the_case_with_its_volume_and_reporter() {
    let cites = detect(
        "Upjohn Co. v. United States, 449 U.S. 383 (1981); Smith v. Jones, 123 F.3d 456 (9th Cir. 2001). Upjohn, 449 U.S. at 389. Later: 999 F.3d at 4.",
    );
    let shorts: Vec<_> = cites.iter().filter(|c| c.kind == Kind::CaseShort).collect();
    assert_eq!(shorts.len(), 2, "{cites:#?}");
    assert_eq!(shorts[0].antecedent, Some(0));
    assert_eq!(shorts[1].antecedent, None);
}

#[test]
fn supra_resolves_by_name_not_to_the_nearest_full_cite() {
    let cites = detect(
        "Upjohn Co. v. United States, 449 U.S. 383 (1981); 42 U.S.C. § 1983. See Upjohn, supra, at 389.",
    );
    let supra = cites.iter().find(|c| c.kind == Kind::Supra).unwrap();
    assert_eq!(supra.antecedent, Some(0));
    assert_eq!(supra.authority.as_deref(), Some("case/449/u.s./383"));
}

/// A test rule of `kind` with its own authority key.
fn keyed_rule(id: &str, kind: &str, pattern: &str, key: &str) -> String {
    serde_json::json!({
        "id": id,
        "kind": kind,
        "jurisdiction": "test",
        "pattern": pattern,
        "authority_key": key,
        "url_templates": [],
        "url_decoders": [],
        "provenance": {"encodes": "Test rule", "opened": false, "how_to_verify": "n/a"},
        "examples": {"positive": [], "negative": []}
    })
    .to_string()
}

#[test]
fn a_short_case_keeps_its_own_authority_other_short_forms_inherit() {
    use legal_citations::citation::assemble;
    use legal_citations::recognize::recognize;
    use legal_citations::Registry;
    let case = keyed_rule(
        "test.case",
        "case",
        r"\b(?P<volume>[0-9]+) (?P<reporter>Rep\.) (?P<page>[0-9]+)\b",
        "case/{volume}/{page}",
    );
    let short = keyed_rule(
        "test.short",
        "case_short",
        r"\b(?P<volume>[0-9]+) (?P<reporter>Rep\.) at (?P<pin>[0-9]+)\b",
        "short/{volume}/{pin}",
    );
    let id = keyed_rule("test.id", "id", r"\bIdem\b", "own/id");
    let reg = Registry::from_sources(&[
        ("rules/case/test.json", &case),
        ("rules/case_short/test.json", &short),
        ("rules/id/test.json", &id),
    ]);
    let text = "Smith v. Jones, 1 Rep. 2. Later 1 Rep. at 5. Idem.";
    let cites = assemble(&reg, text, recognize(&reg, text));
    let kinds: Vec<Kind> = cites.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [Kind::Case, Kind::CaseShort, Kind::Id], "{cites:#?}");
    assert_eq!(cites[1].antecedent, Some(0));
    assert_eq!(cites[1].authority.as_deref(), Some("short/1/5"));
    assert_eq!(cites[2].antecedent, Some(1));
    assert_eq!(cites[2].authority.as_deref(), Some("short/1/5"));
}

#[test]
fn footnote_pincites_and_public_domain_parallels() {
    let c = detect(
        "Bowling Green v. Godwin, 110 Ohio St.3d 58, 2006-Ohio-3563, 850 N.E.2d 698, ¶ 13, fn. 1",
    );
    assert_eq!(c.len(), 3);
    assert!(
        !c[0].fields.contains_key("pincite"),
        "2006 is the public-domain year, not a pin"
    );
    assert_eq!(c[2].fields["pincite"], "13, fn. 1");
    assert!(c[2].text.ends_with("fn. 1"));
}

#[test]
fn id_section_takes_the_antecedents_code_and_its_own_section() {
    let c = detect("7 U.S.C. § 7101 (2012). Id. § 7102(26). Id.");
    assert_eq!(c[1].kind, Kind::Id);
    assert_eq!(c[1].antecedent, Some(0));
    assert_eq!(c[1].authority.as_deref(), Some("us.usc/7/7102"));
    assert_eq!(c[2].authority.as_deref(), Some("us.usc/7/7101"));
    // After a case, `Id. § 5` has no code to re-section: it inherits as is.
    let c = detect("Smith v. Jones, 1 F.3d 1 (2d Cir. 1990). Id. § 5.");
    assert_eq!(c[1].authority, c[0].authority);
}

#[test]
fn a_bare_section_resolves_to_the_statute_with_that_section() {
    let c = detect("Claims arise under 42 U.S.C. § 1983 and 28 U.S.C. § 1331. Under § 1983, they fail. See § 5 of the lease.");
    let short: Vec<_> = c.iter().filter(|c| c.kind == Kind::StatuteShort).collect();
    assert_eq!(short.len(), 2);
    assert_eq!(short[0].antecedent, Some(0));
    assert_eq!(short[0].authority.as_deref(), Some("us.usc/42/1983"));
    assert!(short[0].confidence >= 0.5);
    assert_eq!(short[1].antecedent, None);
    assert!(short[1].authority.is_none());
    assert!(
        short[1].confidence < 0.5,
        "unresolved: a cue, not a citation"
    );
    // A section sign after a code name or title stays with the code rules.
    for text in [
        "Cal. Civ. Code § 1550",
        "V.I. Code Ann. tit. 1, § 97-89",
        "Pub. L. No. 111-148, § 1101",
    ] {
        assert!(
            detect(text).iter().all(|c| c.kind != Kind::StatuteShort),
            "{text}"
        );
    }
    // `Id. § 7` after a bare section re-sections the resolved code.
    let c = detect("42 U.S.C. § 1983. Under § 1983, they fail. Id. § 1985.");
    assert_eq!(c[2].authority.as_deref(), Some("us.usc/42/1985"));
}
