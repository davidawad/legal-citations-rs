//! Format coverage: run detection over every example of a citation-format
//! inventory (`corpus/formats/indigo.jsonl`, one line per Indigo Book
//! format) and say, per line, whether each example was found.
//!
//! An example is **detected** when citations of a compatible kind (see
//! [`accepts`]) together span its numbered core ([`core()`]): from the first
//! digit, `§` or `¶` to the last digit before any trailing parenthetical
//! (the whole trimmed text when it has no digit). A case name or act title
//! before the core, and a `(West 2014)` or `(Ala. 1914)` after it, need not
//! be covered; volume, reporter, page, pincite and section must be. A cite
//! linked as subsequent history continues the run of its decision. A format
//! is detected when every one of its examples is.

use crate::rule::Kind;
use crate::{detect, Citation};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One inventory line.
#[derive(Clone, Debug, Deserialize)]
pub struct FormatLine {
    /// Indigo section (`T3 Ohio: ...`, `R18.1 ...`).
    pub section: String,
    /// Dotted jurisdiction.
    pub jurisdiction: String,
    /// Kind name (`case`, `statute`, ...).
    pub kind: String,
    /// The format as Indigo prints it.
    pub form: String,
    /// Example citations.
    pub examples: Vec<String>,
}

/// One example's result.
#[derive(Clone, Debug, Serialize)]
pub struct ExampleResult {
    /// The example text.
    pub text: String,
    /// Covered by citations of a compatible kind.
    pub detected: bool,
    /// What detection found: `kind rule_id "text"`.
    pub found: Vec<String>,
}

/// One inventory line's result.
#[derive(Clone, Debug, Serialize)]
pub struct LineResult {
    /// 1-based line number in the inventory.
    pub line: usize,
    /// Indigo section.
    pub section: String,
    /// Dotted jurisdiction.
    pub jurisdiction: String,
    /// Kind name.
    pub kind: String,
    /// Every example detected.
    pub detected: bool,
    /// Per-example results.
    pub examples: Vec<ExampleResult>,
}

/// Counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
    /// Formats (or examples) seen.
    pub total: usize,
    /// Detected.
    pub detected: usize,
    /// Missed.
    pub missed: usize,
}

impl Tally {
    fn add(&mut self, detected: bool) {
        self.total += 1;
        if detected {
            self.detected += 1;
        } else {
            self.missed += 1;
        }
    }
}

/// The whole run.
#[derive(Clone, Debug, Serialize)]
pub struct CoverageReport {
    /// Format lines.
    pub formats: Tally,
    /// Individual examples.
    pub examples: Tally,
    /// Format lines by kind.
    pub by_kind: BTreeMap<String, Tally>,
    /// Format lines by jurisdiction.
    pub by_jurisdiction: BTreeMap<String, Tally>,
    /// Every line, in inventory order.
    pub lines: Vec<LineResult>,
}

/// The citation kinds that count for an inventory kind: short forms count
/// for the kind they shorten (Indigo lists `Id.` and `545 U.S. at 8` under
/// case, `Id. § 7102(26)` and `§ 1983` under statute, `Skinner, supra, at
/// 21` under secondary sources), a case-shaped cite counts for an agency
/// decision (`55 Agric. Dec. 1121`, `11 E.A.D. 622`: the reporters-db rules
/// for agency reporters are kind `case`), and agency decisions and agency
/// manuals count for each other (Indigo's T2 lines mix them: `I.R.S. Notice
/// 2014-21` sits with Treasury Orders). `None` for an
/// unknown kind.
pub fn accepts(kind: &str) -> Option<Vec<Kind>> {
    let k = Kind::parse(kind)?;
    Some(match k {
        Kind::Case => vec![Kind::Case, Kind::CaseShort, Kind::Id],
        Kind::Statute | Kind::Regulation => vec![k, Kind::Id, Kind::StatuteShort],
        Kind::Secondary => vec![k, Kind::Supra, Kind::Id],
        Kind::AgencyDecision => vec![k, Kind::AgencyManual, Kind::Case],
        Kind::AgencyManual => vec![k, Kind::AgencyDecision],
        _ => vec![k],
    })
}

/// `text` without its trailing parentheticals (`(West 2014)`, `(Ala. 1914)
/// (No. 3591)`), balanced from the end. Only a parenthetical set off by a
/// space counts: `§ 501(c)(3)` keeps its subsections.
fn strip_trailing_parens(text: &str) -> &str {
    let mut t = text.trim_end().trim_end_matches(['.', ',', ';']);
    while t.ends_with(')') {
        let mut depth = 0usize;
        let mut open = None;
        for (i, c) in t.char_indices().rev() {
            match c {
                ')' => depth += 1,
                '(' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        open = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        match open {
            Some(i) if t.get(..i).is_some_and(|h| h.ends_with(char::is_whitespace)) => {
                t = t.get(..i).unwrap_or("").trim_end();
            }
            _ => break,
        }
    }
    t
}

/// The byte range of `example` that must be covered: from the first
/// comma-separated part that opens with a digit, `§` or `¶` (else the first
/// such character) to its last digit before any trailing parenthetical (a
/// code's `(West 2014)` year and publisher is not part of what rules match).
pub fn core(example: &str) -> (usize, usize) {
    let numbered = |c: char| c.is_ascii_digit() || c == '§' || c == '¶';
    let body = strip_trailing_parens(example);
    let body = if body.contains(|c: char| c.is_ascii_digit()) {
        body
    } else {
        example
    };
    // A name before the cite may hold digits (`Communications Act of 1934,
    // 47 U.S.C. § 223`): start at the first comma-separated part that opens
    // with a digit or `§` when there is one.
    let mut offset = 0;
    let mut first = None;
    for part in body.split(',') {
        let lead = part.len() - part.trim_start().len();
        if part.trim_start().starts_with(numbered) {
            first = Some(offset + lead);
            break;
        }
        offset += part.len() + 1;
    }
    match (
        first.or_else(|| body.find(numbered)),
        body.rfind(|c: char| c.is_ascii_digit()),
    ) {
        (Some(a), Some(b)) => (a, b + 1),
        (Some(a), None) => (
            a,
            a + body
                .get(a..)
                .and_then(|t| t.chars().next())
                .map_or(1, char::len_utf8),
        ),
        _ => {
            let start = example.len() - example.trim_start().len();
            (start, example.trim_end().len())
        }
    }
}

/// True when citations of a kind in `kinds` together span `example`'s core.
/// Subsequent history (`, aff’d, 793 F.3d 1169`) continues the run of the
/// decision it is linked to, phrase included.
pub fn covers(example: &str, cites: &[Citation], kinds: &[Kind]) -> bool {
    let (a, b) = core(example);
    let mut spans: Vec<(usize, usize)> = cites
        .iter()
        .filter(|c| kinds.contains(&c.kind))
        .map(|c| {
            let decision = c
                .details
                .history_of()
                .and_then(|p| cites.get(p))
                .filter(|p| kinds.contains(&p.kind));
            (
                decision
                    .map_or(c.full_span.start, |p| {
                        p.full_span.end.min(c.full_span.start)
                    })
                    .min(c.span.start),
                c.full_span.end.max(c.span.end),
            )
        })
        .collect();
    spans.sort_unstable();
    // Merge spans that touch (or are separated only by `, ` / `; `), then
    // ask whether one merged run covers the core.
    let mut run: Option<(usize, usize)> = None;
    for (s, e) in spans {
        run = match run {
            Some((rs, re)) if s <= re + 2 => Some((rs, re.max(e))),
            Some((rs, re)) if rs <= a && re >= b => return true,
            _ => Some((s, e)),
        };
    }
    run.is_some_and(|(s, e)| s <= a && e >= b)
}

fn describe(c: &Citation) -> String {
    format!("{} {} {:?}", c.kind, c.rule_id, c.text)
}

/// Run detection over every line of an inventory (JSON Lines).
pub fn run(jsonl: &str) -> Result<CoverageReport, String> {
    let mut report = CoverageReport {
        formats: Tally::default(),
        examples: Tally::default(),
        by_kind: BTreeMap::new(),
        by_jurisdiction: BTreeMap::new(),
        lines: Vec::new(),
    };
    for (i, raw) in jsonl.lines().enumerate() {
        if raw.trim().is_empty() {
            continue;
        }
        let line: FormatLine =
            serde_json::from_str(raw).map_err(|e| format!("line {}: {e}", i + 1))?;
        let kinds = accepts(&line.kind)
            .ok_or_else(|| format!("line {}: unknown kind {:?}", i + 1, line.kind))?;
        let examples: Vec<ExampleResult> = line
            .examples
            .iter()
            .map(|ex| {
                let cites = detect(ex);
                ExampleResult {
                    text: ex.clone(),
                    detected: covers(ex, &cites, &kinds),
                    found: cites.iter().map(describe).collect(),
                }
            })
            .collect();
        for e in &examples {
            report.examples.add(e.detected);
        }
        let detected = !examples.is_empty() && examples.iter().all(|e| e.detected);
        report.formats.add(detected);
        report
            .by_kind
            .entry(line.kind.clone())
            .or_default()
            .add(detected);
        report
            .by_jurisdiction
            .entry(line.jurisdiction.clone())
            .or_default()
            .add(detected);
        report.lines.push(LineResult {
            line: i + 1,
            section: line.section,
            jurisdiction: line.jurisdiction,
            kind: line.kind,
            detected,
            examples,
        });
    }
    Ok(report)
}

fn pct(t: &Tally) -> String {
    if t.total == 0 {
        return "-".into();
    }
    // Integer per-mille keeps the rendering exact and float-free.
    let permille = t.detected * 1000 / t.total;
    format!("{}.{}%", permille / 10, permille % 10)
}

fn table(title: &str, rows: &BTreeMap<String, Tally>) -> String {
    let mut s = format!("| {title} | formats | detected | missed | detected % |\n| --- | ---: | ---: | ---: | ---: |\n");
    for (k, t) in rows {
        s.push_str(&format!(
            "| {k} | {} | {} | {} | {} |\n",
            t.total,
            t.detected,
            t.missed,
            pct(t)
        ));
    }
    s
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Markdown rendering: totals, by kind, by jurisdiction, then every miss.
pub fn markdown(r: &CoverageReport, source: &str) -> String {
    let mut s = format!(
        "# Indigo Book format coverage\n\nGenerated by `cite-detect coverage {source}`; do not hand-edit.\n\
         An example is detected when citations of a compatible kind span its numbered core\n\
         (first digit, `§` or `¶` through the last digit before any trailing parenthetical); a\n\
         format is detected when all of its examples are. Short forms count for the kind they\n\
         shorten and case-shaped cites for agency decisions. See\n\
         `crates/legal-citations/src/coverage.rs`.\n\n\
         | | total | detected | missed | detected % |\n| --- | ---: | ---: | ---: | ---: |\n\
         | formats | {} | {} | {} | {} |\n| examples | {} | {} | {} | {} |\n\n",
        r.formats.total,
        r.formats.detected,
        r.formats.missed,
        pct(&r.formats),
        r.examples.total,
        r.examples.detected,
        r.examples.missed,
        pct(&r.examples)
    );
    s.push_str("## By kind\n\n");
    s.push_str(&table("kind", &r.by_kind));
    s.push_str("\n## By jurisdiction\n\n");
    s.push_str(&table("jurisdiction", &r.by_jurisdiction));
    let misses: Vec<&LineResult> = r.lines.iter().filter(|l| !l.detected).collect();
    s.push_str(&format!(
        "\n## Misses ({})\n\n| line | kind | jurisdiction | section | missed examples | found instead |\n| ---: | --- | --- | --- | --- | --- |\n",
        misses.len()
    ));
    for l in misses {
        let missed: Vec<&ExampleResult> = l.examples.iter().filter(|e| !e.detected).collect();
        let texts: Vec<String> = missed
            .iter()
            .map(|e| format!("`{}`", cell(&e.text)))
            .collect();
        let found: Vec<String> = missed
            .iter()
            .flat_map(|e| e.found.iter())
            .map(|f| cell(f))
            .collect();
        s.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            l.line,
            l.kind,
            l.jurisdiction,
            cell(&l.section),
            if texts.is_empty() {
                "(no examples)".to_string()
            } else {
                texts.join("<br>")
            },
            if found.is_empty() {
                "nothing".to_string()
            } else {
                found.join("<br>")
            }
        ));
    }
    s
}

/// The regression baseline: counts that must not drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    /// Detected formats.
    pub formats_detected: usize,
    /// Detected examples.
    pub examples_detected: usize,
}

impl CoverageReport {
    /// This run's counts as a baseline.
    pub fn baseline(&self) -> Baseline {
        Baseline {
            formats_detected: self.formats.detected,
            examples_detected: self.examples.detected,
        }
    }

    /// Ways this run falls below `base` (empty when it does not).
    pub fn regressions(&self, base: &Baseline) -> Vec<String> {
        let now = self.baseline();
        let mut out = Vec::new();
        if now.formats_detected < base.formats_detected {
            out.push(format!(
                "detected formats dropped: {} < baseline {}",
                now.formats_detected, base.formats_detected
            ));
        }
        if now.examples_detected < base.examples_detected {
            out.push(format!(
                "detected examples dropped: {} < baseline {}",
                now.examples_detected, base.examples_detected
            ));
        }
        out
    }
}

/// Markers around the generated table in `README.md`.
pub const TABLE_START: &str = "<!-- rules-table:start (cite-detect rules table) -->";
/// See [`TABLE_START`].
pub const TABLE_END: &str = "<!-- rules-table:end -->";

/// The README coverage table: one row per kind and family over the active
/// rules, with counts and up to six representative forms (each the span of
/// a rule's first positive example, hand-written rules first, spread over
/// the ids).
pub fn rules_table(reg: &crate::Registry) -> String {
    struct Row<'a> {
        rules: Vec<&'a crate::registry::Rule>,
    }
    let mut rows: BTreeMap<(String, String), Row<'_>> = BTreeMap::new();
    for (_, r) in reg.active() {
        rows.entry((
            r.def.kind.as_str().to_string(),
            r.def.family().as_str().to_string(),
        ))
        .or_insert(Row { rules: Vec::new() })
        .rules
        .push(r);
    }
    let form = |r: &crate::registry::Rule| {
        let p = r.def.examples.positive.first()?;
        let [s, e] = *p.spans.first()?;
        p.text
            .get(s..e)
            .map(|t| format!("`{}`", cell(t).replace('`', "'")))
    };
    let mut out = format!(
        "{TABLE_START}\n| Kind | Family | Rules | Generated | Verified | Jurisdictions | Representative forms |\n| --- | --- | ---: | ---: | ---: | ---: | --- |\n"
    );
    let mut total = (0, 0, 0);
    for ((kind, family), mut row) in rows {
        row.rules.sort_by(|a, b| a.def.id.cmp(&b.def.id));
        let generated = row
            .rules
            .iter()
            .filter(|r| r.path.contains("/generated/"))
            .count();
        let verified = row.rules.iter().filter(|r| r.def.provenance.opened).count();
        let jurisdictions: std::collections::BTreeSet<&str> = row
            .rules
            .iter()
            .map(|r| r.def.jurisdiction.as_str())
            .collect();
        let (hand, gen): (Vec<&crate::registry::Rule>, Vec<&crate::registry::Rule>) = row
            .rules
            .iter()
            .copied()
            .partition(|r| !r.path.contains("/generated/"));
        let mut picks: Vec<String> = Vec::new();
        for pool in [hand, gen] {
            let step = (pool.len() / 6).max(1);
            for r in pool.iter().step_by(step) {
                if picks.len() >= 6 {
                    break;
                }
                if let Some(f) = form(r) {
                    if !picks.contains(&f) {
                        picks.push(f);
                    }
                }
            }
        }
        total = (
            total.0 + row.rules.len(),
            total.1 + generated,
            total.2 + verified,
        );
        out.push_str(&format!(
            "| {kind} | {family} | {} | {generated} | {verified} | {} | {} |\n",
            row.rules.len(),
            jurisdictions.len(),
            picks.join(", ")
        ));
    }
    out.push_str(&format!(
        "| **all** | | **{}** | **{}** | **{}** | | |\n{TABLE_END}\n",
        total.0, total.1, total.2
    ));
    out
}
