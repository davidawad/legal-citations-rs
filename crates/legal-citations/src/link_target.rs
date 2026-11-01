//! Offline link-target knowledge: build a URL for a citation from a rule's
//! `url_templates`, and decode a URL back to an authority key with the
//! rules' `url_decoders`. No network.

use crate::citation::canonical_key;
use crate::registry::Registry;
use crate::rule::{fill_template, fill_template_with};
use std::collections::BTreeMap;

/// A URL decoded to the authority it points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// Rule whose decoder matched.
    pub rule_id: String,
    /// Canonical authority key.
    pub authority: String,
}

/// Lowercased host without a leading `www.`.
pub fn host_of(url: &url::Url) -> Option<String> {
    url.host_str()
        .map(|h| h.trim_start_matches("www.").to_ascii_lowercase())
}

/// True when `host` is `allowed` or a subdomain of it.
pub fn host_allowed(host: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|a| {
        let a = a.trim_start_matches("www.").to_ascii_lowercase();
        host == a || host.ends_with(&format!(".{a}"))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok());
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b == b'+' { b' ' } else { b });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Match one path segment against a segment template: literals (compared
/// case-insensitively) and `{name}` placeholders, e.g. `{volume}-FR-{page}`.
/// A placeholder takes the shortest non-empty text before the next literal,
/// or the rest of the segment when it is last.
fn match_segment(template: &str, segment: &str, groups: &mut BTreeMap<String, String>) -> bool {
    let Some(open) = template.find('{') else {
        return template.eq_ignore_ascii_case(segment);
    };
    let (Some(prefix), Some(after)) = (template.get(..open), template.get(open + 1..)) else {
        return false;
    };
    let Some((name, rest)) = after.split_once('}') else {
        return false;
    };
    let Some(value_and_rest) = segment
        .get(..prefix.len())
        .filter(|p| p.eq_ignore_ascii_case(prefix))
        .and_then(|_| segment.get(prefix.len()..))
    else {
        return false;
    };
    let literal = rest
        .split('{')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    // The value is never empty: the search starts after its first character.
    let first = value_and_rest
        .char_indices()
        .nth(1)
        .map_or(value_and_rest.len(), |(i, _)| i);
    let end = if rest.is_empty() {
        Some(value_and_rest.len())
    } else if literal.is_empty() {
        // Two adjacent placeholders: the first takes one character.
        Some(first)
    } else {
        value_and_rest
            .to_ascii_lowercase()
            .get(first..)
            .and_then(|tail| tail.find(&literal))
            .map(|i| i + first)
    };
    let Some((value, tail)) = end.and_then(|e| value_and_rest.split_at_checked(e)) else {
        return false;
    };
    if value.is_empty() || !match_segment(rest, tail, groups) {
        return false;
    }
    groups.insert(name.to_string(), value.to_string());
    true
}

/// Decode `url` with one decoder template `host/seg/{name}/...`. Extra
/// trailing URL segments are allowed.
pub fn decode_with(decoder: &str, url: &url::Url) -> Option<BTreeMap<String, String>> {
    let host = host_of(url)?;
    let mut parts = decoder.split('/');
    let want_host = parts
        .next()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if host != want_host {
        return None;
    }
    let segments: Vec<String> = url
        .path_segments()?
        .filter(|s| !s.is_empty())
        .map(percent_decode)
        .collect();
    let mut groups = BTreeMap::new();
    let mut seg_iter = segments.iter();
    for template in parts.filter(|p| !p.is_empty()) {
        let seg = seg_iter.next()?;
        if !match_segment(template, seg, &mut groups) {
            return None;
        }
    }
    Some(groups)
}

/// Decode a URL to an authority using every rule's decoders.
pub fn decode(reg: &Registry, url: &url::Url) -> Option<Decoded> {
    reg.active().find_map(|(_, rule)| {
        rule.def.url_decoders.iter().find_map(|d| {
            let groups = decode_with(d, url)?;
            let key = fill_template(&rule.def.authority_key, &groups)?;
            Some(Decoded {
                rule_id: rule.def.id.clone(),
                authority: canonical_key(&key),
            })
        })
    })
}

/// Percent-encode the characters that break a markdown link destination
/// (how [`candidates`] fills a template).
pub fn encode_url_value(v: &str) -> String {
    v.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            '(' => "%28".to_string(),
            ')' => "%29".to_string(),
            '’' => "%E2%80%99".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// First URL the rule can build for these fields, values percent-encoded.
pub fn suggest(
    reg: &Registry,
    rule_index: usize,
    fields: &BTreeMap<String, String>,
) -> Option<String> {
    candidates(reg, rule_index, fields).into_iter().next()
}

/// Every URL the rule's `url_templates` build for these fields, in template
/// order. Templates exist only for hosts verified by fetch; decoders never
/// produce candidates.
pub fn candidates(
    reg: &Registry,
    rule_index: usize,
    fields: &BTreeMap<String, String>,
) -> Vec<String> {
    let Some(rule) = reg.rules.get(rule_index) else {
        return Vec::new();
    };
    rule.def
        .url_templates
        .iter()
        .filter_map(|t| fill_template_with(t, fields, encode_url_value))
        .collect()
}
