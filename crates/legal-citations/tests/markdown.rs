//! L4: markdown parsing, masking and citation-to-link binding.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::document::{LinkStyle, Via, ZoneKind};
use legal_citations::{detect_markdown, Format};

const CORNELL_1331: &str = "https://www.law.cornell.edu/uscode/text/28/1331";

#[test]
fn inline_link_binds_directly() {
    let md = format!("Jurisdiction lies under [28 U.S.C. § 1331]({CORNELL_1331}).");
    let doc = detect_markdown(&md);
    assert_eq!(doc.format, Format::Markdown);
    assert_eq!(doc.citations.len(), 1);
    assert!(doc.is_linked(0));
    let b = doc.bindings[0][0];
    assert_eq!(b.via, Via::Direct);
    let link = &doc.links[b.link];
    assert_eq!(link.style, LinkStyle::Inline);
    assert_eq!(link.dest, CORNELL_1331);
    assert_eq!(
        &md[link.text_span.start..link.text_span.end],
        "28 U.S.C. § 1331"
    );
    assert_eq!(doc.citations[0].text, "28 U.S.C. § 1331");
}

#[test]
fn citation_in_destination_is_not_detected() {
    let doc = detect_markdown(
        "[the statute](https://example.com/28%20U.S.C.%20%C2%A7%201331 \"28 U.S.C. § 1331\")",
    );
    assert!(doc.citations.is_empty());
}

#[test]
fn unlinked_citation_is_unlinked() {
    let doc = detect_markdown("Jurisdiction lies under 28 U.S.C. § 1331.");
    assert_eq!(doc.citations.len(), 1);
    assert!(!doc.is_linked(0));
}

#[test]
fn link_on_case_name_only_still_counts() {
    let md = "[*Brown v. Board of Education*](https://www.courtlistener.com/c/U.S./347/483/), 347 U.S. 483 (1954).";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1);
    let c = &doc.citations[0];
    assert_eq!(c.fields["case_name"], "Brown v. Board of Education");
    assert!(doc.is_linked(0));
    assert!(md[c.span.start..c.span.end].ends_with("(1954)"));
}

#[test]
fn italic_case_names_are_found_through_emphasis() {
    let doc = detect_markdown("*See* *Upjohn Co. v. United States*, 449 U.S. 383, 389 (1981).");
    let c = &doc.citations[0];
    assert_eq!(c.fields["case_name"], "Upjohn Co. v. United States");
    assert_eq!(c.signal.as_deref(), Some("See"));
}

#[test]
fn reference_links_and_definitions() {
    let md = "See [28 U.S.C. § 1331][usc].\n\n[usc]: https://www.law.cornell.edu/uscode/text/28/1331 \"42 U.S.C. § 1983\"\n";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1, "the definition line is masked");
    assert!(doc.is_linked(0));
    assert_eq!(doc.links[0].style, LinkStyle::Reference);
    assert_eq!(doc.links[0].dest, CORNELL_1331);
    assert!(doc.zones.iter().any(|z| z.kind == ZoneKind::LinkDefinition));
}

#[test]
fn autolinks_do_not_link_neighbouring_citations() {
    let doc = detect_markdown("28 U.S.C. § 1331 <https://www.law.cornell.edu/uscode/text/28/1331>");
    assert_eq!(doc.citations.len(), 1);
    assert!(!doc.is_linked(0));
    assert_eq!(doc.links[0].style, LinkStyle::Autolink);
}

#[test]
fn html_anchor_links() {
    let md = "Under <a href=\"https://www.law.cornell.edu/uscode/text/28/1331\">28 U.S.C. § 1331</a>, yes.";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1);
    assert!(doc.is_linked(0));
    assert_eq!(doc.links[0].style, LinkStyle::Html);
}

#[test]
fn footnote_reference_binds_to_links_in_its_definition() {
    let md = "Jurisdiction lies under 28 U.S.C. § 1331.[^1]\n\n[^1]: Text at <https://www.law.cornell.edu/uscode/text/28/1331>.\n";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1);
    assert!(doc.is_linked(0));
    assert_eq!(doc.bindings[0][0].via, Via::Footnote);
    assert_eq!(doc.links[0].footnote.as_deref(), Some("1"));
}

#[test]
fn front_matter_code_and_html_are_masked() {
    let md = "---\nauthorities:\n  usc: {cite: \"28 U.S.C. § 1331\"}\n---\n\nText `42 U.S.C. § 1983` here.\n\n<div class=\"x\">35 U.S.C. § 101</div>\n\n```\n5 U.S.C. § 552\n```\n\n| A | B |\n|---|---|\n| 17 U.S.C. § 101 | x |\n";
    let doc = detect_markdown(md);
    let texts: Vec<&str> = doc.citations.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, vec!["35 U.S.C. § 101", "17 U.S.C. § 101"]);
    for kind in [ZoneKind::FrontMatter, ZoneKind::Code, ZoneKind::Html] {
        assert!(doc.zones.iter().any(|z| z.kind == kind), "{kind:?}");
    }
    let c = &doc.citations[1];
    assert_eq!(&md[c.span.start..c.span.end], "17 U.S.C. § 101");
}

#[test]
fn directive_comments_keep_the_rest_of_the_line() {
    // formbook-style paragraph directive: CommonMark makes the line an HTML block.
    let md = "<!--u-->WHEREFORE, under 28 U.S.C. § 1331, Plaintiff prays.\n";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1);
    assert_eq!(doc.citations[0].text, "28 U.S.C. § 1331");
    assert!(doc
        .zones
        .iter()
        .any(|z| z.kind == ZoneKind::Html && &md[z.span.start..z.span.end] == "<!--u-->"));
}

#[test]
fn indirect_scheme_links_bind() {
    let doc = detect_markdown("under [28 U.S.C. § 1331](toa:usc1331).");
    assert!(doc.is_linked(0));
    assert_eq!(doc.links[0].dest, "toa:usc1331");
}

#[test]
fn images_are_masked() {
    let doc = detect_markdown("![28 U.S.C. § 1331](figure.png)");
    assert!(doc.citations.is_empty());
}

#[test]
fn suppressions_are_parsed() {
    let md = "<!-- cite-detect: ignore CITE001 reason=\"quoted from the order\" -->\n42 U.S.C. § 1983\n<!-- cite-detect: ignore-file CITE003 -->\n";
    let doc = detect_markdown(md);
    assert_eq!(doc.suppressions.len(), 2);
    assert_eq!(doc.suppressions[0].codes, vec!["CITE001"]);
    assert_eq!(doc.suppressions[0].reason, "quoted from the order");
    assert_eq!(doc.suppressions[0].scope, "next");
    assert_eq!(doc.suppressions[1].scope, "file");
    assert!(doc.suppressions[1].reason.is_empty());
}

#[test]
fn nested_link_text_and_strikethrough() {
    let doc = detect_markdown(
        "~~old~~ [**28 U.S.C. § 1331**](https://www.law.cornell.edu/uscode/text/28/1331)",
    );
    assert_eq!(doc.citations.len(), 1);
    assert!(doc.is_linked(0));
}

#[test]
fn plain_documents_have_no_links() {
    let doc =
        legal_citations::document::Document::plain(legal_citations::detect("42 U.S.C. § 1983"));
    assert_eq!(doc.format, Format::Text);
    assert!(!doc.is_linked(0));
    assert!(!doc.is_linked(7));
}

#[test]
fn link_on_short_case_name_counts_for_the_short_form() {
    let md = "[Bruen](https://www.courtlistener.com/c/U.S./597/1/), 597 U.S. at 17.";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations.len(), 1);
    let c = &doc.citations[0];
    assert_eq!(c.fields["case_name"], "Bruen");
    assert!(doc.is_linked(0));
}

#[test]
fn a_pin_before_a_parallel_cite_stops_at_the_link_text() {
    // The link destination is masked to spaces; the span must not run over it.
    let md = "See [Roe v. Wade, 410 U.S. 113, 115](https://www.courtlistener.com/c/U.S./410/113/), 93 S. Ct. 705 (1973).";
    let doc = detect_markdown(md);
    assert_eq!(doc.citations[0].text, "Roe v. Wade, 410 U.S. 113, 115");
    assert_eq!(doc.citations[0].fields["pincite"], "115");
    assert!(doc.is_linked(0));
    assert_eq!(doc.citations[1].text, "93 S. Ct. 705 (1973)");
}
