//! Indigo Book format coverage: the criterion, the report, and the
//! regression gate (the detected counts must not drop below
//! `docs/measurements/indigo-baseline.json`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::coverage::{accepts, core, covers, markdown, run, Baseline};
use legal_citations::{detect, Kind};
use std::path::Path;

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn indigo_coverage_does_not_drop_below_the_baseline() {
    let jsonl = std::fs::read_to_string(root().join("corpus/formats/indigo.jsonl")).unwrap();
    let base: Baseline = serde_json::from_str(
        &std::fs::read_to_string(root().join("docs/measurements/indigo-baseline.json")).unwrap(),
    )
    .unwrap();
    let report = run(&jsonl).unwrap();
    assert_eq!(report.formats.total, 1225);
    let regressions = report.regressions(&base);
    assert!(
        regressions.is_empty(),
        "{regressions:?}; misses: {:?}",
        report
            .lines
            .iter()
            .filter(|l| !l.detected)
            .map(|l| l.line)
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_core_skips_names_and_trailing_parentheticals() {
    let ex = "Smith v. Jones, 374 So. 609, 613 (Ala. 1914)";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "374 So. 609, 613");
    let ex = "Ala. Code § 41-65-49 (West (2d) 2015).";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "§ 41-65-49");
    let ex = "Communications Act of 1934, 47 U.S.C. § 223 (2012)";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "47 U.S.C. § 223");
    let ex = "I.R.C. § 501(c)(3) (2012)";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "§ 501(c)(3");
    // An attached parenthetical is not stripped.
    let ex = "Pub. L. 5(1999)";
    assert_eq!(&ex[core(ex).0..core(ex).1], "5(1999");
    let ex = "  Id.  ";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "Id.");
    // Only a parenthetical has digits: the whole example counts.
    let ex = "Act (1999)";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "1999");
    let ex = "Smith § x";
    let (a, b) = core(ex);
    assert_eq!(&ex[a..b], "§");
    // An unbalanced trailing paren is left alone.
    let ex = "5 U.S. 1)";
    assert_eq!(&ex[..core(ex).1], "5 U.S. 1");
}

#[test]
fn coverage_needs_a_compatible_kind_spanning_the_core() {
    let case = accepts("case").unwrap();
    assert!(accepts("nonsense").is_none());
    assert_eq!(
        accepts("statute").unwrap(),
        vec![Kind::Statute, Kind::Id, Kind::StatuteShort]
    );
    assert_eq!(accepts("treaty").unwrap(), vec![Kind::Treaty]);
    assert_eq!(
        accepts("secondary").unwrap(),
        vec![Kind::Secondary, Kind::Supra, Kind::Id]
    );
    assert_eq!(
        accepts("agency_decision").unwrap(),
        vec![Kind::AgencyDecision, Kind::AgencyManual, Kind::Case]
    );
    assert_eq!(
        accepts("agency_manual").unwrap(),
        vec![Kind::AgencyManual, Kind::AgencyDecision]
    );
    let ex = "Smith v. Jones, 501 F.3d 12, 19 (1st Cir. 2007)";
    assert!(covers(ex, &detect(ex), &case));
    assert!(!covers(ex, &detect(ex), &[Kind::Statute]));
    assert!(!covers(ex, &[], &case));
    // Two cites separated by `, ` together cover a parallel citation.
    let ex = "261 Ill. App. 3d 443, 633 N.E.2d 764";
    assert!(covers(ex, &detect(ex), &case));
    // A cite that covers only part of the core does not count.
    let ex = "See 28 U.S.C. § 1331 and 999 Nonsense 12";
    assert!(!covers(ex, &detect(ex), &[Kind::Statute]));
    // A covering run followed by an unrelated, separate cite still counts.
    let ex = "501 F.3d 12; and later 28 U.S.C. § 1331";
    let mut cites = detect(ex);
    for c in &mut cites {
        c.kind = Kind::Case;
    }
    assert!(covers("501 F.3d 12", &cites, &case));
    // Subsequent history continues its decision's run, phrase included.
    let ex = "United States v. Schmuck, 840 F.2d 384 (7th Cir. 1988), aff’d, 489 U.S. 705 (1989)";
    assert!(covers(ex, &detect(ex), &case));
    let mut cites = detect(ex);
    for c in &mut cites {
        c.details = legal_citations::citation::Details::Other;
    }
    assert!(!covers(ex, &cites, &case), "unlinked, the phrase is a gap");
}

#[test]
fn reports_tally_and_render_misses() {
    let jsonl = r#"{"section":"T1 A","jurisdiction":"us.fed","kind":"case","form":"f","examples":["Smith v. Jones, 501 F.3d 12 (1st Cir. 2007)"]}

{"section":"T1 B | pipe","jurisdiction":"us.fed","kind":"statute","form":"f","examples":["Nothing 12 Here"]}
{"section":"T1 C","jurisdiction":"us.oh","kind":"treaty","form":"f","examples":[]}
"#;
    let r = run(jsonl).unwrap();
    assert_eq!(
        (r.formats.total, r.formats.detected, r.formats.missed),
        (3, 1, 2)
    );
    assert_eq!((r.examples.total, r.examples.detected), (2, 1));
    assert_eq!(r.by_kind["case"].detected, 1);
    assert_eq!(r.by_jurisdiction["us.fed"].total, 2);
    assert_eq!(r.lines[1].line, 3);
    let md = markdown(&r, "x.jsonl");
    assert!(md.contains("| formats | 3 | 1 | 2 | 33.3% |"), "{md}");
    assert!(md.contains("## Misses (2)"));
    assert!(md.contains("T1 B \\| pipe"));
    assert!(md.contains("(no examples)"));
    assert!(md.contains("| nothing |"));
    let base = r.baseline();
    assert!(r.regressions(&base).is_empty());
    let higher = Baseline {
        formats_detected: 2,
        examples_detected: 2,
    };
    assert_eq!(r.regressions(&higher).len(), 2);
    assert!(run("{").unwrap_err().starts_with("line 1"));
    assert!(
        run(r#"{"section":"s","jurisdiction":"j","kind":"zzz","form":"f","examples":[]}"#)
            .unwrap_err()
            .contains("unknown kind")
    );
    let empty = run("").unwrap();
    assert!(markdown(&empty, "e").contains("| formats | 0 | 0 | 0 | - |"));
}
