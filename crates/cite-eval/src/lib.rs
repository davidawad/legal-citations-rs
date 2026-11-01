//! Measure `legal-citations` against hand-labeled gold citations.
//!
//! The gold corpus is a directory (default `corpus/gold/`) holding:
//!
//! - `manifest.json`: public documents ([`manifest::Manifest`]): URL, how the
//!   text was extracted, an optional excerpt and the sha256 of the text;
//! - `<doc>.jsonl`: one [`gold::Label`] per citation in the document (byte
//!   span, kind, jurisdiction, normalized text), listing EVERY citation;
//! - `gate.json`: regression thresholds ([`gate::Gate`]).
//!
//! The texts themselves live in a cache directory (default
//! `corpus/cache/gold/`, gitignored); `cite-eval fetch` rebuilds it and
//! checks every sha256. [`score`] matches detections to labels,
//! [`report`] aggregates recall, precision and exact-span rate with Wilson
//! 95% intervals and renders the markdown and JSON reports.

pub mod fetch;
pub mod gate;
pub mod gold;
pub mod manifest;
pub mod report;
pub mod score;
pub mod stats;

use std::path::{Path, PathBuf};

/// One document ready to score: its manifest entry, text and labels.
#[derive(Clone, Debug)]
pub struct LoadedDoc {
    /// Manifest entry.
    pub entry: manifest::DocEntry,
    /// The document text (the excerpt, when the entry has one).
    pub text: String,
    /// Gold labels, sorted by start.
    pub labels: Vec<gold::Label>,
}

/// Load every document of `corpus_dir/manifest.json`: text from
/// `cache_dir/<id>.txt` (sha256 checked) and labels from
/// `corpus_dir/<id>.jsonl` (checked against the text).
pub fn load_corpus(corpus_dir: &Path, cache_dir: &Path) -> Result<Vec<LoadedDoc>, String> {
    let manifest = manifest::Manifest::read(&corpus_dir.join("manifest.json"))?;
    let mut docs = Vec::new();
    for entry in manifest.documents {
        let text_path = text_path(cache_dir, &entry.id);
        let text = std::fs::read_to_string(&text_path).map_err(|e| {
            format!(
                "{}: {e} (run `cite-eval fetch` to rebuild the cache)",
                text_path.display()
            )
        })?;
        let digest = fetch::sha256_hex(text.as_bytes());
        if digest != entry.sha256 {
            return Err(format!(
                "{}: sha256 {digest} does not match the manifest's {}",
                text_path.display(),
                entry.sha256
            ));
        }
        let label_path = corpus_dir.join(format!("{}.jsonl", entry.id));
        let src = std::fs::read_to_string(&label_path)
            .map_err(|e| format!("{}: {e}", label_path.display()))?;
        let labels = gold::parse(&src)
            .and_then(|l| gold::check(&text, l))
            .map_err(|e| format!("{}: {e}", label_path.display()))?;
        docs.push(LoadedDoc {
            entry,
            text,
            labels,
        });
    }
    Ok(docs)
}

/// Where the cached text of document `id` lives.
pub fn text_path(cache_dir: &Path, id: &str) -> PathBuf {
    cache_dir.join(format!("{id}.txt"))
}
