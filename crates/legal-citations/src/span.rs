//! L0: positions in the ORIGINAL text.
//!
//! A [`Span`] is a half-open UTF-8 byte range plus the derived Unicode-scalar,
//! UTF-16 and line/column positions. Byte offsets are the source of truth;
//! every other coordinate is computed from them by one [`OffsetMap`], so the
//! coordinates can never disagree with each other.

use serde::{Deserialize, Serialize};

/// A located range of the original input.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
pub struct Span {
    /// Inclusive start, UTF-8 bytes.
    pub start: usize,
    /// Exclusive end, UTF-8 bytes.
    pub end: usize,
    /// Inclusive start, Unicode scalar values.
    pub char_start: usize,
    /// Exclusive end, Unicode scalar values.
    pub char_end: usize,
    /// Inclusive start, UTF-16 code units (editor/LSP positions).
    pub utf16_start: usize,
    /// Exclusive end, UTF-16 code units.
    pub utf16_end: usize,
    /// 1-based line of `start`.
    pub line: usize,
    /// 1-based column of `start`, in Unicode scalar values.
    pub col: usize,
    /// 1-based line of `end`.
    pub end_line: usize,
    /// 1-based column of `end`, in Unicode scalar values.
    pub end_col: usize,
}

impl Span {
    /// Byte length.
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// True when the span covers no bytes.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// True when `other` lies entirely inside `self`.
    pub fn contains(&self, other: &Span) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// True when the two spans share at least one byte.
    pub fn overlaps(&self, other: &Span) -> bool {
        self.start < other.end && other.start < self.end
    }
}

/// Converts byte offsets of one text into every other coordinate system.
#[derive(Clone, Debug)]
pub struct OffsetMap {
    /// Byte offset of each char boundary, plus `text.len()` at the end.
    char_bytes: Vec<usize>,
    /// UTF-16 offset at each char boundary, parallel to `char_bytes`.
    utf16: Vec<usize>,
    /// Byte offset where each line starts.
    line_starts: Vec<usize>,
}

impl OffsetMap {
    /// Build the map for `text`.
    pub fn new(text: &str) -> Self {
        let mut char_bytes = Vec::with_capacity(text.len() + 1);
        let mut utf16 = Vec::with_capacity(text.len() + 1);
        let mut line_starts = vec![0];
        let mut u16_pos = 0;
        for (byte, ch) in text.char_indices() {
            char_bytes.push(byte);
            utf16.push(u16_pos);
            u16_pos += ch.len_utf16();
            if ch == '\n' {
                line_starts.push(byte + 1);
            }
        }
        char_bytes.push(text.len());
        utf16.push(u16_pos);
        Self {
            char_bytes,
            utf16,
            line_starts,
        }
    }

    /// Char index of a byte offset. A byte inside a multibyte char maps to
    /// that char (callers only pass boundaries; this keeps it total).
    fn char_index(&self, byte: usize) -> usize {
        match self.char_bytes.binary_search(&byte) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
    }

    /// 1-based (line, col) of a byte offset; col counts Unicode scalars.
    fn line_col(&self, byte: usize) -> (usize, usize) {
        let line_idx = match self.line_starts.binary_search(&byte) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let line_start = self.line_starts.get(line_idx).copied().unwrap_or(0);
        let col = self.char_index(byte) - self.char_index(line_start) + 1;
        (line_idx + 1, col)
    }

    /// Full [`Span`] for the byte range `start..end`.
    pub fn span(&self, start: usize, end: usize) -> Span {
        let cs = self.char_index(start);
        let ce = self.char_index(end);
        let (line, col) = self.line_col(start);
        let (end_line, end_col) = self.line_col(end);
        Span {
            start,
            end,
            char_start: cs,
            char_end: ce,
            utf16_start: self.utf16.get(cs).copied().unwrap_or(0),
            utf16_end: self.utf16.get(ce).copied().unwrap_or(0),
            line,
            col,
            end_line,
            end_col,
        }
    }
}
