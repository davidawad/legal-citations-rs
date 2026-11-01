//! Court parentheticals resolve a case citation's jurisdiction, and
//! selection follows it (lcr-k5o).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::court::{combine, courts, jurisdiction_of, key, Court, CourtTable};
use legal_citations::registry::registry;
use legal_citations::{detect, detect_with, Kind, Selector};

#[test]
fn court_strings_resolve_to_jurisdictions() {
    for (court, want) in [
        ("Ohio", "us.oh.sup"),
        ("Ohio Ct. App.", "us.oh.app"),
        // Not in the table: the state, not its supreme court.
        ("Ohio Ct. Com. Pl.", "us.oh"),
        ("Ohio C.P.", "us.oh.trial"),
        ("9th Cir.", "us.fed.app.ca9"),
        ("1st Cir.", "us.fed.app.ca1"),
        ("11th Cir.", "us.fed.app.ca11"),
        ("D.C. Cir.", "us.fed.app.cadc"),
        ("Fed. Cir.", "us.fed.app.cafc"),
        ("D.C.", "us.dc.sup"),
        ("E.D. Tex.", "us.fed.dist.txed"),
        ("S.D.N.Y.", "us.fed.dist.nysd"),
        ("S.D. N.Y.", "us.fed.dist.nysd"),
        ("N.D. Cal.", "us.fed.dist.cand"),
        ("N.D.", "us.nd.sup"),
        ("D. Mass.", "us.fed.dist.mad"),
        ("D.N.J.", "us.fed.dist.njd"),
        ("Bankr. N.D. Ill.", "us.fed.bankr.ilnd"),
        ("Bankr. D. Del.", "us.fed.bankr.ded"),
        ("B.A.P. 9th Cir.", "us.fed.bankr.bap.ca9"),
        ("C.C.S.D.N.Y.", "us.fed.cc.nysd"),
        ("U.S.", "us.scotus"),
        ("Tex. Crim. App.", "us.tx.sup"),
        ("Tex. App.", "us.tx.app"),
        ("Tex. App.\u{2014}Houston [1st Dist.]", "us.tx.app"),
        ("N.Y. App. Div.", "us.ny.app"),
        ("N.Y. Sup. Ct.", "us.ny.trial"),
        ("Del. Ch.", "us.de.trial"),
        ("W. Va.", "us.wv.sup"),
        ("Va.", "us.va.sup"),
        ("Ct. Int\u{2019}l Trade", "us.fed.cit"),
        ("Ct. Int'l Trade", "us.fed.cit"),
        ("C.A.A.F.", "us.fed.military.caaf"),
        ("Jackson, Circuit Justice, 2d Cir.", "us.fed.app.ca2"),
        ("Guam", "us.gu.sup"),
    ] {
        assert_eq!(jurisdiction_of(court), Some(want), "{court}");
    }
    for court in ["", "Ohioan Ct.", "Smith, J.", "No.", "Alab."] {
        assert_eq!(jurisdiction_of(court), None, "{court}");
    }
    assert!(courts().courts().len() > 1000);
    // Every federal district T1 describes, for every state.
    for d in [
        "txed", "txnd", "txsd", "txwd", "nyed", "cacd", "ohsd", "prd",
    ] {
        assert!(
            legal_citations::court::court_jurisdictions().contains(&format!("us.fed.dist.{d}")),
            "{d}"
        );
    }
    assert!(courts()
        .courts()
        .iter()
        .all(|c| !c.source.is_empty() && c.jurisdiction.starts_with("us")));
}

#[test]
fn tables_match_on_token_or_period_boundaries() {
    let c = |court: &str, j: &str| Court {
        court: court.into(),
        jurisdiction: j.into(),
        source: "test".into(),
        partial: None,
    };
    let t = CourtTable::new(vec![
        c("Ohio", "us.oh"),
        c("Ohio", "us.dup"),
        c("X.", "us.x"),
    ]);
    assert_eq!(t.courts().len(), 3);
    assert_eq!(t.lookup("Ohio App.").unwrap().jurisdiction, "us.oh");
    assert!(t.lookup("OhioApp.").is_none());
    assert_eq!(t.lookup("X.Y.").unwrap().jurisdiction, "us.x");
    assert_eq!(key(" A\u{2019}b  c "), "A'bc");
    assert!(CourtTable::from_json("{").is_err());
    assert_eq!(
        CourtTable::from_json(r#"{"courts": []}"#).unwrap().courts(),
        &[]
    );
    assert_eq!(combine("us.fed.dist", "us.fed"), "us.fed.dist");
    assert_eq!(combine("us.fed", "us.fed"), "us.fed");
    assert_eq!(combine("us.state", "us.oh"), "us.oh");
    assert_eq!(combine("us", "us.fed"), "us.fed");
    assert_eq!(
        combine("us.fed.dist", "us.fed.dist.txed"),
        "us.fed.dist.txed"
    );
    assert_eq!(combine("us.state", "us.oh.sup"), "us.oh.sup");
    let partial = Court {
        partial: Some("us.oh".into()),
        ..c("Ohio", "us.oh.sup")
    };
    assert_eq!(partial.jurisdiction_for(true), "us.oh.sup");
    assert_eq!(partial.jurisdiction_for(false), "us.oh");
    assert_eq!(c("X.", "us.x").jurisdiction_for(false), "us.x");
}

const MIXED: &str = "Smith v. Jones, 100 N.E.2d 1 (Ohio 2010). \
Doe v. Roe, 2010 WL 123456 (Ohio Ct. App. 2010). \
Fed v. Case, 250 F.3d 1 (9th Cir. 2001). Id. at 3. \
Doe v. Ohio, 5 N.E.3d 9 (Ohio Ct. App. 2015). 5 N.E.3d at 12.";

#[test]
fn citations_carry_court_and_rule_jurisdictions() {
    let cites = detect(MIXED);
    let got: Vec<(&str, &str, &str)> = cites
        .iter()
        .map(|c| {
            (
                c.text.as_str(),
                c.jurisdiction.as_str(),
                c.rule_jurisdiction.as_str(),
            )
        })
        .collect();
    let find = |t: &str| got.iter().find(|g| g.0.contains(t)).unwrap();
    assert_eq!(find("100 N.E.2d").1, "us.oh.sup");
    assert!(!find("100 N.E.2d").2.starts_with("us.oh"));
    assert_eq!(find("WL 123456").1, "us.oh.app");
    assert_eq!(find("250 F.3d").1, "us.fed.app.ca9");
    // Short forms take their antecedent's.
    let id = cites.iter().find(|c| c.kind == Kind::Id).unwrap();
    assert_eq!(id.jurisdiction, "us.fed.app.ca9", "{id:#?}");
    let short = cites.iter().find(|c| c.kind == Kind::CaseShort).unwrap();
    assert_eq!(short.jurisdiction, "us.oh.app");
    // No court parenthetical: the rule's.
    let plain = detect("See 100 Ohio St.3d 1.");
    assert_eq!(plain[0].jurisdiction, plain[0].rule_jurisdiction);
}

#[test]
fn jurisdiction_selection_follows_the_court() {
    let only = |include: &[&str], exclude: &[&str]| {
        let s = registry()
            .select(&Selector {
                include: include.iter().map(|s| s.to_string()).collect(),
                exclude: exclude.iter().map(|s| s.to_string()).collect(),
                profiles: vec![],
            })
            .unwrap();
        detect_with(&s, MIXED)
            .into_iter()
            .map(|c| c.text)
            .collect::<Vec<_>>()
    };
    let ohio = only(&["jurisdiction:us.oh"], &[]);
    assert!(
        ohio.iter().any(|t| t.contains("100 N.E.2d 1 (Ohio 2010)")),
        "{ohio:?}"
    );
    assert!(
        ohio.iter().any(|t| t.contains("2010 WL 123456")),
        "{ohio:?}"
    );
    assert!(ohio.iter().any(|t| t == "5 N.E.3d at 12"), "{ohio:?}");
    assert!(!ohio.iter().any(|t| t.contains("9th Cir.")), "{ohio:?}");
    assert!(!ohio.iter().any(|t| t.starts_with("Id.")), "{ohio:?}");
    // Exclude follows the court too.
    let not_ohio = only(&[], &["jurisdiction:us.oh"]);
    assert!(
        not_ohio.iter().any(|t| t.contains("9th Cir.")),
        "{not_ohio:?}"
    );
    assert!(
        !not_ohio.iter().any(|t| t.contains("(Ohio")),
        "{not_ohio:?}"
    );
    // A federal selection drops the Ohio N.E.2d cite.
    let fed = only(&["jurisdiction:us.fed*"], &[]);
    assert!(fed.iter().any(|t| t.contains("9th Cir.")), "{fed:?}");
    assert!(!fed.iter().any(|t| t.contains("(Ohio")), "{fed:?}");
    // Profiles go through the same patterns.
    let s = registry()
        .select(&Selector {
            include: vec![],
            exclude: vec![],
            profiles: vec!["federal-litigation".into()],
        })
        .unwrap();
    let texts: Vec<String> = detect_with(&s, MIXED).into_iter().map(|c| c.text).collect();
    // `rule:us.case.westlaw` is named outright, so the Ohio WL cite stays.
    assert!(!texts.iter().any(|t| t.contains("100 N.E.2d")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("2010 WL")), "{texts:?}");
}
