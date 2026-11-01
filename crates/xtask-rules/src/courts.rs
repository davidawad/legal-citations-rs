//! Generate `crates/legal-citations/courts/courts.json`, the court table
//! (lcr-k5o), from the Indigo Book inventory `corpus/formats/indigo.jsonl`
//! (CC0; see `corpus/formats/README.md`).
//!
//! Every entry carries a court-level id (see `legal_citations::court`).
//!
//! - State and territory abbreviations: the R23.3 constitution rows
//!   (`Ohio Const.` -> `Ohio`, `us.oh.sup`, or `us.oh` when more court name
//!   follows), plus D.C., Guam and V.I. from their T3 headings (no
//!   constitution row).
//! - State court names: every court in a T3 case form or example
//!   parenthetical (`(Ohio Ct. App. <year>)`) that one jurisdiction uses and
//!   that starts with that jurisdiction's abbreviation, at level `sup`,
//!   `app` or `trial` (an override list, else `App` in the name).
//! - Federal courts: the T1 courts (circuits `us.fed.app.ca9`, `Fed. Cir.`,
//!   `C.A.A.F.`, `Ct. Int'l Trade`, ...) plus the district courts T1
//!   describes as `[N.|S.|E.|W.|M.|C.]D. <state>`, generated over the state
//!   abbreviations (`us.fed.dist.txed`), and the same districts after
//!   `Bankr.` and `C.C.`, and the circuits after `B.A.P.`.

use legal_citations::court::{key, Court, CourtTable};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The Indigo inventory, relative to the repo root.
pub const INDIGO_JSONL: &str = "corpus/formats/indigo.jsonl";
/// The generated table, relative to the repo root.
pub const COURTS_JSON: &str = "crates/legal-citations/courts/courts.json";

#[derive(Deserialize)]
struct Format {
    section: String,
    jurisdiction: String,
    kind: String,
    form: String,
    examples: Vec<String>,
}

#[derive(Serialize)]
struct CourtsOut<'a> {
    note: &'static str,
    source_url: &'static str,
    courts: &'a [Court],
}

/// Abbreviations with no R23.3 row, from their T3 headings.
const EXTRA_STATES: [(&str, &str, &str); 3] = [
    ("D.C.", "us.dc", "T3 District of Columbia"),
    ("Guam", "us.gu", "T3 Guam"),
    ("V.I.", "us.vi", "T3 Virgin Islands"),
];

/// The circuits (T1 Courts of Appeals) and their ids.
const CIRCUITS: [(&str, &str); 13] = [
    ("1st Cir.", "ca1"),
    ("2d Cir.", "ca2"),
    ("3d Cir.", "ca3"),
    ("4th Cir.", "ca4"),
    ("5th Cir.", "ca5"),
    ("6th Cir.", "ca6"),
    ("7th Cir.", "ca7"),
    ("8th Cir.", "ca8"),
    ("9th Cir.", "ca9"),
    ("10th Cir.", "ca10"),
    ("11th Cir.", "ca11"),
    ("D.C. Cir.", "cadc"),
    ("Fed. Cir.", "cafc"),
];

/// Federal courts T1 names without a district or circuit slot, with their
/// ids. Bare `Bankr.`, `B.A.P.` and `C.C.` cover a court string whose
/// district the table does not know.
const FEDERAL_FIXED: [(&str, &str, &str); 20] = [
    ("U.S.", "us.scotus", "T1 Supreme Court"),
    ("Bankr.", "us.fed.bankr", "T1 Bankruptcy Courts and B.A.P."),
    (
        "B.A.P.",
        "us.fed.bankr.bap",
        "T1 Bankruptcy Courts and B.A.P.",
    ),
    (
        "C.C.",
        "us.fed.cc",
        "T1 Old circuit courts (abolished 1912)",
    ),
    (
        "Ct. Int'l Trade",
        "us.fed.cit",
        "T1 Court of International Trade",
    ),
    (
        "Ct. Cl.",
        "us.fed.ctcl",
        "T1 Federal Circuit / C.C.P.A. / Ct. Cl.",
    ),
    (
        "C.C.P.A.",
        "us.fed.ccpa",
        "T1 Federal Circuit / C.C.P.A. / Ct. Cl.",
    ),
    (
        "J.P.M.L.",
        "us.fed.jpml",
        "T1 J.P.M.L. and Regional Rail Reorganization Court",
    ),
    (
        "Reg'l Rail Reorg. Ct.",
        "us.fed.rrrc",
        "T1 J.P.M.L. and Regional Rail Reorganization Court",
    ),
    (
        "Temp. Emer. Ct. App.",
        "us.fed.teca",
        "T1 Temporary Emergency Court of Appeals / Emergency Court of Appeals / Commerce Court",
    ),
    (
        "Emer. Ct. App.",
        "us.fed.eca",
        "T1 Temporary Emergency Court of Appeals / Emergency Court of Appeals / Commerce Court",
    ),
    (
        "Comm. Ct.",
        "us.fed.commerce",
        "T1 Temporary Emergency Court of Appeals / Emergency Court of Appeals / Commerce Court",
    ),
    (
        "C.A.A.F.",
        "us.fed.military.caaf",
        "T1 Court of Appeals for the Armed Forces / Court of Military Appeals",
    ),
    (
        "C.M.A.",
        "us.fed.military.cma",
        "T1 Court of Appeals for the Armed Forces / Court of Military Appeals",
    ),
    (
        "A. Ct. Crim. App.",
        "us.fed.military.acca",
        "T1 Military Courts of Criminal Appeals",
    ),
    (
        "A.F. Ct. Crim. App.",
        "us.fed.military.afcca",
        "T1 Military Courts of Criminal Appeals",
    ),
    (
        "C.G. Ct. Crim. App.",
        "us.fed.military.cgcca",
        "T1 Military Courts of Criminal Appeals",
    ),
    (
        "N-M. Ct. Crim. App.",
        "us.fed.military.nmcca",
        "T1 Military Courts of Criminal Appeals",
    ),
    (
        "A.C.M.R.",
        "us.fed.military.acmr",
        "T1 Military Courts of Criminal Appeals",
    ),
    (
        "A.B.R.",
        "us.fed.military.abr",
        "T1 Military Courts of Criminal Appeals",
    ),
];

/// District prefixes (`D.`, `N.D.`, ...), T1 District Courts, with the
/// letter they add to the district id (`E.D. Tex.` is `txed`).
const DISTRICTS: [(&str, &str); 7] = [
    ("D.", "d"),
    ("N.D.", "nd"),
    ("S.D.", "sd"),
    ("E.D.", "ed"),
    ("W.D.", "wd"),
    ("M.D.", "md"),
    ("C.D.", "cd"),
];

/// Old spellings of a state abbreviation in T1 examples.
const STATE_ALIASES: [(&str, &str); 1] = [("Tx.", "Tex.")];

/// State courts whose name says "appeals" but that are courts of last
/// resort, or that say "superior" or "circuit" but are appellate.
const STATE_LEVELS: [(&str, &str); 6] = [
    ("Tex. Crim. App.", "sup"),
    ("Okla. Crim. App.", "sup"),
    ("Tex. Comm'n App.", "sup"),
    ("Pa. Super. Ct.", "app"),
    ("Pa. Commw. Ct.", "app"),
    ("Ohio Cir. Ct.", "app"),
];

fn court(court: &str, jurisdiction: &str, source: &str) -> Court {
    Court {
        court: court.to_string(),
        jurisdiction: jurisdiction.to_string(),
        source: source.to_string(),
        partial: None,
    }
}

/// `us.oh` -> `oh`.
fn postal(jurisdiction: &str) -> &str {
    jurisdiction.strip_prefix("us.").unwrap_or(jurisdiction)
}

/// The district code of a district court name (`E.D. Tex.` -> `txed`,
/// `D.D.C.` -> `dcd`), if it is `<prefix> <state>` for a known state.
fn district_code(name: &str, states: &CourtTable) -> Option<String> {
    let k = key(name);
    // Longest prefix first, so `N.D.` is not read as `D.` after `N.`.
    DISTRICTS.iter().rev().find_map(|(p, letter)| {
        let rest = k.strip_prefix(&key(p))?;
        let rest = STATE_ALIASES
            .iter()
            .find(|(a, _)| key(a) == rest)
            .map_or(rest.to_string(), |(_, s)| key(s));
        let state = states.courts().iter().find(|s| key(&s.court) == rest)?;
        Some(format!("{}{letter}", postal(&state.jurisdiction)))
    })
}

/// The id of a federal court name: a circuit, a district, a bankruptcy
/// court or B.A.P., an old circuit court, a fixed T1 court, else `us.fed`.
fn federal_id(name: &str, states: &CourtTable) -> String {
    let k = key(name);
    let circuit = |s: &str| {
        CIRCUITS
            .iter()
            .find(|(c, _)| key(c) == s)
            .map(|(_, id)| *id)
    };
    if let Some(id) = circuit(&k) {
        return format!("us.fed.app.{id}");
    }
    if let Some(code) = district_code(name, states) {
        return format!("us.fed.dist.{code}");
    }
    for (prefix, base) in [("Bankr.", "us.fed.bankr"), ("C.C.", "us.fed.cc")] {
        if let Some(code) = k
            .strip_prefix(&key(prefix))
            .and_then(|rest| district_code(rest, states))
        {
            return format!("{base}.{code}");
        }
    }
    if let Some(id) = k.strip_prefix("B.A.P.").and_then(circuit) {
        return format!("us.fed.bankr.bap.{id}");
    }
    FEDERAL_FIXED
        .iter()
        .find(|(c, _, _)| key(c) == k)
        .map_or_else(|| "us.fed".to_string(), |(_, id, _)| (*id).to_string())
}

/// The level of a state court: `sup` (court of last resort), `app`
/// (intermediate appellate) or `trial`, from its name after the state.
fn state_level(court: &str, state: &str) -> &'static str {
    let k = key(court);
    if let Some((_, level)) = STATE_LEVELS.iter().find(|(c, _)| key(c) == k) {
        return level;
    }
    let rest = k.strip_prefix(&key(state)).unwrap_or(&k);
    if rest.is_empty() {
        "sup"
    } else if rest.contains("App") {
        "app"
    } else {
        "trial"
    }
}

/// The court part of every date parenthetical in `text`, alternatives
/// (`a|b`) split, slots (`<district>`) and comma forms dropped.
fn parenthetical_courts(re: &Regex, text: &str) -> Vec<String> {
    re.captures_iter(text)
        .filter_map(|c| c.get(1))
        .flat_map(|m| m.as_str().split('|'))
        .map(|s| s.trim().trim_end_matches(',').trim().to_string())
        .filter(|s| !s.is_empty() && !s.contains('<') && !s.contains(','))
        // `(No. 1843)` is a docket number, not a court.
        .filter(|s| !s.starts_with("No."))
        .collect()
}

/// A district court name: `S.D.` + `N.Y.` closes up (`S.D.N.Y.`, adjacent
/// single capitals), `N.D.` + `Cal.` keeps the space (`N.D. Cal.`).
fn district(prefix: &str, state: &str) -> String {
    let closed = state
        .split('.')
        .filter(|p| !p.is_empty())
        .all(|p| p.len() == 1 && p.chars().all(|c| c.is_ascii_uppercase()));
    if closed && state.ends_with('.') {
        format!("{prefix}{state}")
    } else {
        format!("{prefix} {state}")
    }
}

/// Build the table from the inventory text.
pub fn generate(jsonl: &str) -> Result<Vec<Court>, String> {
    let formats: Vec<Format> = jsonl
        .lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, l)| serde_json::from_str(l).map_err(|e| format!("line {}: {e}", i + 1)))
        .collect::<Result<_, _>>()?;
    let paren = Regex::new(
        r"\(([^()]*?)\s*(?:<year>|(?:[A-Z][a-z]{2,4}\.?\s[0-9]{1,2},\s)?(?:1[6-9]|20)[0-9]{2})\)",
    )
    .map_err(|e| e.to_string())?;

    // State abbreviations.
    let mut states: Vec<Court> = formats
        .iter()
        .filter(|f| f.kind == "constitution" && f.section.starts_with("R23.3"))
        .filter(|f| !f.jurisdiction.starts_with("us.tribal"))
        .filter_map(|f| {
            let ex = f.examples.first()?;
            let (abbr, _) = ex.split_once(" Const.")?;
            Some(court(abbr, &f.jurisdiction, &f.section))
        })
        .collect();
    states.extend(EXTRA_STATES.iter().map(|(a, j, s)| court(a, j, s)));
    let state_table = CourtTable::new(states.clone());

    // Federal courts.
    let mut federal: Vec<Court> = CIRCUITS
        .iter()
        .map(|(c, id)| court(c, &format!("us.fed.app.{id}"), "T1 Courts of Appeals"))
        .collect();
    federal.extend(FEDERAL_FIXED.iter().map(|(c, id, s)| court(c, id, s)));
    for s in &states {
        for (p, _) in DISTRICTS {
            let name = district(p, &s.court);
            let Some(code) = district_code(&name, &state_table) else {
                continue;
            };
            let source = "T1 District Courts (generated over the state abbreviations)";
            federal.push(court(&name, &format!("us.fed.dist.{code}"), source));
            let source = "T1 Bankruptcy Courts and B.A.P. (generated over the districts)";
            federal.push(court(
                &format!("Bankr. {name}"),
                &format!("us.fed.bankr.{code}"),
                source,
            ));
            let source = "T1 Old circuit courts (generated over the districts)";
            federal.push(court(
                &format!("C.C.{name}"),
                &format!("us.fed.cc.{code}"),
                source,
            ));
        }
    }
    for (c, id) in &CIRCUITS[..11] {
        let source = "T1 Bankruptcy Courts and B.A.P. (generated over the circuits)";
        federal.push(court(
            &format!("B.A.P. {c}"),
            &format!("us.fed.bankr.bap.{id}"),
            source,
        ));
    }
    for f in formats
        .iter()
        .filter(|f| f.kind == "case" && f.jurisdiction == "us.fed" && f.section.starts_with("T1"))
    {
        let texts = std::iter::once(&f.form).chain(&f.examples);
        for c in texts.flat_map(|t| parenthetical_courts(&paren, t)) {
            // T1 rows also show state courts in parallel-cite examples.
            if state_table.lookup(&c).is_none() {
                federal.push(court(&c, &federal_id(&c, &state_table), &f.section));
            }
        }
    }

    // State court names, one jurisdiction each, starting with its abbreviation.
    let mut seen: BTreeMap<String, (BTreeSet<String>, String, String)> = BTreeMap::new();
    for f in formats
        .iter()
        .filter(|f| f.kind == "case" && f.section.starts_with("T3"))
    {
        let texts = std::iter::once(&f.form).chain(&f.examples);
        for c in texts.flat_map(|t| parenthetical_courts(&paren, t)) {
            let e = seen
                .entry(key(&c))
                .or_insert_with(|| (BTreeSet::new(), c.clone(), f.section.clone()));
            e.0.insert(f.jurisdiction.clone());
        }
    }
    let state_courts = seen.into_values().filter_map(|(js, c, section)| {
        let mut js = js.into_iter();
        let (Some(j), None) = (js.next(), js.next()) else {
            return None;
        };
        let lead = state_table.lookup(&c)?;
        (lead.jurisdiction == j && key(&lead.court) != key(&c)).then(|| {
            let level = state_level(&c, &lead.court);
            court(&c, &format!("{j}.{level}"), &section)
        })
    });
    // A bare abbreviation is the state's court of last resort; followed by
    // a court the table does not know, just the state.
    let states = states.into_iter().map(|s| Court {
        jurisdiction: format!("{}.sup", s.jurisdiction),
        partial: Some(s.jurisdiction),
        ..s
    });

    // Federal first: `S.D.N.Y.` is federal even though it starts with `S.D.`.
    let mut all: Vec<Court> = federal;
    all.extend(states);
    all.extend(state_courts);
    let mut keys = BTreeSet::new();
    all.retain(|c| keys.insert(key(&c.court)));
    all.sort_by(|a, b| a.court.cmp(&b.court));
    Ok(all)
}

/// The file text for `courts`.
pub fn to_json(courts: &[Court]) -> Result<String, String> {
    let out = CourtsOut {
        note: "GENERATED by crates/xtask-rules from corpus/formats/indigo.jsonl (the Indigo Book, CC0); do not hand-edit (run `cargo run -p xtask-rules`).",
        source_url: "https://law.resource.org/pub/us/code/blue/IndigoBook.html",
        courts,
    };
    let mut s = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
    s.push('\n');
    Ok(s)
}

/// Regenerate (or, with `check`, compare) the committed table. Returns the
/// number of courts and whether the committed file was stale.
pub fn run(root: &Path, check: bool) -> Result<(usize, bool), String> {
    let jsonl = std::fs::read_to_string(root.join(INDIGO_JSONL))
        .map_err(|e| format!("{INDIGO_JSONL}: {e}"))?;
    let courts = generate(&jsonl)?;
    let text = to_json(&courts)?;
    let path = root.join(COURTS_JSON);
    let stale = std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str());
    if stale && !check {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok((courts.len(), stale))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    const INDIGO: &str = r#"{"section":"R23.3 State constitutions (Ohio)","jurisdiction":"us.oh","kind":"constitution","form":"Ohio Const. art. <art>","examples":["Ohio Const. art. II, § 15"]}
{"section":"R23.3 State constitutions (New York)","jurisdiction":"us.ny","kind":"constitution","form":"N.Y. Const.","examples":["N.Y. Const. art. I, § 29"]}
{"section":"R23.3 State constitutions (Navajo Nation)","jurisdiction":"us.tribal.navajo","kind":"constitution","form":"x","examples":["Navajo Const. art. VII, § 25"]}
{"section":"R23.3 Odd","jurisdiction":"us.zz","kind":"constitution","form":"x","examples":["No constitution here"]}

{"section":"T3 Ohio: Court of Appeals","jurisdiction":"us.oh","kind":"case","form":"<case name>, <vol> N.E.2d <page> (Ohio Ct. App. <year>)","examples":["A v. B, 1 N.E.2d 2 (Ohio Ct. App. Jan. 2, 2001)","C v. D, 3 N.E. 4 (Ct. App. 1901)"]}
{"section":"T3 New York: Courts","jurisdiction":"us.ny","kind":"case","form":"<case name> (Ct. App. <year>) (No. 1843)","examples":[]}
{"section":"T1 District Courts","jurisdiction":"us.fed","kind":"case","form":"<case name> (<court> <year>)","examples":["A v. B, 1 F. Supp. 2 (S.D.N.Y. 1999)","C v. D, 1 N.E.2d 2 (Ohio 1990)","E v. F, 1 F. 2 (J.P.M.L.|Temp. Emer. Ct. App. 1974)","G v. H, 1 F. 2 (Foo Ct. 1880)","I v. J, 1 F. 2 (E.D. Oh. 1880)"]}
"#;

    #[test]
    fn generates_states_federal_and_state_courts() {
        let courts = generate(INDIGO).unwrap();
        let find = |c: &str| {
            courts
                .iter()
                .find(|x| x.court == c)
                .map(|x| x.jurisdiction.as_str())
        };
        assert_eq!(find("Ohio"), Some("us.oh.sup"));
        assert_eq!(find("Ohio Ct. App."), Some("us.oh.app"));
        assert_eq!(find("N.Y."), Some("us.ny.sup"));
        assert_eq!(find("D.C."), Some("us.dc.sup"));
        assert_eq!(find("S.D.N.Y."), Some("us.fed.dist.nysd"));
        assert_eq!(find("Bankr. S.D.N.Y."), Some("us.fed.bankr.nysd"));
        assert_eq!(find("C.C.S.D.N.Y."), Some("us.fed.cc.nysd"));
        assert_eq!(find("N.D. Ohio"), Some("us.fed.dist.ohnd"));
        assert_eq!(find("D.D.C."), Some("us.fed.dist.dcd"));
        assert_eq!(find("J.P.M.L."), Some("us.fed.jpml"));
        assert_eq!(find("Temp. Emer. Ct. App."), Some("us.fed.teca"));
        assert_eq!(find("9th Cir."), Some("us.fed.app.ca9"));
        assert_eq!(find("B.A.P. 9th Cir."), Some("us.fed.bankr.bap.ca9"));
        // An unknown T1 court is just federal.
        assert_eq!(find("Foo Ct."), Some("us.fed"));
        let ohio = courts.iter().find(|x| x.court == "Ohio").unwrap();
        assert_eq!(ohio.partial.as_deref(), Some("us.oh"));
        // `Ct. App.` is used by two states; `No.` is a docket number;
        // a T1 example's state court is not federal; tribal has no districts.
        assert_eq!(find("Ct. App."), None);
        assert_eq!(find("No."), None);
        assert!(!courts.iter().any(|c| c.court.contains("Navajo")));
        assert_eq!(courts.iter().filter(|c| c.court == "Ohio").count(), 1);
        let json = to_json(&courts).unwrap();
        assert_eq!(CourtTable::from_json(&json).unwrap().courts(), &courts[..]);
        assert!(generate("{").unwrap_err().starts_with("line 1"));
    }

    #[test]
    fn ids_for_federal_and_state_courts() {
        let states = CourtTable::new(vec![
            court("Tex.", "us.tx", "t"),
            court("N.Y.", "us.ny", "t"),
            court("S.D.", "us.sd", "t"),
        ]);
        let id = |c: &str| federal_id(c, &states);
        assert_eq!(id("E.D. Tex."), "us.fed.dist.txed");
        assert_eq!(id("E.D. Tx."), "us.fed.dist.txed");
        assert_eq!(id("D.S.D."), "us.fed.dist.sdd");
        assert_eq!(id("S.D.S.D."), "us.fed.dist.sdsd");
        assert_eq!(id("Bankr. S.D.N.Y."), "us.fed.bankr.nysd");
        assert_eq!(id("C.C.D. Tex."), "us.fed.cc.txd");
        assert_eq!(id("B.A.P. 2d Cir."), "us.fed.bankr.bap.ca2");
        assert_eq!(id("D.C. Cir."), "us.fed.app.cadc");
        assert_eq!(id("U.S."), "us.scotus");
        assert_eq!(id("E.D. Nowhere"), "us.fed");
        assert_eq!(id("Bankr. Nowhere"), "us.fed");
        assert_eq!(state_level("Tex.", "Tex."), "sup");
        assert_eq!(state_level("Tex. Crim. App.", "Tex."), "sup");
        assert_eq!(state_level("Tex. App.", "Tex."), "app");
        assert_eq!(state_level("Pa. Super. Ct.", "Pa."), "app");
        assert_eq!(state_level("Del. Ch.", "Del."), "trial");
        assert_eq!(postal("us.tx"), "tx");
        assert_eq!(postal("x"), "x");
    }

    #[test]
    fn district_names_close_up_single_capitals() {
        assert_eq!(district("S.D.", "N.Y."), "S.D.N.Y.");
        assert_eq!(district("D.", "D.C."), "D.D.C.");
        assert_eq!(district("N.D.", "Cal."), "N.D. Cal.");
        assert_eq!(district("D.", "Guam"), "D. Guam");
        assert_eq!(district("D.", "N. Mar. I."), "D. N. Mar. I.");
    }

    #[test]
    fn run_writes_then_reports_fresh() {
        let root = std::env::temp_dir().join(format!("xtask-courts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        assert!(run(&root, true).is_err());
        std::fs::create_dir_all(root.join("corpus/formats")).unwrap();
        std::fs::write(root.join(INDIGO_JSONL), INDIGO).unwrap();
        let (n, stale) = run(&root, true).unwrap();
        assert!(stale && n > 0);
        assert!(!root.join(COURTS_JSON).exists(), "--check must not write");
        assert_eq!(run(&root, false).unwrap(), (n, true));
        assert_eq!(run(&root, true).unwrap(), (n, false));
        let _ = std::fs::remove_dir_all(&root);
    }
}
