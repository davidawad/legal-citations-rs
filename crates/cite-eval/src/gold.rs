//! Gold labels: one JSON object per line, one line per citation.

use legal_citations::Kind;
use serde::{Deserialize, Serialize};

/// A hand-labeled citation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Label {
    /// Byte offset of the first character (case name, not the signal).
    pub start: usize,
    /// Byte offset one past the last character.
    pub end: usize,
    /// Citation kind, as `legal_citations::Kind` names it (`case`, `statute`).
    pub kind: String,
    /// Dotted jurisdiction (`us.scotus`, `us.fed.app.ca5`, `us.oh.sup`, `us.fed`).
    pub jurisdiction: String,
    /// `text[start..end]`, checked on load.
    pub text: String,
    /// Normalized form (`347 U.S. 483`, `42 U.S.C. § 1983`).
    pub normalized: String,
    /// Citation form, for grouping misses (`Ohio R.C. section`, `PTAB paper`).
    #[serde(default)]
    pub form: String,
    /// Borderline reference: not counted when missed, and a detection on it
    /// is neither a true nor a false positive.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// Labeler's note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// Parse a `.jsonl` label file (blank lines skipped).
pub fn parse(src: &str) -> Result<Vec<Label>, String> {
    src.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect()
}

/// Check labels against the text and sort them: spans in bounds on char
/// boundaries, `text` equal to the slice, a known kind, and no two labels
/// overlapping.
pub fn check(text: &str, mut labels: Vec<Label>) -> Result<Vec<Label>, String> {
    labels.sort_by_key(|l| (l.start, l.end));
    for l in &labels {
        let slice = text
            .get(l.start..l.end)
            .filter(|_| l.start < l.end)
            .ok_or_else(|| format!("label {}..{} is not a valid span", l.start, l.end))?;
        if slice != l.text {
            return Err(format!(
                "label {}..{}: text {:?} but the document has {:?}",
                l.start, l.end, l.text, slice
            ));
        }
        if Kind::parse(&l.kind).is_none_or(Kind::is_catch_all) {
            return Err(format!(
                "label {}..{}: unknown kind {:?}",
                l.start, l.end, l.kind
            ));
        }
    }
    for pair in labels.windows(2) {
        if let [a, b] = pair {
            if b.start < a.end {
                return Err(format!(
                    "labels {}..{} and {}..{} overlap",
                    a.start, a.end, b.start, b.end
                ));
            }
        }
    }
    Ok(labels)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn label(start: usize, end: usize, text: &str, kind: &str) -> Label {
        Label {
            start,
            end,
            kind: kind.into(),
            jurisdiction: "us.fed".into(),
            text: text.into(),
            normalized: text.into(),
            form: String::new(),
            optional: false,
            note: String::new(),
        }
    }

    #[test]
    fn parse_skips_blank_lines_and_reports_line_numbers() {
        let ok =
            r#"{"start":0,"end":1,"kind":"case","jurisdiction":"us","text":"a","normalized":"a"}"#;
        assert_eq!(parse(&format!("{ok}\n\n{ok}\n")).map(|v| v.len()), Ok(2));
        let err = parse(&format!("{ok}\n{{bad")).err().unwrap_or_default();
        assert!(err.starts_with("line 2:"), "{err}");
    }

    #[test]
    fn check_sorts_and_validates() {
        let text = "See 42 U.S.C. § 1983 and 28 U.S.C. § 1331.";
        let a = label(4, 21, "42 U.S.C. § 1983", "statute");
        let b = label(26, 43, "28 U.S.C. § 1331", "statute");
        let sorted = check(text, vec![b.clone(), a.clone()]).unwrap_or_default();
        assert_eq!(sorted, vec![a.clone(), b]);

        let bad_text = label(4, 21, "42 USC § 1983", "statute");
        assert!(check(text, vec![bad_text]).is_err_and(|e| e.contains("document has")));
        let out_of_bounds = label(4, 400, "x", "statute");
        assert!(check(text, vec![out_of_bounds]).is_err_and(|e| e.contains("valid span")));
        let mid_char = label(4, 15, "42 U.S.C. §", "statute");
        assert!(check(text, vec![mid_char]).is_err());
        let empty = label(4, 4, "", "statute");
        assert!(check(text, vec![empty]).is_err());
        let unknown = label(4, 21, "42 U.S.C. § 1983", "nonsense");
        assert!(check(text, vec![unknown]).is_err_and(|e| e.contains("unknown kind")));
        let catch_all = label(4, 21, "42 U.S.C. § 1983", "unrecognized");
        assert!(check(text, vec![catch_all]).is_err());
        let inner = label(7, 21, "U.S.C. § 1983", "statute");
        assert!(check(text, vec![a, inner]).is_err_and(|e| e.contains("overlap")));
    }
}
