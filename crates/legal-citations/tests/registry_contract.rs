//! The rule-registry contract: every embedded rule keeps every promise, and
//! the validator itself catches each kind of broken promise.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::registry::{registry, summaries, validate, Registry};

#[test]
fn embedded_registry_loads_without_errors() {
    let reg = registry();
    assert!(reg.load_errors.is_empty(), "{:#?}", reg.load_errors);
    assert!(reg.rules.len() >= 100, "only {} rules", reg.rules.len());
}

#[test]
fn embedded_registry_keeps_its_contract() {
    let violations = validate(registry());
    let report: Vec<String> = violations
        .iter()
        .map(|v| format!("{} [{}] {}", v.rule, v.check, v.detail))
        .collect();
    assert!(
        report.is_empty(),
        "{} violations:\n{}",
        report.len(),
        report.join("\n")
    );
}

#[test]
fn registry_hash_is_stable_and_hex() {
    let a = registry().hash();
    assert_eq!(a, registry().hash());
    assert_eq!(a.len(), 16);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn summaries_cover_every_rule_and_report_unverified() {
    let reg = registry();
    let rows = summaries(reg);
    assert_eq!(rows.len(), reg.rules.len());
    let usc = rows
        .iter()
        .find(|r| r.id == "us.fed.statute.usc")
        .expect("usc rule");
    assert!(!usc.verified);
    assert_eq!(
        usc.links_to.as_deref(),
        Some("https://www.law.cornell.edu/uscode/text/28/1331")
    );
    let legacy = rows
        .iter()
        .find(|r| r.id == "us.case.legacy_full")
        .expect("legacy case rule");
    assert!(legacy.retired.is_some());
    assert!(reg.get("us.fed.statute.usc").is_some());
    assert!(reg.get("no.such.rule").is_none());
}

#[test]
fn courtlistener_templates_have_fetch_notes() {
    for rule in &registry().rules {
        if rule
            .def
            .url_templates
            .iter()
            .any(|template| template.contains("courtlistener.com/c/"))
        {
            assert!(
                rule.def.note.contains("verified by HTTP fetch"),
                "{} has a CourtListener template without a fetch note",
                rule.def.id
            );
        }
    }
}

const GOOD: &str = r#"{
  "id": "test.statute.widget",
  "kind": "statute",
  "jurisdiction": "test",
  "pattern": "\\bWidget Code § (?P<section>[0-9]+)",
  "authority_key": "test/{section}",
  "url_templates": ["https://example.com/{section}"],
  "url_decoders": ["example.com/w/{section}"],
  "provenance": {"encodes": "Test rule", "opened": false, "how_to_verify": "n/a"},
  "examples": {
    "positive": [
      {"text": "See Widget Code § 1.", "spans": [[4, 20]], "groups": {"section": "1"}},
      {"text": "Widget Code § 22", "spans": [[0, 17]]},
      {"text": "x Widget Code § 3 y", "spans": [[2, 18]]}
    ],
    "negative": ["widget code 1", "Gadget Code § 1", "nothing"]
  }
}"#;

fn with(mutate: impl Fn(&mut serde_json::Value)) -> String {
    let mut v: serde_json::Value = serde_json::from_str(GOOD).expect("GOOD parses");
    mutate(&mut v);
    v.to_string()
}

fn checks(files: &[(&str, &str)]) -> Vec<&'static str> {
    let reg = Registry::from_sources(files);
    validate(&reg).into_iter().map(|v| v.check).collect()
}

#[test]
fn a_good_rule_validates_cleanly() {
    assert_eq!(
        checks(&[("rules/statute/good.json", GOOD)]),
        Vec::<&str>::new()
    );
}

#[test]
fn load_errors_are_reported_not_panicked() {
    let bad_regex = with(|v| v["pattern"] = "(unclosed".into());
    let bad_legacy =
        with(|v| v["legacy"] = serde_json::json!({"type": "t", "order": 0, "pattern": "(bad"}));
    let got = checks(&[
        ("rules/statute/a.json", "{not json"),
        ("rules/statute/b.json", &bad_regex),
        ("rules/statute/c.json", &bad_legacy),
    ]);
    assert_eq!(got, vec!["parse", "compile", "compile-legacy"]);
}

#[test]
fn each_broken_promise_has_its_own_check() {
    let cases: Vec<(String, &str, &str)> = vec![
        (
            with(|v| v["id"] = "Bad Id".into()),
            "rules/statute/x.json",
            "id-format",
        ),
        (
            with(|v| v["id"] = "widget".into()),
            "rules/statute/x.json",
            "id-format",
        ),
        (
            with(|v| v["id"] = "test..widget".into()),
            "rules/statute/x.json",
            "id-format",
        ),
        (GOOD.to_string(), "rules/case/x.json", "path"),
        (
            with(|v| v["authority_key"] = "test/{nope}".into()),
            "rules/statute/x.json",
            "template-group",
        ),
        (
            with(|v| v["url_decoders"] = serde_json::json!(["example.com/{other}"])),
            "rules/statute/x.json",
            "decoder-group",
        ),
        (
            with(|v| v["provenance"]["encodes"] = "".into()),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["provenance"]["opened"] = true.into()),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["provenance"]["how_to_verify"] = "".into()),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["measured"] = serde_json::json!({"precision": 0.9})),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["measured"] = serde_json::json!({"precision": 7})),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["measured"] = serde_json::json!({"precision": 7, "n": 10})),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["measured"] = serde_json::json!({"precision": "high"})),
            "rules/statute/x.json",
            "provenance",
        ),
        (
            with(|v| v["examples"]["negative"] = serde_json::json!(["a"])),
            "rules/statute/x.json",
            "examples-count",
        ),
        (
            with(|v| v["examples"]["positive"][2]["spans"] = serde_json::json!([[0, 1]])),
            "rules/statute/x.json",
            "positive-spans",
        ),
        (
            with(|v| v["examples"]["positive"][0]["groups"]["section"] = "9".into()),
            "rules/statute/x.json",
            "positive-groups",
        ),
        (
            with(|v| v["examples"]["negative"][0] = "Widget Code § 5".into()),
            "rules/statute/x.json",
            "negative",
        ),
        (
            with(|v| v["legacy"] = serde_json::json!({"type": "t", "order": 3, "pattern": "x"})),
            "rules/statute/x.json",
            "legacy-order",
        ),
    ];
    for (json, path, want) in cases {
        let got = checks(&[(path, &json)]);
        assert!(
            got.contains(&want),
            "expected {want} for {json}, got {got:?}"
        );
    }
}

#[test]
fn positive_count_is_enforced() {
    let json = with(|v| {
        let first = v["examples"]["positive"][0].clone();
        v["examples"]["positive"] = serde_json::json!([first]);
    });
    assert!(checks(&[("rules/statute/x.json", &json)]).contains(&"examples-count"));
}

#[test]
fn self_example_guard_catches_a_rival_and_duplicates() {
    // A second rule with higher priority steals the documentation example.
    let rival = with(|v| {
        v["id"] = "test.statute.rival".into();
        v["priority"] = 99.into();
        v["examples"]["negative"] = serde_json::json!(["x", "y", "z"]);
    });
    let got = checks(&[
        ("rules/statute/good.json", GOOD),
        ("rules/statute/rival.json", &rival),
    ]);
    assert!(got.contains(&"self-example"), "{got:?}");
    let got = checks(&[
        ("rules/statute/a.json", GOOD),
        ("rules/statute/b.json", GOOD),
    ]);
    assert!(got.contains(&"duplicate-id"), "{got:?}");
}

#[test]
fn retired_rules_skip_example_checks() {
    let json = with(|v| {
        v["retired"] = "superseded".into();
        v["examples"]["positive"] = serde_json::json!([]);
    });
    assert!(checks(&[("rules/statute/x.json", &json)]).is_empty());
}

#[test]
fn measured_precision_with_n_is_accepted() {
    let json = with(|v| {
        v["measured"] =
            serde_json::json!({"precision": 0.95, "n": 200, "corpus": "abc", "date": "2026-10-22"})
    });
    assert!(checks(&[("rules/statute/x.json", &json)]).is_empty());
}

#[test]
fn registry_hash_is_fnv1a_over_paths_and_patterns() {
    let reg = Registry::from_sources(&[("rules/statute/good.json", GOOD)]);
    let pattern = reg.rules[0].def.pattern.clone();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in "rules/statute/good.json".bytes().chain(pattern.bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    assert_eq!(reg.hash(), format!("{h:016x}"));
}

fn self_example_violations(files: &[(&str, &str)]) -> Vec<String> {
    let reg = Registry::from_sources(files);
    validate(&reg)
        .into_iter()
        .filter(|v| v.rule == "test.statute.widget" && v.check == "self-example")
        .map(|v| v.detail)
        .collect()
}

#[test]
fn self_example_guard_needs_the_exact_span() {
    for span in [[5, 20], [4, 19]] {
        let json = with(|v| v["examples"]["positive"][0]["spans"] = serde_json::json!([span]));
        let got = self_example_violations(&[("rules/statute/good.json", &json)]);
        assert_eq!(got.len(), 1, "{span:?}");
    }
    assert!(self_example_violations(&[("rules/statute/good.json", GOOD)]).is_empty());
}

#[test]
fn self_example_guard_ignores_other_rules_that_do_not_overlap() {
    let json = with(|v| {
        v["examples"]["positive"][0] = serde_json::json!({
            "text": "[ref] [ref]Widget Code § 1[ref] [ref]",
            "spans": [[11, 27]],
            "groups": {"section": "1"}
        });
    });
    let other = with(|v| {
        v["id"] = "test.record.ref".into();
        v["kind"] = "record".into();
        v["pattern"] = "\\[ref\\]".into();
        v["authority_key"] = "".into();
        v["url_templates"] = serde_json::json!([]);
        v["url_decoders"] = serde_json::json!([]);
    });
    let got = self_example_violations(&[
        ("rules/statute/good.json", &json),
        ("rules/record/ref.json", &other),
    ]);
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn rule_defaults_and_measured_value() {
    let json = with(|v| {
        v.as_object_mut().unwrap().remove("priority");
        v["measured"] = serde_json::json!({"precision": 0.75, "n": 40});
    });
    let reg = Registry::from_sources(&[("rules/statute/good.json", &json)]);
    let def = &reg.rules[0].def;
    assert_eq!(def.priority, 50);
    assert!((def.base_confidence - 0.9).abs() < f64::EPSILON);
    assert_eq!(def.measured.value(), Some(0.75));
}
