//! Regression gate: minimum recall, precision and exact-span rate.

use crate::report::{Measures, Report};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// `corpus/gold/gate.json`.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Gate {
    /// Why these values.
    #[serde(default)]
    pub note: String,
    /// Thresholds on the overall measures.
    #[serde(default)]
    pub overall: Thresholds,
    /// Thresholds per kind (`case`, `statute`).
    #[serde(default)]
    pub kinds: BTreeMap<String, Thresholds>,
}

/// Minimum point estimates; an absent field is not gated.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
pub struct Thresholds {
    /// Minimum recall.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall: Option<f64>,
    /// Minimum precision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<f64>,
    /// Minimum exact-span rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact_span: Option<f64>,
}

impl Gate {
    /// Read a gate file.
    pub fn read(path: &Path) -> Result<Gate, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Every threshold the report falls below, as a sentence.
    pub fn check(&self, report: &Report) -> Vec<String> {
        let mut failures = check_one("overall", &self.overall, Some(&report.overall));
        for (kind, t) in &self.kinds {
            failures.extend(check_one(
                &format!("kind {kind}"),
                t,
                report.by_kind.get(kind),
            ));
        }
        failures
    }
}

fn check_one(what: &str, t: &Thresholds, m: Option<&Measures>) -> Vec<String> {
    let pairs = [
        ("recall", t.recall, m.and_then(|m| m.recall.value)),
        ("precision", t.precision, m.and_then(|m| m.precision.value)),
        (
            "exact span",
            t.exact_span,
            m.and_then(|m| m.exact_span.value),
        ),
    ];
    pairs
        .into_iter()
        .filter_map(|(name, min, got)| {
            let min = min?;
            match got {
                Some(v) if v >= min => None,
                Some(v) => Some(format!("{what} {name} {v:.4} is below the gate {min:.4}")),
                None => Some(format!("{what} {name} has no data (gate {min:.4})")),
            }
        })
        .collect()
}
