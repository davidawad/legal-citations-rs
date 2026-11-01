//! Small units: offsets, templates, kinds, URL decoding.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::link_target::{decode, decode_with, host_allowed, host_of, suggest};
use legal_citations::recognize::{recognize, resolve_overlaps, RawMatch};
use legal_citations::registry;
use legal_citations::rule::{fill_template, template_names, Kind, Measured};
use legal_citations::span::{OffsetMap, Span};
use std::collections::BTreeMap;

#[test]
fn offset_map_handles_multibyte_lines_and_ends() {
    let text = "a§\nb😀c";
    let map = OffsetMap::new(text);
    let s = map.span(5, 10); // "😀c"
    assert_eq!((s.char_start, s.char_end), (4, 6));
    assert_eq!((s.utf16_start, s.utf16_end), (4, 7));
    assert_eq!((s.line, s.col, s.end_line, s.end_col), (2, 2, 2, 4));
    let mid = map.span(2, 2); // inside '§' maps to that char
    assert_eq!(mid.char_start, 1);
    assert!(mid.is_empty());
    assert_eq!(s.len(), 5);
    let empty = OffsetMap::new("").span(0, 0);
    assert_eq!((empty.line, empty.col), (1, 1));
}

#[test]
fn span_relations() {
    let a = Span {
        start: 0,
        end: 10,
        ..Span::default()
    };
    let b = Span {
        start: 2,
        end: 4,
        ..Span::default()
    };
    let c = Span {
        start: 10,
        end: 12,
        ..Span::default()
    };
    assert!(a.contains(&b) && !b.contains(&a));
    assert!(a.overlaps(&b) && !a.overlaps(&c));
    // Adjacent spans touch but neither overlaps nor contains the other.
    assert!(!c.overlaps(&a) && !a.contains(&c) && !c.contains(&a));
}

#[test]
fn templates_fill_and_name() {
    let mut g = BTreeMap::new();
    g.insert("a".to_string(), "1".to_string());
    g.insert("b".to_string(), "x y".to_string());
    assert_eq!(fill_template("p/{a}/{b}", &g).as_deref(), Some("p/1/x y"));
    assert_eq!(fill_template("p/{missing}", &g), None);
    assert_eq!(fill_template("p/{a", &g), None);
    assert_eq!(fill_template("plain", &g).as_deref(), Some("plain"));
    assert_eq!(template_names("{a}/x/{b}/{"), vec!["a", "b"]);
    assert_eq!(template_names("{a"), Vec::<String>::new());
}

#[test]
fn kinds_round_trip() {
    for k in Kind::ALL {
        assert_eq!(Kind::parse(k.as_str()), Some(k));
        assert_eq!(k.to_string(), k.as_str());
        assert_eq!(
            serde_json::to_string(&k).unwrap(),
            format!("\"{}\"", k.as_str())
        );
    }
    assert_eq!(Kind::parse("nope"), None);
    assert!(
        Kind::Id.is_short_form() && Kind::Supra.is_short_form() && Kind::CaseShort.is_short_form()
    );
    assert!(!Kind::Case.is_short_form());
}

#[test]
fn measured_defaults_to_unmeasured() {
    let m = Measured::default();
    assert_eq!(m.precision, serde_json::json!("unmeasured"));
    assert_eq!(m.value(), None);
}

#[test]
fn url_decoding_and_hosts() {
    let u = url::Url::parse("https://www.law.cornell.edu/uscode/text/28/1331#a").unwrap();
    assert_eq!(host_of(&u).as_deref(), Some("law.cornell.edu"));
    let g = decode_with("law.cornell.edu/uscode/text/{title}/{section}", &u).unwrap();
    assert_eq!(g["title"], "28");
    assert_eq!(g["section"], "1331");
    assert!(decode_with("ecfr.gov/current/title-{title}", &u).is_none());
    // A segment needs both the literal prefix and the literal suffix.
    let mpep = "uspto.gov/web/offices/pac/mpep/s{section}.html";
    let ok = url::Url::parse("https://www.uspto.gov/web/offices/pac/mpep/s2106.html").unwrap();
    assert_eq!(decode_with(mpep, &ok).unwrap()["section"], "2106");
    for bad in ["s2106.htm", "x2106.html"] {
        let u =
            url::Url::parse(&format!("https://www.uspto.gov/web/offices/pac/mpep/{bad}")).unwrap();
        assert!(decode_with(mpep, &u).is_none(), "{bad}");
    }
    assert!(decode_with("law.cornell.edu/cfr/text/{title}", &u).is_none());
    let short = url::Url::parse("https://www.law.cornell.edu/uscode").unwrap();
    assert!(decode_with("law.cornell.edu/uscode/text/{title}", &short).is_none());
    let e = url::Url::parse("https://www.ecfr.gov/current/title-37/section-1.56").unwrap();
    let d = decode(registry(), &e).unwrap();
    assert_eq!(d.authority, "us.cfr/37/1.56");
    assert_eq!(d.rule_id, "us.fed.regulation.cfr");
    let bad = url::Url::parse("https://www.ecfr.gov/current/title-/section-1.56").unwrap();
    assert!(decode(registry(), &bad).is_none());
    let cl = url::Url::parse("https://www.courtlistener.com/c/F.%20Supp.%202d/100/200/").unwrap();
    assert_eq!(
        decode(registry(), &cl).unwrap().authority,
        "case/100/f.supp.2d/200"
    );
    let plus = url::Url::parse("https://www.courtlistener.com/c/F.+Supp.+2d/100/200/").unwrap();
    assert_eq!(
        decode(registry(), &plus).unwrap().authority,
        "case/100/f.supp.2d/200"
    );
    let pct = url::Url::parse("https://www.courtlistener.com/c/U.S.%ZZ/1/2/").unwrap();
    assert_eq!(
        decode(registry(), &pct).unwrap().authority,
        "case/1/u.s.%zz/2"
    );
    let allowed = vec!["www.Example.com".to_string()];
    assert!(host_allowed("example.com", &allowed));
    assert!(host_allowed("sub.example.com", &allowed));
    assert!(!host_allowed("badexample.com", &allowed));
    let mpep = url::Url::parse("https://www.uspto.gov/web/offices/pac/mpep/s2143.html").unwrap();
    assert_eq!(
        decode(registry(), &mpep).unwrap().authority,
        "us.uspto.mpep/2143"
    );
}

#[test]
fn suggestions_encode_values() {
    let reg = registry();
    let idx = reg
        .rules
        .iter()
        .position(|r| r.def.id == "us.case.federal_appendix")
        .unwrap();
    let mut f = BTreeMap::new();
    f.insert("volume".to_string(), "700".to_string());
    f.insert("reporter".to_string(), "F. App’x".to_string());
    f.insert("page".to_string(), "1".to_string());
    assert_eq!(
        suggest(reg, idx, &f).as_deref(),
        Some("https://www.courtlistener.com/c/F.%20App%E2%80%99x/700/1/")
    );
    assert!(suggest(reg, 99_999, &f).is_none());
}

#[test]
fn overlap_resolution_prefers_priority_then_length() {
    let m = |id: &str, s, e, p| RawMatch {
        rule_index: 0,
        rule_id: id.into(),
        start: s,
        end: e,
        priority: p,
        groups: BTreeMap::new(),
    };
    let kept = resolve_overlaps(vec![
        m("short.hi", 2, 6, 60),
        m("long.lo", 0, 10, 10),
        m("other", 12, 14, 10),
    ]);
    let ids: Vec<&str> = kept.iter().map(|k| k.rule_id.as_str()).collect();
    assert_eq!(ids, vec!["short.hi", "other"]);
    let kept = resolve_overlaps(vec![m("a", 0, 4, 50), m("b", 0, 8, 50)]);
    assert_eq!(kept[0].rule_id, "b");
    // Length, not position, decides between equal priorities, in either input order.
    for input in [
        vec![
            m("later.short", 105, 112, 50),
            m("early.long", 100, 110, 50),
        ],
        vec![
            m("early.long", 100, 110, 50),
            m("later.short", 105, 112, 50),
        ],
    ] {
        let kept = resolve_overlaps(input);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].rule_id, "early.long");
    }
    assert!(recognize(registry(), "").is_empty());
}
