//! Backwards name scans for L2: the full case name before a reporter cite
//! (`Smith v. Jones, `) and the short name before a short form (`Bruen, `).

/// Words that may sit inside the first party's name besides capitalized words.
const CONNECTORS: &[&str] = &[
    "of", "the", "and", "&", "for", "ex", "rel.", "de", "du", "la", "von", "van", "der", "on", "to",
];

/// Words that end a backwards name scan.
const STOP_WORDS: &[&str] = &[
    "See",
    "see",
    "Cf.",
    "cf.",
    "But",
    "but",
    "Accord",
    "accord",
    "Contra",
    "Compare",
    "also",
    "generally",
    "In",
    "in",
    "quoting",
    "Quoting",
    "citing",
    "Citing",
];

pub(super) fn is_md_noise(c: char) -> bool {
    matches!(c, '*' | '_' | '[' | ']')
}

/// Whitespace-separated tokens with their byte offsets.
fn tokens(s: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        match (c.is_whitespace(), start) {
            (true, Some(st)) => {
                if let Some(tok) = s.get(st..i) {
                    out.push((st, tok));
                }
                start = None;
            }
            (false, None) => start = Some(i),
            _ => {}
        }
    }
    if let Some(st) = start {
        if let Some(tok) = s.get(st..) {
            out.push((st, tok));
        }
    }
    out
}

/// Find the case name that ends right before `core_start`
/// (`Name v. Name, ` or `In re Name, `). Returns (byte start, name).
pub fn case_name_before(text: &str, core_start: usize) -> Option<(usize, String)> {
    let prefix = text.get(..core_start)?;
    let window_start = prefix
        .char_indices()
        .map(|(i, _)| i)
        .find(|i| core_start - i <= 240)
        .unwrap_or(core_start);
    let window = prefix.get(window_start..)?;
    let body = window.trim_end_matches(|c: char| c.is_whitespace() || is_md_noise(c));
    let body = body.strip_suffix(',')?;
    let body = body.trim_end_matches(|c: char| c.is_whitespace() || is_md_noise(c));
    let toks = tokens(body);
    let clean = |t: &str| t.trim_matches(is_md_noise).to_string();
    // The party separator or `In re` / `Ex parte` marker nearest the end
    // decides the name's shape.
    let marker = toks.iter().rposition(|(_, t)| {
        matches!(clean(t).as_str(), "v." | "vs." | "versus" | "re" | "parte")
    })?;
    let first_idx = match clean(toks.get(marker)?.1).as_str() {
        "re" | "parte" => in_re_start(&toks, marker)?,
        _ => first_party_start(&toks, marker)?,
    };
    // The second half may not contain clause breaks.
    let tail = toks.get(first_idx..)?;
    if tail.len() > 24
        || tail
            .iter()
            .any(|(_, t)| t.contains(';') || t.contains('(') || t.contains(')'))
    {
        return None;
    }
    let (rel_start, first_tok) = *toks.get(first_idx)?;
    let lead = first_tok.len() - first_tok.trim_start_matches(is_md_noise).len();
    let start = window_start + rel_start + lead;
    let end = window_start + body.len();
    let name = text.get(start..end)?.trim_end_matches(is_md_noise).trim();
    Some((start, name.to_string()))
}

fn in_re_start(toks: &[(usize, &str)], re_idx: usize) -> Option<usize> {
    let prev = re_idx.checked_sub(1)?;
    let word = toks.get(prev)?.1.trim_matches(is_md_noise);
    (word == "In" || word == "in" || word == "Ex" || word == "ex").then_some(prev)
}

fn first_party_start(toks: &[(usize, &str)], sep: usize) -> Option<usize> {
    let mut start = None;
    for i in (0..sep).rev().take(10) {
        let t = toks.get(i)?.1.trim_matches(is_md_noise);
        let first = t.chars().next()?;
        if STOP_WORDS.contains(&t) || t.ends_with(';') || t.ends_with(':') || t.ends_with(')') {
            break;
        }
        // A number closing a sentence (`... 3d 456. Smith v. Jones`) ends the name.
        if t.len() > 1
            && t.ends_with('.')
            && t.trim_end_matches('.').chars().all(|c| c.is_ascii_digit())
        {
            break;
        }
        // A lowercase word that is not a connector ends the name, as does a
        // sentence end on a long lowercase-tailed word ("otherwise.").
        if !(first.is_uppercase() || first.is_ascii_digit() || CONNECTORS.contains(&t)) {
            break;
        }
        start = Some(i);
    }
    // Names start with a capitalized word, never a connector.
    (start?..sep).find(|&i| {
        toks.get(i).is_some_and(|(_, t)| {
            t.trim_matches(is_md_noise)
                .chars()
                .next()
                .is_some_and(char::is_uppercase)
        })
    })
}

/// Find the short case name that ends right before a short form's core
/// (`Bruen, 597 U.S. at 17`): up to four capitalized words or connectors,
/// ending in a comma, that do not start with a signal. Returns (byte start, name).
pub fn short_name_before(text: &str, core_start: usize) -> Option<(usize, String)> {
    let prefix = text.get(..core_start)?;
    let window_start = prefix
        .char_indices()
        .map(|(i, _)| i)
        .find(|i| core_start - i <= 80)
        .unwrap_or(core_start);
    let window = prefix.get(window_start..)?;
    let body = window.trim_end_matches(|c: char| c.is_whitespace() || is_md_noise(c));
    let body = body.strip_suffix(',')?;
    let body = body.trim_end_matches(|c: char| c.is_whitespace() || is_md_noise(c));
    let toks = tokens(body);
    let mut start = None;
    for i in (0..toks.len()).rev().take(4) {
        let raw = toks.get(i)?.1.trim_matches(is_md_noise);
        let t = raw.trim_start_matches(['(', '[']);
        let first = t.chars().next()?;
        if STOP_WORDS.contains(&t)
            || t.ends_with([';', ':', ')', ','])
            || !(first.is_uppercase() || CONNECTORS.contains(&t))
        {
            break;
        }
        start = Some(i);
        if t.len() != raw.len() {
            break;
        }
    }
    // Names start with a capitalized word, never a connector.
    let s = (start?..toks.len()).find(|&i| {
        toks.get(i)
            .is_some_and(|(_, t)| !CONNECTORS.contains(&t.trim_matches(is_md_noise)))
    })?;
    let (rel_start, tok) = *toks.get(s)?;
    let lead = tok.len()
        - tok
            .trim_start_matches(|c: char| is_md_noise(c) || c == '(')
            .len();
    let start = window_start + rel_start + lead;
    let name = text.get(start..window_start + body.len())?.trim();
    Some((start, name.to_string()))
}
