//! Legacy parity: the frozen patterns reproduce the dotfiles
//! `legal-citation-patterns` crate's fixture (generated from Python `re`)
//! exactly: same types, same order, same byte spans, same groups.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::legacy::{citation_types, detect_legacy, statute_types};
use std::collections::BTreeMap;

type Fixture = serde_json::Value;

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/legacy_parity.json")).expect("fixture parses")
}

#[test]
fn types_and_order_match() {
    let f = fixture();
    let types: Vec<String> = f["types"]
        .as_array()
        .expect("types")
        .iter()
        .map(|t| t.as_str().unwrap_or("").to_string())
        .collect();
    assert_eq!(citation_types(), types);
    let mut want: Vec<String> = f["statute_types"]
        .as_array()
        .expect("statute_types")
        .iter()
        .map(|t| t.as_str().unwrap_or("").to_string())
        .collect();
    let mut got = statute_types();
    want.sort();
    got.sort();
    assert_eq!(got, want);
    assert!(!statute_types().contains(&"case"));
}

#[test]
fn static_accessors_match_the_old_crate_api() {
    let pats = legal_citations::legacy::citation_patterns();
    assert_eq!(pats.len(), citation_types().len());
    assert_eq!(pats[0].0, "case");
    let usc = legal_citations::legacy::pattern("usc").expect("usc");
    assert!(usc.is_match("42 U.S.C. § 1983"));
    assert!(std::ptr::eq(
        usc,
        pats.iter().find(|(t, _)| *t == "usc").unwrap().1
    ));
    assert!(legal_citations::legacy::pattern("nope").is_none());
}

#[test]
fn every_match_span_and_group_matches() {
    let f = fixture();
    let mut total = 0;
    for case in f["cases"].as_array().expect("cases") {
        let text = case["text"].as_str().expect("text");
        let got = detect_legacy(text);
        for (cite_type, expected) in case["matches"].as_object().expect("matches") {
            let mine: Vec<_> = got.iter().filter(|m| &m.cite_type == cite_type).collect();
            let expected = expected.as_array().expect("array");
            assert_eq!(mine.len(), expected.len(), "{cite_type} in {text:?}");
            for (m, e) in mine.iter().zip(expected) {
                let span = [m.start as u64, m.end as u64];
                let want = [
                    e["span"][0].as_u64().unwrap_or(u64::MAX),
                    e["span"][1].as_u64().unwrap_or(u64::MAX),
                ];
                assert_eq!(span, want, "{cite_type} span in {text:?}");
                let want_groups: BTreeMap<String, String> = e["groups"]
                    .as_object()
                    .expect("groups")
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect();
                assert_eq!(m.groups, want_groups, "{cite_type} groups in {text:?}");
                total += 1;
            }
        }
    }
    assert!(total > 100, "only {total} matches compared");
}

#[test]
fn legacy_matches_carry_char_offsets_and_rule_ids() {
    let text = "§ 42 U.S.C. § 1983";
    let got = detect_legacy(text);
    let usc = got.iter().find(|m| m.cite_type == "usc").expect("usc");
    assert_eq!(usc.rule_id, "us.fed.statute.usc");
    assert_eq!(&text[usc.start..usc.end], "42 U.S.C. § 1983");
    assert_eq!(usc.char_start, 2);
    assert_eq!(usc.char_end, 18);
}
