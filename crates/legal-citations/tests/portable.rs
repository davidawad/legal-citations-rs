//! Regex portability and the release bundle: which patterns other engines
//! can run, the ECMAScript form, and the bundle and test vectors built from
//! the registry.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::export::{self, Polarity};
use legal_citations::portable::{issues, to_js, unsupported_hint, JS_FLAGS};
use legal_citations::registry::{portability_warnings, registry, validate, Registry};

const W: &str = r"\p{L}\p{Nl}\p{M}\p{Nd}\p{Pc}";

fn fatal(pattern: &str) -> Vec<String> {
    issues(pattern)
        .into_iter()
        .filter(|i| i.breaks_export)
        .map(|i| i.what)
        .collect()
}

#[test]
fn plain_patterns_are_portable_and_get_a_js_form() {
    let p = r"(?P<title>[0-9]{1,2})\s+U\.S\.C\.?\s*§+\s*(?P<section>[0-9a-z\-]+)";
    assert!(issues(p).is_empty(), "{:?}", issues(p));
    assert_eq!(
        to_js(p).unwrap(),
        r"(?<title>[0-9]{1,2})\s+U\.S\.C\.?\s*§+\s*(?<section>[0-9a-z\-]+)"
    );
    assert_eq!(JS_FLAGS, "u");
}

#[test]
fn js_form_drops_escapes_u_mode_rejects_and_escapes_lone_brackets() {
    assert_eq!(to_js(r"Stew\.\s+\&\s+P\.").unwrap(), r"Stew\.\s+&\s+P\.");
    assert_eq!(to_js(r"a\-b\#c\~d").unwrap(), "a-b#c~d");
    assert_eq!(to_js(r"[\-\&a]").unwrap(), r"[\-&a]");
    assert_eq!(to_js(r"\%\/").unwrap(), r"%\/");
    assert_eq!(to_js("a]").unwrap(), r"a\]");
    assert_eq!(to_js("[]a]").unwrap(), r"[\]a]");
    assert_eq!(to_js(r"[\x41-\x5A]\t").unwrap(), r"[\x41-\x5A]\t");
}

#[test]
fn js_form_keeps_unicode_meaning_of_perl_classes_word_boundaries_and_dot() {
    assert_eq!(to_js(r"\d\D").unwrap(), r"\p{Nd}\P{Nd}");
    assert_eq!(to_js(r"\w\W").unwrap(), format!("[{W}][^{W}]"));
    assert_eq!(to_js(r"[\w.\d]").unwrap(), format!(r"[{W}.\p{{Nd}}]"));
    assert_eq!(to_js(r"\s[\s]\S").unwrap(), r"\s[\s]\S");
    assert_eq!(
        to_js(r"\bx\B").unwrap(),
        format!(
            "(?:(?<=[{W}])(?![{W}])|(?<![{W}])(?=[{W}]))x(?:(?<=[{W}])(?=[{W}])|(?<![{W}])(?![{W}]))"
        )
    );
    assert_eq!(to_js("a.b").unwrap(), r"a[^\n]b");
    assert_eq!(to_js("^a$").unwrap(), "^a$");
}

#[test]
fn constructs_without_a_form_in_both_dialects_break_the_export() {
    for (pattern, says) in [
        ("(?i)x", "inline flags (?i)"),
        ("(?-x)x", "inline flags (?-x)"),
        ("(?imsUuRx:x)", "inline flags (?imsUuRx:..)"),
        ("(?<n>x)", "write (?P<n>"),
        (r"\Ax\z", r"\A or \z"),
        (r"\b{start}x", "word boundary"),
        (r"\<x\>", "word boundary"),
        (r"\pL", "Unicode class"),
        (r"[\p{Greek}a]", "Unicode class"),
        ("[[:alpha:]]", "ASCII class"),
        ("[a[bc]]", "nested character class"),
        ("[a-z&&[^aeiou]]", "set operation"),
        (r"\x{41}", "HexBrace"),
        ("\\u0041", "HexFixed(UnicodeShort)"),
        (r"\U00000041", "HexFixed(UnicodeLong)"),
        (r"\a", r"\a"),
        (r"[\W]", r"\W inside a class"),
        ("(?P<a.b>x)", "group name"),
        ("(unclosed", "does not parse"),
        ("(?<=a)b", "lookbehind"),
    ] {
        let got = fatal(pattern);
        assert!(got.iter().any(|w| w.contains(says)), "{pattern}: {got:?}");
        assert!(to_js(pattern).is_none(), "{pattern}");
    }
}

#[test]
fn constructs_only_re2_rejects_are_reported_not_fatal() {
    for (pattern, says) in [
        ("a{1001}", "1001 > 1000"),
        ("(?:ab){2,1001}", "1001 > 1000"),
    ] {
        let got = issues(pattern);
        assert!(
            got.iter()
                .any(|i| !i.breaks_export && i.what.contains(says)),
            "{pattern}: {got:?}"
        );
        assert!(to_js(pattern).is_some(), "{pattern}");
    }
    assert!(issues("a{2,1000}").is_empty());
    assert!(issues("a{1001,}").iter().all(|i| !i.breaks_export));
}

#[test]
fn constructs_rust_regex_refuses_are_named() {
    for (pattern, says) in [
        ("(?<=a)b", "lookbehind"),
        ("(?<!a)b", "lookbehind"),
        ("a(?=b)", "lookahead"),
        ("a(?!b)", "lookahead"),
        (r"(a)\1", "backreference"),
        (r"(?P<n>a)\k<n>", "backreference"),
        ("(?P<n>a)(?P=n)", "backreference"),
        ("(?>ab)", "atomic group"),
        ("a++", "possessive quantifier"),
        ("a*+", "possessive quantifier"),
        ("a?+", "possessive quantifier"),
        ("a{2}+", "possessive quantifier"),
    ] {
        let hint = unsupported_hint(pattern).unwrap_or_default();
        assert!(hint.contains(says), "{pattern}: {hint}");
        assert!(hint.contains("not portable"), "{hint}");
    }
    for pattern in [r"[(?=]", r"\(?=", r"[*+]", r"\++", r"[\1]", "plain", r"\kx"] {
        assert_eq!(unsupported_hint(pattern), None, "{pattern}");
    }
    assert_eq!(
        unsupported_hint("(?<=a)(?<=b)").unwrap(),
        "lookbehind (not portable: Go RE2 and Rust regex have none)"
    );
}

const RULE: &str = r#"{
  "id": "test.statute.widget",
  "kind": "statute",
  "jurisdiction": "test",
  "pattern": "\\bWidget Code § (?P<section>[0-9]+)",
  "provenance": {"encodes": "Test rule", "opened": false, "how_to_verify": "n/a"},
  "examples": {
    "positive": [
      {"text": "See Widget Code § 1.", "spans": [[4, 20]], "groups": {"section": "1"}},
      {"text": "Widget Code § 22 and Widget Code § 3", "spans": [[0, 17], [22, 38]]},
      {"text": "x Widget Code § 3 y", "spans": [[2, 18]]}
    ],
    "negative": ["widget code 1", "Gadget Code § 1", "nothing"]
  }
}"#;

fn rule(mutate: impl Fn(&mut serde_json::Value)) -> String {
    let mut v: serde_json::Value = serde_json::from_str(RULE).unwrap();
    mutate(&mut v);
    v.to_string()
}

fn checks(reg: &Registry) -> Vec<&'static str> {
    validate(reg).into_iter().map(|v| v.check).collect()
}

#[test]
fn validate_fails_on_export_breakers_and_warns_on_re2_only() {
    let good = Registry::from_sources(&[("rules/statute/x.json", RULE)]);
    assert!(checks(&good).is_empty(), "{:?}", validate(&good));
    assert!(portability_warnings(&good).is_empty());

    let flagged = rule(|v| v["reject"] = serde_json::json!(["(?i)gadget"]));
    let reg = Registry::from_sources(&[("rules/statute/x.json", &flagged)]);
    let v = validate(&reg);
    let portable: Vec<_> = v.iter().filter(|x| x.check == "portable").collect();
    assert_eq!(portable.len(), 1, "{v:?}");
    assert!(portable[0].detail.contains("(?i)gadget"));

    let re2 = rule(|v| v["reject_before"] = serde_json::json!(["x{1001}$"]));
    let reg = Registry::from_sources(&[("rules/statute/x.json", &re2)]);
    assert!(checks(&reg).is_empty(), "{:?}", validate(&reg));
    let w = portability_warnings(&reg);
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].check, "portable-re2");

    let retired_legacy = rule(|v| {
        v["retired"] = "old".into();
        v["legacy"] = serde_json::json!({"type": "t", "order": 0, "pattern": "(?<n>x)"});
    });
    let reg = Registry::from_sources(&[("rules/statute/x.json", &retired_legacy)]);
    assert_eq!(checks(&reg), vec!["portable"]);
}

#[test]
fn compile_errors_name_the_unportable_construct() {
    let lookbehind = rule(|v| v["pattern"] = "(?<!x)Widget".into());
    let reg = Registry::from_sources(&[("rules/statute/x.json", &lookbehind)]);
    assert_eq!(reg.load_errors.len(), 1);
    assert_eq!(reg.load_errors[0].check, "compile");
    assert!(reg.load_errors[0].detail.starts_with("lookbehind"));
}

#[test]
fn embedded_rules_bundle_with_a_js_form_for_every_pattern() {
    let reg = registry();
    assert!(portability_warnings(reg).is_empty());
    let b = export::bundle(reg).unwrap();
    assert_eq!(b.format, "legal-citation-rules");
    assert_eq!(b.version, 1);
    assert_eq!(b.license, "MIT");
    assert!(b.notice.contains("BSD-2-Clause") && b.notice.contains("CC0"));
    assert_eq!(b.rules_hash, reg.hash());
    assert_eq!(b.before_window, legal_citations::rule::BEFORE_WINDOW);
    assert_eq!(b.rules.len(), reg.rules.len());
    assert!(!b.matching.is_empty());
    for (bundled, rule) in b.rules.iter().zip(&reg.rules) {
        assert_eq!(bundled.path, rule.path);
        assert!(bundled.path.starts_with("rules/"));
        assert_eq!(bundled.rule.id, rule.def.id);
        assert_eq!(bundled.js.flags, "u");
        assert!(!bundled.js.pattern.contains("(?P<"), "{}", bundled.rule.id);
        assert_eq!(bundled.js.reject.len(), rule.def.reject.len());
        assert_eq!(bundled.js.reject_before.len(), rule.def.reject_before.len());
        assert_eq!(bundled.js.legacy.is_some(), rule.def.legacy.is_some());
    }
}

#[test]
fn bundle_refuses_a_pattern_without_a_js_form() {
    let bad = rule(|v| v["reject"] = serde_json::json!(["\\pL"]));
    let reg = Registry::from_sources(&[
        ("rules/statute/x.json", RULE),
        ("rules/statute/y.json", &bad),
    ]);
    let err = export::bundle(&reg).unwrap_err();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].check, "portable");
    assert!(err[0].detail.contains("\\\\pL"), "{}", err[0].detail);
}

#[test]
fn vectors_are_rule_level_spans_of_the_examples() {
    let matched_negative = rule(|v| {
        v["examples"]["negative"] = serde_json::json!(["Widget Code § 9", "none", "zero"]);
    });
    let retired = rule(|v| {
        v["id"] = "test.statute.old".into();
        v["retired"] = "old".into();
    });
    let reg = Registry::from_sources(&[
        ("rules/statute/x.json", &matched_negative),
        ("rules/statute/y.json", &retired),
    ]);
    let v = export::vectors(&reg);
    assert_eq!(v.format, "legal-citation-rules/test-vectors");
    assert_eq!(v.license, "MIT");
    assert_eq!(v.rules_hash, reg.hash());
    assert!(v.vectors.iter().all(|x| x.rule == "test.statute.widget"));
    let pos: Vec<_> = v
        .vectors
        .iter()
        .filter(|x| x.polarity == Polarity::Positive)
        .collect();
    assert_eq!(pos.len(), 3);
    assert_eq!(pos[0].spans, vec![[4, 20]]);
    assert_eq!(pos[0].groups["section"], "1");
    assert_eq!(pos[1].spans, vec![[0, 17], [22, 38]]);
    assert_eq!(pos[1].groups["section"], "22");
    let neg: Vec<_> = v
        .vectors
        .iter()
        .filter(|x| x.polarity == Polarity::Negative)
        .collect();
    assert_eq!(
        neg.iter().map(|x| x.text.as_str()).collect::<Vec<_>>(),
        vec!["none", "zero"]
    );
    assert!(neg
        .iter()
        .all(|x| x.spans.is_empty() && x.groups.is_empty()));
}

#[test]
fn embedded_vectors_match_every_positive_example() {
    let reg = registry();
    let v = export::vectors(reg);
    let positives: usize = reg
        .active()
        .map(|(_, r)| r.def.examples.positive.len())
        .sum();
    assert_eq!(
        v.vectors
            .iter()
            .filter(|x| x.polarity == Polarity::Positive)
            .count(),
        positives
    );
    let mut want = reg
        .active()
        .flat_map(|(_, r)| r.def.examples.positive.iter().map(|p| p.spans.clone()));
    for x in v
        .vectors
        .iter()
        .filter(|x| x.polarity == Polarity::Positive)
    {
        assert_eq!(
            Some(x.spans.clone()),
            want.next(),
            "{} {:?}",
            x.rule,
            x.text
        );
    }
}
