//! Detection behavior (L1-L3) on realistic text: spans, fields, authority
//! keys, overlap resolution, short forms, confidence.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::citation::{canonical_key, case_name_before, score, Term};
use legal_citations::{detect, Kind};

fn only(text: &str) -> legal_citations::Citation {
    let mut cites = detect(text);
    assert_eq!(
        cites.len(),
        1,
        "{text:?} -> {:#?}",
        cites
            .iter()
            .map(|c| (&c.rule_id, &c.text))
            .collect::<Vec<_>>()
    );
    cites.remove(0)
}

#[test]
fn full_case_citation_gets_name_pincite_court_year() {
    let text = "See Brown v. Board of Education, 347 U.S. 483, 495 (1954) (holding segregation unconstitutional).";
    let c = only(text);
    assert_eq!(c.kind, Kind::Case);
    assert_eq!(c.rule_id, "us.case.us_reports");
    assert_eq!(
        c.text,
        "Brown v. Board of Education, 347 U.S. 483, 495 (1954)"
    );
    assert_eq!(&text[c.core_span.start..c.core_span.end], "347 U.S. 483");
    assert_eq!(c.fields["case_name"], "Brown v. Board of Education");
    assert_eq!(c.fields["pincite"], "495");
    assert_eq!(c.fields["year"], "1954");
    assert!(!c.fields.contains_key("court"));
    assert_eq!(c.signal.as_deref(), Some("See"));
    assert_eq!(c.full_span.start, 0);
    let paren = c.parenthetical.expect("explanatory parenthetical");
    assert_eq!(
        &text[paren.start..paren.end],
        "(holding segregation unconstitutional)"
    );
    assert_eq!(c.authority.as_deref(), Some("case/347/u.s./483"));
    assert_eq!(c.normalized, "347 U.S. 483");
}

#[test]
fn federal_reporter_with_digits_in_reporter_is_detected() {
    // The legacy case pattern missed this (reporter class had no digits).
    let c = only("Smith v. Jones, 123 F.3d 456 (9th Cir. 2001) held otherwise.");
    assert_eq!(c.fields["reporter"], "F.3d");
    assert_eq!(c.fields["court"], "9th Cir.");
    assert_eq!(c.fields["case_name"], "Smith v. Jones");
    assert_eq!(c.authority.as_deref(), Some("case/123/f.3d/456"));
}

#[test]
fn in_re_and_corporate_names() {
    let c = only("In re U.S. Dep't of Educ., 25 F.4th 692, 695 (9th Cir. 2022).");
    assert_eq!(c.fields["case_name"], "In re U.S. Dep't of Educ.");
    let c = only("Upjohn Co. v. United States, 449 U.S. 383 (1981).");
    assert_eq!(c.fields["case_name"], "Upjohn Co. v. United States");
}

#[test]
fn case_name_scan_stops_at_lowercase_prose() {
    let text = "the court in Smith v. Jones, 123 F.3d 456";
    let core = text.find("123").unwrap();
    let (start, name) = case_name_before(text, core).unwrap();
    assert_eq!(name, "Smith v. Jones");
    assert_eq!(&text[start..start + 5], "Smith");
    assert!(case_name_before("no comma here 123 F.3d 456", 14).is_none());
    assert!(case_name_before("Alpha Beta, 1 F.3d 2", 12).is_none());
}

#[test]
fn federal_supplement_series_is_not_split() {
    let c = only("Doe v. Roe, 100 F. Supp. 2d 200, 205 (S.D.N.Y. 2000)");
    assert_eq!(c.fields["reporter"], "F. Supp. 2d");
    assert_eq!(c.fields["page"], "200");
    assert_eq!(c.fields["court"], "S.D.N.Y.");
}

#[test]
fn statutes_split_section_and_subsection() {
    let c = only("Jurisdiction lies under 28 U.S.C. § 1331(a)(1).");
    assert_eq!(c.fields["section"], "1331");
    assert_eq!(c.fields["subsection"], "(a)(1)");
    assert_eq!(c.authority.as_deref(), Some("us.usc/28/1331"));
    assert_eq!(c.normalized, "28 U.S.C. § 1331(a)(1)");
    let c = only("The jurisdiction of this Court rests on 28 U.S.C. 1254(1).");
    assert_eq!(c.fields["section"], "1254");
}

#[test]
fn spans_are_bytes_and_chars_into_the_original() {
    let text = "Müller wrote: “see 42 U.S.C. § 1983” — naïve.";
    let c = only(text);
    assert_eq!(&text[c.span.start..c.span.end], "42 U.S.C. § 1983");
    let chars: Vec<char> = text.chars().collect();
    let by_char: String = chars[c.span.char_start..c.span.char_end].iter().collect();
    assert_eq!(by_char, "42 U.S.C. § 1983");
    assert_eq!(c.span.line, 1);
    assert_eq!(c.span.col, c.span.char_start + 1);
    assert_eq!(
        c.span.utf16_end - c.span.utf16_start,
        "42 U.S.C. § 1983".encode_utf16().count()
    );
}

#[test]
fn ascii_digits_only_in_forward_rules() {
    // Arabic-Indic and fullwidth digits are not U.S. citations.
    assert!(detect("٤٢ U.S.C. § ٣٤ and １２ U.S.C. § １").is_empty());
}

#[test]
fn overlap_prefers_the_longer_more_specific_rule() {
    // "N.C.G.S." contains the Connecticut shape "C.G.S."; the longer North
    // Carolina match must win.
    let c = only("N.C.G.S. § 14-17");
    assert_eq!(c.rule_id, "us.nc.statute.code");
    let cites = detect("N.C. Gen. Stat. § 14-17 and N.C.G.S. § 14-17.");
    assert_eq!(cites.len(), 2);
    assert!(cites.iter().all(|c| c.rule_id == "us.nc.statute.code"));
    let c = only("See Cal. Civ. Code § 1550.");
    assert_eq!(c.rule_id, "us.ca.statute.code");
}

#[test]
fn legacy_port_bugs_are_fixed_in_forward_rules() {
    assert_eq!(
        only("18 Pa. Cons. Stat. § 3701").rule_id,
        "us.pa.statute.code"
    );
    assert_eq!(
        only("Va. Code Ann. § 18.2-51").rule_id,
        "us.va.statute.code"
    );
    assert_eq!(only("W. Va. Code § 61-2-1").rule_id, "us.wv.statute.code");
    // Same lost-escape bug (`Fla(?:\.|\s+Stat)\.` required "Fla.."): the
    // standard Bluebook forms never matched before.
    assert_eq!(only("Fla. Stat. § 784.045").rule_id, "us.fl.statute.code");
    assert_eq!(only("La. Rev. Stat. § 14:30").rule_id, "us.la.statute.code");
    assert_eq!(
        only("Mo. Rev. Stat. § 565.021").rule_id,
        "us.mo.statute.code"
    );
    assert_eq!(
        only("Ky. Rev. Stat. Ann. § 508.010").rule_id,
        "us.ky.statute.code"
    );
    assert_eq!(
        only("Me. Rev. Stat. tit. 17-A, § 202").rule_id,
        "us.me.statute.code"
    );
    assert_eq!(
        only("Vt. Stat. Ann. tit. 13, § 2301").rule_id,
        "us.vt.statute.code"
    );
}

#[test]
fn id_resolves_to_the_previous_authority() {
    let cites = detect("Upjohn Co. v. United States, 449 U.S. 383 (1981). Id. at 389. Id.");
    assert_eq!(cites.len(), 3);
    assert_eq!(cites[1].kind, Kind::Id);
    assert_eq!(cites[1].antecedent, Some(0));
    assert_eq!(cites[1].fields["pin"], "389");
    assert_eq!(cites[2].antecedent, Some(0));
    assert_eq!(cites[2].authority, cites[0].authority);
}

#[test]
fn short_case_resolves_by_volume_and_reporter() {
    let cites = detect("Upjohn Co. v. United States, 449 U.S. 383 (1981); 42 U.S.C. § 1983. Later: Upjohn, 449 U.S. at 389.");
    let short = cites
        .iter()
        .position(|c| c.kind == Kind::CaseShort)
        .unwrap();
    assert_eq!(cites[short].antecedent, Some(0));
    assert_eq!(cites[short].authority.as_deref(), Some("case/449/u.s./383"));
}

#[test]
fn supra_resolves_by_name() {
    let cites =
        detect("Upjohn Co. v. United States, 449 U.S. 383 (1981). See Upjohn, supra, at 389.");
    let supra = cites.iter().find(|c| c.kind == Kind::Supra).unwrap();
    assert_eq!(supra.antecedent, Some(0));
    assert_eq!(supra.fields["name"], "Upjohn");
}

#[test]
fn unresolved_short_forms_lose_confidence() {
    let c = only("Id. at 5.");
    assert!(c.antecedent.is_none());
    assert!(c.confidence < 0.7, "{}", c.confidence);
    let c = only("Nobody, supra, at 3.");
    assert!(c.antecedent.is_none());
}

#[test]
fn confidence_is_explained_and_capped() {
    let c = only("42 U.S.C. § 1983");
    assert!((c.confidence - 0.8).abs() < 1e-9, "{}", c.confidence);
    assert!(c
        .confidence_terms
        .iter()
        .any(|t| matches!(t, Term::Cap { label, .. } if label == "rule text not opened")));
    assert_eq!(
        score(&[Term::Add {
            label: "a".into(),
            value: 2.0
        }]),
        1.0
    );
    assert_eq!(
        score(&[
            Term::Add {
                label: "a".into(),
                value: 0.5
            },
            Term::Cap {
                label: "c".into(),
                value: 0.3
            }
        ]),
        0.3
    );
    assert_eq!(
        score(&[Term::Add {
            label: "a".into(),
            value: -1.0
        }]),
        0.0
    );
}

#[test]
fn future_years_reduce_confidence() {
    let c = only("Smith v. Jones, 123 F.3d 456 (9th Cir. 2999)");
    // 2999 is outside the year pattern, so there is no date parenthetical.
    assert!(!c.fields.contains_key("year"));
    let c = only("Smith v. Jones, 123 F.3d 456 (9th Cir. 2099)");
    assert!(c
        .confidence_terms
        .iter()
        .any(|t| matches!(t, Term::Add { label, .. } if label == "year in the future")));
}

#[test]
fn canonical_keys_ignore_space_case_and_section_signs() {
    assert_eq!(
        canonical_key("Case/100/F. Supp. 2d/200"),
        "case/100/f.supp.2d/200"
    );
    assert_eq!(canonical_key("us.usc/28/§ 1331"), "us.usc/28/1331");
}

#[test]
fn many_kinds_are_recognized() {
    let cases: &[(&str, &str)] = &[
        ("Fed. R. Civ. P. 12(b)(6)", "us.fed.rules.civil_procedure"),
        ("Fed. R. Evid. 501", "us.fed.rules.evidence"),
        ("9th Cir. R. 28-2.7", "us.fed.rules.circuit"),
        ("MPEP § 2143.01", "us.fed.uspto.mpep"),
        ("TBMP § 1203.02", "us.fed.uspto.tbmp"),
        ("85 Fed. Reg. 12,345", "us.fed.federal_register"),
        ("Pub. L. No. 111-148", "us.fed.session_law.public_law"),
        ("124 Stat. 119", "us.fed.session_law.statutes_at_large"),
        ("U.S. Const. art. I, § 8, cl. 3", "us.fed.constitution"),
        ("IPR2020-00019", "us.fed.uspto.ptab_proceeding"),
        ("15 TTABVUE 4", "us.fed.uspto.ttabvue"),
        ("Ex. 1001", "us.fed.uspto.ptab_exhibit"),
        ("ER-139", "us.record.appendix"),
        ("Pet. App. 23a", "us.record.petition_appendix"),
        ("ECF No. 12", "us.record.ecf"),
        ("No. 1:20-cv-01234", "us.docket.number"),
        ("4 Harv. L. Rev. 193", "us.secondary.law_review"),
        ("1155 U.N.T.S. 331", "intl.treaty.unts"),
        ("[2019] UKSC 41", "foreign.uk.neutral"),
        ("Case C-131/12", "foreign.eu.cjeu_case"),
        ("2020 WL 1234567", "us.case.westlaw"),
        ("Treas. Reg. § 1.501(c)(3)-1", "us.fed.regulation.treasury"),
    ];
    for (text, rule) in cases {
        assert_eq!(&only(text).rule_id, rule, "{text}");
    }
}

#[test]
fn clean_prose_has_no_citations() {
    let prose = [
        "The parties met 3 times in 2019 and signed 2 agreements worth $1,983.",
        "Chapter 11 of the novel runs from page 483 to page 495.",
        "Please bring a valid ID; the hearing starts at 9 a.m. in Room 12.",
        "Our code of conduct, at section 4, covers gifts and travel.",
        "We shipped 347 units to the U.S. in 1954 and 12 more later.",
    ];
    for p in prose {
        assert!(
            detect(p).is_empty(),
            "{p:?} -> {:?}",
            detect(p).iter().map(|c| &c.rule_id).collect::<Vec<_>>()
        );
    }
}
