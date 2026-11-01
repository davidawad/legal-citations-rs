//! The compiled rule registry and its contract.
//!
//! Loading never panics: a rule that fails to parse or compile is skipped
//! and recorded in [`Registry::load_errors`]. The contract test asserts there
//! are none, and [`validate`] checks everything else a rule promises.

use crate::rule::{
    fill_template, template_filters, template_names, Family, Kind, RuleDef, TEMPLATE_FILTERS,
};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/rule_files.rs"));

/// A rule with its compiled pattern.
#[derive(Debug)]
pub struct Rule {
    /// The rule definition as written.
    pub def: RuleDef,
    /// Compiled forward pattern.
    pub regex: Regex,
    /// Compiled frozen legacy pattern, if the rule has one.
    pub legacy_regex: Option<Regex>,
    /// Compiled `reject` patterns.
    pub reject: Vec<Regex>,
    /// Compiled `reject_before` patterns.
    pub reject_before: Vec<Regex>,
    /// Embedded file path, for messages.
    pub path: String,
}

/// Every embedded rule, compiled.
#[derive(Debug, Default)]
pub struct Registry {
    /// Active and retired rules, in file-path order.
    pub rules: Vec<Rule>,
    /// Files that failed to parse or compile.
    pub load_errors: Vec<Violation>,
}

/// One broken promise of the rule contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Violation {
    /// Rule id, or file path when the id is unknown.
    pub rule: String,
    /// Stable check name, e.g. `self-example`.
    pub check: &'static str,
    /// What is wrong.
    pub detail: String,
}

impl Registry {
    /// Build a registry from `(path, json)` pairs.
    pub fn from_sources(files: &[(&str, &str)]) -> Registry {
        let mut reg = Registry::default();
        for (path, text) in files {
            let def: RuleDef = match serde_json::from_str(text) {
                Ok(def) => def,
                Err(e) => {
                    reg.load_errors.push(Violation {
                        rule: (*path).to_string(),
                        check: "parse",
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            let regex = match Regex::new(&def.pattern) {
                Ok(r) => r,
                Err(e) => {
                    reg.load_errors.push(Violation {
                        rule: def.id.clone(),
                        check: "compile",
                        detail: crate::portable::unsupported_hint(&def.pattern)
                            .unwrap_or_else(|| e.to_string()),
                    });
                    continue;
                }
            };
            let legacy_regex = match def.legacy.as_ref().map(|l| Regex::new(&l.pattern)) {
                None => None,
                Some(Ok(r)) => Some(r),
                Some(Err(e)) => {
                    reg.load_errors.push(Violation {
                        rule: def.id.clone(),
                        check: "compile-legacy",
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            let reject = match def.reject.iter().map(|p| Regex::new(p)).collect() {
                Ok(r) => r,
                Err(e) => {
                    reg.load_errors.push(Violation {
                        rule: def.id.clone(),
                        check: "compile-reject",
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            let reject_before = match def.reject_before.iter().map(|p| Regex::new(p)).collect() {
                Ok(r) => r,
                Err(e) => {
                    reg.load_errors.push(Violation {
                        rule: def.id.clone(),
                        check: "compile-reject",
                        detail: e.to_string(),
                    });
                    continue;
                }
            };
            reg.rules.push(Rule {
                def,
                regex,
                legacy_regex,
                reject,
                reject_before,
                path: (*path).to_string(),
            });
        }
        reg
    }

    /// Rules that participate in detection.
    pub fn active(&self) -> impl Iterator<Item = (usize, &Rule)> {
        self.rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.def.retired.is_none())
    }

    /// Look a rule up by id.
    pub fn get(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.def.id == id)
    }

    /// Hash of every rule file, for reproducible reports.
    pub fn hash(&self) -> String {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for rule in &self.rules {
            for b in rule.path.bytes().chain(rule.def.pattern.bytes()) {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        format!("{h:016x}")
    }
}

/// `(path, json)` of every embedded rule file, for building a registry
/// with local rules added (`Registry::from_sources`).
pub fn rule_files() -> &'static [(&'static str, &'static str)] {
    RULE_FILES
}

/// The embedded registry, compiled once.
pub fn registry() -> &'static Registry {
    static CELL: OnceLock<Registry> = OnceLock::new();
    CELL.get_or_init(|| Registry::from_sources(RULE_FILES))
}

fn id_is_well_formed(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

fn check_templates(rule: &Rule, out: &mut Vec<Violation>) {
    let names: BTreeSet<&str> = rule.regex.capture_names().flatten().collect();
    let def = &rule.def;
    let mut templates: Vec<(&'static str, &str)> = vec![
        ("normalize", def.normalize.as_str()),
        ("authority_key", def.authority_key.as_str()),
    ];
    templates.extend(
        def.url_templates
            .iter()
            .map(|t| ("url_templates", t.as_str())),
    );
    for (field, template) in templates {
        for filter in template_filters(template) {
            if !TEMPLATE_FILTERS.contains(&filter.as_str()) {
                out.push(Violation {
                    rule: def.id.clone(),
                    check: "template-filter",
                    detail: format!("{field} uses the unknown filter |{filter}"),
                });
            }
        }
        for name in template_names(template) {
            if !names.contains(name.as_str()) {
                out.push(Violation {
                    rule: def.id.clone(),
                    check: "template-group",
                    detail: format!("{field} uses {{{name}}} but the pattern has no such group"),
                });
            }
        }
    }
    for decoder in &def.url_decoders {
        if !template_filters(decoder).is_empty() {
            out.push(Violation {
                rule: def.id.clone(),
                check: "decoder-group",
                detail: "url_decoders placeholders take no |filter".to_string(),
            });
        }
        for name in template_names(decoder) {
            if !template_names(&def.authority_key).contains(&name) {
                out.push(Violation {
                    rule: def.id.clone(),
                    check: "decoder-group",
                    detail: format!(
                        "url_decoders uses {{{name}}} which authority_key does not use"
                    ),
                });
            }
        }
    }
}

fn check_provenance(def: &RuleDef, out: &mut Vec<Violation>) {
    let p = &def.provenance;
    let mut fail = |detail: &str| {
        out.push(Violation {
            rule: def.id.clone(),
            check: "provenance",
            detail: detail.to_string(),
        });
    };
    if p.encodes.trim().is_empty() {
        fail("provenance.encodes is empty");
    }
    if p.opened && p.source_url.trim().is_empty() {
        fail("opened=true requires source_url");
    }
    if !p.opened && p.how_to_verify.trim().is_empty() {
        fail("opened=false requires how_to_verify");
    }
    match &def.measured.precision {
        serde_json::Value::String(s) if s == "unmeasured" => {}
        serde_json::Value::Number(n) if n.as_f64().is_some_and(|v| (0.0..=1.0).contains(&v)) => {
            if def.measured.n.is_none() {
                fail("measured precision requires n");
            }
        }
        other => fail(&format!(
            "measured.precision must be \"unmeasured\" or 0..=1, got {other}"
        )),
    }
}

fn raw_spans(rule: &Rule, text: &str) -> Vec<[usize; 2]> {
    crate::recognize::rule_matches(rule, text)
        .into_iter()
        .map(|c| c.get(0).map_or([0, 0], |m| [m.start(), m.end()]))
        .collect()
}

fn check_examples(rule: &Rule, out: &mut Vec<Violation>) {
    let def = &rule.def;
    let mut fail = |check: &'static str, detail: String| {
        out.push(Violation {
            rule: def.id.clone(),
            check,
            detail,
        });
    };
    if def.examples.positive.len() < 3 {
        fail(
            "examples-count",
            format!(
                "{} positive examples, need >= 3",
                def.examples.positive.len()
            ),
        );
    }
    if def.examples.negative.len() < 3 {
        fail(
            "examples-count",
            format!(
                "{} negative examples, need >= 3",
                def.examples.negative.len()
            ),
        );
    }
    for (i, pos) in def.examples.positive.iter().enumerate() {
        let got = raw_spans(rule, &pos.text);
        if got != pos.spans {
            fail(
                "positive-spans",
                format!(
                    "positive[{i}] {:?}: expected {:?}, got {got:?}",
                    pos.text, pos.spans
                ),
            );
            continue;
        }
        if pos.groups.is_empty() {
            continue;
        }
        let Some(caps) = crate::recognize::rule_matches(rule, &pos.text)
            .into_iter()
            .next()
        else {
            continue;
        };
        for (name, want) in &pos.groups {
            let have = caps.name(name).map(|m| m.as_str());
            if have != Some(want.as_str()) {
                fail(
                    "positive-groups",
                    format!("positive[{i}] group {name}: expected {want:?}, got {have:?}"),
                );
            }
        }
    }
}

/// Pipeline-level checks: the self-example guard and kind-level negatives.
fn check_pipeline(reg: &Registry, idx: usize, rule: &Rule, out: &mut Vec<Violation>) {
    let def = &rule.def;
    if let Some(first) = def.examples.positive.first() {
        if let Some(want) = first.spans.first() {
            let cites = crate::detect_with(reg, &first.text);
            let own = cites.iter().any(|c| {
                c.rule_index == idx && c.core_span.start == want[0] && c.core_span.end == want[1]
            });
            let rivals: Vec<&str> = cites
                .iter()
                .filter(|c| {
                    c.rule_index != idx && c.core_span.start < want[1] && want[0] < c.core_span.end
                })
                .map(|c| c.rule_id.as_str())
                .collect();
            if !own || !rivals.is_empty() {
                out.push(Violation {
                    rule: def.id.clone(),
                    check: "self-example",
                    detail: format!(
                        "positive[0] {:?} must be matched by exactly this rule at {want:?}; own={own}, rivals={rivals:?}",
                        first.text
                    ),
                });
            }
        }
    }
    for neg in &def.examples.negative {
        let hits: Vec<String> = crate::detect_with(reg, neg)
            .into_iter()
            .filter(|c| c.kind == def.kind)
            .map(|c| format!("{} {:?}", c.rule_id, c.text))
            .collect();
        if !hits.is_empty() {
            out.push(Violation {
                rule: def.id.clone(),
                check: "negative",
                detail: format!("negative {neg:?} matched {} rule(s): {hits:?}", def.kind),
            });
        }
    }
}

fn check_legacy(reg: &Registry, out: &mut Vec<Violation>) {
    let mut orders: Vec<(usize, &str)> = reg
        .rules
        .iter()
        .filter_map(|r| r.def.legacy.as_ref().map(|l| (l.order, r.def.id.as_str())))
        .collect();
    orders.sort_unstable();
    for (expected, (order, id)) in orders.iter().enumerate() {
        if *order != expected {
            out.push(Violation {
                rule: (*id).to_string(),
                check: "legacy-order",
                detail: format!("legacy.order {order}, expected contiguous order {expected}"),
            });
        }
    }
}

/// Every pattern a rule carries: `pattern`, `legacy`, `reject`, `reject_before`.
fn patterns(def: &RuleDef) -> impl Iterator<Item = &str> {
    std::iter::once(def.pattern.as_str())
        .chain(def.legacy.iter().map(|l| l.pattern.as_str()))
        .chain(def.reject.iter().map(String::as_str))
        .chain(def.reject_before.iter().map(String::as_str))
}

/// Portability issues of every rule's patterns: those that break the
/// bundled export (`breaks_export`) or only Go RE2 (the rest).
fn portability(reg: &Registry, breaks_export: bool) -> Vec<Violation> {
    let mut out = Vec::new();
    for rule in &reg.rules {
        for pattern in patterns(&rule.def) {
            for issue in crate::portable::issues(pattern) {
                if issue.breaks_export == breaks_export {
                    out.push(Violation {
                        rule: rule.def.id.clone(),
                        check: if breaks_export {
                            "portable"
                        } else {
                            "portable-re2"
                        },
                        detail: format!("{}: {pattern:?}", issue.what),
                    });
                }
            }
        }
    }
    out
}

/// Patterns Go RE2 cannot compile but the bundle can carry: reported by
/// `cite-detect rules validate`, never a violation.
pub fn portability_warnings(reg: &Registry) -> Vec<Violation> {
    portability(reg, false)
}

/// Check every promise every rule makes. An empty result is the contract.
/// That includes portability: a pattern the bundled export cannot carry in
/// both dialects (see [`crate::portable`]) is a `portable` violation.
pub fn validate(reg: &Registry) -> Vec<Violation> {
    let mut out = reg.load_errors.clone();
    let mut seen = BTreeMap::new();
    for (idx, rule) in reg.rules.iter().enumerate() {
        let def = &rule.def;
        if let Some(prev) = seen.insert(def.id.clone(), rule.path.clone()) {
            out.push(Violation {
                rule: def.id.clone(),
                check: "duplicate-id",
                detail: format!("also defined in {prev}"),
            });
        }
        if !id_is_well_formed(&def.id) {
            out.push(Violation {
                rule: def.id.clone(),
                check: "id-format",
                detail: "ids are dotted lowercase segments [a-z0-9_]".into(),
            });
        }
        let mut tags = BTreeSet::new();
        for tag in &def.tags {
            let ok = !tag.is_empty()
                && tag
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
            if !ok || !tags.insert(tag) {
                out.push(Violation {
                    rule: def.id.clone(),
                    check: "tags",
                    detail: format!(
                        "tag {tag:?} is malformed or repeated; tags are unique [a-z0-9_-]+"
                    ),
                });
            }
        }
        let expected_dir = format!("rules/{}/", def.kind.as_str());
        if !rule.path.starts_with(&expected_dir) {
            out.push(Violation {
                rule: def.id.clone(),
                check: "path",
                detail: format!("{} should live under {expected_dir}", rule.path),
            });
        }
        if def.retired.is_some() {
            continue;
        }
        check_templates(rule, &mut out);
        check_provenance(def, &mut out);
        check_examples(rule, &mut out);
        check_pipeline(reg, idx, rule, &mut out);
    }
    check_legacy(reg, &mut out);
    out.extend(portability(reg, true));
    out
}

/// Summary row for `cite-detect rules list`.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct RuleSummary {
    /// Rule id.
    pub id: String,
    /// Kind.
    pub kind: Kind,
    /// Jurisdiction.
    pub jurisdiction: String,
    /// Family (`family:` selector).
    pub family: Family,
    /// Tags (`tag:` selector).
    pub tags: Vec<String>,
    /// What it encodes.
    pub encodes: String,
    /// Whether the encoded rule was opened.
    pub verified: bool,
    /// Measured precision or `"unmeasured"`.
    pub precision: serde_json::Value,
    /// Example of a URL the rule can build, if any.
    pub links_to: Option<String>,
    /// Retirement reason, if retired.
    pub retired: Option<String>,
}

/// One summary row per rule.
pub fn summaries(reg: &Registry) -> Vec<RuleSummary> {
    reg.rules
        .iter()
        .map(|r| {
            let groups = r
                .def
                .examples
                .positive
                .first()
                .and_then(|p| r.regex.captures(&p.text))
                .map(|caps| crate::recognize::groups_of(&r.regex, &caps))
                .unwrap_or_default();
            RuleSummary {
                id: r.def.id.clone(),
                kind: r.def.kind,
                jurisdiction: r.def.jurisdiction.clone(),
                family: r.def.family(),
                tags: r.def.tags.clone(),
                encodes: r.def.provenance.encodes.clone(),
                verified: r.def.provenance.opened,
                precision: r.def.measured.precision.clone(),
                links_to: r
                    .def
                    .url_templates
                    .first()
                    .and_then(|t| fill_template(t, &groups)),
                retired: r.def.retired.clone(),
            }
        })
        .collect()
}
