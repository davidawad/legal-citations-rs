//! Match detections to gold labels in one document.
//!
//! Each detection is assigned to the label it overlaps most (a counted label
//! before an optional one; ties go to the earlier label). A detection
//! assigned to a counted label is a true positive, one assigned to an
//! optional label is ignored, and one that overlaps no label is a false
//! positive. A counted label is detected when at least one detection is
//! assigned to it; its [`Outcome`] says how well the spans agree.

use crate::gold::Label;
use legal_citations::{Citation, Kind};
use serde::Serialize;

/// A detection, reduced to what scoring needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Detection {
    /// Byte start of `Citation.span`.
    pub start: usize,
    /// Byte end of `Citation.span`.
    pub end: usize,
    /// Kind name.
    pub kind: String,
    /// Rule id.
    pub rule_id: String,
    /// Resolved jurisdiction.
    pub jurisdiction: String,
    /// Span text.
    pub text: String,
}

impl Detection {
    /// From a library citation.
    pub fn from_citation(c: &Citation) -> Detection {
        Detection {
            start: c.span.start,
            end: c.span.end,
            kind: c.kind.as_str().to_string(),
            rule_id: c.rule_id.clone(),
            jurisdiction: c.jurisdiction.clone(),
            text: c.text.clone(),
        }
    }
}

/// Run the embedded registry on `text`: `(real, catch_all)` detections.
/// Catch-all (`unrecognized`) hits are not citations the tool recognized;
/// they are kept apart and only reported against misses.
pub fn detect(text: &str) -> (Vec<Detection>, Vec<Detection>) {
    legal_citations::detect(text)
        .iter()
        .map(Detection::from_citation)
        .partition(|d| Kind::parse(&d.kind).is_none_or(|k| !k.is_catch_all()))
}

/// How a counted label fared.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// A detection has exactly the label's span.
    Exact,
    /// One detection, inside the label.
    Truncated,
    /// One detection, containing the label.
    Overextended,
    /// One detection, overlapping but neither inside nor containing it.
    Shifted,
    /// Several detections, none exact.
    Split,
    /// No detection.
    Missed,
}

impl Outcome {
    /// Every outcome, in report order.
    pub const ALL: [Outcome; 6] = [
        Outcome::Exact,
        Outcome::Truncated,
        Outcome::Overextended,
        Outcome::Shifted,
        Outcome::Split,
        Outcome::Missed,
    ];

    /// Snake-case name.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Exact => "exact",
            Outcome::Truncated => "truncated",
            Outcome::Overextended => "overextended",
            Outcome::Shifted => "shifted",
            Outcome::Split => "split",
            Outcome::Missed => "missed",
        }
    }

    /// Detected at all (every outcome but [`Outcome::Missed`]).
    pub fn detected(self) -> bool {
        self != Outcome::Missed
    }
}

/// Result for one counted label.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LabelResult {
    /// Index into the document's labels.
    pub label: usize,
    /// Span agreement.
    pub outcome: Outcome,
    /// Indices of the detections assigned to it.
    pub detections: Vec<usize>,
    /// The primary detection's kind equals the label's (`None` when missed).
    pub kind_match: Option<bool>,
    /// For a miss: a catch-all (`unrecognized`, CITE011) hit overlaps it.
    pub catch_all: bool,
}

/// Status of one detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DetStatus {
    /// Assigned to a counted label.
    TruePositive,
    /// Overlaps only optional labels.
    Ignored,
    /// Overlaps no label.
    FalsePositive,
}

/// Scores of one document.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DocScore {
    /// One entry per counted (non-optional) label, in label order.
    pub labels: Vec<LabelResult>,
    /// One entry per detection, in detection order.
    pub detections: Vec<DetStatus>,
}

fn overlap(a: (usize, usize), b: (usize, usize)) -> usize {
    a.1.min(b.1).saturating_sub(a.0.max(b.0))
}

/// Score `detections` against `labels` (sorted, non-overlapping).
pub fn score(labels: &[Label], detections: &[Detection], catch_all: &[Detection]) -> DocScore {
    let mut assigned: Vec<Vec<usize>> = vec![Vec::new(); labels.len()];
    let mut statuses = Vec::with_capacity(detections.len());
    for (di, d) in detections.iter().enumerate() {
        let best = labels
            .iter()
            .enumerate()
            .map(|(li, l)| (li, l, overlap((d.start, d.end), (l.start, l.end))))
            .filter(|(_, _, o)| *o > 0)
            // max_by_key keeps the last maximum; rev() makes ties go to the earlier label.
            .rev()
            .max_by_key(|(_, l, o)| (!l.optional, *o));
        let status = match best {
            Some((li, l, _)) if !l.optional => {
                if let Some(v) = assigned.get_mut(li) {
                    v.push(di);
                }
                DetStatus::TruePositive
            }
            Some(_) => DetStatus::Ignored,
            None => DetStatus::FalsePositive,
        };
        statuses.push(status);
    }
    let results = labels
        .iter()
        .zip(assigned)
        .enumerate()
        .filter(|(_, (l, _))| !l.optional)
        .map(|(li, (l, dets))| {
            let span = (l.start, l.end);
            let get = |i: &usize| detections.get(*i);
            let exact = dets
                .iter()
                .find(|i| get(i).is_some_and(|d| (d.start, d.end) == span));
            let outcome = match (exact, dets.as_slice()) {
                (Some(_), _) => Outcome::Exact,
                (None, []) => Outcome::Missed,
                (None, [one]) => match get(one) {
                    Some(d) if d.start >= l.start && d.end <= l.end => Outcome::Truncated,
                    Some(d) if d.start <= l.start && d.end >= l.end => Outcome::Overextended,
                    _ => Outcome::Shifted,
                },
                (None, _) => Outcome::Split,
            };
            let primary = exact.or_else(|| {
                dets.iter()
                    .rev()
                    .max_by_key(|i| get(i).map_or(0, |d| overlap((d.start, d.end), span)))
            });
            let kind_match = primary.and_then(get).map(|d| d.kind == l.kind);
            let catch_all = !outcome.detected()
                && catch_all
                    .iter()
                    .any(|c| overlap((c.start, c.end), span) > 0);
            LabelResult {
                label: li,
                outcome,
                detections: dets,
                kind_match,
                catch_all,
            }
        })
        .collect();
    DocScore {
        labels: results,
        detections: statuses,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn label(start: usize, end: usize, kind: &str, optional: bool) -> Label {
        Label {
            start,
            end,
            kind: kind.into(),
            jurisdiction: "us.fed".into(),
            text: String::new(),
            normalized: String::new(),
            form: String::new(),
            optional,
            note: String::new(),
        }
    }

    fn det(start: usize, end: usize, kind: &str) -> Detection {
        Detection {
            start,
            end,
            kind: kind.into(),
            rule_id: format!("r.{kind}"),
            jurisdiction: "us.fed".into(),
            text: String::new(),
        }
    }

    #[test]
    fn outcomes_and_statuses() {
        let labels = vec![
            label(0, 10, "case", false),     // exact
            label(20, 30, "case", false),    // truncated
            label(40, 50, "statute", false), // overextended, wrong kind
            label(60, 70, "case", false),    // shifted
            label(80, 90, "case", false),    // split
            label(100, 110, "case", false),  // missed, catch-all
            label(120, 130, "record", true), // optional
            label(140, 150, "case", false),  // missed, no catch-all
        ];
        let dets = vec![
            det(0, 10, "case"),
            det(22, 28, "case"),
            det(38, 52, "case"),
            det(65, 75, "case"),
            det(80, 84, "case"),
            det(85, 90, "case"),
            det(121, 125, "record"),
            det(200, 210, "statute"),
        ];
        let catch = vec![det(101, 103, "unrecognized"), det(300, 301, "unrecognized")];
        let s = score(&labels, &dets, &catch);
        let outcomes: Vec<_> = s.labels.iter().map(|r| r.outcome).collect();
        assert_eq!(
            outcomes,
            vec![
                Outcome::Exact,
                Outcome::Truncated,
                Outcome::Overextended,
                Outcome::Shifted,
                Outcome::Split,
                Outcome::Missed,
                Outcome::Missed,
            ]
        );
        assert_eq!(s.labels[2].kind_match, Some(false));
        assert_eq!(s.labels[0].kind_match, Some(true));
        assert_eq!(s.labels[5].kind_match, None);
        assert!(s.labels[5].catch_all);
        assert!(!s.labels[6].catch_all);
        assert_eq!(s.labels[6].label, 7);
        assert_eq!(
            s.detections,
            vec![
                DetStatus::TruePositive,
                DetStatus::TruePositive,
                DetStatus::TruePositive,
                DetStatus::TruePositive,
                DetStatus::TruePositive,
                DetStatus::TruePositive,
                DetStatus::Ignored,
                DetStatus::FalsePositive,
            ]
        );
    }

    #[test]
    fn a_detection_credits_one_label_and_prefers_counted_ones() {
        // One detection across two labels goes to the larger overlap; the
        // other label is missed.
        let labels = vec![label(0, 10, "id", false), label(12, 30, "id", false)];
        let s = score(&labels, &[det(5, 30, "id")], &[]);
        assert_eq!(s.labels[0].outcome, Outcome::Missed);
        assert_eq!(s.labels[1].outcome, Outcome::Overextended);
        // Equal overlap: the earlier label wins.
        let s = score(&labels, &[det(5, 17, "id")], &[]);
        assert_eq!(s.labels[0].outcome, Outcome::Shifted);
        assert_eq!(s.labels[1].outcome, Outcome::Missed);
        // A counted label beats a larger optional one.
        let labels = vec![label(0, 4, "case", false), label(4, 30, "record", true)];
        let s = score(&labels, &[det(2, 30, "case")], &[]);
        assert_eq!(s.detections, vec![DetStatus::TruePositive]);
        // An exact detection plus an extra one is still exact.
        let labels = vec![label(0, 10, "case", false)];
        let s = score(&labels, &[det(0, 3, "case"), det(0, 10, "case")], &[]);
        assert_eq!(s.labels[0].outcome, Outcome::Exact);
        assert_eq!(s.labels[0].detections, vec![0, 1]);
        assert_eq!(Outcome::Split.as_str(), "split");
        assert!(Outcome::ALL.iter().filter(|o| o.detected()).count() == 5);
    }

    #[test]
    fn detect_splits_out_catch_all_hits() {
        let (real, catch) = detect("See 42 U.S.C. § 1983. Also 45 Haw. Trib. 112.");
        assert_eq!(real.len(), 1);
        assert_eq!(real[0].text, "42 U.S.C. § 1983");
        assert_eq!(catch.len(), 1);
        assert_eq!(catch[0].kind, "unrecognized");
    }
}
