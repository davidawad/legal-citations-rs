//! The release bundle for other languages (`just dist`): every rule in one
//! JSON file with its ECMAScript form, and test vectors drawn from the rule
//! examples. The rule data is MIT (`rules/LICENSE`); the bundle carries
//! `rules/NOTICE`, which must travel with it.

use crate::portable::{to_js, JS_FLAGS};
use crate::registry::{Registry, Violation};
use crate::rule::{RuleDef, BEFORE_WINDOW};
use serde::Serialize;
use std::collections::BTreeMap;

/// `format` of [`Bundle`].
pub const BUNDLE_FORMAT: &str = "legal-citation-rules";
/// `format` of [`Vectors`].
pub const VECTORS_FORMAT: &str = "legal-citation-rules/test-vectors";
/// Version of both formats.
pub const VERSION: u32 = 1;
/// The license of the rule data.
pub const LICENSE: &str = "MIT";
/// `rules/NOTICE`: third-party notices that must travel with the rules.
pub const NOTICE: &str = include_str!("../../../rules/NOTICE");

/// How to run a bundled rule outside Rust, one statement per line.
pub const MATCHING: &[&str] = &[
    "rule.pattern, rule.reject, rule.reject_before and rule.legacy.pattern are written with (?P<name>) groups: Python re, Go regexp (RE2) and PCRE compile them as written.",
    "js holds the same patterns for ECMAScript (new RegExp(pattern, js.flags)) and .NET: (?<name>) groups, and \\d, \\w, \\b and . rewritten to keep the reference meaning (Unicode \\d, \\w and \\b; . matches anything but \\n).",
    "A rule's matches: search left to right. Drop a match when any reject pattern finds a match anywhere inside the matched text, or when any reject_before pattern finds a match in the up to before_window bytes of text that end at the match start (moved forward to a character boundary). After a dropped match resume the search one character after its start; otherwise resume at its end, or one character past it when the match is empty.",
    "Offsets are UTF-8 byte offsets, half-open.",
    "The reference engine is Rust regex: \\d, \\w, \\s and \\b are Unicode-aware, as in Python re on str. Go RE2 and PCRE without UCP read rule.pattern's \\d, \\w and \\b as ASCII, so they can disagree on non-ASCII digits and letters (Arabic-Indic digits, Hebrew and Arabic words).",
    "Rules with retired set are kept for history and take no part in detection.",
];

/// Every rule, for use from other languages.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Bundle {
    /// Always `legal-citation-rules`.
    pub format: String,
    /// Format version.
    pub version: u32,
    /// License of the rule data (`MIT`).
    pub license: String,
    /// Third-party notices that must travel with the bundle (`rules/NOTICE`).
    pub notice: String,
    /// Registry hash, as in reports.
    pub rules_hash: String,
    /// Bytes of text before a match that `reject_before` patterns see.
    pub before_window: usize,
    /// How to run a rule outside Rust.
    pub matching: Vec<String>,
    /// Every rule, active and retired, in file-path order.
    pub rules: Vec<BundledRule>,
}

/// One rule in the bundle.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct BundledRule {
    /// Path in the repository, e.g. `rules/statute/us.fed.statute.usc.json`.
    pub path: String,
    /// The rule (schema: `rule.schema.json`).
    pub rule: RuleDef,
    /// Its patterns in ECMAScript form.
    pub js: JsPatterns,
}

/// A rule's patterns rewritten for ECMAScript and .NET.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct JsPatterns {
    /// RegExp flags to compile every pattern here with.
    pub flags: String,
    /// `rule.pattern`.
    pub pattern: String,
    /// `rule.reject`.
    pub reject: Vec<String>,
    /// `rule.reject_before`.
    pub reject_before: Vec<String>,
    /// `rule.legacy.pattern`, if any.
    pub legacy: Option<String>,
}

/// Build the bundle, or every pattern that cannot be bundled.
pub fn bundle(reg: &Registry) -> Result<Bundle, Vec<Violation>> {
    let mut bad = Vec::new();
    let mut rules = Vec::new();
    for rule in &reg.rules {
        let def = &rule.def;
        let mut js = |p: &str| {
            to_js(p).unwrap_or_else(|| {
                bad.push(Violation {
                    rule: def.id.clone(),
                    check: "portable",
                    detail: format!("{p:?} has no ECMAScript form; see `rules validate`"),
                });
                String::new()
            })
        };
        let js = JsPatterns {
            flags: JS_FLAGS.into(),
            pattern: js(&def.pattern),
            reject: def.reject.iter().map(|p| js(p)).collect(),
            reject_before: def.reject_before.iter().map(|p| js(p)).collect(),
            legacy: def.legacy.as_ref().map(|l| js(&l.pattern)),
        };
        rules.push(BundledRule {
            path: rule.path.clone(),
            rule: def.clone(),
            js,
        });
    }
    if !bad.is_empty() {
        return Err(bad);
    }
    Ok(Bundle {
        format: BUNDLE_FORMAT.into(),
        version: VERSION,
        license: LICENSE.into(),
        notice: NOTICE.into(),
        rules_hash: reg.hash(),
        before_window: BEFORE_WINDOW,
        matching: MATCHING.iter().map(|s| (*s).to_string()).collect(),
        rules,
    })
}

/// Expected rule-level matches on the rule examples, to check a port.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Vectors {
    /// Always `legal-citation-rules/test-vectors`.
    pub format: String,
    /// Format version.
    pub version: u32,
    /// License of the rule data (`MIT`).
    pub license: String,
    /// `rules_hash` of the bundle these vectors belong to.
    pub rules_hash: String,
    /// One vector per example of every active rule.
    pub vectors: Vec<Vector>,
}

/// Whether an example is one the rule must match.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    /// `examples.positive`: the spans are the example's own.
    Positive,
    /// `examples.negative`: the rule matches nothing.
    Negative,
}

/// One rule run on one text.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Vector {
    /// Rule id.
    pub rule: String,
    /// Positive or negative example.
    pub polarity: Polarity,
    /// Input text.
    pub text: String,
    /// Every `[start, end)` UTF-8 byte span the rule matches, with `reject`
    /// and `reject_before` applied.
    pub spans: Vec<[usize; 2]>,
    /// Named groups that took part in the first match.
    pub groups: BTreeMap<String, String>,
}

/// Test vectors from every active rule's examples. A negative example that
/// the rule itself matches (the pipeline, not the rule, keeps it out) is
/// skipped, so every negative vector expects no spans.
pub fn vectors(reg: &Registry) -> Vectors {
    let mut out = Vec::new();
    for (_, rule) in reg.active() {
        let run = |polarity, text: &str| {
            let caps = crate::recognize::rule_matches(rule, text);
            Vector {
                rule: rule.def.id.clone(),
                polarity,
                text: text.to_string(),
                spans: caps
                    .iter()
                    .filter_map(|c| c.get(0).map(|m| [m.start(), m.end()]))
                    .collect(),
                groups: caps
                    .first()
                    .map(|c| crate::recognize::groups_of(&rule.regex, c))
                    .unwrap_or_default(),
            }
        };
        let ex = &rule.def.examples;
        out.extend(ex.positive.iter().map(|p| run(Polarity::Positive, &p.text)));
        out.extend(
            ex.negative
                .iter()
                .map(|n| run(Polarity::Negative, n))
                .filter(|v| v.spans.is_empty()),
        );
    }
    Vectors {
        format: VECTORS_FORMAT.into(),
        version: VERSION,
        license: LICENSE.into(),
        rules_hash: reg.hash(),
        vectors: out,
    }
}
