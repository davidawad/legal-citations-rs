//! Typed citation fields and the table of authorities (lcr-bq9).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::authority::{sort_name, Form};
use legal_citations::citation::{
    canonical_reporter, parties, split_pins, CaseDetails, CodeDetails, Details, PartyStyle,
};
use legal_citations::{authorities, detect, registry, Selector};

fn case(d: &Details) -> &CaseDetails {
    match d {
        Details::Case(c) => c,
        other => panic!("not a case: {other:?}"),
    }
}

fn code(d: &Details) -> &CodeDetails {
    match d {
        Details::Code(c) => c,
        other => panic!("not code: {other:?}"),
    }
}

const ROE: &str = "See Roe v. Wade, 410 U.S. 113, 115, 93 S. Ct. 705, 35 L. Ed. 2d 147 (1973). \
Id. at 120. Later, 410 U.S. at 116. Roe v. Wade, 410 U.S. 113, 117 (1973).";

#[test]
fn reporter_spacing_follows_the_single_capital_rule() {
    for (raw, want) in [
        ("F.Supp.2d", "F. Supp. 2d"),
        ("F. Supp. 2d", "F. Supp. 2d"),
        ("N. E. 2d", "N.E.2d"),
        ("N.E.2d", "N.E.2d"),
        ("S.Ct.", "S. Ct."),
        ("U. S.", "U.S."),
        ("F.3d", "F.3d"),
        ("L. Ed. 2d", "L. Ed. 2d"),
        ("Ohio St.3d", "Ohio St. 3d"),
        ("F. App'x", "F. App'x"),
        ("So.2d", "So. 2d"),
        ("Cal.Rptr.3d", "Cal. Rptr. 3d"),
        ("F.4th", "F.4th"),
        ("WL", "WL"),
        ("123", "123"),
    ] {
        assert_eq!(canonical_reporter(raw), want, "{raw}");
    }
}

#[test]
fn parties_and_pins() {
    let p = parties("Smith v. Jones").unwrap();
    assert_eq!(p.style, PartyStyle::Versus);
    assert_eq!(
        (p.first.as_deref(), p.second.as_deref()),
        (Some("Smith"), Some("Jones"))
    );
    let p = parties("In re Acme Corp.").unwrap();
    assert_eq!(p.style, PartyStyle::InRe);
    assert_eq!(p.subject.as_deref(), Some("Acme Corp."));
    assert_eq!(
        parties("Ex parte Young").unwrap().style,
        PartyStyle::ExParte
    );
    assert_eq!(
        parties("Brown versus Board").unwrap().second.as_deref(),
        Some("Board")
    );
    assert!(parties("Acme").is_none());
    assert_eq!(split_pins("389-90, 395 ,, *3"), vec!["389-90", "395", "*3"]);
}

#[test]
fn case_fields_parallels_and_short_forms() {
    let c = detect(ROE);
    let first = case(&c[0].details);
    assert_eq!(first.case_name.as_deref(), Some("Roe v. Wade"));
    assert_eq!(first.volume.as_deref(), Some("410"));
    assert_eq!(first.reporter_canonical.as_deref(), Some("U.S."));
    assert_eq!(first.page.as_deref(), Some("113"));
    assert_eq!(first.pincites, vec!["115"]);
    assert_eq!(
        first.year.as_deref(),
        Some("1973"),
        "shared from the last parallel"
    );
    assert_eq!(first.parallels, vec![1, 2]);
    let sct = case(&c[1].details);
    assert_eq!(sct.parallel_of, Some(0));
    assert_eq!(sct.case_name.as_deref(), Some("Roe v. Wade"));
    assert_eq!(c[0].details.parallel_of(), None);
    assert!(c[0].urls[0].starts_with("https://www.courtlistener.com/c/U.S./410/113"));
    // Id. and the short case cite carry their pins; Id. inherits URLs.
    assert_eq!(c[3].details.pincites(), ["120"]);
    assert!(!c[3].urls.is_empty());
    assert_eq!(case(&c[4].details).pincites, vec!["116"]);
}

#[test]
fn subsequent_history_hangs_off_the_first_decision() {
    let text =
        "Smith v. Jones, 100 F. Supp. 2d 1, 5 (S.D.N.Y. 2000), aff'd in part, rev'd in part, \
250 F.3d 9 (2d Cir. 2001), cert. denied, 534 U.S. 1000 (2001).";
    let c = detect(text);
    assert_eq!(c.len(), 3);
    let d = case(&c[0].details);
    let phrases: Vec<(&str, usize)> = d
        .subsequent_history
        .iter()
        .map(|h| (h.phrase.as_str(), h.citation))
        .collect();
    assert_eq!(
        phrases,
        vec![("aff'd in part, rev'd in part", 1), ("cert. denied", 2)]
    );
    assert_eq!(c[1].details.history_of(), Some(0));
    assert_eq!(c[2].details.history_of(), Some(0));
    assert_eq!(case(&c[0].details).court.as_deref(), Some("S.D.N.Y."));
    let auth = authorities(&c);
    assert_eq!(auth.len(), 1);
    let h: Vec<&str> = auth[0]
        .subsequent_history
        .iter()
        .map(|h| h.phrase.as_str())
        .collect();
    assert_eq!(h, vec!["aff'd in part, rev'd in part", "cert. denied"]);
    assert_eq!(
        auth[0].subsequent_history[0].key.as_deref(),
        Some("case/250/f.3d/9")
    );
    assert_eq!(auth[0].occurrences[1].form, Form::History);
    assert_eq!(
        auth[0].pages_cited,
        vec!["5"],
        "history pins are not this authority's pages"
    );
    // `sub nom.` introduces the later name without a comma.
    let c = detect(
        "Lerman v. Comm’r, 939 F.2d 44 (3d Cir. 1991), rev’d sub nom. Horn v. Comm’r, 968 F.2d 1229 (D.C. Cir. 1992).",
    );
    assert_eq!(c.len(), 2);
    assert_eq!(c[1].details.history_of(), Some(0));
    assert_eq!(case(&c[0].details).subsequent_history[0].phrase, "rev’d");
    // Not history: a semicolon or a plain comma between unrelated cases.
    let c = detect("Smith v. Jones, 1 F.3d 1 (2d Cir. 1990); Doe v. Roe, 2 F.3d 2 (2d Cir. 1991).");
    assert!(c.iter().all(|x| x.details.history_of().is_none()));
}

#[test]
fn code_and_agency_fields() {
    let c = detect(
        "Under 28 U.S.C. § 1332(a)(1) (2018), Ohio Rev. Code § 2907.02(A)(1), \
Fed. R. Civ. P. 12(b)(6), 40 C.F.R. § 52.21(b), Cal. Civ. Proc. Code § 425.16, \
Pub. L. No. 111-148, MPEP § 2141.03, Rev. Rul. 2020-1, IPR2025-00687, 86 Fed. Reg. 1234 (Jan. 5, 2021).",
    );
    let usc = code(&c[0].details);
    assert_eq!(usc.code.as_deref(), Some("U.S.C."));
    assert_eq!(usc.title.as_deref(), Some("28"));
    assert_eq!(usc.sections, vec!["1332"]);
    assert_eq!(usc.subsections, vec!["a", "1"]);
    assert_eq!(usc.year.as_deref(), Some("2018"));
    let oh = code(&c[1].details);
    assert_eq!(oh.code.as_deref(), Some("Ohio Rev. Code"));
    assert_eq!(oh.subsections, vec!["A", "1"]);
    let frcp = code(&c[2].details);
    assert_eq!(frcp.code.as_deref(), Some("Fed. R. Civ. P."));
    assert_eq!(frcp.sections, vec!["12"]);
    assert_eq!(frcp.subsections, vec!["b", "6"]);
    let cfr = code(&c[3].details);
    assert_eq!(
        (cfr.code.as_deref(), cfr.title.as_deref()),
        (Some("C.F.R."), Some("40"))
    );
    assert_eq!(cfr.subsections, vec!["b"]);
    let cal = code(&c[4].details);
    assert_eq!(cal.code.as_deref(), Some("Cal. Civ. Proc."));
    let pl = code(&c[5].details);
    assert_eq!(pl.code.as_deref(), Some("Pub. L."));
    assert_eq!(pl.title.as_deref(), Some("111"));
    let agency = |i: usize| match &c[i].details {
        Details::Agency(a) => a.clone(),
        other => panic!("{other:?}"),
    };
    let mpep = agency(6);
    assert_eq!(mpep.agency.as_deref(), Some("USPTO"));
    assert_eq!(mpep.document_type.as_deref(), Some("MPEP"));
    assert_eq!(mpep.number.as_deref(), Some("2141.03"));
    let rr = agency(7);
    assert_eq!(
        (rr.agency.as_deref(), rr.document_type.as_deref()),
        (Some("IRS"), Some("Rev. Rul."))
    );
    let ipr = agency(8);
    assert_eq!(ipr.document_type.as_deref(), Some("IPR"));
    assert_eq!(ipr.number.as_deref(), Some("IPR2025-00687"));
    let fr = agency(9);
    assert_eq!(fr.agency, None);
    assert_eq!(fr.document_type.as_deref(), Some("Fed. Reg."));
    assert_eq!(
        (fr.volume.as_deref(), fr.page.as_deref()),
        (Some("86"), Some("1234"))
    );
    assert_eq!(fr.date.as_deref(), Some("Jan. 5, 2021"));
}

#[test]
fn other_kinds_have_no_typed_shape() {
    let c = detect("See ER-139 and Restatement (Second) of Torts § 402A.");
    assert!(c.iter().all(|x| x.details == Details::Other));
    assert!(c[0].details.pincites().is_empty());
    assert_eq!(c[0].details.parallel_of(), None);
}

#[test]
fn authorities_group_parallels_short_forms_and_repeats() {
    let c = detect(ROE);
    let auth = authorities(&c);
    assert_eq!(auth.len(), 1, "{auth:#?}");
    let a = &auth[0];
    assert_eq!(a.key, "case/410/u.s./113");
    assert_eq!(
        a.parallel_keys,
        vec!["case/93/s.ct./705", "case/35/l.ed.2d/147"]
    );
    assert_eq!(
        a.full_cite,
        "Roe v. Wade, 410 U.S. 113, 115, 93 S. Ct. 705, 35 L. Ed. 2d 147 (1973)"
    );
    let forms: Vec<Form> = a.occurrences.iter().map(|o| o.form).collect();
    assert_eq!(
        forms,
        vec![
            Form::Full,
            Form::Parallel,
            Form::Parallel,
            Form::Short,
            Form::Short,
            Form::Full
        ]
    );
    assert_eq!(a.short_forms, 2);
    assert_eq!(a.pages_cited, vec!["115", "120", "116", "117"]);
    assert_eq!(a.occurrences[0].signal.as_deref(), Some("See"));
    assert!(
        a.urls.len() >= 3,
        "every parallel's verified URL: {:?}",
        a.urls
    );
    assert_eq!(sort_name(a), "roe v. wade");
}

#[test]
fn a_later_bare_parallel_reporter_joins_the_authority() {
    let c = detect("Roe v. Wade, 410 U.S. 113, 93 S. Ct. 705 (1973). See also 93 S. Ct. 705.");
    let auth = authorities(&c);
    assert_eq!(auth.len(), 1);
    assert_eq!(auth[0].occurrences.last().unwrap().form, Form::Full);
}

#[test]
fn authorities_sort_by_kind_then_name_and_skip_unresolved_short_forms() {
    let c = detect(
        "Id. at 4. 42 U.S.C. § 1983. In re Zeta, 1 B.R. 1 (Bankr. D. Del. 1990). \
Alpha v. Beta, 2 F.3d 2 (2d Cir. 1991). Ohio Rev. Code § 1.01. Ohio Rev. Code § 1.01. ER-139.",
    );
    let auth = authorities(&c);
    let keys: Vec<String> = auth.iter().map(|a| a.key.clone()).collect();
    assert_eq!(
        keys,
        vec![
            "case/2/f.3d/2",
            "case/1/b.r./1",
            "us.usc/42/1983",
            "statute:ohiorev.code1.01",
            "record:er-139",
        ],
        "In re is ignored when sorting; keyless cites group by text"
    );
    assert_eq!(auth[3].occurrences.len(), 2);
    assert_eq!(auth[3].normalized, "Ohio Rev. Code § 1.01");
    assert!(authorities(&[]).is_empty());
}

#[test]
fn selection_keeps_typed_links_consistent() {
    let sel = registry()
        .select(&Selector {
            include: vec!["rule:us.case.supreme_court_reporter".into()],
            ..Selector::default()
        })
        .unwrap();
    let c = legal_citations::detect_with(&sel, ROE);
    assert_eq!(
        c.len(),
        1,
        "only the S. Ct. cite; its group root is dropped"
    );
    assert_eq!(c[0].details.parallel_of(), None);
    // Selecting the root keeps its parallels and short forms, renumbered.
    let sel = registry()
        .select(&Selector {
            include: vec!["rule:us.case.us_reports".into()],
            ..Selector::default()
        })
        .unwrap();
    let c = legal_citations::detect_with(&sel, ROE);
    assert_eq!(c.len(), 6);
    assert_eq!(case(&c[0].details).parallels, vec![1, 2]);
}
