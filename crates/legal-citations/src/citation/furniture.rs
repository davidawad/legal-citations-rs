//! Page furniture: running heads and repeated captions that PDF text
//! extraction leaves in the body. They are real citations, so they stay in
//! the output, but the link policy does not demand links on them.

use super::Citation;
use serde::Serialize;
use std::collections::HashMap;

/// Why a citation is page furniture rather than a cite in the prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Furniture {
    /// A slip-opinion running head: `Cite as: 606 U. S. 656 (2025)`.
    RunningHead,
    /// A citation alone on a line that recurs verbatim, such as a PTAB
    /// caption (`IPR2025-00687`) repeated on every page.
    RepeatedCaption,
}

/// The line around `[start, end)`: (text before, text after) on that line.
fn line_around(text: &str, start: usize, end: usize) -> (&str, &str) {
    let head = text.get(..start).unwrap_or("");
    let tail = text.get(end..).unwrap_or("");
    (
        head.rsplit('\n').next().unwrap_or(""),
        tail.split('\n').next().unwrap_or(""),
    )
}

/// True for nothing, a page number, or one parenthetical (`(Patent 9,000,000 B2)`).
fn trailing_furniture(after: &str) -> bool {
    let t = after.trim();
    t.is_empty()
        || t.chars().all(|c| c.is_ascii_digit())
        || (t.starts_with('(') && t.ends_with(')') && t.matches('(').count() == 1)
}

/// Mark running heads and repeated captions in place.
pub(super) fn mark(text: &str, cites: &mut [Citation]) {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.is_empty() {
            *seen.entry(line).or_insert(0) += 1;
        }
    }
    for c in cites.iter_mut() {
        let (before, after) = line_around(text, c.span.start, c.span.end);
        if before.trim_end().ends_with("Cite as:") && trailing_furniture(after) {
            c.furniture = Some(Furniture::RunningHead);
            continue;
        }
        if c.kind.is_short_form() || !before.trim().is_empty() || !trailing_furniture(after) {
            continue;
        }
        let line = format!("{before}{}{after}", c.text);
        if seen.get(line.trim()).is_some_and(|n| *n >= 2) {
            c.furniture = Some(Furniture::RepeatedCaption);
        }
    }
}
