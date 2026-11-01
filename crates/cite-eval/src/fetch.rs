//! Rebuild the text cache: download, extract, excerpt, check sha256.
//!
//! Downloads use `curl` and PDF extraction uses `pdftotext` (poppler), so
//! the crate needs no HTTP or PDF dependency; neither runs in the default
//! test gate, which uses committed fixtures.

use crate::manifest::{DocEntry, Excerpt, Extraction};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Lowercase hex sha256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Keep `from` through `to` (both included); `None` when a marker is absent.
pub fn excerpt(text: &str, ex: &Excerpt) -> Option<String> {
    let start = text.find(&ex.from)?;
    let rest = text.get(start..)?;
    let end = rest.find(&ex.to)? + ex.to.len();
    rest.get(..end).map(str::to_string)
}

const BLOCK_TAGS: [&str; 22] = [
    "p",
    "div",
    "br",
    "li",
    "ul",
    "ol",
    "tr",
    "table",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "section",
    "article",
    "header",
    "footer",
    "pre",
    "dd",
    "dt",
];
const SKIP_TAGS: [&str; 4] = ["script", "style", "head", "noscript"];

fn tag_name(tag: &str) -> String {
    tag.trim_start_matches('/')
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn entity(name: &str) -> Option<char> {
    let num = |s: &str, radix| u32::from_str_radix(s, radix).ok().and_then(char::from_u32);
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return num(hex, 16);
    }
    if let Some(dec) = name.strip_prefix('#') {
        return num(dec, 10);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "sect" => '§',
        "para" => '¶',
        "ndash" => '–',
        "mdash" => '—',
        "lsquo" => '‘',
        "rsquo" => '’',
        "ldquo" => '“',
        "rdquo" => '”',
        "hellip" => '…',
        "copy" => '©',
        _ => return None,
    })
}

/// Strip tags and decode entities. `script`, `style`, `head` and `noscript`
/// contents are dropped, block tags become line breaks, runs of spaces
/// collapse to one, lines are trimmed, blank-line runs collapse to one and
/// trailing blank lines are dropped.
pub fn html_to_text(html: &str) -> String {
    let mut raw = String::with_capacity(html.len() / 2);
    let mut rest = html;
    let mut skip: Option<String> = None;
    while let Some(lt) = rest.find('<') {
        let (before, after) = rest.split_at(lt);
        if skip.is_none() {
            raw.push_str(before);
        }
        let Some(gt) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = after.get(1..gt).unwrap_or_default();
        rest = after.get(gt + 1..).unwrap_or_default();
        if tag.starts_with("!--") {
            // Comments may contain `>`: skip to the real end.
            if let Some(end) = after.find("-->") {
                rest = after.get(end + 3..).unwrap_or_default();
            }
            continue;
        }
        let name = tag_name(tag);
        let closing = tag.starts_with('/');
        match &skip {
            Some(s) if closing && *s == name => skip = None,
            Some(_) => {}
            None if !closing && SKIP_TAGS.contains(&name.as_str()) && !tag.ends_with('/') => {
                skip = Some(name)
            }
            None if BLOCK_TAGS.contains(&name.as_str()) => raw.push('\n'),
            None if name == "td" || name == "th" => raw.push(' '),
            None => {}
        }
    }
    if skip.is_none() {
        raw.push_str(rest);
    }
    let mut decoded = String::with_capacity(raw.len());
    let mut rest = raw.as_str();
    while let Some(amp) = rest.find('&') {
        let (before, after) = rest.split_at(amp);
        decoded.push_str(before);
        let semi = after.get(1..after.len().min(12)).and_then(|s| s.find(';'));
        match semi.and_then(|i| after.get(1..=i).map(|n| (i, entity(n)))) {
            Some((i, Some(ch))) => {
                decoded.push(ch);
                rest = after.get(i + 2..).unwrap_or_default();
            }
            _ => {
                decoded.push('&');
                rest = after.get(1..).unwrap_or_default();
            }
        }
    }
    decoded.push_str(rest);
    let mut out = String::with_capacity(decoded.len());
    let mut blank = true;
    for line in decoded.lines() {
        let line = line
            .split([' ', '\t'])
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if line.is_empty() {
            if !blank {
                out.push('\n');
            }
            blank = true;
        } else {
            out.push_str(&line);
            out.push('\n');
            blank = false;
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

/// Turn fetched bytes into text with `method` (`raw_path` is where the
/// bytes were saved, for `pdftotext`).
pub fn extract(method: Extraction, raw_path: &Path) -> Result<String, String> {
    let read = || std::fs::read(raw_path).map_err(|e| format!("{}: {e}", raw_path.display()));
    let utf8 =
        |b: Vec<u8>| String::from_utf8(b).map_err(|e| format!("{}: {e}", raw_path.display()));
    match method {
        Extraction::Text => utf8(read()?),
        Extraction::Html => utf8(read()?).map(|s| html_to_text(&s)),
        Extraction::Pdftotext => {
            let out = Command::new("pdftotext")
                .args(["-enc", "UTF-8"])
                .arg(raw_path)
                .arg("-")
                .output()
                .map_err(|e| format!("pdftotext: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "pdftotext {}: {}",
                    raw_path.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            utf8(out.stdout)
        }
    }
}

/// Download `url` to `dest` with curl.
pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    let out = Command::new("curl")
        .args([
            "-sSfL",
            "--max-time",
            "120",
            "-A",
            "legal-citations-rs cite-eval",
            "-o",
        ])
        .arg(dest)
        .arg(url)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "curl {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// What [`fetch`] did with one document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    /// The cached text already matched.
    Cached,
    /// Downloaded and written.
    Written,
}

/// Make `cache_dir/<id>.txt` match the manifest: keep it when its sha256
/// matches, else download to `cache_dir/raw/`, extract, excerpt and check.
pub fn fetch(entry: &DocEntry, cache_dir: &Path) -> Result<Fetched, String> {
    let text_path = crate::text_path(cache_dir, &entry.id);
    if std::fs::read(&text_path).is_ok_and(|b| sha256_hex(&b) == entry.sha256) {
        return Ok(Fetched::Cached);
    }
    let raw_dir = cache_dir.join("raw");
    std::fs::create_dir_all(&raw_dir).map_err(|e| format!("{}: {e}", raw_dir.display()))?;
    let raw: PathBuf = raw_dir.join(&entry.id);
    download(&entry.url, &raw)?;
    let mut text = extract(entry.extraction, &raw)?;
    if let Some(ex) = &entry.excerpt {
        text =
            excerpt(&text, ex).ok_or_else(|| format!("{}: excerpt markers not found", entry.id))?;
    }
    let digest = sha256_hex(text.as_bytes());
    if digest != entry.sha256 {
        // Keep the text for inspection; labels index the pinned version.
        let kept = cache_dir.join(format!("{}.mismatch.txt", entry.id));
        std::fs::write(&kept, &text).map_err(|e| format!("{}: {e}", kept.display()))?;
        return Err(format!(
            "{}: extracted text has sha256 {digest}, the manifest says {} (the source changed? see {})",
            entry.id,
            entry.sha256,
            kept.display()
        ));
    }
    std::fs::write(&text_path, text).map_err(|e| format!("{}: {e}", text_path.display()))?;
    Ok(Fetched::Written)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn excerpt_includes_both_markers() {
        let ex = Excerpt {
            from: "B".into(),
            to: "D".into(),
        };
        assert_eq!(excerpt("ABCDEDB", &ex).as_deref(), Some("BCD"));
        let missing = Excerpt {
            from: "X".into(),
            to: "D".into(),
        };
        assert_eq!(excerpt("ABCD", &missing), None);
        let no_end = Excerpt {
            from: "C".into(),
            to: "A".into(),
        };
        assert_eq!(excerpt("ABCD", &no_end), None);
    }

    #[test]
    fn html_to_text_strips_and_decodes() {
        let html = "<html><head><title>t</title></head><body>\
            <script>var x = '<p>';</script><!-- a > b -->\
            <p>See  <i>Roe</i>,&nbsp;410&#32;U.S. 113 &sect;&#x2013;&amp; &bogus; &;</p>\
            <table><tr><td>a</td><td>b</td></tr></table>\
            <style/>tail<br/>\n\n\n<p>  last  </p>&amp";
        assert_eq!(
            html_to_text(html),
            "See Roe, 410 U.S. 113 §–& &bogus; &;\n\na b\n\ntail\n\nlast\n&amp\n"
        );
        assert_eq!(html_to_text("a <b"), "a\n");
        assert_eq!(html_to_text("<style>never closed"), "");
    }

    #[test]
    fn extract_text_and_html_and_errors() {
        let dir = std::env::temp_dir().join(format!("cite-eval-extract-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.html");
        std::fs::write(&p, "<p>x &amp; y</p>").unwrap();
        assert_eq!(extract(Extraction::Html, &p).unwrap(), "x & y\n");
        assert_eq!(extract(Extraction::Text, &p).unwrap(), "<p>x &amp; y</p>");
        let bad = dir.join("bad.bin");
        std::fs::write(&bad, [0xff, 0xfe]).unwrap();
        assert!(extract(Extraction::Text, &bad).is_err());
        assert!(extract(Extraction::Text, &dir.join("missing")).is_err());
        // Not a PDF: pdftotext fails (or is absent); either way an error.
        assert!(extract(Extraction::Pdftotext, &p).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
