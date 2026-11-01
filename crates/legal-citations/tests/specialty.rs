//! Specialty-court short forms and the federal/state executive-order split
//! (lcr-g step 5: B3 gaps).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::{detect, Kind};

#[test]
fn specialty_reporter_short_forms_resolve_to_their_full_cites() {
    for (full, short) in [
        (
            "Smith v. Comm'r, 145 T.C. 1 (2015).",
            "Smith, 145 T.C. at 95.",
        ),
        (
            "United States v. Doe, 70 M.J. 1 (C.A.A.F. 2011).",
            "Doe, 70 M.J. at 12.",
        ),
        (
            "Roe v. McDonald, 30 Vet. App. 1 (2018).",
            "Roe, 30 Vet. App. at 5.",
        ),
        (
            "Doe v. United States, 38 Ct. Int'l Trade 1123 (2014).",
            "Doe, 38 Ct. Int'l Trade at 1130.",
        ),
    ] {
        let text = format!("{full} {short}");
        let cites = detect(&text);
        assert_eq!(cites.len(), 2, "{text}: {cites:#?}");
        assert_eq!(cites[0].kind, Kind::Case, "{text}");
        assert_eq!(cites[1].kind, Kind::CaseShort, "{text}");
        assert_eq!(cites[1].antecedent, Some(0), "{text}");
        assert_eq!(cites[1].authority, cites[0].authority, "{text}");
        assert_eq!(cites[1].jurisdiction, cites[0].jurisdiction, "{text}");
    }
    // Without a full cite the short form is still found, unresolved.
    let alone = detect("See 145 T.C. at 95.");
    assert_eq!(alone[0].kind, Kind::CaseShort);
    assert_eq!(alone[0].antecedent, None);
}

#[test]
fn state_executive_orders_are_not_federal() {
    let federal = |t: &str| {
        detect(t)
            .into_iter()
            .filter(|c| c.rule_id == "us.fed.executive.order")
            .map(|c| c.text)
            .collect::<Vec<_>>()
    };
    for t in [
        "Ohio Executive Order 2020-01D closed schools.",
        "Under Exec. Order No. 2020-01, the Governor declared an emergency.",
        "Executive Order 2021-12 of the Governor of Texas.",
        "See E.O. 2019-3.",
    ] {
        assert!(federal(t).is_empty(), "{t}: {:?}", federal(t));
    }
    assert_eq!(
        federal("See Exec. Order No. 14,028 and Executive Order 12866."),
        vec!["Exec. Order No. 14,028", "Executive Order 12866"]
    );
    // A federal order followed by a state one in the same sentence.
    assert_eq!(
        federal("Exec. Order No. 13,985; cf. Executive Order 2020-01."),
        vec!["Exec. Order No. 13,985"]
    );
    let c = &detect("See Exec. Order No. 14,028.")[0];
    assert!((c.confidence - 0.8).abs() < 1e-9, "{}", c.confidence);
}
