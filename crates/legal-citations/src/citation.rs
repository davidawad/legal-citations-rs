//! L2 (assemble) and L3 (resolve): raw matches become [`Citation`]s with
//! case names, pincites, parentheticals, signals, authority keys, short-form
//! antecedents and an explainable confidence.

use crate::recognize::RawMatch;
use crate::registry::Registry;
use crate::rule::{fill_template, Kind};
use crate::span::{OffsetMap, Span};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

mod details;
mod furniture;
mod names;
pub use details::{
    canonical_reporter, parties, split_pins, AgencyDetails, CaseDetails, CodeDetails, Details,
    History, Parties, PartyStyle, ShortDetails,
};
pub use furniture::Furniture;
pub use names::{case_name_before, short_name_before};

/// One step of the confidence calculation.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Term {
    /// Add (or subtract) a value.
    Add {
        /// Why.
        label: String,
        /// Amount.
        value: f64,
    },
    /// Cap the final value.
    Cap {
        /// Why.
        label: String,
        /// Ceiling.
        value: f64,
    },
}

impl Term {
    fn add(label: &str, value: f64) -> Term {
        Term::Add {
            label: label.to_string(),
            value,
        }
    }
    fn cap(label: &str, value: f64) -> Term {
        Term::Cap {
            label: label.to_string(),
            value,
        }
    }
}

/// Fold terms: sum the adds, apply the lowest cap, clamp to `[0, 1]`.
pub fn score(terms: &[Term]) -> f64 {
    let mut sum = 0.0;
    let mut cap: f64 = 1.0;
    for t in terms {
        match t {
            Term::Add { value, .. } => sum += value,
            Term::Cap { value, .. } => cap = cap.min(*value),
        }
    }
    sum.min(cap).clamp(0.0, 1.0)
}

/// A detected citation. All spans index the ORIGINAL text.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Citation {
    /// Citation kind.
    pub kind: Kind,
    /// Id of the rule that matched (`cite-detect explain <rule_id>`).
    pub rule_id: String,
    /// Index into the registry (not serialized; ids are the stable handle).
    #[serde(skip)]
    pub rule_index: usize,
    /// The citation proper: case name (if any) through the closing date
    /// parenthetical. Link status is decided on this span.
    pub span: Span,
    /// Exactly what the rule matched.
    pub core_span: Span,
    /// Signal through explanatory parenthetical.
    pub full_span: Span,
    /// Text of `span`.
    pub text: String,
    /// Named fields: rule groups plus `case_name`, `pincite`, `court`, `year`.
    pub fields: BTreeMap<String, String>,
    /// Dotted jurisdiction: the court's, when the court parenthetical names
    /// one the court table knows (`(Ohio Ct. App. 2010)` is `us.oh`), else
    /// the rule's. A resolved short form has its antecedent's.
    pub jurisdiction: String,
    /// Dotted jurisdiction of the rule that matched.
    pub rule_jurisdiction: String,
    /// Display form from the rule's `normalize` template.
    pub normalized: String,
    /// Canonical authority identity; short forms inherit their antecedent's.
    pub authority: Option<String>,
    /// Index (in the returned vector) of the citation a short form refers to.
    pub antecedent: Option<usize>,
    /// Signal preceding the citation (`See`, `Cf.`, ...).
    pub signal: Option<String>,
    /// Explanatory parenthetical after the citation.
    pub parenthetical: Option<Span>,
    /// Confidence in `[0, 1]`.
    pub confidence: f64,
    /// Each term of the confidence calculation, in order.
    pub confidence_terms: Vec<Term>,
    /// True when the rule's encoded authority was actually opened.
    pub verified_rule: bool,
    /// Set when the citation is page furniture (running head, repeated
    /// caption); the link policy does not require a link on it.
    pub furniture: Option<Furniture>,
    /// Typed fields by shape: case parts, parallel cites and subsequent
    /// history; code, title, sections; agency, document type, number.
    pub details: Details,
    /// Candidate source URLs from the rule's verified `url_templates` (a
    /// resolved short form inherits its antecedent's).
    pub urls: Vec<String>,
}

fn compiled(cell: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

fn pincite_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"^,\s*(?:at\s+)?(?:¶¶?\s*)?(?P<pin>\*?[0-9]+(?:\s*[-–—‒]\s*\*?[0-9]+)?(?:\s*,?\s*(?:nn?|fn)\.\s*[0-9]+)?(?:\s*,\s*\*?[0-9]+(?:\s*[-–—‒]\s*\*?[0-9]+)?)*)",
    )
}

/// A reporter or database token right after a pin list means its last number
/// was the volume of a parallel cite (`58, 62, 552 N.E.2d`, `130, 1995 WL`),
/// or the year of a public-domain cite (`58, 2006-Ohio-3563`).
fn parallel_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"^(?:\s+(?:WL\b|[A-Z][A-Za-z]*\.|[A-Z][a-z]+\s+[A-Z][A-Za-z]*\.)|-[A-Z][A-Za-z]{1,10}-[0-9])",
    )
}

/// A month and day closing a court parenthetical (`E.D. La. Sept. 10, 2002`).
fn month_day_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"^(?P<court>.*?)[,\s]*(?P<date>(?:Jan|Feb|Mar|Apr|May|June?|July?|Aug|Sept?|Oct|Nov|Dec)\.?\s+[0-9]{1,2})$",
    )
}

fn date_paren_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"^\s*\((?P<court>[^()]{0,120}?)\s*(?P<year>1[6-9][0-9]{2}|20[0-9]{2})\)",
    )
}

fn explanatory_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(&C, r"^\s*\([^()]*(?:\([^()]*\)[^()]*)*\)")
}

fn signal_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"(?:^|[\s(;])[*_]*(?P<signal>[Ss]ee,?\s+e\.g\.,|[Ss]ee\s+also|[Ss]ee\s+generally|[Bb]ut\s+see|[Bb]ut\s+cf\.|[Ss]ee|[Cc]f\.|[Aa]ccord|[Cc]ontra|[Cc]ompare|[Ee]\.g\.,)[*_]*[\s,]*$",
    )
}

/// Canonical form of an authority key: no whitespace, lowercase, no `§`.
pub fn canonical_key(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace() && *c != '§')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Upper bound for a plausible decision year: this year.
fn current_year() -> u32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    1970 + u32::try_from(secs / 31_556_952).unwrap_or(0)
}

/// Extensions after a case core: pincite, date parenthetical. Returns the
/// new end and adds fields/terms.
fn extend_case(
    text: &str,
    mut end: usize,
    fields: &mut BTreeMap<String, String>,
    terms: &mut Vec<Term>,
) -> usize {
    let rest = text.get(end..).unwrap_or("");
    if let Some(c) = pincite_re().and_then(|r| r.captures(rest)) {
        if let (Some(all), Some(pin)) = (c.get(0), c.name("pin")) {
            let after = rest.get(all.end()..).unwrap_or("");
            // Byte 0 is the comma before the first pin: nothing is left.
            // Stop before the comma, and before any masked markup ahead of it.
            let stop = if parallel_re().is_some_and(|r| r.is_match(after)) {
                let comma = all.as_str().rfind(',').unwrap_or(0);
                all.as_str().get(..comma).map_or(0, |s| s.trim_end().len())
            } else {
                all.end()
            };
            if stop > 0 {
                let pin = rest.get(pin.start()..stop).unwrap_or("").trim_end();
                fields
                    .entry("pincite".into())
                    .or_insert_with(|| pin.to_string());
                end += stop;
            }
        }
    }
    let rest = text.get(end..).unwrap_or("");
    if let Some(c) = date_paren_re().and_then(|r| r.captures(rest)) {
        if let (Some(all), Some(year)) = (c.get(0), c.name("year")) {
            let court = c
                .name("court")
                .map_or("", |m| m.as_str())
                .trim()
                .trim_end_matches(',')
                .trim();
            let (court, date) = match month_day_re().and_then(|r| r.captures(court)) {
                Some(d) => (
                    d.name("court").map_or("", |m| m.as_str()),
                    d.name("date").map(|m| m.as_str()),
                ),
                None => (court, None),
            };
            if !court.is_empty() {
                fields.insert("court".into(), court.to_string());
            }
            if let Some(date) = date {
                fields.insert("date".into(), format!("{date}, {}", year.as_str()));
            }
            fields.insert("year".into(), year.as_str().to_string());
            end += all.end();
            let y: u32 = year.as_str().parse().unwrap_or(0);
            if y > current_year() {
                terms.push(Term::add("year in the future", -0.3));
            } else {
                terms.push(Term::add("date parenthetical", 0.05));
            }
        }
    }
    end
}

/// Assemble raw, overlap-resolved matches into citations (L2), then resolve
/// short forms and authority keys (L3).
pub fn assemble(reg: &Registry, text: &str, matches: Vec<RawMatch>) -> Vec<Citation> {
    let map = OffsetMap::new(text);
    let mut out: Vec<Citation> = Vec::with_capacity(matches.len());
    for m in matches {
        let Some(rule) = reg.rules.get(m.rule_index) else {
            continue;
        };
        let def = &rule.def;
        let mut fields = m.groups.clone();
        let mut start = m.start;
        let mut end = m.end;
        let mut terms = vec![Term::add("rule base confidence", def.base_confidence)];
        let prev_end = out.last().map_or(0, |p: &Citation| p.full_span.end);
        // A name may sit inside the previous citation's explanatory
        // parenthetical (`(quoting Monell v. ..., 436 U.S. 658)`), so only
        // the previous citation proper bounds it.
        let prev_cite_end = out.last().map_or(0, |p: &Citation| p.span.end);
        let name = match def.kind {
            Kind::Case => case_name_before(text, m.start),
            Kind::CaseShort => {
                case_name_before(text, m.start).or_else(|| short_name_before(text, m.start))
            }
            _ => None,
        };
        if let Some((name_start, name)) = name {
            if prev_cite_end <= name_start {
                start = name_start;
                fields.entry("case_name".into()).or_insert(name);
                terms.push(Term::add("case name found", 0.05));
            }
        }
        if matches!(def.kind, Kind::Case | Kind::CaseShort) {
            end = extend_case(text, end, &mut fields, &mut terms);
        }
        let mut full_end = end;
        let mut parenthetical = None;
        if matches!(
            def.kind,
            Kind::Case | Kind::CaseShort | Kind::Id | Kind::Supra
        ) {
            if let Some(all) = explanatory_re().and_then(|r| r.find(text.get(end..).unwrap_or("")))
            {
                let lead = all.as_str().len() - all.as_str().trim_start().len();
                parenthetical = Some(map.span(end + lead, end + all.end()));
                full_end = end + all.end();
            }
        }
        let mut full_start = start;
        let mut signal = None;
        if let Some(c) = signal_re().and_then(|r| r.captures(text.get(..start).unwrap_or(""))) {
            if let Some(s) = c.name("signal") {
                if prev_end <= s.start() {
                    signal = Some(s.as_str().to_string());
                    full_start = s.start();
                }
            }
        }
        if !def.provenance.opened {
            terms.push(Term::cap("rule text not opened", 0.8));
        }
        match def.measured.value() {
            Some(p) => terms.push(Term::cap("measured precision", p)),
            None => terms.push(Term::cap("precision unmeasured", 0.9)),
        }
        let core_text = text.get(m.start..m.end).unwrap_or("");
        let normalized =
            fill_template(&def.normalize, &fields).unwrap_or_else(|| core_text.to_string());
        let authority = if def.authority_key.is_empty() {
            None
        } else {
            fill_template(&def.authority_key, &fields).map(|k| canonical_key(&k))
        };
        out.push(Citation {
            kind: def.kind,
            rule_id: def.id.clone(),
            rule_index: m.rule_index,
            span: map.span(start, end),
            core_span: map.span(m.start, m.end),
            full_span: map.span(full_start, full_end),
            text: text.get(start..end).unwrap_or("").to_string(),
            fields,
            jurisdiction: def.jurisdiction.clone(),
            rule_jurisdiction: def.jurisdiction.clone(),
            normalized,
            authority,
            antecedent: None,
            signal,
            parenthetical,
            confidence: 0.0,
            confidence_terms: terms,
            verified_rule: def.provenance.opened,
            furniture: None,
            details: Details::Other,
            urls: Vec::new(),
        });
    }
    resolve_short_forms(&mut out);
    furniture::mark(text, &mut out);
    details::annotate(reg, text, &mut out);
    crate::court::resolve(&mut out);
    for c in &mut out {
        c.confidence = score(&c.confidence_terms);
    }
    out
}

/// Keep the citations whose `keep` flag is set and renumber the citation
/// indices they hold; a reference to a dropped citation becomes `None`.
pub(crate) fn retain(cites: Vec<Citation>, keep: &[bool]) -> Vec<Citation> {
    let mut map = vec![None; cites.len()];
    let mut next = 0;
    for (slot, k) in map.iter_mut().zip(keep) {
        if *k {
            *slot = Some(next);
            next += 1;
        }
    }
    let remap = |i: Option<usize>| i.and_then(|i| map.get(i).copied().flatten());
    cites
        .into_iter()
        .zip(keep)
        .filter(|(_, k)| **k)
        .map(|(mut c, _)| {
            c.antecedent = remap(c.antecedent);
            c.details.remap(&|i| remap(Some(i)));
            c
        })
        .collect()
}

/// Merge `extra` (catch-all citations, which refer to nothing) into `cites`
/// in text order, renumbering the citation indices `cites` hold.
pub(crate) fn merge(cites: Vec<Citation>, extra: Vec<Citation>) -> Vec<Citation> {
    let mut all: Vec<(Option<usize>, Citation)> = cites
        .into_iter()
        .enumerate()
        .map(|(i, c)| (Some(i), c))
        .chain(extra.into_iter().map(|c| (None, c)))
        .collect();
    all.sort_by_key(|(_, c)| (c.span.start, c.span.end));
    let mut map = vec![None; all.len()];
    for (new, (old, _)) in all.iter().enumerate() {
        if let Some(slot) = old.and_then(|o| map.get_mut(o)) {
            *slot = Some(new);
        }
    }
    let remap = |i: usize| map.get(i).copied().flatten();
    all.into_iter()
        .map(|(_, mut c)| {
            c.antecedent = c.antecedent.and_then(remap);
            c.details.remap(&remap);
            c
        })
        .collect()
}

fn reporter_key(fields: &BTreeMap<String, String>) -> Option<(String, String)> {
    Some((
        fields.get("volume")?.clone(),
        canonical_key(fields.get("reporter")?),
    ))
}

/// `key` with its last path segment replaced by `section`
/// (`us.usc/7/7101` + `7102` = `us.usc/7/7102`).
fn with_section(key: &str, section: &str) -> String {
    match key.rsplit_once('/') {
        Some((head, _)) => format!("{head}/{}", canonical_key(section)),
        None => key.to_string(),
    }
}

/// Index of the citation a short form at `i` refers to (backwards only).
fn find_antecedent(cites: &[Citation], i: usize) -> Option<usize> {
    let me = cites.get(i)?;
    let earlier = cites.get(..i)?;
    let found = match me.kind {
        Kind::Id => earlier
            .iter()
            .rposition(|c| c.kind != Kind::Id || c.antecedent.is_some()),
        Kind::CaseShort => {
            let key = reporter_key(&me.fields)?;
            earlier.iter().rposition(|c| {
                c.kind == Kind::Case && reporter_key(&c.fields).as_ref() == Some(&key)
            })
        }
        Kind::StatuteShort => {
            let section = me.fields.get("section")?;
            earlier.iter().rposition(|c| {
                matches!(c.kind, Kind::Statute | Kind::Regulation)
                    && c.fields.get("section") == Some(section)
            })
        }
        Kind::Supra => {
            let name = me.fields.get("name")?.to_lowercase();
            earlier.iter().rposition(|c| {
                !c.kind.is_short_form()
                    && c.fields
                        .get("case_name")
                        .or_else(|| c.fields.get("author"))
                        .is_some_and(|n| n.to_lowercase().contains(&name))
            })
        }
        _ => None,
    }?;
    // An Id. pointing at another Id. inherits that one's antecedent.
    let target = earlier.get(found)?;
    Some(if target.kind == Kind::Id {
        target.antecedent.unwrap_or(found)
    } else {
        found
    })
}

fn resolve_short_forms(cites: &mut [Citation]) {
    for i in 0..cites.len() {
        let Some(kind) = cites.get(i).map(|c| c.kind) else {
            continue;
        };
        if !kind.is_short_form() {
            continue;
        }
        let antecedent = find_antecedent(cites, i);
        let inherited = antecedent
            .and_then(|j| cites.get(j))
            .and_then(|c| c.authority.clone());
        let code_antecedent = antecedent.and_then(|j| cites.get(j)).is_some_and(|c| {
            matches!(
                c.kind,
                Kind::Statute | Kind::Regulation | Kind::StatuteShort
            )
        });
        let Some(me) = cites.get_mut(i) else { continue };
        match antecedent {
            Some(j) => {
                me.antecedent = Some(j);
                if me.authority.is_none() || kind != Kind::CaseShort {
                    // `Id. § 7102` after `7 U.S.C. § 7101` names the same
                    // code, another section.
                    me.authority = match me.fields.get("section") {
                        Some(section) if kind == Kind::Id && code_antecedent => {
                            inherited.map(|a| with_section(&a, section))
                        }
                        _ => inherited,
                    };
                }
                me.confidence_terms
                    .push(Term::add("antecedent resolved", 0.05));
            }
            None => me
                .confidence_terms
                .push(Term::add("antecedent not found", -0.3)),
        }
    }
}
