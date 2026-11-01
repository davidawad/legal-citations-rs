//! The rule registry's data model: one JSON file per recognizer rule.
//!
//! A rule file is the ONLY place a pattern, its examples, its provenance and
//! its URL knowledge live. Everything else (code paths, docs, coverage
//! tables, TNET presets) is derived from these records.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Bytes of preceding text a [`RuleDef::reject_before`] pattern sees.
pub const BEFORE_WINDOW: usize = 24;
/// Closed citation-kind taxonomy. Extending it is a code change on purpose:
/// policy, findings and consumers all switch on it.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Full case citation: volume, reporter, page.
    Case,
    /// Short case citation: `449 U.S. at 389`.
    CaseShort,
    /// `Id.` / `Ibid.`
    Id,
    /// `Smith, supra`
    Supra,
    /// Codified statute (U.S.C., state codes).
    Statute,
    /// Session law (Pub. L., Stat.).
    SessionLaw,
    /// Codified regulation (C.F.R., state administrative codes).
    Regulation,
    /// Federal Register.
    FederalRegister,
    /// Constitution.
    Constitution,
    /// Court rule (FRCP, FRAP, FRE, circuit and local rules).
    CourtRule,
    /// Agency manual (MPEP, TBMP, TMEP).
    AgencyManual,
    /// Agency decision or ruling (Rev. Rul., PTAB/TTAB decisions).
    AgencyDecision,
    /// Record cite (ER-139, JA 16, Ex. 1001, TTABVUE, ECF No.).
    Record,
    /// Secondary source (Restatement, treatise, law review).
    Secondary,
    /// Treaty.
    Treaty,
    /// Foreign authority.
    Foreign,
    /// Docket number.
    Docket,
    /// Legislative material (bills, committee reports, Cong. Rec., hearings, CRS reports).
    Legislative,
    /// Executive material (executive orders, proclamations, presidential memoranda).
    Executive,
    /// Short statute or regulation citation by section alone: `§ 1983`.
    StatuteShort,
    /// Citation-shaped text no specific rule matched (catch-all layer,
    /// confidence below 0.5, finding CITE011).
    Unrecognized,
}

impl Kind {
    /// Every kind, in declaration order.
    pub const ALL: [Kind; 21] = [
        Kind::Case,
        Kind::CaseShort,
        Kind::Id,
        Kind::Supra,
        Kind::Statute,
        Kind::SessionLaw,
        Kind::Regulation,
        Kind::FederalRegister,
        Kind::Constitution,
        Kind::CourtRule,
        Kind::AgencyManual,
        Kind::AgencyDecision,
        Kind::Record,
        Kind::Secondary,
        Kind::Treaty,
        Kind::Foreign,
        Kind::Docket,
        Kind::Legislative,
        Kind::Executive,
        Kind::StatuteShort,
        Kind::Unrecognized,
    ];

    /// The snake_case name used in JSON and on the CLI.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Case => "case",
            Kind::CaseShort => "case_short",
            Kind::Id => "id",
            Kind::Supra => "supra",
            Kind::Statute => "statute",
            Kind::SessionLaw => "session_law",
            Kind::Regulation => "regulation",
            Kind::FederalRegister => "federal_register",
            Kind::Constitution => "constitution",
            Kind::CourtRule => "court_rule",
            Kind::AgencyManual => "agency_manual",
            Kind::AgencyDecision => "agency_decision",
            Kind::Record => "record",
            Kind::Secondary => "secondary",
            Kind::Treaty => "treaty",
            Kind::Foreign => "foreign",
            Kind::Docket => "docket",
            Kind::Legislative => "legislative",
            Kind::Executive => "executive",
            Kind::StatuteShort => "statute_short",
            Kind::Unrecognized => "unrecognized",
        }
    }

    /// Parse a CLI/JSON name.
    pub fn parse(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// True for the catch-all kind, which runs after every other rule.
    pub fn is_catch_all(self) -> bool {
        self == Kind::Unrecognized
    }

    /// True for short forms that refer back to an earlier citation.
    pub fn is_short_form(self) -> bool {
        matches!(
            self,
            Kind::CaseShort | Kind::Id | Kind::Supra | Kind::StatuteShort
        )
    }

    /// The family a rule of this kind belongs to unless its file says otherwise.
    pub fn family(self) -> Family {
        match self {
            Kind::Case | Kind::CaseShort => Family::Case,
            Kind::Id | Kind::Supra => Family::ShortForm,
            Kind::Statute | Kind::StatuteShort | Kind::Constitution => Family::Statute,
            Kind::SessionLaw | Kind::Legislative => Family::Legislative,
            Kind::Executive => Family::Agency,
            Kind::Regulation => Family::Regulation,
            Kind::FederalRegister | Kind::AgencyManual | Kind::AgencyDecision => Family::Agency,
            Kind::CourtRule => Family::CourtRule,
            Kind::Record | Kind::Docket => Family::Record,
            Kind::Secondary => Family::Secondary,
            Kind::Treaty | Kind::Foreign => Family::International,
            Kind::Unrecognized => Family::Unrecognized,
        }
    }
}

/// Coarse grouping of rules for selection (`family:agency`). Derived from the
/// kind ([`Kind::family`]); a rule file may override it with `"family"`.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// Cases and short case cites.
    Case,
    /// Codified statutes and constitutions.
    Statute,
    /// Regulations (C.F.R., state administrative codes).
    Regulation,
    /// Court rules.
    CourtRule,
    /// Agency materials: Federal Register, manuals, decisions, guidance.
    Agency,
    /// Legislative materials: session laws, bills, reports.
    Legislative,
    /// Record and docket cites.
    Record,
    /// Secondary sources.
    Secondary,
    /// Treaties and foreign authority.
    International,
    /// `Id.` and `supra`, which refer to any family.
    ShortForm,
    /// The catch-all layer.
    Unrecognized,
}

impl Family {
    /// Every family, in declaration order.
    pub const ALL: [Family; 11] = [
        Family::Case,
        Family::Statute,
        Family::Regulation,
        Family::CourtRule,
        Family::Agency,
        Family::Legislative,
        Family::Record,
        Family::Secondary,
        Family::International,
        Family::ShortForm,
        Family::Unrecognized,
    ];

    /// The snake_case name used in JSON and selectors.
    pub fn as_str(self) -> &'static str {
        match self {
            Family::Case => "case",
            Family::Statute => "statute",
            Family::Regulation => "regulation",
            Family::CourtRule => "court_rule",
            Family::Agency => "agency",
            Family::Legislative => "legislative",
            Family::Record => "record",
            Family::Secondary => "secondary",
            Family::International => "international",
            Family::ShortForm => "short_form",
            Family::Unrecognized => "unrecognized",
        }
    }

    /// Parse a selector or JSON name.
    pub fn parse(name: &str) -> Option<Family> {
        Family::ALL.into_iter().find(|f| f.as_str() == name)
    }
}

impl fmt::Display for Family {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a rule's knowledge comes from. `opened` is true only when the rule
/// text (Bluebook rule, court rule, statute) was actually read by whoever
/// wrote the pattern; otherwise the rule is reported as unverified everywhere.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// The rule or convention the pattern encodes, e.g. `Bluebook R. 12.3 (21st ed.)`.
    pub encodes: String,
    /// True only if `encodes` was actually opened and read.
    pub opened: bool,
    /// Where the encoded rule can be read, if public.
    #[serde(default)]
    pub source_url: String,
    /// Concrete steps to verify an unopened rule.
    #[serde(default)]
    pub how_to_verify: String,
    /// Lineage: earlier patterns this rule descends from.
    #[serde(default)]
    pub derived_from: Vec<String>,
    /// Third-party data or wording used, with license (see NOTICE).
    #[serde(default)]
    pub third_party: Vec<String>,
}

/// A positive example: text plus every expected span and optional groups of
/// the first span.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Positive {
    /// Input text.
    pub text: String,
    /// Every expected `[start, end)` UTF-8 byte span for this rule.
    pub spans: Vec<[usize; 2]>,
    /// Expected named groups of the first match (subset check).
    #[serde(default)]
    pub groups: BTreeMap<String, String>,
}

/// Examples that pin a rule's behavior. `positive[0]` is the documentation
/// example and must be matched by exactly this rule (self-example guard).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Examples {
    /// Texts the rule must match, with exact spans.
    pub positive: Vec<Positive>,
    /// Realistic texts no rule of this kind may match.
    pub negative: Vec<String>,
}

/// Measured precision, written only by `cite-detect eval`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Measured {
    /// Precision in `[0, 1]` or the string `"unmeasured"`.
    pub precision: serde_json::Value,
    /// Sample size behind `precision`.
    #[serde(default)]
    pub n: Option<u64>,
    /// Corpus identity (manifest hash).
    #[serde(default)]
    pub corpus: Option<String>,
    /// ISO date of the measurement.
    #[serde(default)]
    pub date: Option<String>,
}

impl Default for Measured {
    fn default() -> Self {
        Self {
            precision: serde_json::Value::String("unmeasured".into()),
            n: None,
            corpus: None,
            date: None,
        }
    }
}

impl Measured {
    /// Numeric precision, if measured.
    pub fn value(&self) -> Option<f64> {
        self.precision.as_f64()
    }
}

/// The frozen pre-extraction pattern this rule replaces, kept only so the
/// legacy parity mode reproduces the old behavior byte for byte.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Legacy {
    /// Old `cite_type` name.
    #[serde(rename = "type")]
    pub cite_type: String,
    /// Position in the old `CITATION_PATTERNS` list.
    pub order: usize,
    /// Old pattern text, verbatim.
    pub pattern: String,
}

/// One recognizer rule, deserialized from `rules/**/<id>.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleDef {
    /// Stable dotted id, never reused.
    pub id: String,
    /// Citation kind.
    pub kind: Kind,
    /// Dotted jurisdiction, e.g. `us.fed`, `us.ca`, `us.fed.uspto`.
    pub jurisdiction: String,
    /// Rust `regex` pattern with named groups.
    pub pattern: String,
    /// Overlap tie-break, higher wins.
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Display template over named groups, e.g. `{title} U.S.C. § {section}`.
    #[serde(default)]
    pub normalize: String,
    /// Authority identity template, e.g. `us.usc/{title}/{section}`.
    /// `{name|filter}` passes the group through a filter, in this and every
    /// other template: `digits` keeps the ASCII digits (`12,345` -> `12345`),
    /// `slug` the lowercased ASCII letters and digits (`H.R. ` -> `hr`).
    #[serde(default)]
    pub authority_key: String,
    /// Templates that build a source URL from named groups (only for hosts
    /// verified by an HTTP fetch; the rule's note records the date and a
    /// sample URL).
    #[serde(default)]
    pub url_templates: Vec<String>,
    /// URL shapes (host + path, `{group}` placeholders) that decode a link
    /// target back to this rule's groups.
    #[serde(default)]
    pub url_decoders: Vec<String>,
    /// Base confidence before feature adjustments.
    #[serde(default = "default_confidence")]
    pub base_confidence: f64,
    /// Provenance.
    pub provenance: Provenance,
    /// Examples.
    pub examples: Examples,
    /// Measured precision (written by `eval`).
    #[serde(default)]
    pub measured: Measured,
    /// Frozen legacy pattern for parity mode.
    #[serde(default)]
    pub legacy: Option<Legacy>,
    /// Retired rules stay in the registry with a reason.
    #[serde(default)]
    pub retired: Option<String>,
    /// Patterns that veto a match: when one finds a match inside the matched
    /// text, the match is dropped and the search resumes one character after
    /// its start. Stands in for the look-around the `regex` crate lacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reject: Vec<String>,
    /// Patterns that veto a match by what precedes it: each is run on the up
    /// to [`BEFORE_WINDOW`] bytes ending at the match start (so anchor it with
    /// `$`, e.g. `[A-Za-z]\\.$` keeps `L.R.` from matching inside `A.L.R.`).
    /// A vetoed match is dropped like a `reject`ed one. Stands in for the
    /// look-behind the `regex` crate lacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reject_before: Vec<String>,
    /// Family override; omit to use the kind's family ([`Kind::family`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<Family>,
    /// Cross-cutting collections (`ptab`, `tax`), selected with `tag:<name>`.
    /// Lowercase `[a-z0-9_-]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Free-form notes.
    #[serde(default)]
    pub note: String,
}

impl RuleDef {
    /// The rule's family: the file's override or the kind's default.
    pub fn family(&self) -> Family {
        self.family.unwrap_or_else(|| self.kind.family())
    }

    /// Every collection the rule belongs to, as selector strings: its kind,
    /// family, every prefix of its jurisdiction, its tags and its id.
    pub fn collections(&self) -> Vec<String> {
        self.collections_in(&self.jurisdiction)
    }

    /// [`RuleDef::collections`] with `jurisdiction` in place of the rule's:
    /// the collections of a citation whose court resolved elsewhere.
    pub fn collections_in(&self, jurisdiction: &str) -> Vec<String> {
        let mut out = vec![
            format!("kind:{}", self.kind),
            format!("family:{}", self.family()),
        ];
        let mut prefix = String::new();
        for part in jurisdiction.split('.') {
            if !prefix.is_empty() {
                prefix.push('.');
            }
            prefix.push_str(part);
            out.push(format!("jurisdiction:{prefix}"));
        }
        out.extend(self.tags.iter().map(|t| format!("tag:{t}")));
        out.push(format!("rule:{}", self.id));
        out
    }
}

fn default_priority() -> i32 {
    50
}

fn default_confidence() -> f64 {
    0.9
}

/// The filters a `{name|filter}` placeholder may name.
/// `digits` keeps the ASCII digits (`12,345` -> `12345`); `slug` keeps the
/// ASCII letters and digits, lowercased (`H.J. Res. ` -> `hjres`).
pub const TEMPLATE_FILTERS: [&str; 2] = ["digits", "slug"];

/// One placeholder's value: the group, through its filter if any.
fn placeholder(spec: &str, groups: &BTreeMap<String, String>) -> Option<String> {
    let (name, filter) = spec
        .split_once('|')
        .map_or((spec, None), |(n, f)| (n, Some(f)));
    let value = groups.get(name)?;
    match filter {
        None => Some(value.clone()),
        Some("digits") => Some(value.chars().filter(char::is_ascii_digit).collect()),
        Some("slug") => Some(
            value
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .map(|c| c.to_ascii_lowercase())
                .collect(),
        ),
        Some(_) => None,
    }
}

/// Fill `{name}` and `{name|filter}` placeholders from `groups`. Unknown or
/// missing names, or an unknown filter, leave the template unusable,
/// signalled by `None`.
pub fn fill_template(template: &str, groups: &BTreeMap<String, String>) -> Option<String> {
    fill_template_with(template, groups, str::to_string)
}

/// [`fill_template`], passing each filtered value through `encode` (URL
/// templates percent-encode after the filter, so `{chamber|slug}` sees `H.R. `).
pub fn fill_template_with(
    template: &str,
    groups: &BTreeMap<String, String>,
    encode: impl Fn(&str) -> String,
) -> Option<String> {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open)?);
        let after = rest.get(open + 1..)?;
        let close = after.find('}')?;
        out.push_str(&encode(&placeholder(after.get(..close)?, groups)?));
        rest = after.get(close + 1..)?;
    }
    out.push_str(rest);
    Some(out)
}

/// Placeholder names used by a template, in order (without filters).
pub fn template_names(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(after) = rest.get(open + 1..) else {
            break;
        };
        let Some((name, tail)) = after.split_once('}') else {
            break;
        };
        names.push(name.split('|').next().unwrap_or(name).to_string());
        rest = tail;
    }
    names
}

/// The `|filter` parts of a template's placeholders, in order.
pub fn template_filters(template: &str) -> Vec<String> {
    let mut filters = Vec::new();
    let mut rest = template;
    while let Some((_, after)) = rest.split_once('{') {
        let Some((spec, tail)) = after.split_once('}') else {
            break;
        };
        if let Some((_, f)) = spec.split_once('|') {
            filters.push(f.to_string());
        }
        rest = tail;
    }
    filters
}
