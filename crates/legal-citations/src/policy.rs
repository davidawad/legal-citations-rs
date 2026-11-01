//! L5 policy and L6 findings.
//!
//! One policy file is shared by every consumer (CLI, TNET, formbook), so they
//! can never disagree about what "compliant" means. Findings carry stable
//! codes; `cite-detect explain CODE` prints [`CodeInfo`].

use crate::document::{Document, Via};
use crate::link_target::{decode, host_allowed, host_of, suggest};
use crate::registry::Registry;
use crate::rule::Kind;
use crate::select::{SelectError, Selector};
use crate::span::Span;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Finding severity, ordered.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Inventory or heuristic note; never fails by default.
    Info,
    /// Should be fixed.
    Warn,
    /// Must be fixed.
    Error,
}

impl Severity {
    /// Parse `info|warn|error`.
    pub fn parse(s: &str) -> Option<Severity> {
        match s {
            "info" => Some(Severity::Info),
            "warn" => Some(Severity::Warn),
            "error" => Some(Severity::Error),
            _ => None,
        }
    }
}

/// How an indirect link scheme (e.g. `toa:`) is treated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SchemeRule {
    /// Trust the indirection; the target is resolved elsewhere.
    Accept,
}

/// The policy. Every field has a documented default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    /// Kinds that must be linked.
    pub require_link: Vec<Kind>,
    /// Hosts accepted as sources (subdomains included).
    pub allowed_hosts: Vec<String>,
    /// Non-http schemes and how to treat them.
    pub link_schemes: BTreeMap<String, SchemeRule>,
    /// Require a decodable URL to decode to the cited authority.
    pub check_congruence: bool,
    /// Below this confidence, a citation only gets CITE020 (info).
    pub min_confidence: f64,
    /// Severity overrides by code, e.g. `{"CITE003": "error"}`.
    pub severity: BTreeMap<String, Severity>,
    /// Only report these kinds (empty = all).
    pub kinds: Vec<Kind>,
    /// Require links on page furniture (running heads, repeated captions) too.
    pub link_furniture: bool,
    /// Rule selection: only rules in these collections report citations
    /// (empty = every rule), e.g. `["jurisdiction:us.oh", "kind:case"]`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Rule selection: never report these collections; exclude wins.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Named profiles unioned with `include` (`cite-detect profiles list`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<String>,
}

impl Policy {
    /// The rule selector this policy names.
    pub fn selector(&self) -> Selector {
        Selector {
            include: self.include.clone(),
            exclude: self.exclude.clone(),
            profiles: self.profiles.clone(),
        }
    }
}

/// The CITE008 finding for a policy whose selector cannot be applied.
pub fn bad_selector(policy: &Policy, err: &SelectError) -> Finding {
    let info = BAD_SELECTOR;
    Finding {
        code: info.code,
        name: info.name,
        severity: severity_for(policy, &info),
        message: err.to_string(),
        span: Span::default(),
        citation: None,
        rule_id: None,
        kind: None,
        text: String::new(),
        link: None,
        fix: None,
        suppressed: false,
    }
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            require_link: Kind::ALL
                .into_iter()
                .filter(|k| !matches!(k, Kind::Record | Kind::Docket | Kind::Unrecognized))
                .collect(),
            allowed_hosts: [
                "courtlistener.com",
                "law.cornell.edu",
                "govinfo.gov",
                "ecfr.gov",
                "uspto.gov",
                "supremecourt.gov",
                "justia.com",
                "congress.gov",
                "federalregister.gov",
                "uscode.house.gov",
                "uscourts.gov",
            ]
            .map(str::to_string)
            .to_vec(),
            link_schemes: BTreeMap::new(),
            check_congruence: true,
            min_confidence: 0.5,
            severity: BTreeMap::new(),
            kinds: Vec::new(),
            link_furniture: false,
            include: Vec::new(),
            exclude: Vec::new(),
            profiles: Vec::new(),
        }
    }
}

/// Static description of a finding code.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
pub struct CodeInfo {
    /// `CITE001`.
    pub code: &'static str,
    /// `unlinked`.
    pub name: &'static str,
    /// Default severity.
    pub severity: Severity,
    /// Deterministic (true) or heuristic cue (false).
    pub deterministic: bool,
    /// What it means and what to do.
    pub help: &'static str,
}

/// CITE008, built by [`bad_selector`].
const BAD_SELECTOR: CodeInfo = CodeInfo { code: "CITE008", name: "bad-selector", severity: Severity::Error, deterministic: true, help: "policy.include, policy.exclude or policy.profiles names an unknown namespace, kind, family or profile, or a pattern that matches no rule. The library then lints with every rule; the CLI exits 3. See `cite-detect profiles list` and `cite-detect rules list --select`." };

/// Every finding code.
pub const CODES: &[CodeInfo] = &[
    CodeInfo { code: "CITE001", name: "unlinked", severity: Severity::Error, deterministic: true, help: "A citation of a kind in policy.require_link is not inside a link and has no footnote link. Wrap it: [citation](url). The finding's fix.url is a template-built source URL when the rule has one. Page furniture (running heads, repeated captions) is exempt unless policy.link_furniture is true." },
    CodeInfo { code: "CITE002", name: "bad-url", severity: Severity::Error, deterministic: true, help: "The link destination is not an absolute http(s) URL and its scheme is not declared in policy.link_schemes." },
    CodeInfo { code: "CITE003", name: "host-not-allowed", severity: Severity::Warn, deterministic: true, help: "The link host is not on policy.allowed_hosts (subdomains count). Link to a primary or allowlisted source, or add the host to the policy." },
    CodeInfo { code: "CITE004", name: "incongruent", severity: Severity::Error, deterministic: true, help: "The URL decodes to a different authority than the text cites (e.g. text cites 28 U.S.C. 1331, link points to 28 U.S.C. 1332)." },
    CodeInfo { code: "CITE005", name: "undecodable", severity: Severity::Info, deterministic: true, help: "The host is allowed but no rule can decode this URL shape, so the target cannot be confirmed offline. Prefer a decodable URL shape (see cite-detect rules show <rule>)." },
    CodeInfo { code: "CITE006", name: "scheme-accepted", severity: Severity::Info, deterministic: true, help: "The link uses an indirect scheme accepted by policy.link_schemes; the real target is resolved by another tool." },
    BAD_SELECTOR,
    CodeInfo { code: "CITE009", name: "bad-suppression", severity: Severity::Error, deterministic: true, help: "A cite-detect ignore comment has no reason=\"...\". Suppressions must say why." },
    CodeInfo { code: "CITE010", name: "detected", severity: Severity::Info, deterministic: true, help: "Inventory record: a citation was detected. Emitted when linting is off." },
    CodeInfo { code: "CITE011", name: "unrecognized-citation", severity: Severity::Warn, deterministic: true, help: "Citation-shaped text that no specific rule recognizes (volume + abbreviation + page, a section sign after an unknown code name, a docket or agency document number). Check the cite; if it is real, add or fix a rule under rules/ so it is recognized and can be linked. Unrecognized citations are never required to be linked." },
    CodeInfo { code: "CITE020", name: "low-confidence", severity: Severity::Info, deterministic: false, help: "A match below policy.min_confidence: possibly not a citation. Heuristic cue only; never blocks." },
];

/// Look up a code.
pub fn code_info(code: &str) -> Option<&'static CodeInfo> {
    CODES.iter().find(|c| c.code == code)
}

/// A machine-applicable or suggested fix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Fix {
    /// Bytes to replace.
    pub span: Span,
    /// Replacement text.
    pub replacement: String,
    /// Suggested URL.
    pub url: String,
    /// True only when the URL comes from a template of a rule whose encoded
    /// authority was opened; `cite-detect fix --apply` applies only these.
    pub safe: bool,
}

/// One finding.
#[derive(Clone, Debug, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Finding {
    /// Stable code.
    pub code: &'static str,
    /// Code name.
    pub name: &'static str,
    /// Severity after policy overrides.
    pub severity: Severity,
    /// Human message.
    pub message: String,
    /// Where (the citation span; for suppressions, the comment).
    pub span: Span,
    /// Citation index in the document, if any.
    pub citation: Option<usize>,
    /// Rule that detected the citation.
    pub rule_id: Option<String>,
    /// Kind of the citation.
    pub kind: Option<Kind>,
    /// Citation text.
    pub text: String,
    /// Link destination involved.
    pub link: Option<String>,
    /// Fix, when one can be built.
    pub fix: Option<Fix>,
    /// True when an inline suppression covered this finding.
    pub suppressed: bool,
}

/// Lint mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Inventory only: CITE010 per citation, CITE020 for low confidence.
    Detect,
    /// Full lint: link requirements and link-target checks.
    Lint,
}

fn severity_for(policy: &Policy, info: &CodeInfo) -> Severity {
    policy
        .severity
        .get(info.code)
        .copied()
        .unwrap_or(info.severity)
}

struct Ctx<'a> {
    reg: &'a Registry,
    doc: &'a Document,
    policy: &'a Policy,
}

impl Ctx<'_> {
    fn finding(
        &self,
        code: &'static str,
        ci: Option<usize>,
        message: String,
        link: Option<String>,
    ) -> Finding {
        let info = code_info(code).copied().unwrap_or(CodeInfo {
            code,
            name: "unknown",
            severity: Severity::Error,
            deterministic: true,
            help: "",
        });
        let cite = ci.and_then(|i| self.doc.citations.get(i));
        Finding {
            code,
            name: info.name,
            severity: severity_for(self.policy, &info),
            message,
            span: cite.map(|c| c.span).unwrap_or_default(),
            citation: ci,
            rule_id: cite.map(|c| c.rule_id.clone()),
            kind: cite.map(|c| c.kind),
            text: cite.map(|c| c.text.clone()).unwrap_or_default(),
            link,
            fix: None,
            suppressed: false,
        }
    }

    fn check_target(&self, ci: usize, dest: &str, out: &mut Vec<Finding>) {
        let Some(c) = self.doc.citations.get(ci) else {
            return;
        };
        let scheme = dest.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
        if let Some(s) = scheme.as_deref() {
            if self.policy.link_schemes.contains_key(s) {
                out.push(self.finding(
                    "CITE006",
                    Some(ci),
                    format!("link uses accepted scheme {s}:"),
                    Some(dest.to_string()),
                ));
                return;
            }
        }
        let parsed = url::Url::parse(dest)
            .ok()
            .filter(|u| u.scheme() == "http" || u.scheme() == "https");
        let Some(u) = parsed else {
            out.push(self.finding(
                "CITE002",
                Some(ci),
                format!("link destination {dest:?} is not an http(s) URL"),
                Some(dest.to_string()),
            ));
            return;
        };
        let host = host_of(&u).unwrap_or_default();
        if !host_allowed(&host, &self.policy.allowed_hosts) {
            out.push(self.finding(
                "CITE003",
                Some(ci),
                format!("host {host} is not on the allowlist"),
                Some(dest.to_string()),
            ));
            return;
        }
        if !self.policy.check_congruence {
            return;
        }
        match (decode(self.reg, &u), &c.authority) {
            (Some(d), Some(cited)) if &d.authority != cited => {
                out.push(self.finding(
                    "CITE004",
                    Some(ci),
                    format!("text cites {cited} but the link points to {}", d.authority),
                    Some(dest.to_string()),
                ));
            }
            (Some(_), _) => {}
            (None, _) => out.push(self.finding(
                "CITE005",
                Some(ci),
                format!("cannot decode {host} URL to an authority offline"),
                Some(dest.to_string()),
            )),
        }
    }

    fn unlinked(&self, ci: usize) -> Finding {
        let mut f = self.finding("CITE001", Some(ci), String::new(), None);
        let Some(c) = self.doc.citations.get(ci) else {
            return f;
        };
        f.message = format!(
            "{} citation {:?} is not linked to its source",
            c.kind, c.text
        );
        if let Some(url) = suggest(self.reg, c.rule_index, &c.fields) {
            f.fix = Some(Fix {
                span: c.span,
                replacement: format!("[{}]({url})", c.text),
                url,
                safe: c.verified_rule,
            });
        }
        f
    }
}

fn suppressed(doc: &Document, f: &Finding) -> bool {
    doc.suppressions.iter().any(|s| {
        !s.reason.is_empty()
            && s.codes.iter().any(|c| c == f.code || c == "ALL")
            && (s.scope == "file"
                || f.span.line == s.span.end_line + 1
                || (f.span.line == s.span.end_line && f.span.start >= s.span.end))
    })
}

/// Apply the policy to a document.
pub fn evaluate(reg: &Registry, doc: &Document, policy: &Policy, mode: Mode) -> Vec<Finding> {
    let ctx = Ctx { reg, doc, policy };
    let mut out = Vec::new();
    for (ci, c) in doc.citations.iter().enumerate() {
        if !policy.kinds.is_empty() && !policy.kinds.contains(&c.kind) {
            continue;
        }
        if c.kind.is_catch_all() {
            out.push(ctx.finding(
                "CITE011",
                Some(ci),
                format!(
                    "unrecognized citation-shaped text {:?} ({})",
                    c.text, c.rule_id
                ),
                None,
            ));
            continue;
        }
        if c.confidence < policy.min_confidence {
            out.push(ctx.finding(
                "CITE020",
                Some(ci),
                format!(
                    "low-confidence match ({:.2}): may not be a citation",
                    c.confidence
                ),
                None,
            ));
            continue;
        }
        if mode == Mode::Detect {
            out.push(ctx.finding(
                "CITE010",
                Some(ci),
                format!("{} citation ({})", c.kind, c.rule_id),
                None,
            ));
            continue;
        }
        let bindings = doc.bindings.get(ci).map_or(&[][..], Vec::as_slice);
        if bindings.is_empty() {
            if policy.require_link.contains(&c.kind)
                && (c.furniture.is_none() || policy.link_furniture)
            {
                out.push(ctx.unlinked(ci));
            }
            continue;
        }
        // Direct links take precedence over footnote links.
        let direct: Vec<_> = bindings.iter().filter(|b| b.via == Via::Direct).collect();
        let chosen = if direct.is_empty() {
            bindings.iter().collect()
        } else {
            direct
        };
        let mut target_findings = Vec::new();
        for b in chosen {
            if let Some(l) = doc.links.get(b.link) {
                let mut fs = Vec::new();
                ctx.check_target(ci, &l.dest, &mut fs);
                // Any clean binding satisfies the citation.
                if fs.iter().all(|f| f.severity == Severity::Info) {
                    target_findings = fs;
                    break;
                }
                target_findings.extend(fs);
            }
        }
        out.extend(target_findings);
    }
    for s in &doc.suppressions {
        if s.reason.is_empty() {
            let mut f = ctx.finding(
                "CITE009",
                None,
                "cite-detect ignore comment without reason=\"...\"".into(),
                None,
            );
            f.span = s.span;
            out.push(f);
        }
    }
    for f in &mut out {
        f.suppressed = suppressed(doc, f);
    }
    out.sort_by(|a, b| a.span.start.cmp(&b.span.start).then(a.code.cmp(b.code)));
    out
}
