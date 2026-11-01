//! The generated case rules against the vendored reporters-db: every string
//! it lists is recognized as volume-reporter-page by exactly one case rule,
//! and the committed files are what the generator produces.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::recognize::recognize;
use legal_citations::{detect, registry, Kind};
use std::path::{Path, PathBuf};
use xtask_rules::{all_strings, hyphenated, parse_db, probe, probes, short_probe, REPORTERS_JSON};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn db() -> xtask_rules::Db {
    parse_db(&std::fs::read_to_string(root().join(REPORTERS_JSON)).unwrap()).unwrap()
}

/// `text` is detected as one citation covering all of it, and exactly one
/// case rule (or, when a hand-written rule of another kind owns the whole
/// cite, no generated rule) matches it whole. Returns the citation's kind.
fn recognized_once(text: &str) -> Result<Kind, String> {
    let cites = detect(text);
    let [c] = cites.as_slice() else {
        let got: Vec<String> = cites
            .iter()
            .map(|c| format!("{} {:?} {:?}", c.rule_id, c.kind, c.text))
            .collect();
        return Err(format!("{text:?}: {got:?}"));
    };
    if c.core_span.start != 0 || c.core_span.end != text.len() {
        return Err(format!("{text:?}: {} matched {:?}", c.rule_id, c.text));
    }
    let raw = whole_case_rules(text);
    let want = usize::from(c.kind == Kind::Case);
    // A hand-written rule (`2009 Ark. 190`, the public-domain form) may share
    // a string with a generated one (`12 Ark. 345`) and win by priority.
    let priority = |id: &str| {
        registry()
            .rules
            .iter()
            .find(|r| r.def.id == id)
            .map_or(i32::MIN, |r| r.def.priority)
    };
    let won_by_priority = !c.rule_id.contains(".rdb.")
        && raw
            .iter()
            .filter(|(id, _)| *id != c.rule_id)
            .all(|(id, _)| id.contains(".rdb.") && priority(id) < priority(&c.rule_id));
    if raw.len() != want && !won_by_priority {
        return Err(format!(
            "{text:?}: detected as {}, case rules {raw:?}",
            c.rule_id
        ));
    }
    Ok(c.kind)
}

#[test]
fn every_reporters_db_string_is_recognized_once() {
    let db = db();
    let strings = all_strings(&db);
    assert!(strings.len() > 3500, "only {} strings", strings.len());
    let mut failures = Vec::new();
    let mut other_kinds = Vec::new();
    for (key, s) in &strings {
        // `12 <s> 345`, or for Westlaw/Lexis-style reporters `2019 <s> 1234`.
        let texts = probes(s);
        for t in &texts {
            let dup = whole_case_rules(t);
            if dup.len() > 1 {
                failures.push(format!("{key}: {t:?} matched by {dup:?}"));
            }
        }
        let first = texts.iter().map(|t| recognized_once(t)).find(Result::is_ok);
        match first.unwrap_or_else(|| recognized_once(&texts[0])) {
            Ok(Kind::Case) => {}
            Ok(kind) => other_kinds.push(format!("{s} ({kind:?})")),
            Err(e) => failures.push(format!("{key}: {e}")),
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} strings:\n{}",
        failures.len(),
        strings.len(),
        failures.join("\n")
    );
    // Journal-shaped reporters (`12 Pitts. L.J. 345`) stay with the
    // hand-written law-review rule, agency reporters (`12 F.C.C. 345`,
    // `370 NLRB No. 128`) with the hand-written agency-decision rules, and
    // `T.C. at` (reporters-db lists the short-form `at` as a variation)
    // reads as a short form; nothing else may leave the case kind.
    assert!(
        other_kinds.iter().all(|k| k.ends_with("(Secondary)")
            || k.ends_with("(AgencyDecision)")
            || k == "T.C. at (CaseShort)"),
        "{other_kinds:?}"
    );
    assert!(other_kinds.len() < 40, "{other_kinds:?}");
}

#[test]
fn hyphenated_families_are_recognized_with_hyphens() {
    let db = db();
    let mut n = 0;
    for (key, reps) in &db {
        if !hyphenated(reps) {
            continue;
        }
        for s in xtask_rules::family_strings(reps) {
            n += 1;
            let text = format!("2009-{s}-345");
            let cites = detect(&text);
            assert!(
                cites.iter().any(|c| c.kind == Kind::Case && c.text == text),
                "{key}: {text:?} -> {cites:?}"
            );
        }
    }
    assert!(n >= 4, "only {n} hyphenated strings");
}

#[test]
fn committed_generated_rules_are_fresh_and_valid() {
    let (out, diff) = xtask_rules::run(&root(), true).unwrap();
    assert!(
        diff.is_empty(),
        "stale (run `cargo run -p xtask-rules`): {diff:?}"
    );
    assert!(out.violations.is_empty(), "{:#?}", out.violations);
    assert!(out.stats.conflicts.is_empty(), "{:?}", out.stats.conflicts);
    let embedded = registry()
        .rules
        .iter()
        .filter(|r| r.path.contains("/generated/"))
        .count();
    assert_eq!(embedded, out.files.len());
}

/// reporters-db's own `examples` that have the plain volume-reporter-page
/// shape (`1 Abb. Adm. 317`, `2014 IL 115811`) are each detected whole.
/// Other shapes it lists (no volume, `¶ 25,508`, `154b`, a year before the
/// page) are outside what the generated rules recognize.
#[test]
fn reporters_db_examples_of_the_plain_shape_are_detected() {
    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root().join(REPORTERS_JSON)).unwrap())
            .unwrap();
    let db = db();
    let mut checked = 0;
    let mut failures = Vec::new();
    for (key, reps) in &db {
        let strings = xtask_rules::family_strings(reps);
        let examples = raw[key]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|r| r["examples"].as_array().into_iter().flatten())
            .filter_map(|e| e.as_str());
        for ex in examples {
            let plain = strings.iter().any(|s| {
                ex.split_once(' ').is_some_and(|(v, rest)| {
                    rest.strip_prefix(s.as_str())
                        .and_then(|p| p.strip_prefix(' '))
                        .is_some_and(|p| {
                            !p.is_empty()
                                && p.len() <= 7
                                && p.bytes().all(|b| b.is_ascii_digit())
                                && !v.is_empty()
                                && v.len() <= 4
                                && v.bytes().all(|b| b.is_ascii_digit())
                        })
                })
            });
            if !plain {
                continue;
            }
            checked += 1;
            if let Err(e) = recognized_once(ex) {
                failures.push(format!("{key}: {e}"));
            }
        }
    }
    assert!(checked >= 150, "only {checked} plain examples");
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Case rules (and generated rules, whatever their kind) whose pattern
/// matches all of `text`.
fn whole_case_rules(text: &str) -> Vec<(String, Kind)> {
    recognize(registry(), text)
        .into_iter()
        .filter(|m| m.start == 0 && m.end == text.len())
        .map(|m| (m.rule_id, registry().rules[m.rule_index].def.kind))
        .filter(|(id, kind)| *kind == Kind::Case || id.contains(".case.rdb."))
        .collect()
}

/// Every string a generated case rule owns also has its short form
/// (`12 Ill. App. at 350`), detected once and resolved to the full cite.
#[test]
fn generated_reporters_have_short_forms_that_resolve() {
    let db = db();
    let mut failures = Vec::new();
    let mut checked = 0;
    for (key, s) in all_strings(&db) {
        let full = probe(&s);
        let owned_by_generated = detect(&full)
            .first()
            .is_some_and(|c| c.rule_id.contains(".case.rdb."));
        if !owned_by_generated {
            continue;
        }
        checked += 1;
        let short = short_probe(&s);
        let cites = detect(&short);
        let whole = matches!(cites.as_slice(), [c] if c.kind == Kind::CaseShort && c.text == short);
        if !whole {
            failures.push(format!(
                "{key}: {short:?} -> {:?}",
                cites
                    .iter()
                    .map(|c| (&c.rule_id, &c.text))
                    .collect::<Vec<_>>()
            ));
            continue;
        }
        let text = format!("See Smith v. Jones, {full} (1900). Smith, {short}.");
        let cites = detect(&text);
        let resolved = matches!(cites.as_slice(), [a, b]
            if a.kind == Kind::Case && b.antecedent == Some(0) && b.authority == a.authority);
        if !resolved {
            failures.push(format!("{key}: {text:?} did not resolve"));
        }
    }
    assert!(checked > 3000, "only {checked}");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// The committed court table is what `xtask-rules` generates from the
/// Indigo inventory, and the generator keeps its federal/state split.
#[test]
fn committed_court_table_is_fresh() {
    let (n, stale) = xtask_rules::courts::run(&root(), true).unwrap();
    assert!(!stale, "stale (run `cargo run -p xtask-rules`)");
    assert_eq!(n, legal_citations::court::courts().courts().len());
}
