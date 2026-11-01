//! `xtask-rules indigo`: author state rules from the Indigo Book inventory
//! `corpus/formats/indigo.jsonl` (CC0; see `corpus/formats/README.md`).
//!
//! A Rust port of the removed `scripts/indigo_rules.py` (lcr-g, lcr-i0p).
//! Per state or territory it writes, under `rules/`:
//!
//! - `session_law/us.<st>.session_law.laws.json`: `<year>[-<pamph>] <series>
//!   <page>` (and `<vol> <series> <page>` where Indigo gives that shape), one
//!   alternation over every T3 session-law series of that state.
//! - `regulation/us.<st>.regulation.volume.json`: `<vol|iss|tit> <series>
//!   <page|rule|sec>`, over the state's register and numbered-code series.
//! - `case/<j>.case.nominative_<official>.json`: `<vol> <Official> (<n>
//!   <Nominative>) <page>`, one rule per official reporter.
//! - `case/<j>.case.indigo_reporters.json`: the simple `<vol> <Reporter>
//!   <page>` forms reporters-db lacks.
//! - `statute|regulation/<j>.<kind>.indigo_forms.json`: every statute or
//!   regulation form the coverage run missed, each form translated to a regex
//!   (priority 45: a fallback below the specific hand-written rules).
//!
//! The last two read `docs/measurements/indigo-coverage.json`: they target
//! the inventory lines it marks undetected, or detected by a rule written
//! here (so a re-run after the rules land writes the same files).
//!
//! Positives are Indigo's own examples (plus ones built from the form with
//! new numbers); negatives are prose that names the series without a
//! citation. Provenance is `opened: false`: the forms come from the
//! inventory, not from reading each Indigo section. Re-running overwrites
//! the files; `--check` (and the freshness test) fails when they differ.

use regex::Regex;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::courts::INDIGO_JSONL;
use crate::RULES_DIR;

/// The coverage report the fallback rules target, relative to the repo root.
pub const COVERAGE_JSON: &str = "docs/measurements/indigo-coverage.json";
/// The note every generated rule starts with; also how `--check` finds
/// committed files the generator no longer writes.
pub const AUTHORED_BY: &str =
    "Authored by `cargo run -p xtask-rules -- indigo` from the Indigo inventory (lcr-g).";
const URL: &str = "https://law.resource.org/pub/us/code/blue/IndigoBook.html";
const YEAR: &str = r"(?:1[6-9]|20)[0-9]{2}";
/// Number-shaped placeholder (`x`, `<sec. no.>`) as a regex.
const NUM: &str = r"[0-9]{1,5}[A-Za-z]?(?:[-.:][0-9]{1,5}[A-Za-z]?)*(?:\([A-Za-z0-9]{1,4}\))*";
/// Word-shaped placeholder (`<agency>`) as a regex.
const WORDS: &str = r"[A-Z][A-Za-z.'’&-]*(?:\s(?:&\s)?[A-Z][A-Za-z.'’&-]*){0,5}";
/// Parenthesized publisher or series markers, not nominative reporters.
const NOT_NOMINATIVE: [&str; 7] = ["BNA", "n.s.", "CCH", "P-H", "RIA", "West", "Callaghan"];
/// Lines an existing rule matches only in part (`1 Del. Admin. Code § 41`
/// inside `57-81-1 Del. Admin. Code § 41`); fix those rules instead.
const FORM_SKIP: [usize; 7] = [67, 133, 200, 371, 556, 663, 664];

#[derive(Deserialize)]
struct Format {
    section: String,
    jurisdiction: String,
    kind: String,
    form: String,
    examples: Vec<String>,
}

impl Format {
    /// `Alabama` from `T3 Alabama: Session laws`.
    fn state_name(&self) -> String {
        let head = self.section.split(':').next().unwrap_or_default();
        head.chars().skip(3).collect()
    }
}

/// One generated rule, serialized in the field order of the rule files.
#[derive(Serialize)]
struct Rule {
    id: String,
    kind: String,
    jurisdiction: String,
    pattern: String,
    priority: i32,
    normalize: String,
    authority_key: String,
    url_templates: Vec<String>,
    url_decoders: Vec<String>,
    base_confidence: f64,
    provenance: Provenance,
    examples: Examples,
    measured: Measured,
    note: String,
}

#[derive(Serialize)]
struct Provenance {
    encodes: String,
    opened: bool,
    source_url: String,
    how_to_verify: String,
    derived_from: Vec<String>,
    third_party: Vec<String>,
}

#[derive(Serialize)]
struct Examples {
    positive: Vec<Positive>,
    negative: Vec<String>,
}

#[derive(Serialize)]
struct Positive {
    text: String,
    spans: Vec<[usize; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    groups: Option<Groups>,
}

/// Named groups in pattern order.
struct Groups(Vec<(String, String)>);

impl Serialize for Groups {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in &self.0 {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct Measured {
    precision: String,
}

/// The fields that differ between case rules and the rest.
struct Fields {
    normalize: &'static str,
    authority_key: &'static str,
    url_decoders: &'static [&'static str],
}

const CASE_FIELDS: Fields = Fields {
    normalize: "{volume} {reporter} {page}",
    authority_key: "case/{volume}/{reporter}/{page}",
    url_decoders: &["courtlistener.com/c/{reporter}/{volume}/{page}"],
};
const FORM_FIELDS: Fields = Fields {
    normalize: "",
    authority_key: "",
    url_decoders: &[],
};

/// What [`build`] needs besides the pattern and examples.
struct Spec<'a> {
    kind: &'a str,
    jurisdiction: &'a str,
    id: String,
    lines: &'a [usize],
    encodes: String,
    fields: Fields,
    priority: i32,
}

/// Python's `re.escape` (3.7+) of one character.
fn escape(ch: char, out: &mut String) {
    if "()[]{}?*+-|^$\\.&~# \t\n\r\u{b}\u{c}".contains(ch) {
        out.push('\\');
    }
    out.push(ch);
}

/// Literal series text as regex: optional space after periods, either
/// apostrophe.
fn lit(s: &str) -> String {
    let mut out = String::new();
    let mut prev = None;
    for ch in s.chars() {
        match ch {
            ' ' if prev.is_some_and(|p| p == '.') => out.push_str(r"\s?"),
            ' ' => out.push_str(r"\s"),
            '\'' | '’' => out.push_str("['’]"),
            _ => escape(ch, &mut out),
        }
        prev = Some(ch);
    }
    out
}

/// `form` without its trailing parentheticals (`(West <year>)`).
fn strip_parens(form: &str) -> &str {
    let mut form = form.trim();
    while form.ends_with(')') {
        let mut depth = 0i32;
        let mut cut = None;
        for (i, b) in form.bytes().enumerate().rev() {
            if b == b')' {
                depth += 1;
            } else if b == b'(' {
                depth -= 1;
                if depth == 0 {
                    cut = Some(i);
                    break;
                }
            }
        }
        match cut.and_then(|c| form.get(..c)) {
            Some(head) => form = head.trim_end(),
            None => break,
        }
    }
    form
}

/// Unique strings, longest (in characters) first, then by text.
fn by_length(items: &[String]) -> Vec<&String> {
    let mut v: Vec<&String> = items.iter().collect::<BTreeSet<_>>().into_iter().collect();
    v.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    v
}

fn series_alt(series: &[String]) -> String {
    by_length(series)
        .into_iter()
        .map(|s| lit(s))
        .collect::<Vec<_>>()
        .join("|")
}

fn first_sorted(items: &[String]) -> &str {
    items.iter().min().map_or("", String::as_str)
}

fn session_pattern(series: &[String], vol_first: bool) -> String {
    let mut lead = format!("(?P<year>{YEAR})(?:-(?P<pamphlet>[0-9]{{1,2}}))?");
    if vol_first {
        lead = format!("(?:{lead}|(?P<volume>[0-9]{{1,3}}))");
    }
    format!(
        r"\b{lead}\s(?P<series>{})\s(?:ch\.\s?[0-9]{{1,4}},\s?§\s?[0-9]{{1,4}}[a-z]?,\s)?(?:No\.\s?[0-9]{{1,3}},\s)?(?P<page>[0-9]{{1,5}}(?:-[0-9]{{1,4}})?)\b",
        series_alt(series)
    )
}

fn volume_pattern(series: &[String]) -> String {
    format!(
        r"\b(?P<volume>[0-9]{{1,3}})\s(?P<series>{})\s(?P<page>[0-9]{{1,5}}(?:\.[0-9]{{1,4}})?)\b",
        series_alt(series)
    )
}

fn compile(pattern: &str) -> Result<Regex, String> {
    Regex::new(pattern).map_err(|e| format!("indigo: {e}"))
}

/// The rule, or `None` when fewer than three examples match it.
fn build(
    spec: Spec<'_>,
    pattern: String,
    examples: &[String],
    made: &[String],
    negatives: Vec<String>,
) -> Result<Option<Rule>, String> {
    let regex = compile(&pattern)?;
    let mut pos: Vec<Positive> = Vec::new();
    for ex in examples.iter().chain(made) {
        let text = format!("See {ex}.");
        if let Some(m) = regex.find(&text) {
            if !pos.iter().any(|p| p.text == text) {
                pos.push(Positive {
                    spans: vec![[m.start(), m.end()]],
                    text,
                    groups: None,
                });
            }
        }
    }
    if pos.len() < 3 {
        return Ok(None);
    }
    if let Some(first) = pos.first_mut() {
        if let Some(caps) = regex.captures(&first.text) {
            let groups = regex
                .capture_names()
                .flatten()
                .filter_map(|n| {
                    caps.name(n)
                        .map(|m| (n.to_string(), m.as_str().to_string()))
                })
                .collect();
            first.groups = Some(Groups(groups));
        }
    }
    let section = spec.encodes.split(": ").next().unwrap_or_default();
    let tail = if spec.kind == "case" {
        "The case name, pincite and date parenthetical are assembled around the match; \
         the authority key is the official reporter's, so a nominative cite and the bare \
         official cite are one authority. url_templates: none; no fetch of CourtListener /c/ \
         is recorded for these reporters (the decoder stays)."
    } else {
        "The trailing (West)/(LexisNexis)/date parenthetical is not part of the match. \
         No authority key: these sources have no stable identifier scheme here."
    };
    Ok(Some(Rule {
        id: spec.id,
        kind: spec.kind.to_string(),
        jurisdiction: spec.jurisdiction.to_string(),
        pattern,
        priority: spec.priority,
        normalize: spec.fields.normalize.to_string(),
        authority_key: spec.fields.authority_key.to_string(),
        url_templates: vec![],
        url_decoders: spec
            .fields
            .url_decoders
            .iter()
            .map(|s| s.to_string())
            .collect(),
        base_confidence: crate::BASE_CONFIDENCE,
        provenance: Provenance {
            how_to_verify: format!(
                "Open the Indigo Book, {}, and compare each series with the alternation.",
                section.replace("Indigo Book ", "")
            ),
            encodes: spec.encodes,
            opened: false,
            source_url: URL.to_string(),
            derived_from: spec
                .lines
                .iter()
                .map(|n| format!("{INDIGO_JSONL} line {n}"))
                .collect(),
            third_party: vec!["The Indigo Book (Public Resource, CC0); see NOTICE".to_string()],
        },
        examples: Examples {
            positive: pos,
            negative: negatives,
        },
        measured: Measured {
            precision: "unmeasured".to_string(),
        },
        note: format!("{AUTHORED_BY} {tail}"),
    }))
}

/// Pretty JSON with each `spans` array on one line, like the hand-written
/// rules.
fn dump(rule: &Rule) -> Result<String, String> {
    let s = serde_json::to_string_pretty(rule).map_err(|e| e.to_string())?;
    let spans = compile(r#""spans": \[\s*((?:\[\s*\d+,\s*\d+\s*\],?\s*)+)\]"#)?;
    let pair = compile(r"\[\s*(\d+),\s*(\d+)\s*\]")?;
    let s = spans.replace_all(&s, |c: &regex::Captures<'_>| {
        let inner = c.get(1).map_or("", |m| m.as_str());
        let pairs: Vec<String> = pair
            .captures_iter(inner)
            .map(|p| format!("[{}, {}]", &p[1], &p[2]))
            .collect();
        format!("\"spans\": [{}]", pairs.join(", "))
    });
    Ok(format!("{s}\n"))
}

#[derive(Default)]
struct Group {
    series: Vec<String>,
    lines: Vec<usize>,
    examples: Vec<String>,
    made: Vec<String>,
    vol: bool,
    name: String,
}

impl Group {
    /// Record line `n` once, with its examples; true the first time.
    fn add_line(&mut self, n: usize, f: &Format) -> bool {
        if self.lines.contains(&n) {
            return false;
        }
        self.lines.push(n);
        self.examples.extend(f.examples.iter().cloned());
        true
    }
}

fn skip_jurisdiction(j: &str) -> bool {
    j == "us" || j == "us.fed" || j.starts_with("us.tribal")
}

/// Session-law and register rules from the T3 forms.
fn series_rules(formats: &[Format]) -> Result<Vec<Rule>, String> {
    let session_re = compile(
        r"^(?P<lead><year>(?:-<pamph\. no\.>)?|<vol\. no\.>)\s+(?P<series>.+?)\s+(?:<ch\. x § x>\s+)?(?:No\. x,\s+)?(?:<page no\.>|<page>|<law no\.>)$",
    )?;
    let volume_re = compile(
        r"^<(?:vol|iss|tit)\. no\.>\s+(?P<series>[^<>§]+?)\s+<(?:page|rule\.|sec\.|reg\.) no\.>$",
    )?;
    let mut names: BTreeMap<&str, String> = BTreeMap::new();
    let mut session: BTreeMap<&str, Group> = BTreeMap::new();
    let mut volume: BTreeMap<&str, Group> = BTreeMap::new();
    for (n, f) in (1..).zip(formats) {
        let j = f.jurisdiction.as_str();
        if !f.section.starts_with("T3") || skip_jurisdiction(j) {
            continue;
        }
        names.entry(j).or_insert_with(|| f.state_name());
        for alt in f.form.split('|') {
            let body = strip_parens(alt);
            if f.kind == "session_law" {
                let Some(m) = session_re.captures(body) else {
                    continue;
                };
                let series = m.name("series").map_or("", |s| s.as_str());
                // D.C. lists the federal Statutes at Large; that has its own rule.
                if series == "Stat." {
                    continue;
                }
                let vol = m.name("lead").is_some_and(|l| l.as_str() == "<vol. no.>");
                let g = session.entry(j).or_default();
                g.series.push(series.to_string());
                g.vol |= vol;
                if g.add_line(n, f) {
                    let lead = if vol { "17" } else { "2013" };
                    g.made.push(format!("{lead} {series} 77"));
                }
            } else if f.kind == "regulation" {
                let Some(m) = volume_re.captures(body) else {
                    continue;
                };
                let series = m.name("series").map_or("", |s| s.as_str());
                let g = volume.entry(j).or_default();
                g.series.push(series.to_string());
                if g.add_line(n, f) {
                    g.made.push(format!("12 {series} 77"));
                }
            }
        }
    }
    let mut out = Vec::new();
    for (j, g) in &session {
        let name = names.get(j).map_or("", String::as_str);
        let s0 = first_sorted(&g.series);
        let negs = vec![
            format!("The {s0} volumes are shelved by year."),
            format!("In {name}, 12 session laws took effect in 2015."),
            format!("Page 2351 of the {name} laws index is blank."),
        ];
        let spec = Spec {
            kind: "session_law",
            jurisdiction: j,
            id: format!("{j}.session_law.laws"),
            lines: &g.lines,
            encodes: format!("Indigo Book T3 {name}: Session laws"),
            fields: Fields {
                normalize: "{year} {series} {page}",
                ..FORM_FIELDS
            },
            priority: 62,
        };
        let pattern = session_pattern(&g.series, g.vol);
        out.extend(build(spec, pattern, &g.examples, &g.made, negs)?);
    }
    for (j, g) in &volume {
        let name = names.get(j).map_or("", String::as_str);
        let s0 = first_sorted(&g.series);
        let negs = vec![
            format!("The {s0} is published monthly."),
            format!("Volume 12 of the {name} register has 77 pages."),
            format!("In 2015 the {s0} had no issue 12."),
        ];
        let spec = Spec {
            kind: "regulation",
            jurisdiction: j,
            id: format!("{j}.regulation.volume"),
            lines: &g.lines,
            encodes: format!("Indigo Book T3 {name}: Administrative register and compilation"),
            fields: Fields {
                normalize: "{volume} {series} {page}",
                ..FORM_FIELDS
            },
            priority: 62,
        };
        let pattern = volume_pattern(&g.series);
        out.extend(build(spec, pattern, &g.examples, &g.made, negs)?);
    }
    Ok(out)
}

/// Inventory lines the fallback rules are for: missed by the last coverage
/// run, or found by a rule written here (so re-running after the rules land
/// writes the same files).
fn targeted(coverage: Option<&str>) -> Result<BTreeSet<usize>, String> {
    let Some(text) = coverage else {
        return Ok(BTreeSet::new());
    };
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("{COVERAGE_JSON}: {e}"))?;
    let own = [".indigo_forms ", ".indigo_reporters "];
    let lines = v
        .get("lines")
        .and_then(serde_json::Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    Ok(lines
        .iter()
        .filter(|l| {
            let ours = l["examples"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|e| e["found"].as_array().into_iter().flatten())
                .filter_map(serde_json::Value::as_str)
                .any(|x| own.iter().any(|o| x.contains(o)));
            l["detected"] != serde_json::Value::Bool(true) || ours
        })
        .filter_map(|l| l["line"].as_u64())
        .filter_map(|n| usize::try_from(n).ok())
        .collect())
}

/// `D.C.` -> `d_c`.
fn slug(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::new();
    for ch in lower.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Nominative-parallel rules (one per official reporter, from every matching
/// T1/T3 form) and, per jurisdiction, one rule over the simple reporters the
/// coverage run missed (reporters-db lacks them).
fn case_rules(formats: &[Format], missed: &BTreeSet<usize>) -> Result<Vec<Rule>, String> {
    let nominative_re = compile(
        r"^<case name>, <vol> (?P<official>[^()<>]+?) \((?:<(?:nominative vol|n)>\s)?(?:& \d+ )?(?P<nominative>[^()<>]+)\) <page>",
    )?;
    let simple_re = compile(r"^<case name>, <vol> (?P<reporter>[^()<>|,]+?) <page>, <pin> \(")?;
    let mut nominative: BTreeMap<String, (Group, String)> = BTreeMap::new();
    let mut simple: BTreeMap<&str, Group> = BTreeMap::new();
    for (n, f) in (1..).zip(formats) {
        if f.kind != "case" {
            continue;
        }
        let form = f.form.as_str();
        // The D.C. row lists three nominatives in one slot.
        let (names, official): (Vec<String>, Option<&str>) =
            if form.contains("(Mackey|MacArth.|Cranch <nominative vol>)") {
                (
                    vec!["Mackey".into(), "MacArth.".into(), "Cranch".into()],
                    Some("D.C."),
                )
            } else {
                let m = nominative_re.captures(form);
                let nom = m
                    .as_ref()
                    .and_then(|m| m.name("nominative"))
                    .map(|x| x.as_str());
                let names = nom
                    .filter(|n| !NOT_NOMINATIVE.contains(n))
                    .map(|n| vec![n.to_string()])
                    .unwrap_or_default();
                let official = m
                    .as_ref()
                    .and_then(|m| m.name("official"))
                    .map(|x| x.as_str());
                (names, official)
            };
        if let (false, Some(official)) = (names.is_empty(), official) {
            let (g, j) = nominative.entry(official.to_string()).or_default();
            g.series.extend(names);
            if j.is_empty() {
                // Official reporters' jurisdictions where the line says us.fed.
                *j = match official {
                    "U.S." => "us.scotus",
                    "D.C." => "us.dc",
                    _ => f.jurisdiction.as_str(),
                }
                .to_string();
            }
            g.lines.push(n);
            g.examples.extend(f.examples.iter().cloned());
            continue;
        }
        let Some(m) = simple_re.captures(form) else {
            continue;
        };
        if missed.contains(&n) && !skip_jurisdiction(&f.jurisdiction) {
            let g = simple.entry(&f.jurisdiction).or_default();
            g.series
                .push(m.name("reporter").map_or("", |r| r.as_str()).to_string());
            g.lines.push(n);
            g.examples.extend(f.examples.iter().cloned());
            g.name = f.state_name();
        }
    }
    let mut out = Vec::new();
    for (official, (g, j)) in &nominative {
        let pattern = format!(
            r"\b(?P<volume>[0-9]{{1,3}})\s(?P<reporter>{})\s\((?:(?P<nominative_volume>[0-9]{{1,2}}(?:\s?&\s?[0-9]{{1,2}})?)\s)?(?P<nominative>{})\)\s(?P<page>[0-9]{{1,4}})\b",
            lit(official),
            series_alt(&g.series)
        );
        let n0 = first_sorted(&g.series);
        let negs = vec![
            format!("Volume 12 of the {official} reports is missing."),
            format!("The {n0} reports cover {official} cases from 1800 to 1850."),
            format!("See {official} ({n0}) generally."),
        ];
        let first = g.series.first().map_or("", String::as_str);
        let made = [format!("Doe v. Roe, 12 {official} (2 {first}) 345 (1850)")];
        let spec = Spec {
            kind: "case",
            jurisdiction: j,
            id: format!("{j}.case.nominative_{}", slug(official)),
            lines: &g.lines,
            encodes: format!("Indigo Book T1/T3: {official} with nominative reporters"),
            fields: CASE_FIELDS,
            priority: 62,
        };
        out.extend(build(spec, pattern, &g.examples, &made, negs)?);
    }
    for (j, g) in &simple {
        let pattern = format!(
            r"\b(?P<volume>[0-9]{{1,3}})\s(?P<reporter>{})\s(?P<page>[0-9]{{1,5}})\b",
            series_alt(&g.series)
        );
        let n0 = first_sorted(&g.series);
        let negs = vec![
            format!("Volume 12 of the {n0} is missing."),
            format!("The {n0} printed 345 opinions."),
            format!("In {}, the {n0} began in 1993.", g.name),
        ];
        let made: Vec<String> = g
            .series
            .iter()
            .map(|r| format!("Doe v. Roe, 12 {r} 345 (1990)"))
            .collect();
        let spec = Spec {
            kind: "case",
            jurisdiction: j,
            id: format!("{j}.case.indigo_reporters"),
            lines: &g.lines,
            encodes: format!("Indigo Book T3 {}: reporters", g.name),
            fields: CASE_FIELDS,
            priority: 62,
        };
        out.extend(build(spec, pattern, &g.examples, &made, negs)?);
    }
    Ok(out)
}

/// The placeholder tokens of a form, as byte ranges: `<...>`, `§`, and `x`
/// chains (`x-x`, `x.x`) not inside a word (Python's
/// `<[^>]+>|(?<![A-Za-z])x(?:[-.:]x)*(?![A-Za-z])|§`).
fn tokens(body: &str) -> Vec<(usize, usize)> {
    let b = body.as_bytes();
    let alpha = |i: usize| b.get(i).is_some_and(u8::is_ascii_alphabetic);
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b.get(i) == Some(&b'<') {
            if let Some(len) = body.get(i + 1..).and_then(|r| r.find('>')) {
                if len > 0 {
                    out.push((i, i + len + 2));
                    i += len + 2;
                    continue;
                }
            }
        }
        if b.get(i) == Some(&b'x') && (i == 0 || !alpha(i - 1)) {
            // Greedy `x([-.:]x)*`, backing off to the longest end not
            // followed by a letter.
            let mut ends = vec![i + 1];
            let mut e = i + 1;
            while matches!(b.get(e), Some(b'-' | b'.' | b':')) && b.get(e + 1) == Some(&b'x') {
                e += 2;
                ends.push(e);
            }
            if let Some(&end) = ends.iter().rev().find(|&&e| !alpha(e)) {
                out.push((i, end));
                i = end;
                continue;
            }
        }
        if body.get(i..).is_some_and(|r| r.starts_with('§')) {
            out.push((i, i + '§'.len_utf8()));
            i += '§'.len_utf8();
            continue;
        }
        i += body
            .get(i..)
            .and_then(|r| r.chars().next())
            .map_or(1, char::len_utf8);
    }
    out
}

/// One Indigo form (trailing parentheticals and `[...]` options removed) as
/// a regex without named groups.
fn form_regex(form: &str, options: &Regex) -> String {
    let stripped = options.replace_all(strip_parens(form), "");
    let body = stripped.trim().trim_end_matches(',');
    let mut out = String::new();
    let mut pos = 0;
    for (start, end) in tokens(body) {
        out.push_str(&lit(body.get(pos..start).unwrap_or_default()));
        let t = body.get(start..end).unwrap_or_default();
        if t == "§" {
            out.push_str("§§?");
        } else if t.starts_with('<')
            && !["no.", "title", "section", "page", "vol"]
                .iter()
                .any(|w| t.contains(w))
        {
            out.push_str(WORDS);
        } else {
            out.push_str(NUM);
        }
        pos = end;
    }
    out.push_str(&lit(body.get(pos..).unwrap_or_default()));
    out
}

/// `text` with every digit moved by three: a new instance of the form.
fn shifted(text: &str) -> String {
    text.chars()
        .map(|c| match c.to_digit(10).filter(|_| c.is_ascii_digit()) {
            Some(d) => char::from_digit((d + 3) % 10, 10)
                .filter(|&c| c != '0')
                .unwrap_or('1'),
            None => c,
        })
        .collect()
}

/// Per jurisdiction and kind, one rule over every statute or regulation
/// form the coverage run missed.
fn form_rules(formats: &[Format], missed: &BTreeSet<usize>) -> Result<Vec<Rule>, String> {
    let options = compile(r"\[[^\]]*\]")?;
    let mut groups: BTreeMap<(&str, &str), Group> = BTreeMap::new();
    for (n, f) in (1..).zip(formats) {
        if !missed.contains(&n)
            || FORM_SKIP.contains(&n)
            || !(f.kind == "statute" || f.kind == "regulation")
            || f.jurisdiction == "us"
        {
            continue;
        }
        for alt in f.form.split('|') {
            let lead = alt.trim_start();
            if ["§", "Id.", "["].iter().any(|p| lead.starts_with(p))
                || alt.contains("<name of act>")
                || alt.contains("U.S.C. §")
            {
                continue;
            }
            let rx = form_regex(alt, &options);
            let g = groups.entry((&f.jurisdiction, &f.kind)).or_default();
            if !g.series.contains(&rx) {
                g.series.push(rx);
            }
            g.add_line(n, f);
        }
    }
    let mut out = Vec::new();
    for ((j, kind), g) in &groups {
        let alts: Vec<&str> = by_length(&g.series)
            .into_iter()
            .map(String::as_str)
            .collect();
        let pattern = format!(r"\b(?:{})", alts.join("|"));
        let name = formats
            .iter()
            .find(|f| f.jurisdiction == *j && f.section.starts_with("T3"))
            .map_or_else(|| (*j).to_string(), Format::state_name);
        let negs = vec![
            format!("The {name} code was amended in 2015."),
            format!("Section 12 of the {name} rules is long."),
            format!("Title five of the {name} compilation has no number."),
        ];
        let made: Vec<String> = g.examples.iter().map(|e| shifted(e)).collect();
        let what = if *kind == "statute" {
            "Statutory compilations"
        } else {
            "Administrative compilation"
        };
        let spec = Spec {
            kind,
            jurisdiction: j,
            id: format!("{j}.{kind}.indigo_forms"),
            lines: &g.lines,
            encodes: format!("Indigo Book T3 {name}: {what}"),
            fields: FORM_FIELDS,
            // Below the specific hand-written rules: these forms are a fallback.
            priority: 45,
        };
        out.extend(build(spec, pattern, &g.examples, &made, negs)?);
    }
    Ok(out)
}

/// Every generated rule file as `(path under the rules dir, contents)`,
/// from the inventory and (optionally) the coverage report.
pub fn generate(jsonl: &str, coverage: Option<&str>) -> Result<Vec<(String, String)>, String> {
    let formats = jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Format>(l).map_err(|e| format!("{INDIGO_JSONL}: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let missed = targeted(coverage)?;
    let mut rules = series_rules(&formats)?;
    rules.extend(case_rules(&formats, &missed)?);
    rules.extend(form_rules(&formats, &missed)?);
    rules
        .iter()
        .map(|r| Ok((format!("{}/{}.json", r.kind, r.id), dump(r)?)))
        .collect()
}

/// Committed rule files (under the rules dir) whose note says this
/// generator wrote them.
fn authored(rules: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(kinds) = std::fs::read_dir(rules) else {
        return out;
    };
    for kind in kinds.flatten() {
        for file in std::fs::read_dir(kind.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            let text = std::fs::read_to_string(file.path()).unwrap_or_default();
            if text.contains(AUTHORED_BY) {
                out.push(format!(
                    "{}/{}",
                    kind.file_name().to_string_lossy(),
                    file.file_name().to_string_lossy()
                ));
            }
        }
    }
    out.sort();
    out
}

/// Regenerate the Indigo rules under `root` (or, with `check`, only compare).
/// Returns the number of rules and the stale paths: files that differ from
/// a fresh run, plus generator-authored files it no longer writes (with
/// `check` they are only reported; otherwise they are removed).
pub fn run(root: &Path, check: bool) -> Result<(usize, Vec<String>), String> {
    let jsonl = std::fs::read_to_string(root.join(INDIGO_JSONL))
        .map_err(|e| format!("{INDIGO_JSONL}: {e}"))?;
    let coverage = std::fs::read_to_string(root.join(COVERAGE_JSON)).ok();
    let files = generate(&jsonl, coverage.as_deref())?;
    let dir = root.join(RULES_DIR);
    let mut stale = Vec::new();
    for (rel, text) in &files {
        let path = dir.join(rel);
        if std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()) {
            continue;
        }
        stale.push(rel.clone());
        if !check {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    for rel in authored(&dir) {
        if files.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        if !check {
            let path = dir.join(&rel);
            std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        stale.push(rel);
    }
    Ok((files.len(), stale))
}

/// `xtask-rules indigo [--check]`: `(ok, stdout, stderr)`.
pub fn cli(root: &Path, check: bool) -> (bool, String, String) {
    match run(root, check) {
        Err(e) => (false, String::new(), format!("xtask-rules indigo: {e}\n")),
        Ok((n, stale)) if check && !stale.is_empty() => (
            false,
            String::new(),
            format!(
                "stale Indigo rules (run `cargo run -p xtask-rules -- indigo`):\n{}",
                stale.iter().map(|s| format!("  {s}\n")).collect::<String>()
            ) + &format!("{n} rules\n"),
        ),
        Ok((n, stale)) => (
            true,
            format!("{n} Indigo rules in {RULES_DIR}, {} updated\n", stale.len()),
            String::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn line(section: &str, j: &str, kind: &str, form: &str, examples: &[&str]) -> String {
        serde_json::json!({
            "section": section, "jurisdiction": j, "kind": kind, "form": form,
            "examples": examples,
        })
        .to_string()
    }

    fn inventory() -> String {
        [
            line(
                "T3 Ohio: Session laws",
                "us.oh",
                "session_law",
                "<year> Ohio Laws <page> (West)",
                &["2015 Ohio Laws 2351", "2014 Ohio Laws 12"],
            ),
            line(
                "T3 Ohio: Session laws",
                "us.oh",
                "session_law",
                "<vol. no.> Ohio Leg. Serv. <page>|<year> Stat. <page>|nothing",
                &["5 Ohio Leg. Serv. 9"],
            ),
            line(
                "T3 Ohio: Register",
                "us.oh",
                "regulation",
                "<vol. no.> Ohio Reg. <page no.>|bad",
                &["3 Ohio Reg. 4", "5 Ohio Reg. 6"],
            ),
            line(
                "T1 Fed",
                "us.fed",
                "session_law",
                "<year> Fed. Laws <page>",
                &[],
            ),
            line(
                "T3 Ohio: Case",
                "us.oh",
                "case",
                "<case name>, <vol> Ohio (<n> Wilc.) <page>",
                &["A v. B, 1 Ohio (2 Wilc.) 3", "C v. D, 4 Ohio (5 Wilc.) 6"],
            ),
            line(
                "T3 Ohio: Case",
                "us.oh",
                "case",
                "<case name>, <vol> Ohio (CCH) <page>",
                &[],
            ),
            line(
                "T3 Ohio: Case",
                "us.oh",
                "case",
                "<case name>, <vol> Ohio Dec. <page>, <pin> (<year>)",
                &[
                    "A v. B, 1 Ohio Dec. 3 (1890)",
                    "C v. D, 4 Ohio Dec. 6 (1891)",
                ],
            ),
            line("T3 Ohio: Case", "us.oh", "case", "no shape", &[]),
            line(
                "T3 Ohio: Code",
                "us.oh",
                "statute",
                "Ohio Code x-x § <section> (<year>)|§ x|<name of act> x",
                &["Ohio Code 1-2 § 3", "Ohio Code 4-5 § 6"],
            ),
            line(
                "T3 Ohio: Code",
                "us.oh",
                "statute",
                "Ohio Code x-x § <section> [opt]",
                &["Ohio Code 7-8 § 9"],
            ),
            line(
                "T3 Ohio: Admin",
                "us.oh",
                "regulation",
                "Ohio Adm. <agency> x",
                &["Ohio Adm. Tax Dep 1"],
            ),
            line("R1 Fed", "us", "statute", "x U.S.C. § x", &[]),
        ]
        .join("\n")
            + "\n\n"
    }

    fn coverage(missed: &[usize], ours: usize) -> String {
        let mut lines: Vec<serde_json::Value> = (1..=12)
            .map(|n| serde_json::json!({"line": n, "detected": !missed.contains(&n), "examples": []}))
            .collect();
        lines[ours - 1]["examples"] =
            serde_json::json!([{"found": ["case us.oh.case.indigo_reporters \"x\""]}]);
        serde_json::json!({ "lines": lines }).to_string()
    }

    #[test]
    fn generates_every_shape_from_a_small_inventory() {
        let cov = coverage(&[9, 10, 11, 12], 7);
        let files = generate(&inventory(), Some(&cov)).unwrap();
        let names: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            names,
            [
                "session_law/us.oh.session_law.laws.json",
                "regulation/us.oh.regulation.volume.json",
                "case/us.oh.case.nominative_ohio.json",
                "case/us.oh.case.indigo_reporters.json",
                "statute/us.oh.statute.indigo_forms.json",
            ],
            "{files:#?}"
        );
        let session = &files[0].1;
        assert!(session.contains(r#""spans": [[4, 23]],"#), "{session}");
        assert!(session.contains(r#""groups": {"#), "{session}");
        assert!(session.contains("(?P<volume>[0-9]{1,3})"), "{session}");
        assert!(session.contains(AUTHORED_BY), "{session}");
        let statute = &files[4].1;
        assert!(statute.contains("§§?"), "{statute}");
        assert!(statute.contains("See Ohio Code 4-5 § 6."), "{statute}");
        assert!(statute.contains("No authority key"), "{statute}");
        // Without a coverage report only the series and nominative rules remain.
        let files = generate(&inventory(), None).unwrap();
        assert_eq!(files.len(), 3);
    }

    #[test]
    fn errors_are_reported() {
        assert!(generate("not json\n", None).is_err());
        assert!(generate(&inventory(), Some("not json")).is_err());
        assert!(compile("(").is_err());
        assert_eq!(targeted(Some("{}")).unwrap(), BTreeSet::new());
    }

    #[test]
    fn helpers_match_the_python_original() {
        assert_eq!(lit("Ala. Laws"), r"Ala\.\s?Laws");
        assert_eq!(
            lit("N.J. Reg's & x-y #"),
            r"N\.J\.\s?Reg['’]s\s\&\sx\-y\s\#"
        );
        assert_eq!(strip_parens(" A (b (c)) (d) "), "A");
        assert_eq!(strip_parens("A b)"), "A b)");
        assert_eq!(shifted("a 1907-x"), "a 4231-x");
        assert_eq!(slug("S.C. Eq."), "s_c_eq");
        assert_eq!(
            by_length(&["b".into(), "aa".into(), "a".into(), "b".into()]),
            ["aa", "a", "b"]
        );
        let body = "x-x <a> § <> ax xb x-xa x";
        let toks: Vec<&str> = tokens(body).into_iter().map(|(s, e)| &body[s..e]).collect();
        assert_eq!(toks, ["x-x", "<a>", "§", "x", "x"]);
        let options = compile(r"\[[^\]]*\]").unwrap();
        assert_eq!(
            form_regex("Code <agency> <sec. no.> [x], (West)", &options),
            // As in Python, `rstrip(",")` leaves the space before `[x]`.
            format!(r"Code\s{WORDS}\s{NUM}\s")
        );
    }

    #[test]
    fn run_writes_checks_and_prunes() {
        let root = std::env::temp_dir().join(format!("xtask-indigo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // No inventory: an error.
        let (ok, _, err) = cli(&root, true);
        assert!(!ok && err.contains(INDIGO_JSONL), "{err}");
        std::fs::create_dir_all(root.join("corpus/formats")).unwrap();
        std::fs::write(root.join(INDIGO_JSONL), inventory()).unwrap();
        let stray = root
            .join(RULES_DIR)
            .join("statute/us.zz.statute.indigo_forms.json");
        std::fs::create_dir_all(stray.parent().unwrap()).unwrap();
        std::fs::write(&stray, format!("{{\"note\": \"{AUTHORED_BY}\"}}")).unwrap();
        std::fs::write(stray.with_file_name("hand.json"), "{}").unwrap();
        let (ok, _, err) = cli(&root, true);
        assert!(!ok && err.contains("us.zz.statute"), "{err}");
        let (ok, out, _) = cli(&root, false);
        assert!(ok && out.contains("3 Indigo rules"), "{out}");
        assert!(!stray.exists());
        let (ok, out, err) = cli(&root, true);
        assert!(ok && out.contains("0 updated"), "{out} {err}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
