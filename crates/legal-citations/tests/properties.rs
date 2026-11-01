//! Properties that must hold for ANY input: no panics, spans slice the
//! original text, coordinates agree, output is ordered and non-overlapping.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::span::OffsetMap;
use legal_citations::{detect, detect_markdown, evaluate, registry, Mode, Policy};
use proptest::prelude::*;

/// Text built from citation-shaped fragments, markdown syntax and noise, so
/// the properties are exercised on inputs that actually produce citations.
fn legalish() -> impl Strategy<Value = String> {
    let fragments = prop::sample::select(vec![
        "Smith v. Jones, ",
        "123 F.3d 456",
        " (9th Cir. 2001)",
        "28 U.S.C. § 1331",
        "Id. at 5",
        "Upjohn, supra",
        "449 U.S. at 389",
        "[",
        "](https://www.law.cornell.edu/uscode/text/28/1331)",
        "*",
        "_",
        "`",
        "\n",
        "\n\n",
        "---\n",
        "<!--u-->",
        "[^1]",
        "[^1]: ",
        " ",
        "; ",
        ", ",
        ". ",
        "See ",
        "é",
        "§",
        "“",
        "”",
        "٤",
        "ER-139",
        "Fed. R. Civ. P. 12(b)(6)",
        "| a | b |\n|---|---|\n",
        "<a href=\"x\">",
        "</a>",
        "MPEP § 2143",
        "(",
        ")",
    ]);
    prop::collection::vec(
        prop_oneof![fragments.prop_map(str::to_string), "\\PC{0,8}"],
        0..24,
    )
    .prop_map(|v| v.concat())
}

fn check_span(text: &str, map: &OffsetMap, s: &legal_citations::Span) {
    assert!(s.start <= s.end && s.end <= text.len());
    assert!(text.is_char_boundary(s.start) && text.is_char_boundary(s.end));
    let again = map.span(s.start, s.end);
    assert_eq!(&again, s);
    assert_eq!(text[..s.start].chars().count(), s.char_start);
    assert_eq!(text[..s.end].chars().count(), s.char_end);
    assert_eq!(text[..s.start].encode_utf16().count(), s.utf16_start);
    assert_eq!(text[..s.start].matches('\n').count() + 1, s.line);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn detect_spans_slice_the_original(text in legalish()) {
        let map = OffsetMap::new(&text);
        let cites = detect(&text);
        let mut prev_end = 0;
        for c in &cites {
            check_span(&text, &map, &c.span);
            check_span(&text, &map, &c.core_span);
            check_span(&text, &map, &c.full_span);
            prop_assert_eq!(&text[c.span.start..c.span.end], c.text.as_str());
            prop_assert!(c.span.contains(&c.core_span));
            prop_assert!(c.full_span.contains(&c.span));
            prop_assert!(c.core_span.start >= prev_end, "core spans overlap");
            prev_end = c.core_span.end;
            prop_assert!((0.0..=1.0).contains(&c.confidence));
            if let Some(a) = c.antecedent { prop_assert!(a < cites.len()); }
        }
    }

    #[test]
    fn markdown_never_panics_and_spans_hold(text in legalish()) {
        let map = OffsetMap::new(&text);
        let doc = detect_markdown(&text);
        prop_assert_eq!(doc.bindings.len(), doc.citations.len());
        for c in &doc.citations {
            check_span(&text, &map, &c.span);
            prop_assert_eq!(&text[c.span.start..c.span.end], c.text.as_str());
        }
        for l in &doc.links {
            check_span(&text, &map, &l.span);
            check_span(&text, &map, &l.text_span);
        }
        for z in &doc.zones { check_span(&text, &map, &z.span); }
        for b in doc.bindings.iter().flatten() { prop_assert!(b.link < doc.links.len()); }
        let findings = evaluate(registry(), &doc, &Policy::default(), Mode::Lint);
        for f in &findings { check_span(&text, &map, &f.span); }
    }

    #[test]
    fn link_free_markdown_matches_plain_text(text in "[A-Za-z0-9 .,;§]{0,120}") {
        // Four leading spaces make an indented code block, which markdown
        // masks (`indented_code_is_not_prose_in_markdown`).
        prop_assume!(!text.starts_with("    "));
        let plain: Vec<_> = detect(&text).into_iter().map(|c| (c.rule_id, c.span)).collect();
        let md: Vec<_> = detect_markdown(&text).citations.into_iter().map(|c| (c.rule_id, c.span)).collect();
        prop_assert_eq!(plain, md);
    }

    #[test]
    fn arbitrary_unicode_never_panics(text in "\\PC{0,200}") {
        let _ = detect(&text);
        let _ = detect_markdown(&text);
        let _ = legal_citations::legacy::detect_legacy(&text);
    }
}

/// The one link-free input where markdown and plain text differ on purpose:
/// proptest found it (`"    §0"`), so it is pinned here rather than left to
/// a random seed.
#[test]
fn indented_code_is_not_prose_in_markdown() {
    let text = "    §0";
    assert_eq!(detect(text).len(), 1);
    assert!(detect_markdown(text).citations.is_empty());
    assert_eq!(detect_markdown(&text[1..]).citations.len(), 1);
}
