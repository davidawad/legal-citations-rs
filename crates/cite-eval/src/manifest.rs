//! The gold corpus manifest: which public documents, from where, and how
//! their text was extracted.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// `corpus/gold/manifest.json`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    /// Schema id (`legal-citations-rs.gold-manifest/1`).
    pub schema: String,
    /// Free-text notes (who labeled, conventions).
    #[serde(default)]
    pub note: String,
    /// The documents, in report order.
    pub documents: Vec<DocEntry>,
}

/// One public document.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DocEntry {
    /// Stable id; labels are `<id>.jsonl`, the cached text `<id>.txt`.
    pub id: String,
    /// Case name or title.
    pub title: String,
    /// Court or issuing body as written (`Ohio Sup. Ct.`, `NLRB`).
    pub court: String,
    /// Report group: `scotus`, `circuit`, `district`, `state`, `agency`,
    /// `secondary`, `foreign`.
    pub category: String,
    /// Date of the document (`YYYY-MM-DD`), when known.
    #[serde(default)]
    pub date: String,
    /// The URL a plain GET fetches.
    pub url: String,
    /// How the text was produced from the fetched bytes.
    pub extraction: Extraction,
    /// Keep only this part of the extracted text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<Excerpt>,
    /// sha256 (hex) of the final text: extracted, then excerpted.
    pub sha256: String,
    /// Where the corpus first listed it (`corpus/manifest.json` id), or a note.
    #[serde(default)]
    pub source: String,
}

/// Text extraction method.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Extraction {
    /// `pdftotext -enc UTF-8` (poppler), default mode.
    Pdftotext,
    /// Tags stripped and entities decoded by [`crate::fetch::html_to_text`].
    Html,
    /// The bytes are the text (UTF-8).
    Text,
}

/// An excerpt: from the first occurrence of `from` through the end of the
/// first occurrence of `to` after it.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Excerpt {
    /// Start marker (included).
    pub from: String,
    /// End marker (included).
    pub to: String,
}

impl Manifest {
    /// Read and parse a manifest file.
    pub fn read(path: &Path) -> Result<Manifest, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_str(&src).map_err(|e| format!("{}: {e}", path.display()))
    }
}
