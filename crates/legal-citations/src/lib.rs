//! Detect legal citations with exact spans, authority keys and markdown link
//! status.
//!
//! The system is a tower of layers, each with a typed, serializable output:
//!
//! | Layer | Module | Output |
//! | --- | --- | --- |
//! | L0 source | [`span`] | [`Span`]: byte, char, UTF-16, line/col into the ORIGINAL text |
//! | L1 recognize | [`recognize`] | [`recognize::RawMatch`] per rule match |
//! | L2/L3 assemble, resolve | [`citation`] | [`Citation`] with authority key and antecedent |
//! | L4 document | [`document`] | [`Document`]: citations, links, bindings, masked zones |
//! | L5/L6 policy, findings | [`policy`] | [`Finding`]s with stable codes |
//! | selection | [`select`] | [`Selection`]: rules picked by collections and profiles |
//! | authorities | [`authority`] | [`Authority`]: citations grouped by what they cite |
//!
//! The single source of truth is the rule registry (`rules/<kind>/<id>.json`,
//! embedded at build time, see [`registry()`]). Detection never panics and
//! every span satisfies `&text[span.start..span.end] == citation.text`.
//!
//! It detects citations; it does not check that a citation supports the
//! proposition it is cited for.
//!
//! ```
//! let cites = legal_citations::detect("Jurisdiction lies under 28 U.S.C. § 1331.");
//! assert_eq!(cites[0].text, "28 U.S.C. § 1331");
//! assert_eq!(cites[0].authority.as_deref(), Some("us.usc/28/1331"));
//!
//! let doc = legal_citations::detect_markdown("See [28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331).");
//! assert!(doc.is_linked(0));
//! ```

pub mod authority;
pub mod citation;
pub mod court;
pub mod coverage;
pub mod document;
pub mod export;
pub mod legacy;
pub mod link_target;
pub mod policy;
pub mod portable;
pub mod recognize;
pub mod registry;
pub mod rule;
pub mod select;
pub mod span;

pub use authority::{authorities, Authority};
pub use citation::Citation;
pub use document::{Document, Format};
pub use policy::{evaluate, Finding, Mode, Policy, Severity};
pub use registry::{registry, Registry};
pub use rule::{Family, Kind};
pub use select::{RuleSet, SelectError, Selection, Selector};
pub use span::Span;

/// Detect citations in plain text (L0-L3).
pub fn detect(text: &str) -> Vec<Citation> {
    detect_with(registry(), text)
}

/// [`detect`] against a registry or a [`Selection`] of it
/// (`registry().select(&selector)?`). Every active rule runs either way, so
/// overlaps and short forms resolve as in an unfiltered run; a selection then
/// filters the citations reported (see [`select`]).
pub fn detect_with<R: RuleSet + ?Sized>(rules: &R, text: &str) -> Vec<Citation> {
    let reg = rules.registry();
    let raw = recognize::recognize_where(reg, text, |r| !r.def.kind.is_catch_all());
    let real = citation::assemble(reg, text, recognize::resolve_overlaps(raw.clone()));
    let cites = catch_all(reg, text, &raw, real);
    match rules.selection() {
        Some(sel) => sel.retain(cites),
        None => cites,
    }
}

/// The lowest layer: catch-all (`unrecognized`) matches that overlap no raw
/// match and no citation of a real rule, merged into `real` in text order.
fn catch_all(
    reg: &Registry,
    text: &str,
    raw: &[recognize::RawMatch],
    real: Vec<Citation>,
) -> Vec<Citation> {
    let overlaps = |s: usize, e: usize, a: usize, b: usize| s < b && a < e;
    let found: Vec<_> = recognize::recognize_where(reg, text, |r| r.def.kind.is_catch_all())
        .into_iter()
        .filter(|m| {
            !raw.iter().any(|r| overlaps(m.start, m.end, r.start, r.end))
                && !real.iter().any(|c| {
                    overlaps(m.start, m.end, c.full_span.start, c.full_span.end)
                        || overlaps(m.start, m.end, c.span.start, c.span.end)
                })
        })
        .collect();
    if found.is_empty() {
        return real;
    }
    let extra = citation::assemble(reg, text, recognize::resolve_overlaps(found));
    citation::merge(real, extra)
}

/// Detect citations in markdown and bind them to links (L0-L4).
pub fn detect_markdown(text: &str) -> Document {
    detect_markdown_with(registry(), text)
}

/// [`detect_markdown`] against a registry or a [`Selection`] of it.
pub fn detect_markdown_with<R: RuleSet + ?Sized>(rules: &R, text: &str) -> Document {
    let parsed = document::parse_markdown(text);
    let mut citations = detect_with(rules, &parsed.masked);
    // Report text and coordinates from the original, not the masked copy:
    // masking keeps byte offsets but turns multibyte chars into several
    // spaces, so char/UTF-16/column positions must be recomputed.
    let map = span::OffsetMap::new(text);
    let remap = |s: &mut Span| *s = map.span(s.start, s.end);
    for c in &mut citations {
        remap(&mut c.span);
        remap(&mut c.core_span);
        remap(&mut c.full_span);
        if let Some(p) = c.parenthetical.as_mut() {
            remap(p);
        }
        if let Some(t) = text.get(c.span.start..c.span.end) {
            c.text = t.to_string();
        }
    }
    let bindings = document::bind(&citations, &parsed.links, &parsed.footnote_refs);
    Document {
        format: Format::Markdown,
        citations,
        links: parsed.links,
        bindings,
        zones: parsed.zones,
        suppressions: parsed.suppressions,
    }
}

/// Lint markdown: detect, bind, apply `policy` (L0-L6). The policy's
/// `include`/`exclude`/`profiles` select the rules; a selector that cannot be
/// applied lints with every rule and adds a CITE008 error finding.
pub fn lint_markdown(text: &str, policy: &Policy) -> (Document, Vec<Finding>) {
    match registry().select(&policy.selector()) {
        Ok(sel) => lint_markdown_with(&sel, text, policy),
        Err(e) => {
            let (doc, mut findings) = lint_markdown_with(registry(), text, policy);
            findings.insert(0, policy::bad_selector(policy, &e));
            (doc, findings)
        }
    }
}

/// [`lint_markdown`] against a registry or a [`Selection`] of it; the
/// policy's own selector is not applied again.
pub fn lint_markdown_with<R: RuleSet + ?Sized>(
    rules: &R,
    text: &str,
    policy: &Policy,
) -> (Document, Vec<Finding>) {
    let doc = detect_markdown_with(rules, text);
    let findings = evaluate(rules.registry(), &doc, policy, Mode::Lint);
    (doc, findings)
}
