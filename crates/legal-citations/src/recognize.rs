//! L1: run every active rule over the text and collect raw matches.

use crate::registry::{Registry, Rule};
use crate::rule::BEFORE_WINDOW;
use regex::{Captures, Regex};
use serde::Serialize;
use std::collections::BTreeMap;

/// One rule match before overlap resolution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct RawMatch {
    /// Index into [`Registry::rules`].
    #[serde(skip)]
    pub rule_index: usize,
    /// Rule id.
    pub rule_id: String,
    /// Byte start.
    pub start: usize,
    /// Byte end.
    pub end: usize,
    /// Rule priority (copied for overlap resolution).
    pub priority: i32,
    /// Non-empty named groups.
    pub groups: BTreeMap<String, String>,
}

/// Named groups that participated in a match.
pub fn groups_of(regex: &Regex, caps: &Captures<'_>) -> BTreeMap<String, String> {
    regex
        .capture_names()
        .flatten()
        .filter_map(|name| {
            caps.name(name)
                .map(|m| (name.to_string(), m.as_str().to_string()))
        })
        .collect()
}

/// The up to [`BEFORE_WINDOW`] bytes of `text` ending at `at`, starting on a
/// char boundary.
fn before(text: &str, at: usize) -> &str {
    let mut from = at.saturating_sub(BEFORE_WINDOW);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    text.get(from..at).unwrap_or("")
}

/// A rule's matches, left to right, with its `reject` and `reject_before`
/// patterns applied: a rejected match is dropped and the search resumes one
/// character after its start. Rules without either behave exactly like
/// `captures_iter`.
pub fn rule_matches<'t>(rule: &Rule, text: &'t str) -> Vec<Captures<'t>> {
    if rule.reject.is_empty() && rule.reject_before.is_empty() {
        return rule.regex.captures_iter(text).collect();
    }
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(caps) = rule.regex.captures_at(text, pos) {
        let Some(whole) = caps.get(0) else { break };
        let next_char = |at: usize| {
            text.get(at..)
                .and_then(|t| t.chars().next())
                .map_or(at + 1, |c| at + c.len_utf8())
        };
        let preceded = || {
            let prefix = before(text, whole.start());
            rule.reject_before.iter().any(|r| r.is_match(prefix))
        };
        if rule.reject.iter().any(|r| r.is_match(whole.as_str())) || preceded() {
            pos = next_char(whole.start());
        } else {
            pos = if whole.is_empty() {
                next_char(whole.end())
            } else {
                whole.end()
            };
            out.push(caps);
        }
        if pos > text.len() {
            break;
        }
    }
    out
}

/// Every match of every active rule, sorted by (start, longest first,
/// priority descending, rule id).
pub fn recognize(reg: &Registry, text: &str) -> Vec<RawMatch> {
    recognize_where(reg, text, |_| true)
}

/// [`recognize`] restricted to the active rules `keep` accepts.
pub fn recognize_where(reg: &Registry, text: &str, keep: impl Fn(&Rule) -> bool) -> Vec<RawMatch> {
    let mut out = Vec::new();
    for (idx, rule) in reg.active().filter(|(_, r)| keep(r)) {
        for caps in rule_matches(rule, text) {
            let Some(whole) = caps.get(0) else { continue };
            if whole.is_empty() {
                continue;
            }
            out.push(RawMatch {
                rule_index: idx,
                rule_id: rule.def.id.clone(),
                start: whole.start(),
                end: whole.end(),
                priority: rule.def.priority,
                groups: groups_of(&rule.regex, &caps),
            });
        }
    }
    out.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then(b.end.cmp(&a.end))
            .then(b.priority.cmp(&a.priority))
            .then(a.rule_id.cmp(&b.rule_id))
    });
    out
}

/// Drop matches that overlap a better one. Better means: higher priority,
/// then longer, then earlier. Returns the survivors in text order.
pub fn resolve_overlaps(matches: Vec<RawMatch>) -> Vec<RawMatch> {
    let mut ranked = matches;
    ranked.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.start.cmp(&b.start))
            .then(a.rule_id.cmp(&b.rule_id))
    });
    let mut kept: Vec<RawMatch> = Vec::new();
    for m in ranked {
        if kept.iter().all(|k| m.end <= k.start || k.end <= m.start) {
            kept.push(m);
        }
    }
    kept.sort_by_key(|m| (m.start, m.end));
    kept
}
