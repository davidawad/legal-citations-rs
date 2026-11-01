//! Regressions from the 2026-10-22 real-text measurement
//! (`docs/measurements/2026-10-22-real-text.md`): false positives, missed
//! state forms, truncated spans and page furniture.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::citation::Furniture;
use legal_citations::{detect, detect_markdown, evaluate, registry, Mode, Policy};

fn texts(text: &str) -> Vec<(String, String)> {
    detect(text)
        .into_iter()
        .map(|c| (c.rule_id, c.text))
        .collect()
}

#[test]
fn alaska_rule_ignores_all_caps_as_and_was() {
    for text in [
        "The motion to dismiss is DENIED AS MOOT.",
        "THOMAS M. COOLEY LAW SCHOOL",
        "THIS OPINION WAS FILED FOR RECORD",
        "Before NICOLAS BAURIN, Administrative Patent Judge.",
        "SUPREME COURT OF TEXAS\ncertiorari granted",
        "HOW PARTLET DIED AND WAS BURIED, AND HOW CHANTICLEER DIED",
        "WHAT FEAR WAS\r\n\r\n\r\nA certain father had two sons",
    ] {
        assert!(
            !texts(text).iter().any(|(r, _)| r == "us.ak.statute.code"),
            "{text:?}"
        );
    }
    assert_eq!(
        texts("Under AS 11.41.100(a)(1), the claim fails."),
        vec![(
            "us.ak.statute.code".to_string(),
            "AS 11.41.100(a)(1)".to_string()
        )]
    );
}

#[test]
fn ohio_revised_code_webcite_and_state_reports() {
    let text = "R.C. 2921.31(A) applies. See State v. Vargas, 2012-Ohio-2768, ¶ 15 \
                (8th Dist.); State v. Moreland, 50 Ohio St.3d 58, 62 (1990).";
    let cites = detect(text);
    let got: Vec<_> = cites
        .iter()
        .map(|c| (c.rule_id.as_str(), c.text.as_str()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("us.oh.statute.rc", "R.C. 2921.31(A)"),
            (
                "us.oh.case.webcite",
                "State v. Vargas, 2012-Ohio-2768, ¶ 15"
            ),
            (
                "us.oh.case.ohio_state",
                "State v. Moreland, 50 Ohio St.3d 58, 62 (1990)"
            ),
        ]
    );
    assert_eq!(cites[0].authority.as_deref(), Some("us.oh.rc/2921.31(a)"));
    assert_eq!(cites[1].fields["pincite"], "15");
    assert_eq!(
        cites[1].authority.as_deref(),
        Some("us.oh.webcite/2012/2768")
    );
    assert_eq!(cites[2].authority.as_deref(), Some("case/50/ohiost.3d/58"));
}

fn has(text: &str, rule: &str, want: &str) {
    let got = texts(text);
    assert!(
        got.iter().any(|(r, t)| r == rule && t == want),
        "{text:?}: want {rule} {want:?}, got {got:#?}"
    );
}

#[test]
fn pin_ranges_with_en_and_figure_dashes_stay_in_the_span() {
    has(
        "See Bruen, 597 U.S. at 31–32.",
        "us.short.case_at",
        "Bruen, 597 U.S. at 31–32",
    );
    has(
        "Bousley, 523 U.S. at 49‒52.",
        "us.short.case_at",
        "Bousley, 523 U.S. at 49‒52",
    );
    has(
        "63 Fed. Reg. 879–880 (Jan. 7, 1998).",
        "us.fed.federal_register",
        "63 Fed. Reg. 879–880",
    );
    has(
        "See 113 Stat. 1659–60.",
        "us.fed.session_law.statutes_at_large",
        "113 Stat. 1659–60",
    );
    has(
        "Garner, supra, at 505–506.",
        "us.short.supra",
        "Garner, supra, at 505–506",
    );
    has("Id. at 49‒52.", "us.short.id", "Id. at 49‒52");
    has(
        "Feldman v. Cutaia, 951 A.2d 727, 730–31 (Del. 2008).",
        "us.case.regional",
        "Feldman v. Cutaia, 951 A.2d 727, 730–31 (Del. 2008)",
    );
}

#[test]
fn case_names_after_quoting_and_citing_inside_parentheticals() {
    let text = "Id. at 5 (quoting Monell v. Dep't of Soc. Servs., 436 U.S. 658, 690 (1978)).";
    has(
        text,
        "us.case.us_reports",
        "Monell v. Dep't of Soc. Servs., 436 U.S. 658, 690 (1978)",
    );
    let text = "Smith v. Jones, 1 F.3d 2 (1st Cir. 1990) (citing Am. Well Works Co. v. Layne & \
                Bowler Co., 241 U.S. 257, 260 (1916)).";
    has(
        text,
        "us.case.federal_reporter",
        "Smith v. Jones, 1 F.3d 2 (1st Cir. 1990)",
    );
    has(
        text,
        "us.case.us_reports",
        "Am. Well Works Co. v. Layne & Bowler Co., 241 U.S. 257, 260 (1916)",
    );
    let text = "Doe v. Roe, 9 F.3d 1 (1st Cir. 1993) (quoting In re Rubiola, 334 S.W.3d 220, 224 (Tex. 2011)).";
    has(
        text,
        "us.case.regional",
        "In re Rubiola, 334 S.W.3d 220, 224 (Tex. 2011)",
    );
    // A capitalized "Quoting" is a stop word, not part of the name.
    has(
        "Quoting Conte v. Bank of Am., N.A., 52 F. Supp. 3d 265, 269 (D. Mass. 2014).",
        "us.case.federal_supplement",
        "Conte v. Bank of Am., N.A., 52 F. Supp. 3d 265, 269 (D. Mass. 2014)",
    );
}

#[test]
fn westlaw_star_pin_and_court_date_parenthetical() {
    let text =
        "Manzella v. United Parcel Serv., Inc., 2002 WL 31040170, at *3 (E.D. La. Sept. 10, 2002).";
    let c = detect(text).remove(0);
    assert_eq!(c.rule_id, "us.case.westlaw");
    assert_eq!(
        c.text,
        "Manzella v. United Parcel Serv., Inc., 2002 WL 31040170, at *3 (E.D. La. Sept. 10, 2002)"
    );
    assert_eq!(c.fields["pincite"], "*3");
    assert_eq!(c.fields["court"], "E.D. La.");
    assert_eq!(c.fields["date"], "Sept. 10, 2002");
    has(
        "ATP Marin, Inc. v. Sentinel Glob. Partners USA, Inc., 2026 WL 1257792, at *3–4 (Del. Ch. Apr. 1, 2026).",
        "us.case.westlaw",
        "ATP Marin, Inc. v. Sentinel Glob. Partners USA, Inc., 2026 WL 1257792, at *3–4 (Del. Ch. Apr. 1, 2026)",
    );
}

#[test]
fn parallel_cite_volume_is_not_a_pin() {
    let text = "State v. Moreland, 50 Ohio St.3d 58, 62, 552 N.E.2d 894 (1990).";
    let cites = detect(text);
    assert_eq!(cites[0].text, "State v. Moreland, 50 Ohio St.3d 58, 62");
    assert_eq!(cites[0].fields["pincite"], "62");
    assert_eq!(cites[1].text, "552 N.E.2d 894 (1990)");
    let text = "Mink v. Gen. Motors Corp., 72 F.3d 130, 1995 WL 723180, at *2 (6th Cir. 1995).";
    let cites = detect(text);
    assert_eq!(cites[0].text, "Mink v. Gen. Motors Corp., 72 F.3d 130");
    assert!(!cites[0].fields.contains_key("pincite"));
    assert_eq!(cites[1].text, "1995 WL 723180, at *2 (6th Cir. 1995)");
}

#[test]
fn short_forms_include_the_short_case_name() {
    has(
        "Bruen, 597 U.S. at 17.",
        "us.short.case_at",
        "Bruen, 597 U.S. at 17",
    );
    has(
        "In Reid, 970 A.2d at 183, the court",
        "us.short.case_at",
        "Reid, 970 A.2d at 183",
    );
    has(
        "See, e.g., Town of Greece, 572 U.S. at 589.",
        "us.short.case_at",
        "Town of Greece, 572 U.S. at 589",
    );
    has(
        "(quoting Bruen, 597 U.S. at 29)",
        "us.short.case_at",
        "Bruen, 597 U.S. at 29",
    );
    has(
        "(Bruen, 597 U.S. at 29)",
        "us.short.case_at",
        "Bruen, 597 U.S. at 29",
    );
    // Lowercase prose before the comma is not a name.
    has(
        "as the court held, 597 U.S. at 17",
        "us.short.case_at",
        "597 U.S. at 17",
    );
    has("597 U.S. at 17", "us.short.case_at", "597 U.S. at 17");
    let c = detect("Bruen, 597 U.S. at 17.").remove(0);
    assert_eq!(c.fields["case_name"], "Bruen");
    // The name never reaches back into the previous citation.
    let text = "Bruen, 597 U.S. 1 (2022), 597 U.S. at 17";
    let cites = detect(text);
    assert_eq!(cites[1].text, "597 U.S. at 17");
}

const FURNITURE: &str = "Cite as: 606 U. S. 656 (2025)\n\nOpinion of the Court\n\n\
    IPR2025-00687\r\nPatent 11,234,567 B2\r\n\r\n\
    The Board in IPR2025-00687 held otherwise.\n\n\
    IPR2025-00687 (Patent 11,234,567 B2)\nIPR2025-00687 (Patent 11,234,567 B2)\n\
    IPR2025-00687\nPage 7\n";

#[test]
fn running_heads_and_repeated_captions_are_marked_not_dropped() {
    let cites = detect(FURNITURE);
    let got: Vec<_> = cites
        .iter()
        .map(|c| (c.text.as_str(), c.furniture))
        .collect();
    assert_eq!(
        got,
        vec![
            ("606 U. S. 656 (2025)", Some(Furniture::RunningHead)),
            ("IPR2025-00687", Some(Furniture::RepeatedCaption)),
            ("IPR2025-00687", None),
            ("IPR2025-00687", Some(Furniture::RepeatedCaption)),
            ("IPR2025-00687", Some(Furniture::RepeatedCaption)),
            ("IPR2025-00687", Some(Furniture::RepeatedCaption)),
        ]
    );
    // A cite alone on a line once, or with prose on its line, is not furniture.
    for text in [
        "Brown v. Board of Education, 347 U.S. 483 (1954)\n",
        "See 347 U.S. 483 (1954).\nSee 347 U.S. 483 (1954).\n",
        "Id.\nId.\n",
        "Cite as: 606 U. S. 656 (2025), and more prose\n",
    ] {
        assert!(
            detect(text).iter().all(|c| c.furniture.is_none()),
            "{text:?}"
        );
    }
}

#[test]
fn link_policy_skips_furniture_unless_asked() {
    let reg = registry();
    let doc = detect_markdown(FURNITURE);
    assert_eq!(doc.citations.len(), 6);
    let unlinked = |policy: &Policy| {
        evaluate(reg, &doc, policy, Mode::Lint)
            .iter()
            .filter(|f| f.code == "CITE001")
            .map(|f| f.text.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(unlinked(&Policy::default()), vec!["IPR2025-00687"]);
    let strict = Policy {
        link_furniture: true,
        ..Policy::default()
    };
    assert_eq!(unlinked(&strict).len(), 6);
}

#[test]
fn short_case_form_with_its_own_authority_key_keeps_it() {
    let mut short: serde_json::Value = serde_json::from_str(include_str!(
        "../../../rules/case_short/us.short.case_at.json"
    ))
    .unwrap();
    short["authority_key"] = "case/{volume}/{reporter}/at/{pin}".into();
    let short = short.to_string();
    let reg = legal_citations::Registry::from_sources(&[
        (
            "us_reports.json",
            include_str!("../../../rules/case/us.case.us_reports.json"),
        ),
        ("case_at.json", &short),
        (
            "id.json",
            include_str!("../../../rules/id/us.short.id.json"),
        ),
    ]);
    let text =
        "Upjohn Co. v. United States, 449 U.S. 383 (1981). Upjohn, 449 U.S. at 389. Id. at 390.";
    let cites = legal_citations::detect_with(&reg, text);
    assert_eq!(cites[1].antecedent, Some(0));
    assert_eq!(cites[1].authority.as_deref(), Some("case/449/u.s./at/389"));
    // Id. refers to the short form just before it and takes its authority.
    assert_eq!(cites[2].antecedent, Some(1));
    assert_eq!(cites[2].authority, cites[1].authority);
}

#[test]
fn furniture_trailers_page_numbers_and_prefixes() {
    let marked = |text: &str| -> Vec<Option<Furniture>> {
        detect(text).into_iter().map(|c| c.furniture).collect()
    };
    // A running head may end in a page number.
    assert_eq!(
        marked("Cite as: 606 U. S. 656 (2025)   3\n"),
        vec![Some(Furniture::RunningHead)]
    );
    // Only one whole parenthetical may follow a repeated caption.
    for text in [
        "IPR2025-00687 (a) (b)\nIPR2025-00687 (a) (b)\n",
        "IPR2025-00687 Patent (B2)\nIPR2025-00687 Patent (B2)\n",
        "IPR2025-00687 (Patent B2\nIPR2025-00687 (Patent B2\n",
        "Case IPR2025-00687\nCase IPR2025-00687\n",
    ] {
        assert_eq!(marked(text), vec![None, None], "{text:?}");
    }
    assert_eq!(
        marked("IPR2025-00687 7\nIPR2025-00687 7\n"),
        vec![Some(Furniture::RepeatedCaption); 2]
    );
}

#[test]
fn case_name_scan_allows_at_most_24_tokens() {
    let name = |n: usize| {
        let text = format!("Alpha v. {}, 1 F.3d 2", vec!["Beta"; n].join(" "));
        let core = text.find("1 F.3d").unwrap();
        legal_citations::citation::case_name_before(&text, core)
    };
    assert!(name(22).is_some());
    assert!(name(23).is_none());
}

#[test]
fn name_scans_handle_long_prefixes_connectors_and_emphasis() {
    let prose =
        "The panel spent many pages on history and tradition before turning to the text here. ";
    assert!(prose.len() > 80);
    has(
        &format!("{prose}Bruen, 597 U.S. at 17"),
        "us.short.case_at",
        "Bruen, 597 U.S. at 17",
    );
    has(
        "Under the Town of Greece, 572 U.S. at 589, prayer is allowed.",
        "us.short.case_at",
        "Town of Greece, 572 U.S. at 589",
    );
    let c = detect("*Bruen*, 597 U.S. at 17.").remove(0);
    assert_eq!(c.fields["case_name"], "Bruen");
    let c = detect("*Smith v. Jones* , 1 F.3d 2 (1st Cir. 1990).").remove(0);
    assert_eq!(c.fields["case_name"], "Smith v. Jones");
    // A connector-led scan still starts the name at a capitalized word.
    let c = detect("the Smith v. Jones, 1 F.3d 2 (1st Cir. 1990).").remove(0);
    assert_eq!(c.fields["case_name"], "Smith v. Jones");
}
