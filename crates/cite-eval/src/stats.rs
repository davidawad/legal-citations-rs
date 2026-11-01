//! Proportions with Wilson 95% score intervals.

use serde::Serialize;

/// `k` successes out of `n`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    /// Successes.
    pub k: u64,
    /// Trials.
    pub n: u64,
}

impl Tally {
    /// Count one trial.
    pub fn add(&mut self, success: bool) {
        self.n += 1;
        self.k += u64::from(success);
    }

    /// Point estimate, `None` when `n == 0`.
    pub fn value(self) -> Option<f64> {
        (self.n > 0).then(|| self.k as f64 / self.n as f64)
    }

    /// The serializable estimate with its Wilson 95% interval.
    pub fn estimate(self) -> Estimate {
        let (lo, hi) = wilson(self.k, self.n);
        Estimate {
            k: self.k,
            n: self.n,
            value: self.value(),
            lo,
            hi,
        }
    }
}

/// A proportion with its Wilson 95% interval (`null` when `n == 0`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Estimate {
    /// Successes.
    pub k: u64,
    /// Trials.
    pub n: u64,
    /// `k / n`.
    pub value: Option<f64>,
    /// Lower bound.
    pub lo: Option<f64>,
    /// Upper bound.
    pub hi: Option<f64>,
}

impl Estimate {
    /// `0.912 (0.871–0.941), n=250` style cell; `n/a` when `n == 0`.
    pub fn cell(&self) -> String {
        match (self.value, self.lo, self.hi) {
            (Some(v), Some(lo), Some(hi)) => {
                format!("{v:.3} ({lo:.3}–{hi:.3}), {}/{}", self.k, self.n)
            }
            _ => "n/a".to_string(),
        }
    }
}

/// Wilson score interval at 95% (z = 1.96); `(None, None)` when `n == 0`.
pub fn wilson(k: u64, n: u64) -> (Option<f64>, Option<f64>) {
    if n == 0 {
        return (None, None);
    }
    let z = 1.96_f64;
    let n = n as f64;
    let p = k as f64 / n;
    let denom = 1.0 + z * z / n;
    let centre = p + z * z / (2.0 * n);
    let margin = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    let lo = ((centre - margin) / denom).clamp(0.0, 1.0);
    let hi = ((centre + margin) / denom).clamp(0.0, 1.0);
    (Some(lo), Some(hi))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn wilson_matches_known_values() {
        // 10/10: the familiar 0.722 lower bound.
        let (lo, hi) = wilson(10, 10);
        assert!((lo.unwrap() - 0.7225).abs() < 1e-3);
        assert!((hi.unwrap() - 1.0).abs() < 1e-9);
        // 50/100.
        let (lo, hi) = wilson(50, 100);
        assert!((lo.unwrap() - 0.4038).abs() < 1e-3);
        assert!((hi.unwrap() - 0.5962).abs() < 1e-3);
        // 0/5.
        let (lo, _) = wilson(0, 5);
        assert!(lo.unwrap().abs() < 1e-9);
        assert_eq!(wilson(0, 0), (None, None));
    }

    #[test]
    fn tally_and_cells() {
        let mut t = Tally::default();
        assert_eq!(t.value(), None);
        assert_eq!(t.estimate().cell(), "n/a");
        t.add(true);
        t.add(false);
        assert_eq!(t, Tally { k: 1, n: 2 });
        assert_eq!(t.value(), Some(0.5));
        assert!(t.estimate().cell().starts_with("0.500 (0.095–0.905), 1/2"));
    }
}
