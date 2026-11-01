//! Aggregate document scores into the gold report, the per-rule precision
//! table and the misses list, and render them.

use crate::score::{self, DetStatus, Detection, DocScore, Outcome};
use crate::stats::{Estimate, Tally};
use crate::LoadedDoc;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Who made the labels; printed at the top of every report.
pub const LABELER: &str = "Every gold label was made by an AI agent (Claude), reading each document in full; a second pass by the same model re-read each document for missed citations and convention errors. That is one rater, not two independent ones, so inter-rater agreement is unmeasured. Treat the numbers as a careful first pass, not ground truth.";

/// Minimum detections for a rule to get a per-rule precision row.
pub const PER_RULE_MIN_N: u64 = 5;

/// Run metadata.
#[derive(Clone, Debug, Serialize)]
pub struct RunInfo {
    /// Report date (`YYYY-MM-DD`).
    pub date: String,
    /// Source revision the tool was built from.
    pub revision: String,
    /// `legal_citations::registry().hash()`.
    pub registry_hash: String,
}

/// Recall, precision, exact-span rate and kind agreement for one slice.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Measures {
    /// Counted labels detected / counted labels.
    pub recall: Estimate,
    /// True positives / (true + false positives).
    pub precision: Estimate,
    /// Exact spans / detected labels.
    pub exact_span: Estimate,
    /// Detected labels whose primary detection has the label's kind.
    pub kind_agreement: Estimate,
}

#[derive(Clone, Copy, Debug, Default)]
struct Acc {
    recall: Tally,
    precision: Tally,
    exact: Tally,
    kind: Tally,
}

impl Acc {
    fn measures(&self) -> Measures {
        Measures {
            recall: self.recall.estimate(),
            precision: self.precision.estimate(),
            exact_span: self.exact.estimate(),
            kind_agreement: self.kind.estimate(),
        }
    }
    fn label(&mut self, outcome: Outcome, kind_match: Option<bool>) {
        self.recall.add(outcome.detected());
        if outcome.detected() {
            self.exact.add(outcome == Outcome::Exact);
        }
        if let Some(k) = kind_match {
            self.kind.add(k);
        }
    }
}

/// Corpus size.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CorpusSummary {
    /// Documents.
    pub documents: usize,
    /// Whitespace-separated words.
    pub words: usize,
    /// Bytes of text.
    pub bytes: usize,
    /// Counted labels.
    pub labels: usize,
    /// Optional (borderline) labels.
    pub optional: usize,
    /// Detections, catch-all excluded.
    pub detections: usize,
    /// Detections that overlap only optional labels (not scored).
    pub ignored: usize,
}

/// One document's row.
#[derive(Clone, Debug, Serialize)]
pub struct DocReport {
    /// Manifest id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Court or body.
    pub court: String,
    /// Report group.
    pub category: String,
    /// Source URL.
    pub url: String,
    /// Words.
    pub words: usize,
    /// Counted labels.
    pub labels: usize,
    /// Optional labels.
    pub optional: usize,
    /// Measures for this document.
    pub measures: Measures,
}

/// A missed or mis-spanned citation.
#[derive(Clone, Debug, Serialize)]
pub struct Miss {
    /// Document id.
    pub doc: String,
    /// Label start.
    pub start: usize,
    /// Label end.
    pub end: usize,
    /// Label text.
    pub text: String,
    /// Label kind.
    pub kind: String,
    /// Label jurisdiction.
    pub jurisdiction: String,
    /// Label form.
    pub form: String,
    /// What happened.
    pub outcome: Outcome,
    /// Texts of the detections assigned to it.
    pub detected: Vec<String>,
    /// Their rules.
    pub rules: Vec<String>,
    /// A catch-all hit overlaps the miss.
    pub catch_all: bool,
}

/// A detection that overlaps no label.
#[derive(Clone, Debug, Serialize)]
pub struct FalsePositive {
    /// Document id.
    pub doc: String,
    /// The detection.
    pub detection: Detection,
    /// Up to 60 bytes either side.
    pub context: String,
}

/// The gold report (`docs/measurements/<date>-gold.json`).
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Schema id.
    pub schema: &'static str,
    /// Run metadata.
    pub run: RunInfo,
    /// [`LABELER`].
    pub labeler: &'static str,
    /// Corpus size.
    pub corpus: CorpusSummary,
    /// Everything.
    pub overall: Measures,
    /// Counted labels by outcome.
    pub outcomes: BTreeMap<&'static str, u64>,
    /// Misses with a catch-all hit on them.
    pub catch_all_on_misses: u64,
    /// By kind (recall by label kind, precision by detected kind).
    pub by_kind: BTreeMap<String, Measures>,
    /// By jurisdiction group (first two segments: `us.fed`, `us.oh`).
    pub by_jurisdiction: BTreeMap<String, Measures>,
    /// By document category.
    pub by_category: BTreeMap<String, Measures>,
    /// Per document.
    pub documents: Vec<DocReport>,
    /// Every missed or mis-spanned label.
    pub misses: Vec<Miss>,
    /// Every false positive.
    pub false_positives: Vec<FalsePositive>,
}

/// Per-rule precision (`docs/measurements/<date>-per-rule.json`), for a
/// later step that writes the rules' `measured` blocks.
#[derive(Clone, Debug, Serialize)]
pub struct PerRule {
    /// Schema id.
    pub schema: &'static str,
    /// Run metadata.
    pub run: RunInfo,
    /// Rows have at least this many scored detections.
    pub min_n: u64,
    /// How to read it.
    pub note: &'static str,
    /// Rules with `n >= min_n`, by id.
    pub rules: Vec<RuleRow>,
    /// Rules that fired with fewer scored detections.
    pub below_min_n: Vec<String>,
}

/// One rule's precision.
#[derive(Clone, Debug, Serialize)]
pub struct RuleRow {
    /// Rule id.
    pub rule_id: String,
    /// Kind.
    pub kind: String,
    /// True positives.
    pub true_positives: u64,
    /// False positives.
    pub false_positives: u64,
    /// Precision with its Wilson interval.
    pub precision: Estimate,
}

/// The jurisdiction group: `us.fed.app.ca5` is `us.fed`, `us.oh.sup` is
/// `us.oh`, `foreign.gb` stays.
pub fn jurisdiction_group(j: &str) -> String {
    j.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// `text` around `start..end`, newlines shown as spaces.
pub fn context(text: &str, start: usize, end: usize) -> String {
    let floor = |mut i: usize| {
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let a = floor(start.saturating_sub(60));
    let b = floor((end + 60).min(text.len()));
    let pre = text.get(a..start).unwrap_or_default();
    let mid = text.get(start..end).unwrap_or_default();
    let post = text.get(end..b).unwrap_or_default();
    format!("{pre}[[{mid}]]{post}").replace(['\n', '\r'], " ")
}

/// Detect and score every document, then aggregate.
pub fn evaluate(docs: &[LoadedDoc], run: RunInfo) -> (Report, PerRule) {
    let mut corpus = CorpusSummary::default();
    let mut overall = Acc::default();
    let mut by_kind: BTreeMap<String, Acc> = BTreeMap::new();
    let mut by_jur: BTreeMap<String, Acc> = BTreeMap::new();
    let mut by_cat: BTreeMap<String, Acc> = BTreeMap::new();
    let mut outcomes: BTreeMap<&'static str, u64> =
        Outcome::ALL.iter().map(|o| (o.as_str(), 0)).collect();
    let mut rules: BTreeMap<String, (String, Tally)> = BTreeMap::new();
    let mut documents = Vec::new();
    let mut misses = Vec::new();
    let mut false_positives = Vec::new();
    let mut catch_all_on_misses = 0;

    for doc in docs {
        let (dets, catch) = score::detect(&doc.text);
        let DocScore {
            labels: results,
            detections: statuses,
        } = score::score(&doc.labels, &dets, &catch);
        let mut acc = Acc::default();
        let words = doc.text.split_whitespace().count();
        corpus.documents += 1;
        corpus.words += words;
        corpus.bytes += doc.text.len();
        corpus.detections += dets.len();
        let optional = doc.labels.iter().filter(|l| l.optional).count();
        corpus.optional += optional;
        corpus.labels += results.len();
        let cat = by_cat.entry(doc.entry.category.clone()).or_default();
        for r in &results {
            let Some(l) = doc.labels.get(r.label) else {
                continue;
            };
            for a in [
                &mut overall,
                &mut acc,
                &mut *cat,
                by_kind.entry(l.kind.clone()).or_default(),
                by_jur
                    .entry(jurisdiction_group(&l.jurisdiction))
                    .or_default(),
            ] {
                a.label(r.outcome, r.kind_match);
            }
            *outcomes.entry(r.outcome.as_str()).or_default() += 1;
            catch_all_on_misses += u64::from(r.catch_all);
            if r.outcome != Outcome::Exact {
                let assigned: Vec<&Detection> =
                    r.detections.iter().filter_map(|i| dets.get(*i)).collect();
                misses.push(Miss {
                    doc: doc.entry.id.clone(),
                    start: l.start,
                    end: l.end,
                    text: l.text.clone(),
                    kind: l.kind.clone(),
                    jurisdiction: l.jurisdiction.clone(),
                    form: l.form.clone(),
                    outcome: r.outcome,
                    detected: assigned.iter().map(|d| d.text.clone()).collect(),
                    rules: assigned.iter().map(|d| d.rule_id.clone()).collect(),
                    catch_all: r.catch_all,
                });
            }
        }
        for (d, status) in dets.iter().zip(&statuses) {
            let tp = match status {
                DetStatus::Ignored => {
                    corpus.ignored += 1;
                    continue;
                }
                DetStatus::TruePositive => true,
                DetStatus::FalsePositive => false,
            };
            for a in [
                &mut overall,
                &mut acc,
                &mut *by_cat.entry(doc.entry.category.clone()).or_default(),
                by_kind.entry(d.kind.clone()).or_default(),
                by_jur
                    .entry(jurisdiction_group(&d.jurisdiction))
                    .or_default(),
            ] {
                a.precision.add(tp);
            }
            rules
                .entry(d.rule_id.clone())
                .or_insert_with(|| (d.kind.clone(), Tally::default()))
                .1
                .add(tp);
            if !tp {
                false_positives.push(FalsePositive {
                    doc: doc.entry.id.clone(),
                    detection: d.clone(),
                    context: context(&doc.text, d.start, d.end),
                });
            }
        }
        documents.push(DocReport {
            id: doc.entry.id.clone(),
            title: doc.entry.title.clone(),
            court: doc.entry.court.clone(),
            category: doc.entry.category.clone(),
            url: doc.entry.url.clone(),
            words,
            labels: results.len(),
            optional,
            measures: acc.measures(),
        });
    }

    let measures = |m: BTreeMap<String, Acc>| -> BTreeMap<String, Measures> {
        m.into_iter().map(|(k, a)| (k, a.measures())).collect()
    };
    let (rows, below): (Vec<_>, Vec<_>) = rules
        .into_iter()
        .partition(|(_, (_, t))| t.n >= PER_RULE_MIN_N);
    let per_rule = PerRule {
        schema: "legal-citations-rs.per-rule-precision/1",
        run: run.clone(),
        min_n: PER_RULE_MIN_N,
        note: "Precision of each rule on the gold corpus: a detection is a true positive when it overlaps a gold citation (span exactness is not required), a false positive when it overlaps none; detections on optional labels are not scored. Labels by one AI agent. Not yet written to any rule's `measured` block.",
        rules: rows
            .into_iter()
            .map(|(rule_id, (kind, t))| RuleRow {
                rule_id,
                kind,
                true_positives: t.k,
                false_positives: t.n - t.k,
                precision: t.estimate(),
            })
            .collect(),
        below_min_n: below.into_iter().map(|(id, _)| id).collect(),
    };
    let report = Report {
        schema: "legal-citations-rs.gold-eval/1",
        run,
        labeler: LABELER,
        corpus,
        overall: overall.measures(),
        outcomes,
        catch_all_on_misses,
        by_kind: measures(by_kind),
        by_jurisdiction: measures(by_jur),
        by_category: measures(by_cat),
        documents,
        misses,
        false_positives,
    };
    (report, per_rule)
}

fn md_cell(s: &str) -> String {
    let s = s.replace('|', "\\|").replace(['\n', '\r'], " ");
    if s.contains('`') {
        s
    } else {
        format!("`{s}`")
    }
}

fn measures_table(out: &mut String, first: &str, rows: &[(String, Measures)]) {
    let _ = writeln!(
        out,
        "| {first} | Recall | Precision | Exact span | Kind agrees |\n| --- | --- | --- | --- | --- |"
    );
    for (name, m) in rows {
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {} |",
            m.recall.cell(),
            m.precision.cell(),
            m.exact_span.cell(),
            m.kind_agreement.cell()
        );
    }
    out.push('\n');
}

/// Slices with the most counted labels first, then by name.
fn by_size(m: &BTreeMap<String, Measures>) -> Vec<(String, Measures)> {
    let mut rows: Vec<_> = m.iter().map(|(k, v)| (k.clone(), *v)).collect();
    rows.sort_by(|a, b| {
        (b.1.recall.n + b.1.precision.n)
            .cmp(&(a.1.recall.n + a.1.precision.n))
            .then_with(|| a.0.cmp(&b.0))
    });
    rows
}

/// Render the gold report as markdown.
pub fn render_md(r: &Report) -> String {
    let mut out = String::new();
    let c = &r.corpus;
    let _ = writeln!(
        out,
        "# Gold-set measurement: recall, precision, exact spans\n\n\
         Date: {}. Tool: `legal-citations` at `{}` (registry hash `{}`), plain-text detection\n\
         (`legal_citations::detect`). Generated by `cite-eval` from `corpus/gold/`; do not edit by\n\
         hand. Machine-readable: the `.json` file of the same name. Misses grouped by form:\n\
         [`misses.md`](misses.md).\n\n\
         > **Who labeled this.** {}\n\n\
         > **Rule `measured` blocks are not written by this run.** Per-rule precision is in the\n\
         > `-per-rule.json` file of the same date, for a later step. Every rule still says\n\
         > `\"precision\": \"unmeasured\"`.\n",
        r.run.date, r.run.revision, r.run.registry_hash, LABELER
    );
    let _ = writeln!(
        out,
        "## Headline\n\n\
         {} documents, {} words, {} gold citations ({} more marked optional), {} detections\n\
         ({} of them only on optional labels, such as page furniture, and not scored).\n\
         Intervals are Wilson 95%; each cell is `value (low–high), k/n`.\n\n\
         Precision is optimistic by construction: the labeler was shown every detection that\n\
         overlapped no label and added a label when it was a real citation, so a false positive\n\
         remains only where the labeler judged the text not a citation; page furniture is optional,\n\
         so it can neither help nor hurt.\n",
        c.documents, c.words, c.labels, c.optional, c.detections, c.ignored
    );
    measures_table(&mut out, "Slice", &[("all".to_string(), r.overall)]);
    let _ = writeln!(
        out,
        "## Span outcomes\n\n| Outcome | Gold citations |\n| --- | ---: |"
    );
    for o in Outcome::ALL {
        let _ = writeln!(
            out,
            "| {} | {} |",
            o.as_str(),
            r.outcomes.get(o.as_str()).copied().unwrap_or(0)
        );
    }
    let _ = writeln!(
        out,
        "\nA catch-all (`unrecognized`, CITE011) hit overlaps {} of the {} misses.\n",
        r.catch_all_on_misses,
        r.outcomes.get("missed").copied().unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "## By kind\n\nRecall and exact span by the gold kind, precision by the detected kind.\n"
    );
    measures_table(&mut out, "Kind", &by_size(&r.by_kind));
    let _ = writeln!(
        out,
        "## By jurisdiction\n\nGrouped by the first two segments of the jurisdiction (`us.fed.app.ca5` is\n\
         `us.fed`). Recall by the gold jurisdiction, precision by the detected one.\n"
    );
    measures_table(&mut out, "Jurisdiction", &by_size(&r.by_jurisdiction));
    let _ = writeln!(out, "## By document category\n");
    measures_table(&mut out, "Category", &by_size(&r.by_category));
    let _ = writeln!(
        out,
        "## Per document\n\n| Document | Court | Words | Gold (optional) | Recall | Precision | Exact span |\n| --- | --- | ---: | ---: | --- | --- | --- |"
    );
    for d in &r.documents {
        let _ = writeln!(
            out,
            "| [{}]({}) | {} | {} | {} ({}) | {} | {} | {} |",
            d.id,
            d.url,
            d.court,
            d.words,
            d.labels,
            d.optional,
            d.measures.recall.cell(),
            d.measures.precision.cell(),
            d.measures.exact_span.cell()
        );
    }
    let _ = writeln!(
        out,
        "\n## False positives\n\n{} detections overlap no gold citation.\n",
        r.false_positives.len()
    );
    if !r.false_positives.is_empty() {
        let _ = writeln!(out, "| Document | Rule | Context |\n| --- | --- | --- |");
        for f in &r.false_positives {
            let _ = writeln!(
                out,
                "| {} | `{}` | {} |",
                f.doc,
                f.detection.rule_id,
                md_cell(&f.context)
            );
        }
        out.push('\n');
    }
    out.push_str(METHOD);
    out
}

const METHOD: &str = "## Method\n\n\
- **Gold.** Each document was read in full and every citation labeled with its byte span,\n\
  kind, jurisdiction, normalized form and a form label (`corpus/gold/<doc>.jsonl`; the\n\
  conventions are in `corpus/gold/README.md`). Borderline references are marked `optional`:\n\
  they are not counted when missed, and a detection on one is neither a true nor a false positive.\n\
- **Matching.** Each detection is assigned to the gold citation it overlaps most. A gold\n\
  citation is detected when a detection is assigned to it (recall). A detection is a true\n\
  positive when assigned to a counted citation, a false positive when it overlaps none\n\
  (precision). The span is exact when a detection's `span` equals the gold span byte for\n\
  byte; otherwise it is truncated, overextended, shifted or split. Catch-all (`unrecognized`)\n\
  hits are not detections here; they are reported only against misses.\n\
- **Kind agreement.** For each detected citation, whether the best-overlapping detection has\n\
  the gold kind.\n\
\n\
## Reproduce\n\n\
```sh\n\
cargo run --release -p cite-eval -- fetch   # rebuilds corpus/cache/gold, checks every sha256\n\
cargo run --release -p cite-eval -- --date YYYY-MM-DD --revision REV\n\
cargo run --release -p cite-eval -- --gate  # thresholds in corpus/gold/gate.json\n\
```\n";

/// Render every missed or mis-spanned citation, grouped by form.
pub fn render_misses(r: &Report) -> String {
    let mut groups: BTreeMap<(String, String), Vec<&Miss>> = BTreeMap::new();
    for m in &r.misses {
        let form = if m.form.is_empty() {
            "(no form)"
        } else {
            &m.form
        };
        groups
            .entry((m.kind.clone(), form.to_string()))
            .or_default()
            .push(m);
    }
    let mut order: Vec<_> = groups.into_iter().collect();
    order.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    let missed = r
        .misses
        .iter()
        .filter(|m| m.outcome == Outcome::Missed)
        .count();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "# Missed and mis-spanned citations, by form\n\n\
         Date: {}. Tool at `{}` (registry hash `{}`). Generated by `cite-eval` from the gold set\n\
         ([report]({}-gold.md)); do not edit by hand. This is the input for the next coverage round.\n\n\
         > {}\n\n\
         {} gold citations are not exact: {} missed, {} detected with a different span.\n\
         Groups are (kind, form), largest first. \"Detected as\" is the tool's span; `catch-all`\n\
         means only the `unrecognized` layer (CITE011) saw it.\n\n\
         ## Summary\n\n| Kind | Form | Missed | Mis-spanned | Example |\n| --- | --- | ---: | ---: | --- |",
        r.run.date,
        r.run.revision,
        r.run.registry_hash,
        r.run.date,
        LABELER,
        r.misses.len(),
        missed,
        r.misses.len() - missed
    );
    for ((kind, form), ms) in &order {
        let n_missed = ms.iter().filter(|m| m.outcome == Outcome::Missed).count();
        let example = ms.first().map(|m| md_cell(&m.text)).unwrap_or_default();
        let _ = writeln!(
            out,
            "| {kind} | {form} | {n_missed} | {} | {example} |",
            ms.len() - n_missed
        );
    }
    for ((kind, form), ms) in &order {
        let _ = writeln!(out, "\n## {kind}: {form} ({})\n", ms.len());
        let _ = writeln!(
            out,
            "| Document | Gold | Outcome | Detected as |\n| --- | --- | --- | --- |"
        );
        for m in ms {
            let detected = if m.detected.is_empty() {
                if m.catch_all { "catch-all" } else { "" }.to_string()
            } else {
                m.detected
                    .iter()
                    .zip(&m.rules)
                    .map(|(t, rule)| format!("{} ({rule})", md_cell(t)))
                    .collect::<Vec<_>>()
                    .join("; ")
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                m.doc,
                md_cell(&m.text),
                m.outcome.as_str(),
                detected
            );
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(jurisdiction_group("us.fed.app.ca5"), "us.fed");
        assert_eq!(jurisdiction_group("us"), "us");
        assert_eq!(md_cell("a|b\nc"), "`a\\|b c`");
        assert_eq!(md_cell("has `tick`"), "has `tick`");
        // Context never splits a multibyte character.
        let text = format!("{}§ 1983{}", "é".repeat(40), "ü".repeat(40));
        let start = 80;
        let c = context(&text, start, start + 7);
        assert!(c.contains("[[§ 1983]]"));
        assert!(c.starts_with('é') && c.ends_with('ü'));
    }

    #[test]
    fn slices_sort_by_size_then_name() {
        let m = |n: u64| {
            let t = Tally { k: n, n };
            Measures {
                recall: t.estimate(),
                precision: Tally::default().estimate(),
                exact_span: t.estimate(),
                kind_agreement: t.estimate(),
            }
        };
        let map: BTreeMap<String, Measures> = [("b", 1), ("a", 1), ("c", 5)]
            .into_iter()
            .map(|(k, n)| (k.to_string(), m(n)))
            .collect();
        let names: Vec<_> = by_size(&map).into_iter().map(|(k, _)| k).collect();
        assert_eq!(names, ["c", "a", "b"]);
    }
}
