//! L4 masking details: emphasis delimiters, zone bookkeeping, footnote
//! scoping of links and suppression-comment spans.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::document::{parse_markdown, ZoneKind};
use legal_citations::{detect_markdown, Kind};

#[test]
fn emphasis_delimiter_runs_are_masked_whole() {
    let md = "**bold** and _it_ and ~~gone~~ and ***both***";
    let masked = parse_markdown(md).masked;
    assert_eq!(masked, md.replace(['*', '_', '~'], " "));
}

#[test]
fn zones_are_never_empty() {
    let md = "<https://example.com> and [x](https://example.com/y).";
    let parsed = parse_markdown(md);
    assert!(parsed.zones.iter().all(|z| !z.span.is_empty()));
    let syntax: Vec<&str> = parsed
        .zones
        .iter()
        .filter(|z| z.kind == ZoneKind::LinkSyntax)
        .map(|z| &md[z.span.start..z.span.end])
        .collect();
    assert_eq!(
        syntax,
        vec!["<https://example.com>", "[", "](https://example.com/y)"]
    );
}

#[test]
fn links_after_a_footnote_definition_are_not_in_it() {
    let md =
        "Text.[^1]\n\n[^1]: See [a](https://example.com/a).\n\nLater [b](https://example.com/b).\n";
    let doc = detect_markdown(md);
    assert_eq!(doc.links.len(), 2);
    assert_eq!(doc.links[0].footnote.as_deref(), Some("1"));
    assert_eq!(doc.links[1].footnote, None);
}

#[test]
fn suppression_span_is_exactly_the_comment() {
    let comment = "<!-- cite-detect: ignore CITE001 reason=\"quoted\" -->";
    let md = format!("Intro.\n\n{comment}\n42 U.S.C. § 1983\n\nInline {comment} 5 U.S.C. § 552\n");
    let doc = detect_markdown(&md);
    assert_eq!(doc.suppressions.len(), 2);
    for s in &doc.suppressions {
        assert_eq!(&md[s.span.start..s.span.end], comment);
    }
    assert!(doc.citations.iter().all(|c| c.kind == Kind::Statute));
}

#[test]
fn html_block_and_footnote_label_zones_cover_exactly_their_syntax() {
    let md = "Intro.\n\n<div>42 U.S.C. § 1983</div>\n\nText.[^1]\n\n[^1]: Footnote text.\n";
    let parsed = parse_markdown(md);
    let zones = |kind: ZoneKind| -> Vec<&str> {
        parsed
            .zones
            .iter()
            .filter(|z| z.kind == kind)
            .map(|z| &md[z.span.start..z.span.end])
            .collect()
    };
    assert_eq!(zones(ZoneKind::Html), vec!["<div>", "</div>"]);
    assert_eq!(zones(ZoneKind::FootnoteLabel), vec!["[^1]", "[^1]:"]);
    assert_eq!(detect_markdown(md).citations.len(), 1);
}

#[test]
fn links_bind_only_citations_they_cover() {
    let md = "[source](https://www.law.cornell.edu/uscode/text/28/1331) and 42 U.S.C. § 1983.";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1);
    assert!(!doc.is_linked(0));
}

#[test]
fn footnote_references_bind_only_shortly_after_the_citation() {
    let def = "\n\n[^1]: [src](https://www.law.cornell.edu/uscode/text/42/1983).\n";
    let far = format!(
        "42 U.S.C. § 1983 {}[^1] and 5 U.S.C. § 552.{def}",
        "x ".repeat(110)
    );
    let doc = detect_markdown(&far);
    assert_eq!(doc.citations.len(), 2);
    assert!(!doc.is_linked(0), "a reference 220 bytes later is too far");
    let far_last = format!("42 U.S.C. § 1983 {}[^1]{def}", "x ".repeat(110));
    assert!(!detect_markdown(&far_last).is_linked(0));
    let before = format!("Text[^1] and later 42 U.S.C. § 1983.{def}");
    assert!(!detect_markdown(&before).is_linked(0));
    let near = format!("42 U.S.C. § 1983.[^1]{def}");
    assert!(detect_markdown(&near).is_linked(0));
}
