//! `cite-detect`: find legal citations and lint markdown so every citation
//! links to its source. Run `cite-detect --help`; every output format has a
//! JSON Schema (`cite-detect schema`), every finding code and rule has an
//! explanation (`cite-detect explain`), and every pipeline layer can be dumped
//! (`cite-detect inspect`).
//!
//! Exit codes: 0 pass, 1 findings at or above `--fail-on`, 2 usage,
//! 3 policy/config, 4 input, 5 runtime.

mod output;

use clap::{Parser, Subcommand, ValueEnum};
use legal_citations::coverage;
use legal_citations::export;
use legal_citations::policy::{code_info, SchemeRule, CODES};
use legal_citations::registry::{portability_warnings, summaries, validate};
use legal_citations::select::{profiles, validate_profiles};
use legal_citations::{
    detect_markdown, detect_markdown_with, detect_with, evaluate, registry, Document, Kind, Mode,
    Policy, RuleSet, Selection, Selector, Severity,
};
use std::io::Read as _;
use std::process::ExitCode;

/// `--version`: the package version and the git commit it was built from.
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("CITE_DETECT_GIT_SHA"),
    ")"
);

/// Detect legal citations; lint markdown so every citation links to its source.
#[derive(Parser, Debug)]
#[command(name = "cite-detect", version = VERSION, about, long_about = None, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    check: CheckArgs,
}

#[derive(clap::Args, Debug, Default)]
struct CheckArgs {
    /// Files to read; `-` or nothing reads stdin.
    files: Vec<String>,
    /// Input format; `auto` = markdown for .md/.markdown/.mdx and stdin, text otherwise.
    #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
    input: InputFormat,
    /// Output format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Human)]
    format: OutputFormat,
    /// Only these kinds (comma separated), e.g. `case,statute`.
    #[arg(long, value_delimiter = ',')]
    kinds: Vec<String>,
    /// Lint: require every citation of a linkable kind to be linked to its source.
    #[arg(long)]
    require_links: bool,
    /// Policy JSON file (implies --require-links). See `cite-detect schema policy`.
    #[arg(long)]
    policy: Option<String>,
    /// Exit 1 when an unsuppressed finding is at least this severe.
    #[arg(long, value_enum, default_value_t = FailOn::Error)]
    fail_on: FailOn,
    /// Override policy.min_confidence.
    #[arg(long)]
    min_confidence: Option<f64>,
    /// Add an allowed link host (repeatable).
    #[arg(long = "allow-host")]
    allow_host: Vec<String>,
    /// Accept an indirect link scheme such as `toa` (repeatable).
    #[arg(long = "accept-scheme")]
    accept_scheme: Vec<String>,
    #[command(flatten)]
    select: SelectArgs,
}

/// Rule selection flags, added to the policy's `include`/`exclude`/`profiles`.
#[derive(clap::Args, Debug, Default, Clone)]
struct SelectArgs {
    /// Use a named profile (repeatable); see `cite-detect profiles list`.
    #[arg(long = "profile", value_delimiter = ',')]
    profile: Vec<String>,
    /// Only report rules in these collections (repeatable, comma separated):
    /// `jurisdiction:us.oh*`, `court:E.D. Tex.`, `kind:case`, `family:agency`,
    /// `tag:ptab`, `rule:<id>` or a bare rule-id glob such as `us.fed.*`.
    #[arg(long, visible_alias = "select", value_delimiter = ',')]
    include: Vec<String>,
    /// Never report rules in these collections; exclude wins over include.
    #[arg(long, value_delimiter = ',')]
    exclude: Vec<String>,
    /// Shorthand for `--include jurisdiction:<J>` (repeatable, ORed, globs
    /// allowed): `--jurisdiction us.oh`, `--jurisdiction us.fed.dist.txed`
    /// (a citation's court; see `cite-detect courts`).
    #[arg(long, value_delimiter = ',')]
    jurisdiction: Vec<String>,
}

impl SelectArgs {
    fn selector(&self) -> Selector {
        let mut include = self.include.clone();
        include.extend(
            self.jurisdiction
                .iter()
                .map(|j| format!("jurisdiction:{j}")),
        );
        Selector {
            include,
            exclude: self.exclude.clone(),
            profiles: self.profile.clone(),
        }
    }
}

/// Resolve a selector against the embedded registry; failures exit 3.
fn selection(sel: &Selector) -> Result<Selection<'static>, Fail> {
    registry()
        .select(sel)
        .map_err(|e| Fail::config(e.to_string()))
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
enum InputFormat {
    #[default]
    Auto,
    Md,
    Text,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
enum OutputFormat {
    /// `file:line:col CODE severity message` lines plus a summary.
    #[default]
    Human,
    /// One JSON document.
    Json,
    /// Header, citation, finding and summary records, one per line.
    Jsonl,
    /// A markdown report table.
    MdReport,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
enum AuthFormat {
    /// A markdown table of authorities, one section per kind.
    #[default]
    Md,
    /// One JSON document.
    Json,
    /// Header, one record per authority, summary.
    Jsonl,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum, Default)]
enum FailOn {
    Info,
    Warn,
    #[default]
    Error,
    Never,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List, show or validate the rule registry.
    Rules {
        #[command(subcommand)]
        action: Option<RulesAction>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Explain a finding code (CITE001) or a rule id (us.fed.statute.usc).
    Explain {
        /// Code or rule id.
        what: String,
    },
    /// Dump one pipeline layer for a file as JSON.
    Inspect {
        /// File (`-` for stdin).
        file: String,
        /// Layer to dump.
        #[arg(long, value_enum, default_value_t = Stage::Document)]
        stage: Stage,
        /// Input format.
        #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
        input: InputFormat,
    },
    /// Print JSON Schema for an output or input format.
    Schema {
        /// Which schema.
        #[arg(value_enum, default_value_t = SchemaKind::Report)]
        which: SchemaKind,
    },
    /// Table of authorities: citations grouped by authority, with every
    /// occurrence, short-form count and pages cited.
    Authorities {
        /// Files to read; `-` or nothing reads stdin.
        files: Vec<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = AuthFormat::Md)]
        format: AuthFormat,
        /// Input format.
        #[arg(long, value_enum, default_value_t = InputFormat::Auto)]
        input: InputFormat,
        #[command(flatten)]
        select: SelectArgs,
    },
    /// List or show the named rule profiles (`--profile NAME`).
    Profiles {
        #[command(subcommand)]
        action: Option<ProfilesAction>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// The court table: each court abbreviation and its court-level
    /// jurisdiction id, for `--jurisdiction` and `court:` selectors.
    Courts {
        /// Only courts whose abbreviation or id contains this text.
        filter: Option<String>,
        /// Emit JSON.
        #[arg(long)]
        json: bool,
    },
    /// Run detection over a citation-format inventory
    /// (`corpus/formats/indigo.jsonl`) and report which formats are found.
    Coverage {
        /// Inventory file (JSON Lines: section, jurisdiction, kind, form, examples).
        file: String,
        /// Output format.
        #[arg(long, value_enum, default_value_t = CoverageFormat::Md)]
        format: CoverageFormat,
        /// Baseline JSON (`{"formats_detected": N, "examples_detected": M}`):
        /// exit 1 when this run detects fewer.
        #[arg(long)]
        baseline: Option<String>,
        /// Write this run's counts to the `--baseline` file instead of checking.
        #[arg(long, requires = "baseline")]
        update_baseline: bool,
    },
    /// Wrap unlinked citations in links built from rule URL templates.
    Fix {
        /// Markdown file to fix.
        file: String,
        /// Write the file. Without it, print the planned edits as JSON.
        #[arg(long)]
        apply: bool,
        /// Also apply fixes from rules whose encoded authority was not opened.
        #[arg(long = "include-unverified")]
        include_unverified: bool,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum CoverageFormat {
    /// Markdown report (totals, by kind, by jurisdiction, misses).
    Md,
    /// The full per-line result as JSON.
    Json,
}

#[derive(Subcommand, Debug)]
enum RulesAction {
    /// One row per rule.
    List {
        /// Only this kind.
        #[arg(long)]
        kind: Option<String>,
        #[command(flatten)]
        select: SelectArgs,
    },
    /// The full rule definition.
    Show {
        /// Rule id.
        id: String,
    },
    /// Check every rule's contract; exit 1 on any violation. Patterns only
    /// Go RE2 cannot compile are reported on stderr and do not fail.
    Validate,
    /// The README coverage table (markdown): one row per kind and family.
    Table,
    /// The release bundle (MIT rule data) for other languages: every rule
    /// with its ECMAScript form, as JSON. Exit 1 if a pattern cannot be bundled.
    Export,
    /// Test vectors from the rule examples, for checking a port, as JSON.
    Vectors,
}

#[derive(Subcommand, Debug)]
enum ProfilesAction {
    /// One row per profile.
    List,
    /// A profile and the rules it selects.
    Show {
        /// Profile name.
        name: String,
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum Stage {
    /// L1 raw rule matches (before overlap resolution).
    Recognize,
    /// Markdown with non-prose masked to spaces (what detection sees).
    Mask,
    /// L4 document: citations, links, bindings, zones, suppressions.
    Document,
    /// L6 findings under the default policy.
    Findings,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum SchemaKind {
    /// The `--format json` report.
    Report,
    /// One JSONL finding record.
    Finding,
    /// One citation.
    Citation,
    /// A rule file.
    Rule,
    /// A policy file.
    Policy,
    /// The `authorities --format json` report.
    Authorities,
    /// The `rules export` bundle.
    Bundle,
    /// The `rules vectors` test vectors.
    Vectors,
}

/// A failure with its exit code.
struct Fail {
    code: u8,
    message: String,
}

impl Fail {
    fn usage(m: impl Into<String>) -> Fail {
        Fail {
            code: 2,
            message: m.into(),
        }
    }
    fn config(m: impl Into<String>) -> Fail {
        Fail {
            code: 3,
            message: m.into(),
        }
    }
    fn input(m: impl Into<String>) -> Fail {
        Fail {
            code: 4,
            message: m.into(),
        }
    }
    fn runtime(m: impl Into<String>) -> Fail {
        Fail {
            code: 5,
            message: m.into(),
        }
    }
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let code = if e.use_stderr() { 2 } else { 0 };
            let _ = e.print();
            return ExitCode::from(code);
        }
    };
    match run(cli) {
        Ok(code) => ExitCode::from(code),
        Err(f) => {
            eprintln!("cite-detect: {}", f.message);
            ExitCode::from(f.code)
        }
    }
}

fn run(cli: Cli) -> Result<u8, Fail> {
    match cli.command {
        None => check(&cli.check),
        Some(Command::Rules { action, json }) => rules(action, json),
        Some(Command::Profiles { action, json }) => profiles_cmd(action, json),
        Some(Command::Courts { filter, json }) => courts_cmd(filter.as_deref(), json),
        Some(Command::Authorities {
            files,
            format,
            input,
            select,
        }) => authorities_cmd(&files, format, input, &select),
        Some(Command::Explain { what }) => explain(&what),
        Some(Command::Inspect { file, stage, input }) => inspect(&file, stage, input),
        Some(Command::Schema { which }) => schema(which),
        Some(Command::Fix {
            file,
            apply,
            include_unverified,
        }) => fix(&file, apply, include_unverified),
        Some(Command::Coverage {
            file,
            format,
            baseline,
            update_baseline,
        }) => coverage_cmd(&file, format, baseline.as_deref(), update_baseline),
    }
}

fn coverage_cmd(
    file: &str,
    format: CoverageFormat,
    baseline: Option<&str>,
    update: bool,
) -> Result<u8, Fail> {
    let text = read_input(file)?;
    let report = coverage::run(&text).map_err(|e| Fail::input(format!("{file}: {e}")))?;
    let body = match format {
        CoverageFormat::Md => coverage::markdown(&report, file),
        CoverageFormat::Json => {
            let mut j =
                serde_json::to_string_pretty(&report).map_err(|e| Fail::runtime(e.to_string()))?;
            j.push('\n');
            j
        }
    };
    print!("{body}");
    let Some(path) = baseline else { return Ok(0) };
    if update {
        let mut j = serde_json::to_string_pretty(&report.baseline())
            .map_err(|e| Fail::runtime(e.to_string()))?;
        j.push('\n');
        std::fs::write(path, j).map_err(|e| Fail::runtime(format!("{path}: {e}")))?;
        return Ok(0);
    }
    let base: coverage::Baseline = serde_json::from_str(
        &std::fs::read_to_string(path).map_err(|e| Fail::config(format!("{path}: {e}")))?,
    )
    .map_err(|e| Fail::config(format!("{path}: {e}")))?;
    let regressions = report.regressions(&base);
    for r in &regressions {
        eprintln!("cite-detect: coverage: {r}");
    }
    Ok(u8::from(!regressions.is_empty()))
}

fn read_input(path: &str) -> Result<String, Fail> {
    if path == "-" {
        let mut s = String::new();
        std::io::stdin()
            .read_to_string(&mut s)
            .map_err(|e| Fail::input(format!("stdin: {e}")))?;
        return Ok(s);
    }
    std::fs::read_to_string(path).map_err(|e| Fail::input(format!("{path}: {e}")))
}

fn is_markdown(path: &str, input: InputFormat) -> bool {
    match input {
        InputFormat::Md => true,
        InputFormat::Text => false,
        InputFormat::Auto => {
            path == "-"
                || [".md", ".markdown", ".mdx"]
                    .iter()
                    .any(|ext| path.to_ascii_lowercase().ends_with(ext))
        }
    }
}

fn document<R: RuleSet + ?Sized>(rules: &R, text: &str, markdown: bool) -> Document {
    if markdown {
        detect_markdown_with(rules, text)
    } else {
        Document::plain(detect_with(rules, text))
    }
}

fn load_policy(args: &CheckArgs) -> Result<Policy, Fail> {
    let mut policy = match &args.policy {
        Some(path) => {
            let text =
                std::fs::read_to_string(path).map_err(|e| Fail::config(format!("{path}: {e}")))?;
            serde_json::from_str::<Policy>(&text)
                .map_err(|e| Fail::config(format!("{path}: {e}")))?
        }
        None => Policy::default(),
    };
    for k in &args.kinds {
        let kind = Kind::parse(k).ok_or_else(|| {
            let all: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
            Fail::usage(format!("unknown kind {k:?}; kinds: {}", all.join(",")))
        })?;
        policy.kinds.push(kind);
    }
    if let Some(m) = args.min_confidence {
        if !(0.0..=1.0).contains(&m) {
            return Err(Fail::usage("--min-confidence must be within 0..=1"));
        }
        policy.min_confidence = m;
    }
    policy.allowed_hosts.extend(args.allow_host.iter().cloned());
    for s in &args.accept_scheme {
        policy
            .link_schemes
            .insert(s.to_ascii_lowercase(), SchemeRule::Accept);
    }
    let extra = args.select.selector();
    policy.include.extend(extra.include);
    policy.exclude.extend(extra.exclude);
    policy.profiles.extend(extra.profiles);
    Ok(policy)
}

fn threshold(f: FailOn) -> Option<Severity> {
    match f {
        FailOn::Info => Some(Severity::Info),
        FailOn::Warn => Some(Severity::Warn),
        FailOn::Error => Some(Severity::Error),
        FailOn::Never => None,
    }
}

fn check(args: &CheckArgs) -> Result<u8, Fail> {
    let policy = load_policy(args)?;
    let sel = selection(&policy.selector())?;
    let mode = if args.require_links || args.policy.is_some() {
        Mode::Lint
    } else {
        Mode::Detect
    };
    let files: Vec<String> = if args.files.is_empty() {
        vec!["-".into()]
    } else {
        args.files.clone()
    };
    let mut reports = Vec::new();
    for path in &files {
        let text = read_input(path)?;
        let doc = document(&sel, &text, is_markdown(path, args.input));
        let findings = evaluate(registry(), &doc, &policy, mode);
        reports.push(output::FileReport {
            file: path.clone(),
            doc,
            findings,
        });
    }
    let fail_at = threshold(args.fail_on);
    let failed = reports
        .iter()
        .flat_map(|r| &r.findings)
        .any(|f| !f.suppressed && fail_at.is_some_and(|t| f.severity >= t));
    let ctx = output::Context {
        policy: &policy,
        mode,
        passed: !failed,
    };
    let rendered = match args.format {
        OutputFormat::Human => output::human(&ctx, &reports),
        OutputFormat::Json => output::json(&ctx, &reports),
        OutputFormat::Jsonl => output::jsonl(&ctx, &reports),
        OutputFormat::MdReport => output::md_report(&ctx, &reports),
    }
    .map_err(Fail::runtime)?;
    print!("{rendered}");
    Ok(u8::from(failed))
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String, Fail> {
    serde_json::to_string_pretty(v)
        .map(|s| s + "\n")
        .map_err(|e| Fail::runtime(e.to_string()))
}

fn rules(action: Option<RulesAction>, json: bool) -> Result<u8, Fail> {
    let reg = registry();
    match action.unwrap_or(RulesAction::List {
        kind: None,
        select: SelectArgs::default(),
    }) {
        RulesAction::List { kind, select } => {
            let want = match kind.as_deref() {
                Some(k) => {
                    Some(Kind::parse(k).ok_or_else(|| Fail::usage(format!("unknown kind {k:?}")))?)
                }
                None => None,
            };
            let sel = select.selector();
            // Without a selector, list retired rules too (they are part of the registry).
            let view = (!sel.is_empty()).then(|| selection(&sel)).transpose()?;
            let rows: Vec<_> = summaries(reg)
                .into_iter()
                .enumerate()
                .filter(|(i, r)| {
                    want.is_none_or(|k| r.kind == k) && view.as_ref().is_none_or(|v| v.contains(*i))
                })
                .map(|(_, r)| r)
                .collect();
            if json {
                print!("{}", to_json(&rows)?);
            } else {
                for r in &rows {
                    let status = match (&r.retired, r.verified) {
                        (Some(_), _) => "retired",
                        (None, true) => "verified",
                        (None, false) => "unverified",
                    };
                    println!(
                        "{:<44} {:<16} {:<13} {:<14} {:<10} {}",
                        r.id,
                        r.kind.as_str(),
                        r.family.as_str(),
                        r.jurisdiction,
                        status,
                        r.encodes
                    );
                }
                println!("{} rules", rows.len());
            }
            Ok(0)
        }
        RulesAction::Show { id } => {
            let rule = reg.get(&id).ok_or_else(|| {
                Fail::usage(format!("no rule {id:?}; see `cite-detect rules list`"))
            })?;
            print!("{}", to_json(&rule.def)?);
            Ok(0)
        }
        RulesAction::Table => {
            print!("{}", coverage::rules_table(reg));
            Ok(0)
        }
        RulesAction::Export => match export::bundle(reg) {
            Ok(b) => {
                print!("{}", to_json(&b)?);
                Ok(0)
            }
            Err(v) => {
                for x in &v {
                    eprintln!("{} [{}] {}", x.rule, x.check, x.detail);
                }
                Ok(1)
            }
        },
        RulesAction::Vectors => {
            print!("{}", to_json(&export::vectors(reg))?);
            Ok(0)
        }
        RulesAction::Validate => {
            let mut v = validate(reg);
            v.extend(validate_profiles(reg, profiles()));
            let warnings = portability_warnings(reg);
            for w in &warnings {
                eprintln!("warning: {} [{}] {}", w.rule, w.check, w.detail);
            }
            if json {
                print!("{}", to_json(&v)?);
            } else {
                for x in &v {
                    println!("{} [{}] {}", x.rule, x.check, x.detail);
                }
                println!(
                    "{} rules, {} profiles, {} violations, {} portability warnings",
                    reg.rules.len(),
                    profiles().profiles.len(),
                    v.len(),
                    warnings.len()
                );
            }
            Ok(u8::from(!v.is_empty()))
        }
    }
}

fn authorities_cmd(
    files: &[String],
    format: AuthFormat,
    input: InputFormat,
    select: &SelectArgs,
) -> Result<u8, Fail> {
    let sel = selection(&select.selector())?;
    let files: Vec<String> = if files.is_empty() {
        vec!["-".into()]
    } else {
        files.to_vec()
    };
    let mut tables = Vec::new();
    for path in &files {
        let text = read_input(path)?;
        let doc = document(&sel, &text, is_markdown(path, input));
        tables.push(output::AuthorityFile {
            file: path.clone(),
            authorities: legal_citations::authorities(&doc.citations),
            citations: doc.citations.len(),
        });
    }
    let rendered = match format {
        AuthFormat::Md => Ok(output::authorities_md(&tables)),
        AuthFormat::Json => output::authorities_json(&tables),
        AuthFormat::Jsonl => output::authorities_jsonl(&tables),
    }
    .map_err(Fail::runtime)?;
    print!("{rendered}");
    Ok(0)
}

fn courts_cmd(filter: Option<&str>, json: bool) -> Result<u8, Fail> {
    let rows: Vec<_> = legal_citations::court::courts()
        .courts()
        .iter()
        .filter(|c| filter.is_none_or(|f| c.court.contains(f) || c.jurisdiction.contains(f)))
        .collect();
    if json {
        print!("{}", to_json(&rows)?);
    } else {
        for c in &rows {
            println!("{:<28} {}", c.court, c.jurisdiction);
        }
        println!("{} courts", rows.len());
    }
    Ok(0)
}

fn profiles_cmd(action: Option<ProfilesAction>, json: bool) -> Result<u8, Fail> {
    let set = profiles();
    let rules_of = |name: &str| -> Result<Vec<String>, Fail> {
        let sel = selection(&Selector {
            profiles: vec![name.to_string()],
            ..Selector::default()
        })?;
        Ok(sel.rules().map(|(_, r)| r.def.id.clone()).collect())
    };
    match action.unwrap_or(ProfilesAction::List) {
        ProfilesAction::List => {
            let mut rows = Vec::new();
            for p in &set.profiles {
                rows.push((p, rules_of(&p.name)?.len()));
            }
            if json {
                let v: Vec<_> = rows
                    .iter()
                    .map(|(p, n)| serde_json::json!({"name": p.name, "rules": n, "description": p.description}))
                    .collect();
                print!("{}", to_json(&v)?);
            } else {
                for (p, n) in &rows {
                    println!("{:<20} {:>4} rules  {}", p.name, n, p.description);
                }
                println!("{} profiles", rows.len());
            }
            Ok(0)
        }
        ProfilesAction::Show { name } => {
            let rules = rules_of(&name)?;
            let p = set
                .get(&name)
                .ok_or_else(|| Fail::config(format!("unknown profile {name:?}")))?;
            if json {
                print!(
                    "{}",
                    to_json(&serde_json::json!({"profile": p, "rules": rules}))?
                );
            } else {
                println!("{}: {}", p.name, p.description);
                println!("include: {}", p.include.join(", "));
                if !p.exclude.is_empty() {
                    println!("exclude: {}", p.exclude.join(", "));
                }
                if !p.note.is_empty() {
                    println!("note: {}", p.note);
                }
                for r in &rules {
                    println!("  {r}");
                }
                println!("{} rules", rules.len());
            }
            Ok(0)
        }
    }
}

fn explain(what: &str) -> Result<u8, Fail> {
    if let Some(info) = code_info(what) {
        print!("{}", to_json(info)?);
        return Ok(0);
    }
    if let Some(rule) = registry().get(what) {
        let row = summaries(registry()).into_iter().find(|r| r.id == what);
        print!(
            "{}",
            to_json(&serde_json::json!({"summary": row, "rule": rule.def}))?
        );
        return Ok(0);
    }
    let codes: Vec<&str> = CODES.iter().map(|c| c.code).collect();
    Err(Fail::usage(format!(
        "{what:?} is neither a finding code ({}) nor a rule id (see `cite-detect rules list`)",
        codes.join(", ")
    )))
}

fn inspect(file: &str, stage: Stage, input: InputFormat) -> Result<u8, Fail> {
    let text = read_input(file)?;
    let markdown = is_markdown(file, input);
    let out = match stage {
        Stage::Recognize => {
            let base = if markdown {
                legal_citations::document::parse_markdown(&text).masked
            } else {
                text.clone()
            };
            to_json(&legal_citations::recognize::recognize(registry(), &base))?
        }
        Stage::Mask => {
            if markdown {
                legal_citations::document::parse_markdown(&text).masked
            } else {
                text.clone()
            }
        }
        Stage::Document => to_json(&document(registry(), &text, markdown))?,
        Stage::Findings => {
            let doc = document(registry(), &text, markdown);
            to_json(&evaluate(registry(), &doc, &Policy::default(), Mode::Lint))?
        }
    };
    print!("{out}");
    Ok(0)
}

fn schema(which: SchemaKind) -> Result<u8, Fail> {
    let s = match which {
        SchemaKind::Report => schemars::schema_for!(output::JsonReport),
        SchemaKind::Finding => schemars::schema_for!(legal_citations::Finding),
        SchemaKind::Citation => schemars::schema_for!(legal_citations::Citation),
        SchemaKind::Rule => schemars::schema_for!(legal_citations::rule::RuleDef),
        SchemaKind::Policy => schemars::schema_for!(Policy),
        SchemaKind::Authorities => schemars::schema_for!(output::AuthoritiesReport),
        SchemaKind::Bundle => schemars::schema_for!(export::Bundle),
        SchemaKind::Vectors => schemars::schema_for!(export::Vectors),
    };
    print!("{}", to_json(&s)?);
    Ok(0)
}

fn fix(file: &str, apply: bool, include_unverified: bool) -> Result<u8, Fail> {
    if file == "-" && apply {
        return Err(Fail::usage("--apply needs a file, not stdin"));
    }
    let text = read_input(file)?;
    let doc = detect_markdown(&text);
    let findings = evaluate(registry(), &doc, &Policy::default(), Mode::Lint);
    let mut edits: Vec<_> = findings
        .iter()
        .filter(|f| !f.suppressed)
        .filter_map(|f| f.fix.as_ref())
        .filter(|fx| fx.safe || include_unverified)
        .cloned()
        .collect();
    edits.sort_by_key(|e| std::cmp::Reverse(e.span.start));
    if !apply {
        print!("{}", to_json(&edits)?);
        return Ok(0);
    }
    let mut out = text.clone();
    for e in &edits {
        if out.get(e.span.start..e.span.end).is_some() {
            out.replace_range(e.span.start..e.span.end, &e.replacement);
        }
    }
    std::fs::write(file, out).map_err(|e| Fail::input(format!("{file}: {e}")))?;
    eprintln!("cite-detect: applied {} fix(es) to {file}", edits.len());
    Ok(0)
}
