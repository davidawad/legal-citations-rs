//! Typed fields per citation (L2/L3): case parts, parallel cites and
//! subsequent history; code, title, sections and subsection path; agency,
//! document type and number. Everything here is read from the rule's named
//! groups, the assembled fields and the text right around the citation; no
//! span changes.

use super::{compiled, Citation};
use crate::registry::{Registry, Rule};
use crate::rule::Kind;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// Typed fields of a citation, by shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Details {
    /// A case or short case cite.
    Case(Box<CaseDetails>),
    /// A codified provision: statute, regulation, constitution, court rule,
    /// session law.
    Code(CodeDetails),
    /// An agency document: Federal Register, manual, decision, ruling.
    Agency(AgencyDetails),
    /// `Id.` or `supra`.
    Short(ShortDetails),
    /// Record, docket, secondary, treaty, foreign, unrecognized: see `fields`.
    #[default]
    Other,
}

impl Details {
    /// Index of the first cite of the parallel group this one belongs to.
    pub fn parallel_of(&self) -> Option<usize> {
        match self {
            Details::Case(c) => c.parallel_of,
            _ => None,
        }
    }

    /// Index of the decision whose subsequent history this cite is.
    pub fn history_of(&self) -> Option<usize> {
        match self {
            Details::Case(c) => c.history_of,
            _ => None,
        }
    }

    /// Pincites (pages, sections or paragraphs cited), if any.
    pub fn pincites(&self) -> &[String] {
        match self {
            Details::Case(c) => &c.pincites,
            Details::Short(s) => &s.pincites,
            _ => &[],
        }
    }

    /// Renumber every citation index held; dropped targets disappear.
    pub(crate) fn remap(&mut self, f: &dyn Fn(usize) -> Option<usize>) {
        if let Details::Case(c) = self {
            c.parallel_of = c.parallel_of.and_then(f);
            c.history_of = c.history_of.and_then(f);
            c.parallels = c.parallels.iter().filter_map(|i| f(*i)).collect();
            c.subsequent_history = c
                .subsequent_history
                .iter()
                .filter_map(|h| {
                    f(h.citation).map(|citation| History {
                        phrase: h.phrase.clone(),
                        citation,
                    })
                })
                .collect();
        }
    }
}

/// How the parties of a case are named.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PartyStyle {
    /// `First v. Second`.
    Versus,
    /// `In re Subject`.
    InRe,
    /// `Ex parte Subject`.
    ExParte,
}

/// The parties of a case name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Parties {
    /// Naming style.
    pub style: PartyStyle,
    /// First-named party (`versus` style).
    pub first: Option<String>,
    /// Second-named party (`versus` style).
    pub second: Option<String>,
    /// The subject of an `In re` or `Ex parte` name.
    pub subject: Option<String>,
}

/// One subsequent-history step: `aff'd, 123 F.3d 456`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct History {
    /// The history phrase as written (`aff'd`, `cert. denied`).
    pub phrase: String,
    /// Index of the citation of the later decision.
    pub citation: usize,
}

/// Typed case fields.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CaseDetails {
    /// Case name (shared by every cite of a parallel group).
    pub case_name: Option<String>,
    /// Parties parsed from the case name.
    pub parties: Option<Parties>,
    /// Reporter volume.
    pub volume: Option<String>,
    /// Reporter as written.
    pub reporter: Option<String>,
    /// Reporter with Bluebook spacing (`F. Supp. 2d`, `N.E.2d`, `S. Ct.`).
    pub reporter_canonical: Option<String>,
    /// First page.
    pub page: Option<String>,
    /// Database or public-domain number (`2001 WL 12345`, `2025-Ohio-2425`).
    pub number: Option<String>,
    /// Pincites, one per comma-separated pin (`389-90`, `*3`, `5 n.2`).
    pub pincites: Vec<String>,
    /// Court from the date parenthetical.
    pub court: Option<String>,
    /// Decision year.
    pub year: Option<String>,
    /// Full date when the parenthetical has one (`Sept. 10, 2002`).
    pub date: Option<String>,
    /// On a parallel cite: index of the first cite of its group.
    pub parallel_of: Option<usize>,
    /// On the first cite of a parallel group: the other cites.
    pub parallels: Vec<usize>,
    /// Subsequent history of this decision (`aff'd`, `cert. denied`).
    pub subsequent_history: Vec<History>,
    /// On a later decision cited as history: index of the earlier one.
    pub history_of: Option<usize>,
}

/// Typed fields of a codified provision.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CodeDetails {
    /// The code as written (`U.S.C.`, `Ohio Rev. Code`, `Fed. R. Civ. P.`).
    pub code: Option<String>,
    /// Title or volume (`28` in `28 U.S.C.`).
    pub title: Option<String>,
    /// Sections or rules cited, without subsections.
    pub sections: Vec<String>,
    /// Subsection path of the first section: `(a)(1)(B)` is `["a", "1", "B"]`.
    pub subsections: Vec<String>,
    /// Year from a code parenthetical (`(2018)`) or the rule's year group.
    pub year: Option<String>,
    /// Edition or publisher from the parenthetical (`West`), or an edition group.
    pub edition: Option<String>,
}

/// Typed fields of an agency document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AgencyDetails {
    /// Agency, from the rule's jurisdiction (`USPTO`, `IRS`), if known.
    pub agency: Option<String>,
    /// Document type as written (`MPEP`, `Rev. Rul.`, `IPR`, `Fed. Reg.`).
    pub document_type: Option<String>,
    /// Document number or section.
    pub number: Option<String>,
    /// Volume (Federal Register).
    pub volume: Option<String>,
    /// Page (Federal Register).
    pub page: Option<String>,
    /// Date or year, when cited.
    pub date: Option<String>,
}

/// Fields of `Id.` and `supra`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ShortDetails {
    /// Pincites.
    pub pincites: Vec<String>,
}

/// Bluebook R. 6.1(a) spacing: adjacent single capitals (and ordinals such as
/// `2d`) close up; anything next to a longer abbreviation gets a space.
/// `F.Supp.2d` becomes `F. Supp. 2d`, `N. E. 2d` becomes `N.E.2d`.
pub fn canonical_reporter(reporter: &str) -> String {
    let mut units: Vec<String> = Vec::new();
    for word in reporter.split_whitespace() {
        let mut cur = String::new();
        let mut chars = word.chars().peekable();
        while let Some(c) = chars.next() {
            cur.push(c);
            let next_is_digit = chars.peek().is_some_and(char::is_ascii_digit);
            if c == '.' || (next_is_digit && !cur.chars().all(|x| x.is_ascii_digit())) {
                units.push(std::mem::take(&mut cur));
            }
        }
        if !cur.is_empty() {
            units.push(cur);
        }
    }
    let single = |u: &str| {
        let body = u.trim_end_matches('.');
        let one_cap = body.chars().count() == 1 && body.chars().all(char::is_uppercase);
        let ordinal = body.starts_with(|c: char| c.is_ascii_digit())
            && ["d", "th", "st", "nd", "rd"]
                .iter()
                .any(|s| body.trim_start_matches(|c: char| c.is_ascii_digit()) == *s);
        one_cap || ordinal
    };
    let mut out = String::new();
    let mut prev: Option<&str> = None;
    for u in &units {
        if let Some(p) = prev {
            if !(single(p) && single(u)) {
                out.push(' ');
            }
        }
        out.push_str(u);
        prev = Some(u);
    }
    out
}

/// Split a case name into parties.
pub fn parties(name: &str) -> Option<Parties> {
    let lower = name.to_ascii_lowercase();
    for (prefix, style) in [
        ("in re ", PartyStyle::InRe),
        ("ex parte ", PartyStyle::ExParte),
    ] {
        if lower.starts_with(prefix) {
            let subject = name.get(prefix.len()..)?.trim();
            return Some(Parties {
                style,
                first: None,
                second: None,
                subject: Some(subject.to_string()),
            });
        }
    }
    for sep in [" v. ", " vs. ", " versus "] {
        if let Some((a, b)) = name.split_once(sep) {
            return Some(Parties {
                style: PartyStyle::Versus,
                first: Some(a.trim().to_string()),
                second: Some(b.trim().to_string()),
                subject: None,
            });
        }
    }
    None
}

/// Split a pin list on commas: `389-90, 395` is `["389-90", "395"]`.
pub fn split_pins(pins: &str) -> Vec<String> {
    pins.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

fn history_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"^\s*,\s*(?P<phrase>(?:aff[’']d|aff[’']g|rev[’']d|rev[’']g|vacated|vacating|modified|cert\.\s+(?:denied|granted|dismissed)|reh[’']g\s+(?:en\s+banc\s+)?(?:denied|granted)|overruled|abrogated|appeal\s+dismissed|dismissed|withdrawn|superseded|amended)[^;()]{0,80}?)(?:\s*,|\s+sub\s+nom\.)\s*$",
    )
}

fn code_paren_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &C,
        r"^\s*\((?:(?P<edition>[^()0-9]{1,40}?)\s+)?(?P<year>1[6-9][0-9]{2}|20[0-9]{2})\)",
    )
}

fn date_paren_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(&C, r"^\s*\(([^()]{0,40}?(?:1[6-9][0-9]{2}|20[0-9]{2}))\)")
}

fn paren_parts_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(&C, r"\(([^()]+)\)")
}

/// Named groups with their byte spans, re-found at the match start.
fn group_spans(rule: &Rule, text: &str, start: usize) -> BTreeMap<String, (usize, usize)> {
    let mut out = BTreeMap::new();
    let Some(caps) = rule.regex.captures_at(text, start) else {
        return out;
    };
    for name in rule.regex.capture_names().flatten() {
        if let Some(m) = caps.name(name) {
            out.insert(name.to_string(), (m.start(), m.end()));
        }
    }
    out
}

/// The literal text of a match before its first number-bearing group, with
/// a leading title/volume group removed: `28 U.S.C. § 1331` gives `U.S.C.`.
fn prefix_label(
    text: &str,
    start: usize,
    end: usize,
    spans: &BTreeMap<String, (usize, usize)>,
) -> Option<String> {
    const ANCHORS: [&str; 10] = [
        "part", "section", "rule", "key", "congress", "chapter", "number", "page", "type",
        "circuit",
    ];
    let cut = ANCHORS
        .iter()
        .filter_map(|a| spans.get(*a).map(|s| s.0))
        .filter(|s| *s > start)
        .min()
        .unwrap_or(end);
    let mut from = start;
    for lead in ["title", "volume", "entry"] {
        if let Some((s, e)) = spans.get(lead) {
            if *s == start {
                from = *e;
            }
        }
    }
    let label = text
        .get(from..cut)?
        .trim()
        .trim_end_matches(|c: char| c.is_whitespace() || c == '§' || c == ',')
        .trim_end_matches("No.")
        .trim();
    (!label.is_empty()).then(|| label.to_string())
}

fn get(fields: &BTreeMap<String, String>, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| fields.get(*n))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn agency_of(jurisdiction: &str) -> Option<String> {
    let mut parts = jurisdiction.split('.');
    if parts.next()? != "us" || parts.next()? != "fed" {
        return None;
    }
    let seg = parts.next()?;
    (!matches!(seg, "app" | "dist" | "bankr" | "cfc" | "ip")).then(|| seg.to_ascii_uppercase())
}

fn case_details(c: &Citation) -> CaseDetails {
    let f = &c.fields;
    let case_name = get(f, &["case_name"]);
    let pins = get(f, &["pincite", "pin"]).map_or_else(Vec::new, |p| split_pins(&p));
    let reporter = get(f, &["reporter"]);
    CaseDetails {
        parties: case_name.as_deref().and_then(parties),
        case_name,
        volume: get(f, &["volume"]),
        reporter_canonical: reporter.as_deref().map(canonical_reporter),
        reporter,
        page: get(f, &["page"]),
        number: get(f, &["number"]),
        pincites: pins,
        court: get(f, &["court"]),
        year: get(f, &["year"]),
        date: get(f, &["date"]),
        ..CaseDetails::default()
    }
}

fn code_details(text: &str, c: &Citation, rule: &Rule) -> CodeDetails {
    let f = &c.fields;
    let spans = group_spans(rule, text, c.core_span.start);
    let code = get(f, &["code"])
        .map(|code| {
            text.get(c.core_span.start..c.core_span.end)
                .and_then(|core| core.find(&code).map(|i| core.get(..i + code.len())))
                .flatten()
                .map_or(code.clone(), |s| s.trim().to_string())
        })
        .or_else(|| prefix_label(text, c.core_span.start, c.core_span.end, &spans));
    let raw_section = get(f, &["section", "rule", "key", "number"]);
    let (sections, mut subsections) = match &raw_section {
        Some(s) => {
            let base = s.split('(').next().unwrap_or("").trim().to_string();
            let subs: Vec<String> = paren_parts_re()
                .map(|r| {
                    r.captures_iter(s)
                        .filter_map(|m| m.get(1).map(|x| x.as_str().to_string()))
                        .collect()
                })
                .unwrap_or_default();
            (vec![base], subs)
        }
        None => (Vec::new(), Vec::new()),
    };
    for g in ["sub", "subsection", "clause"] {
        if let (Some(v), Some(r)) = (f.get(g), paren_parts_re()) {
            let parts: Vec<String> = r
                .captures_iter(v)
                .filter_map(|m| m.get(1).map(|x| x.as_str().to_string()))
                .collect();
            if parts.is_empty() && !v.trim().is_empty() {
                subsections.push(v.trim().to_string());
            }
            subsections.extend(parts);
        }
    }
    let paren = code_paren_re().and_then(|r| r.captures(text.get(c.span.end..).unwrap_or("")));
    CodeDetails {
        code,
        title: get(f, &["title", "volume", "congress", "circuit"]),
        sections,
        subsections,
        year: paren
            .as_ref()
            .and_then(|p| p.name("year"))
            .map(|m| m.as_str().to_string())
            .or_else(|| get(f, &["year"])),
        edition: get(f, &["edition"]).or_else(|| {
            paren
                .as_ref()
                .and_then(|p| p.name("edition"))
                .map(|m| m.as_str().trim().to_string())
        }),
    }
}

fn agency_details(text: &str, c: &Citation, rule: &Rule) -> AgencyDetails {
    let f = &c.fields;
    let spans = group_spans(rule, text, c.core_span.start);
    let number = if f.contains_key("type") {
        text.get(c.core_span.start..c.core_span.end)
            .map(str::to_string)
    } else {
        get(f, &["section", "number", "key"])
    };
    AgencyDetails {
        agency: agency_of(&c.jurisdiction),
        document_type: get(f, &["type"])
            .or_else(|| prefix_label(text, c.core_span.start, c.core_span.end, &spans)),
        number,
        volume: get(f, &["volume"]),
        page: get(f, &["page"]),
        date: date_paren_re()
            .and_then(|r| r.captures(text.get(c.span.end..).unwrap_or("")))
            .and_then(|m| m.get(1))
            .map(|m| m.as_str().trim().to_string())
            .or_else(|| get(f, &["date"])),
    }
}

/// True when the gap between two case cites makes the second a parallel cite.
fn parallel_gap(text: &str, a: &Citation, b: &Citation) -> bool {
    a.full_span.end == a.span.end
        && !b.fields.contains_key("case_name")
        && b.signal.is_none()
        && text
            .get(a.span.end..b.span.start)
            .is_some_and(|g| g.trim() == ",")
}

/// Fill `details` and `urls` for every citation.
pub(super) fn annotate(reg: &Registry, text: &str, cites: &mut [Citation]) {
    for i in 0..cites.len() {
        let Some(c) = cites.get(i) else { continue };
        let Some(rule) = reg.rules.get(c.rule_index) else {
            continue;
        };
        let details = match c.kind {
            Kind::Case | Kind::CaseShort => Details::Case(Box::new(case_details(c))),
            Kind::Statute
            | Kind::Regulation
            | Kind::Constitution
            | Kind::CourtRule
            | Kind::SessionLaw => Details::Code(code_details(text, c, rule)),
            Kind::FederalRegister | Kind::AgencyManual | Kind::AgencyDecision => {
                Details::Agency(agency_details(text, c, rule))
            }
            Kind::Id | Kind::Supra => Details::Short(ShortDetails {
                pincites: get(&c.fields, &["pin"]).map_or_else(Vec::new, |p| split_pins(&p)),
            }),
            _ => Details::Other,
        };
        let urls = crate::link_target::candidates(reg, c.rule_index, &c.fields);
        if let Some(c) = cites.get_mut(i) {
            c.details = details;
            c.urls = urls;
        }
    }
    link_parallels(text, cites);
    link_history(text, cites);
    // A resolved short form links where its antecedent links.
    for i in 0..cites.len() {
        let inherited = cites
            .get(i)
            .filter(|c| c.urls.is_empty())
            .and_then(|c| c.antecedent)
            .and_then(|a| cites.get(a))
            .map(|a| a.urls.clone());
        if let (Some(urls), Some(c)) = (inherited, cites.get_mut(i)) {
            c.urls = urls;
        }
    }
}

fn case_mut(c: &mut Citation) -> Option<&mut CaseDetails> {
    match &mut c.details {
        Details::Case(d) => Some(d),
        _ => None,
    }
}

fn link_parallels(text: &str, cites: &mut [Citation]) {
    let mut i = 0;
    while i < cites.len() {
        let mut j = i;
        while let (Some(a), Some(b)) = (cites.get(j), cites.get(j + 1)) {
            if a.kind == Kind::Case && b.kind == Kind::Case && parallel_gap(text, a, b) {
                j += 1;
            } else {
                break;
            }
        }
        if j > i {
            // The last cite of the group carries the date parenthetical; the
            // first carries the name. Share both across the group.
            let last = cites.get(j).map(case_details).unwrap_or_default();
            let first = cites.get(i).map(case_details).unwrap_or_default();
            for k in i..=j {
                let Some(d) = cites.get_mut(k).and_then(case_mut) else {
                    continue;
                };
                d.case_name = d.case_name.take().or_else(|| first.case_name.clone());
                d.parties = d.parties.take().or_else(|| first.parties.clone());
                d.court = d.court.take().or_else(|| last.court.clone());
                d.year = d.year.take().or_else(|| last.year.clone());
                d.date = d.date.take().or_else(|| last.date.clone());
                if k == i {
                    d.parallels = (i + 1..=j).collect();
                } else {
                    d.parallel_of = Some(i);
                }
            }
        }
        i = j + 1;
    }
}

fn link_history(text: &str, cites: &mut [Citation]) {
    for j in 1..cites.len() {
        let Some(b) = cites.get(j) else { continue };
        let a = cites.get(j - 1);
        let (Some(a), true) = (a, b.kind == Kind::Case) else {
            continue;
        };
        if a.kind != Kind::Case || b.details.parallel_of().is_some() {
            continue;
        }
        let gap = text.get(a.full_span.end..b.full_span.start).unwrap_or("");
        let Some(phrase) = history_re()
            .and_then(|r| r.captures(gap))
            .and_then(|c| c.name("phrase"))
            .map(|m| m.as_str().trim().to_string())
        else {
            continue;
        };
        // History hangs off the first decision of the chain (`cert. denied`
        // after `aff'd` is still that case's history).
        let mut primary = j - 1;
        while let Some(up) = cites
            .get(primary)
            .and_then(|c| c.details.parallel_of().or(c.details.history_of()))
            .filter(|up| *up < primary)
        {
            primary = up;
        }
        if let Some(d) = cites.get_mut(primary).and_then(case_mut) {
            d.subsequent_history.push(History {
                phrase,
                citation: j,
            });
        }
        if let Some(d) = cites.get_mut(j).and_then(case_mut) {
            d.history_of = Some(primary);
        }
    }
}
