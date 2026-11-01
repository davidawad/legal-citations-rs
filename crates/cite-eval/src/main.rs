//! `cite-eval`: measure recall, precision and exact-span rate on the gold
//! corpus, write the reports, or gate on thresholds.
//!
//! Exit codes: 0 pass, 1 below a gate threshold, 2 usage, 4 input (missing
//! cache, sha256 mismatch, bad labels, failed fetch).

use cite_eval::fetch::{self, Fetched};
use cite_eval::gate::Gate;
use cite_eval::manifest::Manifest;
use cite_eval::report::{self, RunInfo};
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "cite-eval",
    version,
    about = "Measure legal-citations against gold labels"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Gold corpus directory (manifest.json, <doc>.jsonl, gate.json).
    #[arg(long, global = true, default_value = "corpus/gold")]
    corpus: PathBuf,
    /// Text cache directory (<doc>.txt).
    #[arg(long, global = true, default_value = "corpus/cache/gold")]
    cache: PathBuf,
    /// Where the reports go.
    #[arg(long, default_value = "docs/measurements")]
    out: PathBuf,
    /// Report date, YYYY-MM-DD (default: today, UTC).
    #[arg(long)]
    date: Option<String>,
    /// Source revision to record in the reports.
    #[arg(long, default_value = "unspecified")]
    revision: String,
    /// Check the thresholds in <corpus>/gate.json (or --gate-file) instead of writing reports.
    #[arg(long)]
    gate: bool,
    /// Gate file (implies --gate).
    #[arg(long)]
    gate_file: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Download, extract and excerpt every manifest document into the cache, checking sha256.
    Fetch,
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    civil_date(i64::try_from(secs / 86_400).unwrap_or(0))
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

fn run_fetch(cli: &Cli) -> Result<ExitCode, String> {
    let manifest = Manifest::read(&cli.corpus.join("manifest.json"))?;
    std::fs::create_dir_all(&cli.cache).map_err(|e| format!("{}: {e}", cli.cache.display()))?;
    let mut failed = 0;
    for entry in &manifest.documents {
        match fetch::fetch(entry, &cli.cache) {
            Ok(Fetched::Cached) => println!("cached  {}", entry.id),
            Ok(Fetched::Written) => println!("fetched {}", entry.id),
            Err(e) => {
                eprintln!("cite-eval: {e}");
                failed += 1;
            }
        }
    }
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(4)
    })
}

fn run_eval(cli: &Cli) -> Result<ExitCode, String> {
    let docs = cite_eval::load_corpus(&cli.corpus, &cli.cache)?;
    let run = RunInfo {
        date: cli.date.clone().unwrap_or_else(today),
        revision: cli.revision.clone(),
        registry_hash: legal_citations::registry().hash(),
    };
    let (report, per_rule) = report::evaluate(&docs, run);
    let o = &report.overall;
    println!(
        "cite-eval: {} documents, {} gold citations: recall {}, precision {}, exact span {}",
        report.corpus.documents,
        report.corpus.labels,
        o.recall.cell(),
        o.precision.cell(),
        o.exact_span.cell()
    );
    if cli.gate || cli.gate_file.is_some() {
        let path = cli
            .gate_file
            .clone()
            .unwrap_or_else(|| cli.corpus.join("gate.json"));
        let failures = Gate::read(&path)?.check(&report);
        for f in &failures {
            eprintln!("cite-eval: gate: {f}");
        }
        return Ok(if failures.is_empty() {
            println!("cite-eval: gate passed ({})", path.display());
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    std::fs::create_dir_all(&cli.out).map_err(|e| format!("{}: {e}", cli.out.display()))?;
    let date = &report.run.date;
    write(
        &cli.out.join(format!("{date}-gold.md")),
        &report::render_md(&report),
    )?;
    write(&cli.out.join(format!("{date}-gold.json")), &pretty(&report))?;
    write(
        &cli.out.join(format!("{date}-per-rule.json")),
        &pretty(&per_rule),
    )?;
    write(&cli.out.join("misses.md"), &report::render_misses(&report))?;
    println!(
        "cite-eval: wrote {}/{date}-gold.{{md,json}}, {date}-per-rule.json, misses.md",
        cli.out.display()
    );
    Ok(ExitCode::SUCCESS)
}

/// Pretty JSON with a trailing newline.
fn pretty<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default() + "\n"
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Cmd::Fetch) => run_fetch(&cli),
        None => run_eval(&cli),
    };
    result.unwrap_or_else(|e| {
        eprintln!("cite-eval: {e}");
        ExitCode::from(4)
    })
}

#[cfg(test)]
mod tests {
    use super::civil_date;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(20_384), "2025-10-23");
        assert_eq!(civil_date(11_016), "2000-02-29");
        assert_eq!(civil_date(-1), "1969-12-31");
    }
}
