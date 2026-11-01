//! Rule collections, selectors, precedence and profiles (lcr-z18).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::registry::{registry, Registry};
use legal_citations::select::{glob_match, profiles, validate_profiles, Pattern, Profiles};
use legal_citations::{
    detect_markdown_with, detect_with, lint_markdown, Family, Kind, Policy, Selection, Selector,
};

fn sel(include: &[&str], exclude: &[&str], profiles: &[&str]) -> Selector {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect();
    Selector {
        include: v(include),
        exclude: v(exclude),
        profiles: v(profiles),
    }
}

fn ids(s: &Selection<'_>) -> Vec<String> {
    s.rules().map(|(_, r)| r.def.id.clone()).collect()
}

fn texts(cites: &[legal_citations::Citation]) -> Vec<&str> {
    cites.iter().map(|c| c.text.as_str()).collect()
}

const MIXED: &str = "See Ohio Rev. Code § 2907.02 and 28 U.S.C. § 1331. Id. at 5. \
See State v. Smith, 100 Ohio St.3d 1 (2003). Id. at 3. Fed. R. Civ. P. 12(b)(6).";

#[test]
fn collections_are_kind_family_jurisdiction_prefixes_tags_and_id() {
    let rule = registry().get("us.fed.uspto.mpep").unwrap();
    assert_eq!(
        rule.def.collections(),
        vec![
            "kind:agency_manual",
            "family:agency",
            "jurisdiction:us",
            "jurisdiction:us.fed",
            "jurisdiction:us.fed.uspto",
            "rule:us.fed.uspto.mpep",
        ]
    );
    for k in Kind::ALL {
        assert!(Family::ALL.contains(&k.family()));
        assert_eq!(Kind::parse(k.as_str()), Some(k));
    }
    for f in Family::ALL {
        assert_eq!(Family::parse(f.as_str()), Some(f));
        assert_eq!(f.to_string(), f.as_str());
    }
    assert_eq!(Family::parse("nope"), None);
}

#[test]
fn family_and_tags_come_from_the_rule_file_when_given() {
    let file = r#"{"id": "t.x", "kind": "secondary", "jurisdiction": "us.fed.house",
        "family": "legislative", "tags": ["congress", "hearing"],
        "pattern": "Hrg\\. (?P<n>[0-9]+)", "provenance": {"encodes": "t", "opened": false, "how_to_verify": "t"},
        "examples": {"positive": [], "negative": []}}"#;
    let reg = Registry::from_sources(&[("rules/secondary/t.x.json", file)]);
    let def = &reg.rules[0].def;
    assert_eq!(def.family(), Family::Legislative);
    let c = def.collections();
    assert!(c.contains(&"family:legislative".to_string()));
    assert!(c.contains(&"tag:congress".to_string()));
    assert!(c.contains(&"jurisdiction:us.fed.house".to_string()));
    let s = Selection::new(&reg, &sel(&["tag:hearing"], &[], &[]), &Profiles::default()).unwrap();
    assert_eq!(ids(&s), vec!["t.x"]);
}

#[test]
fn malformed_and_repeated_tags_break_the_contract() {
    let file = |tags: &str| {
        format!(
            r#"{{"id": "t.x", "kind": "secondary", "jurisdiction": "us", "tags": {tags},
            "pattern": "Q", "provenance": {{"encodes": "t", "opened": false, "how_to_verify": "t"}},
            "examples": {{"positive": [], "negative": []}}}}"#
        )
    };
    for bad in [r#"["Bad Tag"]"#, r#"["a", "a"]"#, r#"[""]"#] {
        let text = file(bad);
        let reg = Registry::from_sources(&[("rules/secondary/t.x.json", &text)]);
        let v = legal_citations::registry::validate(&reg);
        assert!(v.iter().any(|v| v.check == "tags"), "{bad}: {v:?}");
    }
    let text = file(r#"["ok-tag_1"]"#);
    let reg = Registry::from_sources(&[("rules/secondary/t.x.json", &text)]);
    let v = legal_citations::registry::validate(&reg);
    assert!(!v.iter().any(|v| v.check == "tags"));
}

#[test]
fn globs() {
    assert!(glob_match("us.oh*", "us.oh"));
    assert!(glob_match("us.oh*", "us.oh.x"));
    assert!(!glob_match("us.oh*", "us.o"));
    assert!(glob_match("*", ""));
    assert!(glob_match("us.?h", "us.oh"));
    assert!(!glob_match("us.?h", "us.ohh"));
    assert!(glob_match("*.ptab_*", "us.fed.uspto.ptab_proceeding"));
    assert!(glob_match("a*b*c", "aXbYbZc"));
    assert!(!glob_match("a*b*c", "aXbYbZ"));
    assert!(!glob_match("abc", "ab"));
    assert!(!glob_match("", "a"));
}

#[test]
fn each_selector_form_selects_the_right_rules() {
    let reg = registry();
    let jur = reg.select(&sel(&["jurisdiction:us.oh"], &[], &[])).unwrap();
    assert!(!jur.is_empty());
    assert!(jur
        .rules()
        .all(|(_, r)| r.def.jurisdiction.starts_with("us.oh")));
    assert!(ids(&jur).contains(&"us.oh.statute.rc".to_string()));

    let glob = reg
        .select(&sel(&["jurisdiction:us.fed.usp*"], &[], &[]))
        .unwrap();
    assert!(glob
        .rules()
        .all(|(_, r)| r.def.jurisdiction == "us.fed.uspto"));

    let kind = reg.select(&sel(&["kind:case"], &[], &[])).unwrap();
    assert!(kind.rules().all(|(_, r)| r.def.kind == Kind::Case));
    assert!(
        !ids(&kind).contains(&"us.case.legacy_full".to_string()),
        "retired rules never run"
    );

    let fam = reg.select(&sel(&["family:agency"], &[], &[])).unwrap();
    assert!(fam.rules().all(|(_, r)| r.def.family() == Family::Agency));
    assert!(ids(&fam).contains(&"us.fed.federal_register".to_string()));

    let rule = reg
        .select(&sel(&["rule:us.fed.statute.usc"], &[], &[]))
        .unwrap();
    assert_eq!(ids(&rule), vec!["us.fed.statute.usc"]);

    let bare = reg.select(&sel(&["us.fed.uspto.*"], &[], &[])).unwrap();
    let uspto_ids = reg
        .active()
        .filter(|(_, r)| r.def.id.starts_with("us.fed.uspto."))
        .count();
    assert!(uspto_ids >= 6);
    assert_eq!(bare.len(), uspto_ids);

    let all = reg.select(&Selector::default()).unwrap();
    assert_eq!(all.len(), reg.active().count());
    assert_eq!(Selection::all(reg).len(), all.len());
    assert!(!all.contains(usize::MAX));
}

#[test]
fn exclude_wins_over_include_and_profiles() {
    let reg = registry();
    let s = reg
        .select(&sel(&["jurisdiction:us.fed"], &["kind:statute"], &["ptab"]))
        .unwrap();
    assert!(!ids(&s).contains(&"us.fed.statute.usc".to_string()));
    assert!(ids(&s).contains(&"us.fed.uspto.mpep".to_string()));
    let only_exclude = reg.select(&sel(&[], &["family:case"], &[])).unwrap();
    assert!(only_exclude
        .rules()
        .all(|(_, r)| r.def.family() != Family::Case));
    assert!(only_exclude.len() > 50);
}

#[test]
fn unknown_or_empty_selectors_are_errors_with_a_clear_message() {
    let reg = registry();
    for (bad, needle) in [
        ("kind:cases", "no kind matches"),
        ("family:agencies", "no family matches"),
        ("courts:ohio", "unknown namespace"),
        ("court:ohio", "matches no rule or court"),
        ("court:Nowhere Ct.", "unknown court"),
        ("jurisdiction:us.zz", "matches no rule"),
        ("tag:no-such-tag", "matches no rule"),
        ("rule:", "empty pattern"),
    ] {
        let e = reg.select(&sel(&[bad], &[], &[])).unwrap_err();
        assert!(e.to_string().contains(needle), "{bad}: {e}");
        assert_eq!(e.pattern, bad);
        let e = reg.select(&sel(&[], &[bad], &[])).unwrap_err();
        assert!(e.to_string().contains(needle), "exclude {bad}: {e}");
    }
    let e = reg
        .select(&sel(&[], &[], &["no-such-profile"]))
        .unwrap_err();
    assert!(e.reason.contains("unknown profile"), "{e}");
    assert!(e.reason.contains("ohio"), "lists the known profiles: {e}");
    assert!(Pattern::parse("kind:case*").is_ok());
}

#[test]
fn a_jurisdiction_selection_keeps_its_own_short_forms_only() {
    let reg = registry();
    let all = detect_with(reg, MIXED);
    let oh = reg.select(&sel(&["jurisdiction:us.oh"], &[], &[])).unwrap();
    let got = detect_with(&oh, MIXED);
    assert_eq!(
        texts(&got),
        vec![
            "Ohio Rev. Code § 2907.02",
            "State v. Smith, 100 Ohio St.3d 1 (2003)",
            "Id. at 3"
        ]
    );
    // The kept Id. points at the kept Ohio case, renumbered.
    assert_eq!(got[2].antecedent, Some(1));
    assert_eq!(got[2].authority, all[4].authority);
    // Excluding the Id. rule explicitly drops it even after a kept antecedent.
    let no_id = reg
        .select(&sel(&["jurisdiction:us.oh"], &["kind:id"], &[]))
        .unwrap();
    assert_eq!(detect_with(&no_id, MIXED).len(), 2);
    // Selecting the Id. rule itself keeps every Id.; a dropped antecedent becomes None.
    let ids_only = reg.select(&sel(&["kind:id"], &[], &[])).unwrap();
    let got = detect_with(&ids_only, MIXED);
    assert_eq!(texts(&got), vec!["Id. at 5", "Id. at 3"]);
    assert!(got
        .iter()
        .all(|c| c.antecedent.is_none() && c.authority.is_some()));
}

#[test]
fn markdown_and_lint_honor_the_selection() {
    let md = "See [28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331) and Ohio Rev. Code § 2907.02.";
    let reg = registry();
    let fed = reg
        .select(&sel(&["jurisdiction:us.fed"], &[], &[]))
        .unwrap();
    let doc = detect_markdown_with(&fed, md);
    assert_eq!(doc.citations.len(), 1);
    assert!(doc.is_linked(0));

    let policy = Policy {
        include: vec!["jurisdiction:us.oh".into()],
        ..Policy::default()
    };
    let (doc, findings) = lint_markdown(md, &policy);
    assert_eq!(doc.citations.len(), 1);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, "CITE001");

    let bad = Policy {
        profiles: vec!["nope".into()],
        ..Policy::default()
    };
    let (doc, findings) = lint_markdown(md, &bad);
    assert_eq!(
        doc.citations.len(),
        2,
        "a bad selector lints with every rule"
    );
    assert_eq!(findings[0].code, "CITE008");
    assert_eq!(findings[0].name, "bad-selector");
    assert!(findings[0].message.contains("unknown profile"));
    assert!(legal_citations::policy::code_info("CITE008").is_some());
}

#[test]
fn policy_json_carries_the_selector_and_omits_it_when_empty() {
    let p: Policy = serde_json::from_str(
        r#"{"include": ["kind:case"], "exclude": ["rule:us.case.westlaw"], "profiles": ["ohio"]}"#,
    )
    .unwrap();
    assert_eq!(
        p.selector(),
        sel(&["kind:case"], &["rule:us.case.westlaw"], &["ohio"])
    );
    let json = serde_json::to_string(&Policy::default()).unwrap();
    assert!(!json.contains("include") && !json.contains("profiles"));
    let mut a = sel(&["a"], &[], &[]);
    assert!(!a.is_empty());
    a.extend(&sel(&["b"], &["c"], &["d"]));
    assert_eq!(a, sel(&["a", "b"], &["c"], &["d"]));
    assert!(Selector::default().is_empty());
}

#[test]
fn every_embedded_profile_is_valid_and_named_ones_exist() {
    let set = profiles();
    let v = validate_profiles(registry(), set);
    assert!(v.is_empty(), "{v:#?}");
    for name in [
        "federal-litigation",
        "ohio",
        "california",
        "new-york",
        "texas",
        "ptab",
        "ttab",
        "uspto-prosecution",
        "tax",
        "immigration",
        "labor",
        "agency-all",
        "everything",
    ] {
        let p = set.get(name).unwrap_or_else(|| panic!("profile {name}"));
        assert!(!p.description.is_empty());
        let s = registry().select(&sel(&[], &[], &[name])).unwrap();
        assert!(!s.is_empty(), "{name} selects nothing");
    }
    let every = registry().select(&sel(&[], &[], &["everything"])).unwrap();
    assert_eq!(every.len(), registry().active().count());
    let ptab = registry().select(&sel(&[], &[], &["ptab"])).unwrap();
    assert!(ids(&ptab).contains(&"us.fed.uspto.ptab_proceeding".to_string()));
    let usp = registry()
        .select(&sel(&[], &[], &["uspto-prosecution"]))
        .unwrap();
    assert!(
        !ids(&usp).contains(&"us.fed.uspto.ptab_exhibit".to_string()),
        "profile exclude"
    );
}

#[test]
fn profile_validation_catches_broken_profiles() {
    let reg = registry();
    let set = Profiles::from_sources(&[
        (
            "profiles/a.json",
            r#"{"name": "a", "description": "", "include": ["kind:case"]}"#,
        ),
        (
            "profiles/b.json",
            r#"{"name": "wrong", "description": "d", "include": ["kind:case"]}"#,
        ),
        (
            "profiles/c.json",
            r#"{"name": "c", "description": "d", "include": ["tag:none-yet"]}"#,
        ),
        (
            "profiles/d.json",
            r#"{"name": "d", "description": "d", "include": ["bogus:x"]}"#,
        ),
        (
            "profiles/e.json",
            r#"{"name": "e", "description": "d", "extra": 1}"#,
        ),
        (
            "profiles/f.json",
            r#"{"name": "f", "description": "d", "include": ["court:E.D. Tex."]}"#,
        ),
    ]);
    let checks: Vec<String> = validate_profiles(reg, &set)
        .iter()
        .map(|v| format!("{} {}", v.rule, v.detail))
        .collect();
    let has = |s: &str| checks.iter().any(|c| c.contains(s));
    assert!(has("profiles/b.json name \"wrong\""), "{checks:?}");
    assert!(has("profile:a description is empty"), "{checks:?}");
    assert!(has("profile:c selects no rule"), "{checks:?}");
    assert!(has("profile:d selector"), "{checks:?}");
    assert!(has("profiles/e.json"), "{checks:?}");
    // A court-only profile selects citations, not rules, and is fine.
    assert!(!has("profile:f"), "{checks:?}");
    // A profile pattern may name a tag no rule uses yet; selection still works.
    let s = Selection::new(reg, &sel(&[], &[], &["a"]), &set).unwrap();
    assert!(s.rules().all(|(_, r)| r.def.kind == Kind::Case));
    let e = Selection::new(reg, &sel(&[], &[], &["d"]), &set).unwrap_err();
    assert!(e.reason.contains("unknown namespace"));
}
