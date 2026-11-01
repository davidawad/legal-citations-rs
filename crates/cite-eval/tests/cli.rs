//! End-to-end: the committed fixture corpus (no network), the report
//! numbers, the gate, the fetch path (file:// URLs, so it needs `curl` but
//! no network) and every input error.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use cite_eval::report::{self, RunInfo};
use cite_eval::score::Outcome;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cite-eval"))
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = bin().args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cite-eval-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
    }
}

fn s(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn fixture_numbers() {
    let f = fixtures();
    let docs = cite_eval::load_corpus(&f.join("corpus"), &f.join("cache")).unwrap();
    assert_eq!(docs.len(), 2);
    let run = RunInfo {
        date: "2026-01-02".into(),
        revision: "fixture".into(),
        registry_hash: "h".into(),
    };
    let (r, per_rule) = report::evaluate(&docs, run);
    assert_eq!((r.corpus.labels, r.corpus.optional), (6, 1));
    assert_eq!((r.overall.recall.k, r.overall.recall.n), (4, 6));
    assert_eq!((r.overall.precision.k, r.overall.precision.n), (4, 5));
    assert_eq!((r.overall.exact_span.k, r.overall.exact_span.n), (3, 4));
    assert_eq!(r.corpus.ignored, 1);
    assert_eq!(r.catch_all_on_misses, 1);
    assert_eq!(r.outcomes["missed"], 2);
    assert_eq!(r.outcomes["truncated"], 1);
    // The unlabeled Smith v. Jones cite is the one false positive.
    assert_eq!(r.false_positives.len(), 1);
    assert!(r.false_positives[0].context.contains("[[Smith v. Jones"));
    let statute = &r.by_kind["statute"];
    assert_eq!((statute.recall.k, statute.recall.n), (1, 2));
    assert!(r.by_jurisdiction.contains_key("us.il"));
    assert_eq!(r.by_category["scotus"].recall.value, Some(1.0));
    let ilcs = r.misses.iter().find(|m| m.form == "ILCS").unwrap();
    assert_eq!(ilcs.outcome, Outcome::Missed);
    assert!(!ilcs.catch_all);
    // Every rule fired fewer than PER_RULE_MIN_N times.
    assert!(per_rule.rules.is_empty());
    assert!(per_rule
        .below_min_n
        .contains(&"us.case.us_reports".to_string()));

    let md = report::render_md(&r);
    assert!(md.contains("| all | 0.667 (0.300–0.903), 4/6 |"));
    assert!(md.contains("1 detections overlap no gold citation."));
    assert!(md.contains("A catch-all (`unrecognized`, CITE011) hit overlaps 1 of the 2 misses."));
    let misses = report::render_misses(&r);
    assert!(misses.contains("| case | (no form) | 1 | 0 | `45 Haw. Trib. 112` |"));
    assert!(misses.contains("| doc-a | `45 Haw. Trib. 112` | missed | catch-all |"));
    assert!(misses.contains("`42 U.S.C. § 1983` (us.fed.statute.usc)"));
}

#[test]
fn writes_reports_and_gates() {
    let f = fixtures();
    let out = temp("out");
    let corpus = f.join("corpus");
    let cache = f.join("cache");
    let base = ["--corpus", s(&corpus), "--cache", s(&cache)];
    let (code, stdout, _) = run(&[
        &base[..],
        &[
            "--out",
            s(&out),
            "--date",
            "2026-01-02",
            "--revision",
            "fixture",
        ],
    ]
    .concat());
    assert_eq!(code, 0, "{stdout}");
    for name in [
        "2026-01-02-gold.md",
        "2026-01-02-gold.json",
        "2026-01-02-per-rule.json",
        "misses.md",
    ] {
        assert!(out.join(name).is_file(), "{name}");
    }
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("2026-01-02-gold.json")).unwrap())
            .unwrap();
    assert_eq!(json["run"]["revision"], "fixture");
    assert_eq!(json["overall"]["recall"]["n"], 6);

    // Default date: today, so just check the file pattern.
    let today = temp("today");
    assert_eq!(run(&[&base[..], &["--out", s(&today)]].concat()).0, 0);
    let names: Vec<_> = std::fs::read_dir(&today)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert!(names
        .iter()
        .any(|n| n.len() == "2026-01-02-gold.md".len() && n.ends_with("-gold.md")));

    let (code, stdout, _) = run(&[&base[..], &["--gate"]].concat());
    assert_eq!(code, 0);
    assert!(stdout.contains("gate passed"));
    let fail = f.join("gate-fail.json");
    let (code, _, stderr) = run(&[&base[..], &["--gate-file", s(&fail)]].concat());
    assert_eq!(code, 1);
    assert!(stderr.contains("overall recall 0.6667 is below the gate 0.9900"));
    assert!(stderr.contains("kind treaty recall has no data"));
    assert!(!stderr.contains("kind case"));
    let missing = f.join("no-such-gate.json");
    assert_eq!(
        run(&[&base[..], &["--gate-file", s(&missing)]].concat()).0,
        4
    );
    let bad_out = f.join("corpus/manifest.json");
    assert_eq!(run(&[&base[..], &["--out", s(&bad_out)]].concat()).0, 4);
    let _ = std::fs::remove_dir_all(&out);
    let _ = std::fs::remove_dir_all(&today);
}

#[test]
fn input_errors_exit_4() {
    let f = fixtures();
    let corpus = temp("corpus");
    copy_dir(&f.join("corpus"), &corpus);
    let cache = temp("cache");
    let eval = |c: &Path| run(&["--corpus", s(c), "--cache", s(&cache), "--gate"]);

    // Missing cache.
    let (code, _, stderr) = eval(&corpus);
    assert_eq!(code, 4);
    assert!(stderr.contains("run `cite-eval fetch`"), "{stderr}");

    // Text changed: sha256 mismatch.
    copy_dir(&f.join("cache"), &cache);
    std::fs::write(cache.join("doc-b.txt"), "changed\n").unwrap();
    let (code, _, stderr) = eval(&corpus);
    assert_eq!(code, 4);
    assert!(stderr.contains("does not match the manifest"), "{stderr}");
    copy_dir(&f.join("cache"), &cache);

    // Bad label text, then a missing label file, then a bad manifest.
    let labels = corpus.join("doc-b.jsonl");
    let good = std::fs::read_to_string(&labels).unwrap();
    std::fs::write(
        &labels,
        good.replace("\"text\": \"Brown", "\"text\": \"Brwn"),
    )
    .unwrap();
    let (code, _, stderr) = eval(&corpus);
    assert_eq!(code, 4);
    assert!(stderr.contains("doc-b.jsonl"), "{stderr}");
    std::fs::write(&labels, "{not json\n").unwrap();
    assert_eq!(eval(&corpus).0, 4);
    std::fs::remove_file(&labels).unwrap();
    assert_eq!(eval(&corpus).0, 4);
    std::fs::write(corpus.join("manifest.json"), "[]").unwrap();
    assert_eq!(eval(&corpus).0, 4);
    std::fs::remove_file(corpus.join("manifest.json")).unwrap();
    assert_eq!(eval(&corpus).0, 4);
    assert_eq!(run(&["--no-such-flag"]).0, 2);
    let _ = std::fs::remove_dir_all(&corpus);
    let _ = std::fs::remove_dir_all(&cache);
}

/// A corpus whose URLs point at the committed raw fixtures (file://).
fn local_corpus(name: &str, edit: impl Fn(&mut serde_json::Value)) -> PathBuf {
    let f = fixtures();
    let corpus = temp(name);
    copy_dir(&f.join("corpus"), &corpus);
    let path = corpus.join("manifest.json");
    let mut m: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let raw = f.join("raw");
    m["documents"][0]["url"] = format!("file://{}", s(&raw.join("doc-a.txt"))).into();
    m["documents"][1]["url"] = format!("file://{}", s(&raw.join("doc-b.html"))).into();
    edit(&mut m);
    std::fs::write(&path, serde_json::to_string_pretty(&m).unwrap()).unwrap();
    corpus
}

#[test]
fn fetch_rebuilds_the_cache() {
    let corpus = local_corpus("fetch-ok", |_| {});
    let cache = corpus.join("cache");
    let fetch = || run(&["fetch", "--corpus", s(&corpus), "--cache", s(&cache)]);
    let (code, stdout, stderr) = fetch();
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("fetched doc-a") && stdout.contains("fetched doc-b"));
    let f = fixtures();
    for id in ["doc-a", "doc-b"] {
        assert_eq!(
            std::fs::read(cache.join(format!("{id}.txt"))).unwrap(),
            std::fs::read(f.join(format!("cache/{id}.txt"))).unwrap()
        );
    }
    let (code, stdout, _) = fetch();
    assert_eq!(code, 0);
    assert!(stdout.contains("cached  doc-a"));
    // The fetched cache scores like the committed one.
    assert_eq!(
        run(&["--corpus", s(&corpus), "--cache", s(&cache), "--gate"]).0,
        0
    );
    let _ = std::fs::remove_dir_all(&corpus);
}

/// A failing fetch: name, manifest edit, expected stderr text.
type Case = (&'static str, fn(&mut serde_json::Value), &'static str);

#[test]
fn fetch_failures_exit_4() {
    let cases: [Case; 4] = [
        (
            "fetch-sha",
            |m| m["documents"][1]["sha256"] = "0".repeat(64).into(),
            "doc-b.mismatch.txt",
        ),
        (
            "fetch-excerpt",
            |m| m["documents"][0]["excerpt"]["to"] = "NOT THERE".into(),
            "excerpt markers not found",
        ),
        (
            "fetch-404",
            |m| m["documents"][1]["url"] = "file:///nonexistent/cite-eval/doc".into(),
            "curl file:///nonexistent",
        ),
        (
            "fetch-pdf",
            |m| m["documents"][1]["extraction"] = "pdftotext".into(),
            "pdftotext",
        ),
    ];
    for (name, edit, needle) in cases {
        let corpus = local_corpus(name, edit);
        let cache = corpus.join("cache");
        let (code, _, stderr) = run(&["fetch", "--corpus", s(&corpus), "--cache", s(&cache)]);
        assert_eq!(code, 4, "{name}: {stderr}");
        assert!(stderr.contains(needle), "{name}: {stderr}");
        let _ = std::fs::remove_dir_all(&corpus);
    }
    // No manifest at all.
    let empty = temp("fetch-empty");
    assert_eq!(run(&["fetch", "--corpus", s(&empty)]).0, 4);
    // A cache path that is a file.
    let corpus = local_corpus("fetch-cachefile", |_| {});
    let file = corpus.join("manifest.json");
    assert_eq!(
        run(&["fetch", "--corpus", s(&corpus), "--cache", s(&file)]).0,
        4
    );
    let _ = std::fs::remove_dir_all(&corpus);
    let _ = std::fs::remove_dir_all(&empty);
}
