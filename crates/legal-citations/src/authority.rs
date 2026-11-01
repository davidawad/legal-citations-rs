//! Table of authorities: citations grouped by the authority they name.
//!
//! [`authorities`] groups a document's citations by authority key. Parallel
//! cites join their first cite's authority (and a later stand-alone cite of a
//! parallel reporter joins it too), short forms join their antecedent's,
//! and a subsequent-history cite (`aff'd, 250 F.3d 9`) is listed under the
//! decision whose history it is. A citation with no authority key is grouped
//! by its kind and normalized text; an unresolved short form is not an
//! authority and is left out.

use crate::citation::{canonical_key, Citation, Details};
use crate::rule::Kind;
use crate::span::Span;
use serde::Serialize;
use std::collections::BTreeMap;

/// How an occurrence cites its authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Form {
    /// A full citation.
    Full,
    /// A parallel cite of a full citation.
    Parallel,
    /// A short form (`410 U.S. at 116`, `Id.`, `supra`).
    Short,
    /// A later decision cited as subsequent history.
    History,
}

/// One place the authority is cited.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Occurrence {
    /// Index into the citation list.
    pub citation: usize,
    /// How it is cited.
    pub form: Form,
    /// Kind of the citing citation.
    pub kind: Kind,
    /// Citation text.
    pub text: String,
    /// Citation span.
    pub span: Span,
    /// Signal through explanatory parenthetical.
    pub full_span: Span,
    /// Pincites of this occurrence.
    pub pincites: Vec<String>,
    /// Signal, if any.
    pub signal: Option<String>,
}

/// A subsequent-history entry of an authority.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct HistoryEntry {
    /// The phrase (`aff'd`, `cert. denied`).
    pub phrase: String,
    /// Citation index of the later decision.
    pub citation: usize,
    /// Its text.
    pub text: String,
    /// Its authority key.
    pub key: Option<String>,
}

/// One authority with every place it is cited.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Authority {
    /// Authority key (the first full cite's), the grouping identity.
    pub key: String,
    /// Keys of parallel cites joined into this authority.
    pub parallel_keys: Vec<String>,
    /// Kind of the first full cite.
    pub kind: Kind,
    /// Rule of the first full cite.
    pub rule_id: String,
    /// Jurisdiction of the first full cite.
    pub jurisdiction: String,
    /// Full citation as first written, parallel cites included.
    pub full_cite: String,
    /// Display form of the first full cite (its text when the rule has no
    /// `normalize` template).
    pub normalized: String,
    /// Typed fields of the first full cite.
    pub details: Details,
    /// Subsequent history.
    pub subsequent_history: Vec<HistoryEntry>,
    /// Every occurrence in text order.
    pub occurrences: Vec<Occurrence>,
    /// Occurrences that are short forms.
    pub short_forms: usize,
    /// Every pincite across occurrences, first-seen order, deduplicated.
    pub pages_cited: Vec<String>,
    /// Candidate source URLs (verified templates only).
    pub urls: Vec<String>,
}

/// Display form: the rule's normalized form, or the text when the rule has
/// no `normalize` template.
fn display(c: &Citation) -> String {
    if c.normalized.trim().is_empty() {
        c.text.clone()
    } else {
        c.normalized.clone()
    }
}

/// Grouping key of a citation that has no authority key.
fn fallback_key(c: &Citation) -> String {
    format!("{}:{}", c.kind, canonical_key(&display(c)))
}

/// Group citations into authorities, sorted by kind (in [`Kind::ALL`]
/// order), then name, then key.
pub fn authorities(cites: &[Citation]) -> Vec<Authority> {
    // Which group (by first-cite index) each citation joins, and how.
    let mut group: Vec<Option<(usize, Form)>> = vec![None; cites.len()];
    let mut by_key: BTreeMap<String, usize> = BTreeMap::new();
    for (i, c) in cites.iter().enumerate() {
        let root = |j: usize| group.get(j).copied().flatten().map(|(r, _)| r);
        let joined = if let Some(p) = c.details.parallel_of().filter(|p| *p < i) {
            root(p).map(|r| (r, Form::Parallel))
        } else if let Some(h) = c.details.history_of().filter(|h| *h < i) {
            root(h).map(|r| (r, Form::History))
        } else if c.kind.is_short_form() {
            c.antecedent
                .filter(|a| *a < i)
                .and_then(root)
                .map(|r| (r, Form::Short))
        } else {
            let key = c.authority.clone().unwrap_or_else(|| fallback_key(c));
            Some(match by_key.get(&key) {
                Some(r) => (*r, Form::Full),
                None => {
                    by_key.insert(key, i);
                    (i, Form::Full)
                }
            })
        };
        if let Some(slot) = group.get_mut(i) {
            *slot = joined;
        }
        // A parallel cite's own key points at its group from now on.
        if let (Some((r, Form::Parallel)), Some(k)) = (joined, &c.authority) {
            by_key.entry(k.clone()).or_insert(r);
        }
    }
    let mut out: BTreeMap<usize, Authority> = BTreeMap::new();
    for (i, (c, g)) in cites.iter().zip(&group).enumerate() {
        let Some((root, form)) = *g else { continue };
        let entry = out.entry(root).or_insert_with(|| Authority {
            key: c.authority.clone().unwrap_or_else(|| fallback_key(c)),
            parallel_keys: Vec::new(),
            kind: c.kind,
            rule_id: c.rule_id.clone(),
            jurisdiction: c.jurisdiction.clone(),
            full_cite: c.text.clone(),
            normalized: display(c),
            details: c.details.clone(),
            subsequent_history: Vec::new(),
            occurrences: Vec::new(),
            short_forms: 0,
            pages_cited: Vec::new(),
            urls: c.urls.clone(),
        });
        // Only the first full cite's own parallels extend the full cite.
        let first_parallel = c.details.parallel_of() == Some(root);
        if form == Form::Parallel {
            if let Some(k) = &c.authority {
                if !entry.parallel_keys.contains(k) && *k != entry.key {
                    entry.parallel_keys.push(k.clone());
                }
            }
            if first_parallel {
                entry.full_cite = format!("{}, {}", entry.full_cite, c.text);
            }
            for u in &c.urls {
                if !entry.urls.contains(u) {
                    entry.urls.push(u.clone());
                }
            }
        }
        if form == Form::History {
            let phrase = match c.details.history_of().and_then(|h| cites.get(h)) {
                Some(Citation {
                    details: Details::Case(d),
                    ..
                }) => d
                    .subsequent_history
                    .iter()
                    .find(|h| h.citation == i)
                    .map(|h| h.phrase.clone()),
                _ => None,
            }
            .unwrap_or_default();
            entry.subsequent_history.push(HistoryEntry {
                phrase,
                citation: i,
                text: c.text.clone(),
                key: c.authority.clone(),
            });
        }
        if form == Form::Short {
            entry.short_forms += 1;
        }
        let pincites = c.details.pincites().to_vec();
        if form != Form::History {
            for p in &pincites {
                if !entry.pages_cited.contains(p) {
                    entry.pages_cited.push(p.clone());
                }
            }
        }
        entry.occurrences.push(Occurrence {
            citation: i,
            form,
            kind: c.kind,
            text: c.text.clone(),
            span: c.span,
            full_span: c.full_span,
            pincites,
            signal: c.signal.clone(),
        });
    }
    let mut list: Vec<Authority> = out.into_values().collect();
    let rank = |k: Kind| Kind::ALL.iter().position(|x| *x == k).unwrap_or(usize::MAX);
    list.sort_by(|a, b| {
        rank(a.kind)
            .cmp(&rank(b.kind))
            .then_with(|| sort_name(a).cmp(&sort_name(b)))
            .then_with(|| a.key.cmp(&b.key))
    });
    list
}

/// Case name or display form, lowercased, without a leading `In re`/`Ex parte`.
pub fn sort_name(a: &Authority) -> String {
    let name = match &a.details {
        Details::Case(d) => d.case_name.clone().unwrap_or_else(|| a.normalized.clone()),
        _ => a.normalized.clone(),
    };
    let lower = name.to_lowercase();
    ["in re ", "ex parte "]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .map_or(lower.clone(), str::to_string)
}
