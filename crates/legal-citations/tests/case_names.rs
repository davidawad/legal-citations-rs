//! L2 case-name scan (`case_name_before`): which words a name may contain,
//! what ends it, and how markdown noise and long prefixes are handled.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::citation::case_name_before;

/// Case name before the `1 F.3d 2` core at the end of `text`.
fn name(text: &str) -> Option<String> {
    let core = text.rfind("1 F.3d 2").unwrap();
    case_name_before(text, core).map(|(_, n)| n)
}

fn start(text: &str) -> usize {
    let core = text.rfind("1 F.3d 2").unwrap();
    case_name_before(text, core).unwrap().0
}

#[test]
fn every_party_separator_spelling_is_recognized() {
    for sep in ["v.", "vs.", "versus"] {
        let text = format!("Smith {sep} Jones, 1 F.3d 2");
        assert_eq!(name(&text), Some(format!("Smith {sep} Jones")), "{text}");
    }
}

#[test]
fn in_re_and_ex_parte_need_their_lead_word() {
    for (text, want) in [
        ("In re Doe, 1 F.3d 2", "In re Doe"),
        ("in re Doe, 1 F.3d 2", "in re Doe"),
        ("Ex parte Young, 1 F.3d 2", "Ex parte Young"),
        ("ex parte Young, 1 F.3d 2", "ex parte Young"),
        ("Applying In re Doe, 1 F.3d 2", "In re Doe"),
    ] {
        assert_eq!(name(text).as_deref(), Some(want), "{text}");
    }
    assert_eq!(start("Applying In re Doe, 1 F.3d 2"), 9);
    // "re" or "parte" without In/Ex before it is not a case name.
    assert_eq!(name("Matter re Doe, 1 F.3d 2"), None);
    assert_eq!(name("re Doe, 1 F.3d 2"), None);
}

#[test]
fn the_separator_or_in_re_nearest_the_core_wins() {
    // "re" after "v.": the In re name is the citation's name.
    assert_eq!(
        name("Smith v. Jones and In re Doe, 1 F.3d 2").as_deref(),
        Some("In re Doe")
    );
    // "v." after "re": the adversary name stops at the lowercase "re".
    assert_eq!(
        name("In re Smith v. Jones, 1 F.3d 2").as_deref(),
        Some("Smith v. Jones")
    );
}

#[test]
fn first_party_keeps_connectors_and_stops_at_clause_breaks() {
    assert_eq!(
        name("Board of Regents v. Roth, 1 F.3d 2").as_deref(),
        Some("Board of Regents v. Roth")
    );
    assert_eq!(
        name("and the Smith v. Jones, 1 F.3d 2").as_deref(),
        Some("Smith v. Jones")
    );
    for text in [
        "Note: Smith v. Jones, 1 F.3d 2",
        "Roe; Smith v. Jones, 1 F.3d 2",
        "(Roe) Smith v. Jones, 1 F.3d 2",
    ] {
        assert_eq!(name(text).as_deref(), Some("Smith v. Jones"), "{text}");
    }
    // Only connectors before the separator: no name.
    assert_eq!(name("and of v. Jones, 1 F.3d 2"), None);
}

#[test]
fn second_party_may_not_contain_clause_breaks() {
    for text in [
        "Smith v. Jones; Roe, 1 F.3d 2",
        "Smith v. (Jones, 1 F.3d 2",
        "Smith v. Jones), 1 F.3d 2",
    ] {
        assert_eq!(name(text), None, "{text}");
    }
}

#[test]
fn names_are_at_most_24_tokens() {
    // 10 first-party tokens (the scan limit) + separator + 13 = 24.
    let first = "A B C D E F G H I J";
    let ok = format!("{first} v. K L M N O P Q R S T U V W, 1 F.3d 2");
    assert_eq!(
        name(&ok).as_deref(),
        Some(&ok[..ok.find(',').unwrap()]),
        "{ok}"
    );
    let long = format!("{first} v. K L M N O P Q R S T U V W X, 1 F.3d 2");
    assert_eq!(name(&long), None);
}

#[test]
fn markdown_emphasis_is_not_part_of_the_name() {
    let text = "See *Smith v. Jones*, 1 F.3d 2";
    assert_eq!(name(text).as_deref(), Some("Smith v. Jones"));
    assert_eq!(start(text), 5);
    // Emphasis and space between the name and its comma are trimmed too.
    let spaced = "See *Smith v. Jones* , 1 F.3d 2";
    assert_eq!(name(spaced).as_deref(), Some("Smith v. Jones"));
    assert_eq!(start(spaced), 5);
}

#[test]
fn a_long_prefix_does_not_hide_the_name() {
    let text = format!("{} Smith v. Jones, 1 F.3d 2", "word ".repeat(60));
    assert_eq!(name(&text).as_deref(), Some("Smith v. Jones"));
}

#[test]
fn a_number_ending_the_previous_sentence_ends_the_name() {
    assert_eq!(
        name("See 12 Wyo. Comp. 3d 456. Smith v. Jones, 1 F.3d 2").as_deref(),
        Some("Smith v. Jones")
    );
    // A bare number inside a name is still part of it.
    assert_eq!(
        name("Local 100 Union v. Jones, 1 F.3d 2").as_deref(),
        Some("Local 100 Union v. Jones")
    );
}
