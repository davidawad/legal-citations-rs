//! L4: the document model.
//!
//! Markdown is parsed once. Everything that is not prose (front matter, code,
//! HTML, link destinations, emphasis and bracket delimiters, footnote labels,
//! reference definitions) is replaced by spaces of the same byte length, so
//! detection runs on a "masked" text whose offsets are identical to the
//! original. Links are recorded with their text span and destination, and
//! every citation is bound to the links that cover it.

use crate::citation::Citation;
use crate::span::{OffsetMap, Span};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::OnceLock;

/// How a link was written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LinkStyle {
    /// `[text](dest)`
    Inline,
    /// `[text][ref]`, `[text][]`, `[text]` with a definition
    Reference,
    /// `<https://...>`
    Autolink,
    /// `<a href="...">text</a>`
    Html,
}

/// A link in the document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Link {
    /// Whole link source, delimiters included.
    pub span: Span,
    /// The visible link text.
    pub text_span: Span,
    /// Destination as written (reference links resolved).
    pub dest: String,
    /// Syntax used.
    pub style: LinkStyle,
    /// Footnote label when the link sits inside a footnote definition.
    pub footnote: Option<String>,
}

/// How a citation is tied to a link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Via {
    /// The link text covers (part of) the citation.
    Direct,
    /// A footnote reference right after the citation points to a definition
    /// that contains the link.
    Footnote,
}

/// A citation-to-link binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Binding {
    /// Index into [`Document::links`].
    pub link: usize,
    /// How the citation reaches the link.
    pub via: Via,
}

/// What a masked zone was.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    /// YAML/TOML front matter.
    FrontMatter,
    /// Fenced or indented code, or inline code.
    Code,
    /// HTML block, inline HTML, comment.
    Html,
    /// Link destination, title, brackets.
    LinkSyntax,
    /// Reference-link definition line.
    LinkDefinition,
    /// Footnote reference or label.
    FootnoteLabel,
}

/// A region excluded from detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Zone {
    /// Kind.
    pub kind: ZoneKind,
    /// Where.
    pub span: Span,
}

/// An inline suppression comment:
/// `<!-- cite-detect: ignore CITE001 reason="..." -->`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Suppression {
    /// Finding codes suppressed.
    pub codes: Vec<String>,
    /// Mandatory reason (empty means malformed).
    pub reason: String,
    /// `next` (the next line) or `file`.
    pub scope: String,
    /// Where the comment is.
    pub span: Span,
}

/// Input format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Format {
    /// Plain text: no links, nothing masked.
    Text,
    /// CommonMark + GFM tables/footnotes + front matter.
    Markdown,
}

/// The L4 result.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Document {
    /// Input format.
    pub format: Format,
    /// Citations in text order.
    pub citations: Vec<Citation>,
    /// Links in source order.
    pub links: Vec<Link>,
    /// Per citation (same index), its link bindings.
    pub bindings: Vec<Vec<Binding>>,
    /// Masked regions.
    pub zones: Vec<Zone>,
    /// Inline suppressions.
    pub suppressions: Vec<Suppression>,
}

impl Document {
    /// A plain-text document: every citation is unlinked.
    pub fn plain(citations: Vec<Citation>) -> Document {
        let bindings = vec![Vec::new(); citations.len()];
        Document {
            format: Format::Text,
            citations,
            links: Vec::new(),
            bindings,
            zones: Vec::new(),
            suppressions: Vec::new(),
        }
    }

    /// True when citation `i` has at least one link.
    pub fn is_linked(&self, i: usize) -> bool {
        self.bindings.get(i).is_some_and(|b| !b.is_empty())
    }
}

fn suppression_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    C.get_or_init(|| {
        Regex::new(r#"cite-detect:\s*ignore(?:-(?P<scope>file|next))?\s+(?P<codes>[A-Z0-9_,]+)(?:\s+reason="(?P<reason>[^"]*)")?"#).ok()
    })
    .as_ref()
}

fn tag_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    C.get_or_init(|| Regex::new(r"(?s)<!--.*?-->|<[A-Za-z/!?][^>]*>").ok())
        .as_ref()
}

fn href_re() -> Option<&'static Regex> {
    static C: OnceLock<Option<Regex>> = OnceLock::new();
    C.get_or_init(|| Regex::new(r#"(?i)^<a\s[^>]*href\s*=\s*["']([^"']*)["']"#).ok())
        .as_ref()
}

/// Mutable masking state over the original bytes.
struct Masker {
    bytes: Vec<u8>,
}

impl Masker {
    fn mask(&mut self, range: Range<usize>, text: &str) {
        // Replace whole chars in range with spaces of equal byte length so the
        // masked text stays valid UTF-8 with identical offsets.
        let Some(slice) = text.get(range.clone()) else {
            return;
        };
        for (i, ch) in slice.char_indices() {
            if ch == '\n' {
                continue;
            }
            for k in 0..ch.len_utf8() {
                if let Some(b) = self.bytes.get_mut(range.start + i + k) {
                    *b = b' ';
                }
            }
        }
    }

    fn finish(self) -> String {
        String::from_utf8(self.bytes).unwrap_or_default()
    }
}

/// Open link being collected.
struct OpenLink {
    range: Range<usize>,
    dest: String,
    style: LinkStyle,
    inner: Option<Range<usize>>,
}

/// Result of the markdown pass, before detection.
pub struct Parsed {
    /// Text with non-prose masked to spaces.
    pub masked: String,
    /// Links.
    pub links: Vec<Link>,
    /// Zones.
    pub zones: Vec<Zone>,
    /// Footnote references: (label, byte range).
    pub footnote_refs: Vec<(String, Range<usize>)>,
    /// Suppressions.
    pub suppressions: Vec<Suppression>,
}

fn delimiter_run(text: &str, range: &Range<usize>, set: &[char]) -> (usize, usize) {
    let s = text.get(range.clone()).unwrap_or("");
    let rest = s.trim_start_matches(|c| set.contains(&c));
    let trail = rest.len() - rest.trim_end_matches(|c| set.contains(&c)).len();
    (s.len() - rest.len(), trail)
}

/// Parse markdown into masked text, links and zones.
pub fn parse_markdown(text: &str) -> Parsed {
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

    let map = OffsetMap::new(text);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_YAML_STYLE_METADATA_BLOCKS);
    opts.insert(Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS);
    let parser = Parser::new_ext(text, opts);
    let mut masker = Masker {
        bytes: text.as_bytes().to_vec(),
    };
    let mut zones = Vec::new();
    let mut zone = |masker: &mut Masker, kind: ZoneKind, r: Range<usize>| {
        if r.start < r.end {
            masker.mask(r.clone(), text);
            zones.push(Zone {
                kind,
                span: map.span(r.start, r.end),
            });
        }
    };
    for (_, def) in parser.reference_definitions().iter() {
        zone(&mut masker, ZoneKind::LinkDefinition, def.span.clone());
    }
    let mut links = Vec::new();
    let mut open: Vec<OpenLink> = Vec::new();
    let mut html_anchor: Option<(usize, String, Option<Range<usize>>)> = None;
    let mut footnote_refs = Vec::new();
    let mut footnote_def: Option<String> = None;
    let mut suppressions = Vec::new();
    for (event, range) in parser.into_offset_iter() {
        // Extend the inner text range of every open link.
        if !matches!(
            event,
            Event::Start(Tag::Link { .. }) | Event::End(TagEnd::Link)
        ) {
            for l in &mut open {
                l.inner = Some(match &l.inner {
                    Some(r) => r.start.min(range.start)..r.end.max(range.end),
                    None => range.clone(),
                });
            }
            if let Some((_, _, inner)) = &mut html_anchor {
                if !matches!(event, Event::InlineHtml(_)) {
                    *inner = Some(match inner {
                        Some(r) => r.start.min(range.start)..r.end.max(range.end),
                        None => range.clone(),
                    });
                }
            }
        }
        match event {
            Event::Start(Tag::MetadataBlock(_)) => {
                zone(&mut masker, ZoneKind::FrontMatter, range);
            }
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => {
                zone(&mut masker, ZoneKind::Code, range)
            }
            Event::Html(ref html) | Event::InlineHtml(ref html) => {
                let is_block = matches!(event, Event::Html(_));
                let s = html.as_ref();
                if let Some(c) = suppression_re().and_then(|r| r.captures(s)) {
                    // The comment ends at `-->`, not at the block's trailing newline.
                    let src = text.get(range.clone()).unwrap_or("");
                    let end_rel = src.find("-->").map_or(src.trim_end().len(), |i| i + 3);
                    suppressions.push(Suppression {
                        codes: c
                            .name("codes")
                            .map(|m| m.as_str().split(',').map(str::to_string).collect())
                            .unwrap_or_default(),
                        reason: c
                            .name("reason")
                            .map(|m| m.as_str().trim().to_string())
                            .unwrap_or_default(),
                        scope: c.name("scope").map_or("next", |m| m.as_str()).to_string(),
                        span: map.span(range.start, range.start + end_rel),
                    });
                }
                if let Some(href) = href_re().and_then(|r| r.captures(s)).and_then(|c| c.get(1)) {
                    html_anchor = Some((range.start, href.as_str().to_string(), None));
                } else if s.trim().eq_ignore_ascii_case("</a>") {
                    if let Some((start, dest, inner)) = html_anchor.take() {
                        let inner = inner.unwrap_or(range.start..range.start);
                        links.push(Link {
                            span: map.span(start, range.end),
                            text_span: map.span(inner.start, inner.end),
                            dest,
                            style: LinkStyle::Html,
                            footnote: footnote_def.clone(),
                        });
                    }
                }
                if is_block {
                    // An HTML block (e.g. a line starting with a directive
                    // comment `<!--u-->`) keeps its prose: mask only the
                    // comments and tags inside it.
                    let src = text.get(range.clone()).unwrap_or("");
                    for m in tag_re().into_iter().flat_map(|r| r.find_iter(src)) {
                        zone(
                            &mut masker,
                            ZoneKind::Html,
                            range.start + m.start()..range.start + m.end(),
                        );
                    }
                } else {
                    zone(&mut masker, ZoneKind::Html, range);
                }
            }
            Event::FootnoteReference(label) => {
                footnote_refs.push((label.to_string(), range.clone()));
                zone(&mut masker, ZoneKind::FootnoteLabel, range);
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                footnote_def = Some(label.to_string());
                let s = text.get(range.clone()).unwrap_or("");
                if let Some(colon) = s.find("]:") {
                    zone(
                        &mut masker,
                        ZoneKind::FootnoteLabel,
                        range.start..range.start + colon + 2,
                    );
                }
            }
            Event::End(TagEnd::FootnoteDefinition) => footnote_def = None,
            Event::Start(Tag::Emphasis | Tag::Strong | Tag::Strikethrough) => {
                let (lead, trail) = delimiter_run(text, &range, &['*', '_', '~']);
                masker.mask(range.start..range.start + lead, text);
                masker.mask(range.end - trail..range.end, text);
            }
            Event::Start(Tag::Image { .. }) => zone(&mut masker, ZoneKind::LinkSyntax, range),
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let style = match link_type {
                    LinkType::Autolink | LinkType::Email => LinkStyle::Autolink,
                    LinkType::Inline => LinkStyle::Inline,
                    _ => LinkStyle::Reference,
                };
                open.push(OpenLink {
                    range,
                    dest: dest_url.to_string(),
                    style,
                    inner: None,
                });
            }
            Event::End(TagEnd::Link) => {
                if let Some(l) = open.pop() {
                    let inner = match (&l.style, &l.inner) {
                        (LinkStyle::Autolink, _) | (_, None) => l.range.start..l.range.start,
                        (_, Some(r)) => r.clone(),
                    };
                    zone(
                        &mut masker,
                        ZoneKind::LinkSyntax,
                        l.range.start..inner.start.max(l.range.start),
                    );
                    zone(
                        &mut masker,
                        ZoneKind::LinkSyntax,
                        inner.end.max(l.range.start)..l.range.end,
                    );
                    links.push(Link {
                        span: map.span(l.range.start, l.range.end),
                        text_span: map.span(inner.start, inner.end),
                        dest: l.dest,
                        style: l.style,
                        footnote: footnote_def.clone(),
                    });
                    // Outer links see this link's whole range as inner text.
                    for o in &mut open {
                        o.inner = Some(match &o.inner {
                            Some(r) => r.start.min(l.range.start)..r.end.max(l.range.end),
                            None => l.range.clone(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    links.sort_by_key(|l| (l.span.start, l.span.end));
    Parsed {
        masked: masker.finish(),
        links,
        zones,
        footnote_refs,
        suppressions,
    }
}

/// Bind citations to links: direct overlap with link text, or a footnote
/// reference between the citation and the next citation (within 200 bytes)
/// whose definition holds links.
pub fn bind(
    citations: &[Citation],
    links: &[Link],
    footnote_refs: &[(String, Range<usize>)],
) -> Vec<Vec<Binding>> {
    let mut by_footnote: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, l) in links.iter().enumerate() {
        if let Some(label) = &l.footnote {
            by_footnote.entry(label.as_str()).or_default().push(i);
        }
    }
    citations
        .iter()
        .enumerate()
        .map(|(ci, c)| {
            let mut out: Vec<Binding> = links
                .iter()
                .enumerate()
                .filter(|(_, l)| !l.text_span.is_empty() && l.text_span.overlaps(&c.span))
                .map(|(link, _)| Binding {
                    link,
                    via: Via::Direct,
                })
                .collect();
            if out.is_empty() {
                let limit = citations.get(ci + 1).map_or(c.full_span.end + 200, |n| {
                    n.span.start.min(c.full_span.end + 200)
                });
                for (label, r) in footnote_refs {
                    if r.start >= c.span.end && r.start <= limit {
                        for &link in by_footnote
                            .get(label.as_str())
                            .map_or(&[][..], Vec::as_slice)
                        {
                            out.push(Binding {
                                link,
                                via: Via::Footnote,
                            });
                        }
                    }
                }
            }
            out
        })
        .collect()
}
