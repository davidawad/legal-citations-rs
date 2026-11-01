//! Output renderers. The JSON shapes are stable and schema'd
//! (`cite-detect schema report|finding`).

use legal_citations::authority::Form;
use legal_citations::{registry, Authority, Citation, Document, Finding, Kind, Mode, Policy};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Schema id of the report and JSONL records.
pub const SCHEMA_ID: &str = "cite-detect.report/1";

/// Findings for one input.
pub struct FileReport {
    /// Input label.
    pub file: String,
    /// L4 document.
    pub doc: Document,
    /// L6 findings.
    pub findings: Vec<Finding>,
}

/// Run-wide context.
pub struct Context<'a> {
    /// Effective policy.
    pub policy: &'a Policy,
    /// Detect or lint.
    pub mode: Mode,
    /// Overall pass/fail.
    pub passed: bool,
}

/// First record: what produced the report.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Header {
    /// Always `header`.
    #[serde(rename = "type")]
    pub record: &'static str,
    /// `cite-detect.report/1`.
    pub schema: &'static str,
    /// Tool version.
    pub version: &'static str,
    /// Hash of the embedded rule registry.
    pub registry_hash: String,
    /// Hash of the effective policy.
    pub policy_hash: String,
    /// `detect` or `lint`.
    pub mode: &'static str,
}

/// Last record: counts and the verdict.
#[derive(Serialize, schemars::JsonSchema, Default)]
pub struct Summary {
    /// Always `summary`.
    #[serde(rename = "type")]
    pub record: &'static str,
    /// Files read.
    pub files: usize,
    /// Citations detected.
    pub citations: usize,
    /// Citations with at least one link.
    pub linked: usize,
    /// Unsuppressed findings by code.
    pub by_code: BTreeMap<String, usize>,
    /// Citations by kind.
    pub by_kind: BTreeMap<String, usize>,
    /// Citations by rule.
    pub by_rule: BTreeMap<String, usize>,
    /// Findings covered by a suppression comment.
    pub suppressed: usize,
    /// False when an unsuppressed finding reached `--fail-on`.
    pub passed: bool,
}

/// One file in the JSON report.
#[derive(Serialize, schemars::JsonSchema)]
pub struct JsonFile {
    /// Input label.
    pub file: String,
    /// The document (citations, links, bindings, zones, suppressions).
    pub document: Document,
    /// Findings.
    pub findings: Vec<Finding>,
}

/// `--format json`.
#[derive(Serialize, schemars::JsonSchema)]
pub struct JsonReport {
    /// Header.
    pub header: Header,
    /// Per-file results.
    pub files: Vec<JsonFile>,
    /// Summary.
    pub summary: Summary,
}

fn fnv(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn header(ctx: &Context<'_>) -> Header {
    Header {
        record: "header",
        schema: SCHEMA_ID,
        version: env!("CARGO_PKG_VERSION"),
        registry_hash: registry().hash(),
        policy_hash: fnv(&serde_json::to_string(ctx.policy).unwrap_or_default()),
        mode: match ctx.mode {
            Mode::Detect => "detect",
            Mode::Lint => "lint",
        },
    }
}

fn summary(ctx: &Context<'_>, reports: &[FileReport]) -> Summary {
    let mut s = Summary {
        record: "summary",
        files: reports.len(),
        passed: ctx.passed,
        ..Summary::default()
    };
    for r in reports {
        s.citations += r.doc.citations.len();
        s.linked += (0..r.doc.citations.len())
            .filter(|i| r.doc.is_linked(*i))
            .count();
        for c in &r.doc.citations {
            *s.by_kind.entry(c.kind.as_str().to_string()).or_default() += 1;
            *s.by_rule.entry(c.rule_id.clone()).or_default() += 1;
        }
        for f in &r.findings {
            if f.suppressed {
                s.suppressed += 1;
            } else {
                *s.by_code.entry(f.code.to_string()).or_default() += 1;
            }
        }
    }
    s
}

/// `--format json`.
pub fn json(ctx: &Context<'_>, reports: &[FileReport]) -> Result<String, String> {
    let report = JsonReport {
        header: header(ctx),
        files: reports
            .iter()
            .map(|r| JsonFile {
                file: r.file.clone(),
                document: r.doc.clone(),
                findings: r.findings.clone(),
            })
            .collect(),
        summary: summary(ctx, reports),
    };
    serde_json::to_string_pretty(&report)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct CitationRecord<'a> {
    #[serde(rename = "type")]
    record: &'static str,
    file: &'a str,
    index: usize,
    linked: bool,
    #[serde(flatten)]
    citation: &'a Citation,
}

#[derive(Serialize)]
struct FindingRecord<'a> {
    #[serde(rename = "type")]
    record: &'static str,
    file: &'a str,
    #[serde(flatten)]
    finding: &'a Finding,
}

/// `--format jsonl`: header, citation and finding records, summary.
pub fn jsonl(ctx: &Context<'_>, reports: &[FileReport]) -> Result<String, String> {
    let mut out = String::new();
    let mut line = |v: &dyn erased::Ser| -> Result<(), String> {
        out.push_str(&v.to_json()?);
        out.push('\n');
        Ok(())
    };
    line(&header(ctx))?;
    for r in reports {
        for (index, citation) in r.doc.citations.iter().enumerate() {
            line(&CitationRecord {
                record: "citation",
                file: &r.file,
                index,
                linked: r.doc.is_linked(index),
                citation,
            })?;
        }
        for finding in &r.findings {
            line(&FindingRecord {
                record: "finding",
                file: &r.file,
                finding,
            })?;
        }
    }
    line(&summary(ctx, reports))?;
    Ok(out)
}

mod erased {
    /// Object-safe "serialize me to one JSON line".
    pub trait Ser {
        fn to_json(&self) -> Result<String, String>;
    }
    impl<T: serde::Serialize> Ser for T {
        fn to_json(&self) -> Result<String, String> {
            serde_json::to_string(self).map_err(|e| e.to_string())
        }
    }
}

/// `--format human`: one line per unsuppressed finding plus a summary line.
pub fn human(ctx: &Context<'_>, reports: &[FileReport]) -> Result<String, String> {
    let mut out = String::new();
    for r in reports {
        for f in r.findings.iter().filter(|f| !f.suppressed) {
            let _ = writeln!(
                out,
                "{}:{}:{}: {} {} [{:?}] {}",
                r.file, f.span.line, f.span.col, f.code, f.name, f.severity, f.message
            );
            if let Some(fix) = &f.fix {
                let _ = writeln!(out, "    fix: {}", fix.url);
            }
        }
    }
    let s = summary(ctx, reports);
    let _ = writeln!(
        out,
        "{} citation(s), {} linked, {} finding(s), {} suppressed: {}",
        s.citations,
        s.linked,
        s.by_code.values().sum::<usize>(),
        s.suppressed,
        if s.passed { "PASS" } else { "FAIL" }
    );
    Ok(out)
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// `--format md-report`: a markdown table per file.
pub fn md_report(ctx: &Context<'_>, reports: &[FileReport]) -> Result<String, String> {
    let mut out = String::from("# cite-detect report\n\n");
    for r in reports {
        let _ = writeln!(out, "## {}\n", md_escape(&r.file));
        let _ = writeln!(
            out,
            "| Line | Code | Severity | Citation | Message |\n| --- | --- | --- | --- | --- |"
        );
        for f in r.findings.iter().filter(|f| !f.suppressed) {
            let _ = writeln!(
                out,
                "| {} | {} {} | {:?} | {} | {} |",
                f.span.line,
                f.code,
                f.name,
                f.severity,
                md_escape(&f.text),
                md_escape(&f.message)
            );
        }
        out.push('\n');
    }
    let s = summary(ctx, reports);
    let _ = writeln!(
        out,
        "**{}**: {} citation(s), {} linked, {} suppressed.",
        if s.passed { "PASS" } else { "FAIL" },
        s.citations,
        s.linked,
        s.suppressed
    );
    Ok(out)
}

/// Authorities of one input.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AuthorityFile {
    /// Input label.
    pub file: String,
    /// Citations detected (after selection).
    pub citations: usize,
    /// Authorities, grouped and sorted.
    pub authorities: Vec<Authority>,
}

/// Header of an authorities report.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AuthoritiesHeader {
    /// Always `header`.
    #[serde(rename = "type")]
    pub record: &'static str,
    /// `cite-detect.authorities/1`.
    pub schema: &'static str,
    /// Tool version.
    pub version: &'static str,
    /// Hash of the embedded rule registry.
    pub registry_hash: String,
}

/// `authorities --format json`.
#[derive(Serialize, schemars::JsonSchema)]
pub struct AuthoritiesReport {
    /// Header.
    pub header: AuthoritiesHeader,
    /// Per-file tables.
    pub files: Vec<AuthorityFile>,
}

fn authorities_header() -> AuthoritiesHeader {
    AuthoritiesHeader {
        record: "header",
        schema: "cite-detect.authorities/1",
        version: env!("CARGO_PKG_VERSION"),
        registry_hash: registry().hash(),
    }
}

/// `authorities --format json`.
pub fn authorities_json(files: &[AuthorityFile]) -> Result<String, String> {
    #[derive(Serialize)]
    struct Borrowed<'a> {
        header: AuthoritiesHeader,
        files: &'a [AuthorityFile],
    }
    serde_json::to_string_pretty(&Borrowed {
        header: authorities_header(),
        files,
    })
    .map(|s| s + "\n")
    .map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct AuthorityRecord<'a> {
    #[serde(rename = "type")]
    record: &'static str,
    file: &'a str,
    #[serde(flatten)]
    authority: &'a Authority,
}

#[derive(Serialize)]
struct AuthoritySummary {
    #[serde(rename = "type")]
    record: &'static str,
    files: usize,
    citations: usize,
    authorities: usize,
    by_kind: BTreeMap<String, usize>,
}

/// `authorities --format jsonl`: header, one record per authority, summary.
pub fn authorities_jsonl(files: &[AuthorityFile]) -> Result<String, String> {
    use erased::Ser as _;
    let mut out = authorities_header().to_json()? + "\n";
    let mut sum = AuthoritySummary {
        record: "summary",
        files: files.len(),
        citations: 0,
        authorities: 0,
        by_kind: BTreeMap::new(),
    };
    for f in files {
        sum.citations += f.citations;
        for a in &f.authorities {
            sum.authorities += 1;
            *sum.by_kind.entry(a.kind.as_str().to_string()).or_default() += 1;
            out.push_str(
                &AuthorityRecord {
                    record: "authority",
                    file: &f.file,
                    authority: a,
                }
                .to_json()?,
            );
            out.push('\n');
        }
    }
    out.push_str(&sum.to_json()?);
    out.push('\n');
    Ok(out)
}

fn kind_heading(k: Kind) -> &'static str {
    match k {
        Kind::Case | Kind::CaseShort => "Cases",
        Kind::Statute | Kind::StatuteShort => "Statutes",
        Kind::SessionLaw => "Session laws",
        Kind::Regulation => "Regulations",
        Kind::FederalRegister => "Federal Register",
        Kind::Constitution => "Constitutional provisions",
        Kind::CourtRule => "Court rules",
        Kind::AgencyManual => "Agency manuals",
        Kind::AgencyDecision => "Agency decisions",
        Kind::Record => "Record",
        Kind::Secondary => "Secondary sources",
        Kind::Treaty => "Treaties",
        Kind::Foreign => "Foreign authorities",
        Kind::Docket => "Dockets",
        Kind::Legislative => "Legislative materials",
        Kind::Executive => "Executive materials",
        Kind::Unrecognized => "Unrecognized citations",
        Kind::Id | Kind::Supra => "Other",
    }
}

/// `authorities --format md`: a table of authorities per file, one section per kind.
pub fn authorities_md(files: &[AuthorityFile]) -> String {
    let mut out = String::from("# Table of authorities\n");
    for f in files {
        let _ = writeln!(out, "\n## {}\n", md_escape(&f.file));
        if f.authorities.is_empty() {
            out.push_str("No citations.\n");
            continue;
        }
        let mut heading = "";
        for a in &f.authorities {
            let h = kind_heading(a.kind);
            if h != heading {
                heading = h;
                let _ = writeln!(
                    out,
                    "\n### {h}\n\n| Authority | Citations | Short forms | Pages cited | Lines |\n| --- | --- | --- | --- | --- |"
                );
            }
            let mut cite = md_escape(&a.full_cite);
            for h in &a.subsequent_history {
                let _ = write!(cite, ", {}, {}", md_escape(&h.phrase), md_escape(&h.text));
            }
            if let Some(url) = a.urls.first() {
                cite = format!("[{cite}]({url})");
            }
            let mut lines: Vec<usize> = a
                .occurrences
                .iter()
                .filter(|o| o.form != Form::History)
                .map(|o| o.span.line)
                .collect();
            lines.dedup();
            let lines: Vec<String> = lines.iter().map(ToString::to_string).collect();
            let _ = writeln!(
                out,
                "| {cite} | {} | {} | {} | {} |",
                a.occurrences
                    .iter()
                    .filter(|o| matches!(o.form, Form::Full | Form::Short))
                    .count(),
                a.short_forms,
                md_escape(&a.pages_cited.join(", ")),
                lines.join(", ")
            );
        }
    }
    out
}
