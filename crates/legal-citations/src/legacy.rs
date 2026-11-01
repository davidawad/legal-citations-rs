//! Legacy parity mode: the frozen pre-extraction patterns from the dotfiles
//! `legal-citation-patterns` crate (itself ported from
//! `legal_citation_patterns.py`), run with the old semantics: every pattern,
//! in the old order, every match kept. Consumers migrating from the old crate
//! use this for a zero-behavior-change switch.

use crate::recognize::groups_of;
use crate::registry::{registry, Registry};
use crate::span::OffsetMap;
use serde::Serialize;
use std::collections::BTreeMap;

/// One legacy match.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct LegacyMatch {
    /// Old `cite_type`.
    pub cite_type: String,
    /// Rule id that now owns this legacy pattern.
    pub rule_id: String,
    /// Byte start.
    pub start: usize,
    /// Byte end.
    pub end: usize,
    /// Char start (what the Python original reported).
    pub char_start: usize,
    /// Char end.
    pub char_end: usize,
    /// Non-empty named groups.
    pub groups: BTreeMap<String, String>,
}

fn ordered(reg: &Registry) -> Vec<(&str, &str, &regex::Regex)> {
    let mut v: Vec<(usize, &str, &str, &regex::Regex)> = reg
        .rules
        .iter()
        .filter_map(|r| {
            let l = r.def.legacy.as_ref()?;
            Some((
                l.order,
                l.cite_type.as_str(),
                r.def.id.as_str(),
                r.legacy_regex.as_ref()?,
            ))
        })
        .collect();
    v.sort_by_key(|t| t.0);
    v.into_iter().map(|(_, t, id, re)| (t, id, re)).collect()
}

/// Old `CITATION_PATTERNS`: `(cite_type, regex)` in the old precedence order.
/// Drop-in replacement for the dotfiles `legal_citation_patterns::citation_patterns`.
pub fn citation_patterns() -> Vec<(&'static str, &'static regex::Regex)> {
    ordered(registry())
        .into_iter()
        .map(|(t, _, re)| (t, re))
        .collect()
}

/// The frozen legacy regex for one old `cite_type`.
pub fn pattern(cite_type: &str) -> Option<&'static regex::Regex> {
    ordered(registry())
        .into_iter()
        .find(|(t, _, _)| *t == cite_type)
        .map(|(_, _, re)| re)
}

/// Old `CITATION_PATTERNS` type names, in order.
pub fn citation_types() -> Vec<&'static str> {
    ordered(registry()).into_iter().map(|(t, _, _)| t).collect()
}

/// Old `STATUTE_TYPES`: every type except `case`, in order.
pub fn statute_types() -> Vec<&'static str> {
    citation_types()
        .into_iter()
        .filter(|t| *t != "case")
        .collect()
}

/// Every match of every legacy pattern, pattern by pattern in the old order.
pub fn detect_legacy(text: &str) -> Vec<LegacyMatch> {
    detect_legacy_with(registry(), text)
}

/// [`detect_legacy`] against a given registry.
pub fn detect_legacy_with(reg: &Registry, text: &str) -> Vec<LegacyMatch> {
    let map = OffsetMap::new(text);
    let mut out = Vec::new();
    for (cite_type, rule_id, re) in ordered(reg) {
        for caps in re.captures_iter(text) {
            let Some(m) = caps.get(0) else { continue };
            let span = map.span(m.start(), m.end());
            out.push(LegacyMatch {
                cite_type: cite_type.to_string(),
                rule_id: rule_id.to_string(),
                start: m.start(),
                end: m.end(),
                char_start: span.char_start,
                char_end: span.char_end,
                groups: groups_of(re, &caps),
            });
        }
    }
    out
}
