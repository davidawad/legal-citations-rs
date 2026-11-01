//! Generates `case` rules from the vendored reporters-db `reporters.json`.
//!
//! One rule per reporters-db key (a reporter family: `F.` covers `F.`,
//! `F.2d`, `F.3d`, `F.4th` and every listed variation). Each rule is one
//! volume-reporter-page pattern whose reporter group is an alternation of
//! the family's strings, longest first. Pincites, courts and years are left
//! to the existing assembly layer.
//!
//! Hand-written case rules keep priority: a string a hand-written case rule
//! already matches (`12 <string> 345`) is left out, so the two never produce
//! the same match. A string listed by several families goes to the first
//! family in key order. Examples are chosen against the full registry
//! (hand-written plus generated) so every rule passes the self-example guard
//! and its negatives match no case rule.
//!
//! The output is deterministic; `xtask-rules --check` (and the freshness
//! test) fails when the committed files differ from a fresh run.

pub mod courts;
pub mod indigo;

use legal_citations::registry::{validate, Registry, Violation};
use legal_citations::Kind;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Upstream commit of the vendored `reporters.json`.
pub const UPSTREAM_SHA: &str = "faddc86aad58ebde19d81b6f1ba64d229a801092";
/// Priority of generated rules: below every hand-written rule except the
/// generic state-statute fallback, so hand-written rules win any overlap.
pub const PRIORITY: i32 = 30;
/// Priority of a family whose every string loses its example to a
/// higher-priority rule of another kind (`12 Tex. L. R. 345` vs the local-rule
/// pattern): above the hand-written rules, kept only if the whole registry
/// still validates.
pub const ELEVATED_PRIORITY: i32 = 65;
/// Base confidence of generated rules.
pub const BASE_CONFIDENCE: f64 = 0.8;
/// The rule data directory (MIT, embedded by `legal-citations`), relative to the repo root.
pub const RULES_DIR: &str = "rules";
/// Generated rules, relative to [`RULES_DIR`].
pub const GENERATED_DIR: &str = "case/generated";
/// The generated short-form rule, relative to [`RULES_DIR`].
pub const SHORT_GENERATED_DIR: &str = "case_short/generated";
/// Id of the generated short-form rule.
pub const SHORT_ID: &str = "us.short.rdb.case_at";
/// Id of the generated short-form rule for strings another rule would
/// otherwise split. Emitted only when needed: since the local-rule pattern
/// got `reject_before` (`12 A.L.R.3d at 345` no longer yields `L.R. 3`), no
/// string needs it.
pub const SHORT_ELEVATED_ID: &str = "us.short.rdb.case_at_elevated";
/// The vendored data file, relative to the repo root.
pub const REPORTERS_JSON: &str = "third_party/reporters-db/reporters.json";
/// Positive examples per rule, at most.
const MAX_POSITIVES: usize = 6;

/// One reporter object of reporters-db (unused fields are ignored).
#[derive(Clone, Debug, Deserialize)]
pub struct Reporter {
    /// Full name, e.g. `Federal Reporter`.
    pub name: String,
    /// `federal`, `state`, `state_regional`, `neutral`, `specialty`, ...
    #[serde(default)]
    pub cite_type: String,
    /// Edition abbreviation -> date range.
    #[serde(default)]
    pub editions: BTreeMap<String, Edition>,
    /// Variant spelling -> edition abbreviation.
    #[serde(default)]
    pub variations: BTreeMap<String, String>,
    /// MLZ jurisdiction codes, e.g. `us:c9:ca.nd;district.court`.
    #[serde(default)]
    pub mlz_jurisdiction: Vec<String>,
    /// Non-standard citation format, e.g. `{volume}-{reporter}-{page}`.
    #[serde(default)]
    pub cite_format: Option<String>,
}

/// An edition's date range (ISO datetimes, `null` when open).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Edition {
    /// First date covered.
    #[serde(default)]
    pub start: Option<String>,
    /// Last date covered.
    #[serde(default)]
    pub end: Option<String>,
}

/// reporters-db keyed by reporter family.
pub type Db = BTreeMap<String, Vec<Reporter>>;

/// Parse `reporters.json`.
pub fn parse_db(json: &str) -> Result<Db, String> {
    serde_json::from_str(json).map_err(|e| format!("reporters.json: {e}"))
}

/// Every string reporters-db lists for a family: edition abbreviations
/// first, then variations, trimmed, in file order, without repeats.
pub fn family_strings(reporters: &[Reporter]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let editions = reporters.iter().flat_map(|r| r.editions.keys());
    let variations = reporters.iter().flat_map(|r| r.variations.keys());
    for s in editions.chain(variations) {
        let s = s.trim();
        if !s.is_empty() && !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    }
    out
}

/// Every `(family key, string)` pair in the database, for coverage checks.
pub fn all_strings(db: &Db) -> Vec<(String, String)> {
    db.iter()
        .flat_map(|(key, reps)| {
            family_strings(reps)
                .into_iter()
                .map(move |s| (key.clone(), s))
        })
        .collect()
}

/// Whether a family is cited with hyphens (`2009-NMSC-001`).
pub fn hyphenated(reporters: &[Reporter]) -> bool {
    reporters
        .iter()
        .filter_map(|r| r.cite_format.as_deref())
        .any(|f| f.contains("}-{"))
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    Federal,
    State(String),
    Foreign(String),
}

/// Classify one MLZ code: `us;...` and `us:c9...`, `us:fed`, `us:ca.nd`
/// (a federal district) are federal; `us:ca` is a state; `gb:...` is foreign.
fn place(mlz: &str) -> Place {
    let head = mlz.split(';').next().unwrap_or("");
    let mut parts = head.split(':');
    let country = parts.next().unwrap_or("").to_ascii_lowercase();
    if country != "us" {
        // `en:c` (England, courts) files under the country code `gb`.
        return Place::Foreign(if country == "en" {
            "gb".into()
        } else {
            country
        });
    }
    let Some(second) = parts.next() else {
        return Place::Federal;
    };
    let circuit = second
        .strip_prefix('c')
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()));
    if circuit || second == "fed" || second.contains('.') {
        Place::Federal
    } else {
        // reporters-db spells Delaware `del` once (Storey).
        Place::State(if second == "del" {
            "de".into()
        } else {
            slug(second)
        })
    }
}

/// Jurisdiction of a reporter from its `cite_type` and MLZ codes: `us.fed`
/// for federal reporters, `us.<state>` for one state, `us.state` for a
/// regional reporter, `us` when a specialty reporter spans both.
pub fn jurisdiction(reporter: &Reporter) -> String {
    let places: BTreeSet<Place> = reporter.mlz_jurisdiction.iter().map(|m| place(m)).collect();
    let first = reporter.mlz_jurisdiction.first().map(|m| place(m));
    match reporter.cite_type.as_str() {
        "federal" | "scotus_early" => "us.fed".into(),
        "state_regional" => "us.state".into(),
        "state" | "neutral" => match first {
            Some(Place::State(s)) => format!("us.{s}"),
            Some(Place::Foreign(c)) => c,
            _ => "us".into(),
        },
        _ => {
            let mut it = places.iter();
            match (it.next(), it.next()) {
                (Some(Place::Federal), None) => "us.fed".into(),
                (Some(Place::State(s)), None) => format!("us.{s}"),
                (Some(Place::Foreign(c)), None) => c.clone(),
                _ => "us".into(),
            }
        }
    }
}

/// Lowercase `[a-z0-9_]` slug: `F. Supp. 2d` -> `f_supp_2d`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_end_matches('_').to_string();
    if out.is_empty() {
        "x".into()
    } else {
        out
    }
}

/// Regex alternative for one reporter string: escaped, any run of spaces
/// matches any run of whitespace.
fn alternative(s: &str) -> String {
    s.split_whitespace()
        .map(regex::escape)
        .collect::<Vec<_>>()
        .join("\\s+")
}

/// The volume-reporter-page pattern over `strings` (longest first, so a
/// longer edition wins over its prefix). Neutral citations number opinions,
/// not pages, so `neutral` allows seven digits (`2014 IL App (1st) 120583`).
pub fn pattern(strings: &[String], hyphen: bool, neutral: bool) -> String {
    let mut alts: Vec<String> = strings.iter().map(|s| alternative(s)).collect();
    alts.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    alts.dedup();
    let sep = if hyphen { "(?:\\s+|-)" } else { "\\s+" };
    let page = if neutral { 7 } else { 5 };
    format!(
        "\\b(?P<volume>[0-9]{{1,4}}){sep}(?P<reporter>{}){sep}(?P<page>[0-9]{{1,{page}}})\\b",
        alts.join("|")
    )
}

/// The probe text used to ask "does a rule match this reporter string".
pub fn probe(s: &str) -> String {
    format!("12 {s} 345")
}

/// The year-volume probe: Westlaw and Lexis cites (`1928 BTA LEXIS 3389`)
/// always start with a year, and hand-written rules for them require one.
pub fn year_probe(s: &str) -> String {
    format!("2019 {s} 1234")
}

/// The three-digit-volume probe: some hand-written rules (`370 NLRB No.
/// 128`) require the volume width their reporter actually uses.
pub fn volume_probe(s: &str) -> String {
    format!("123 {s} 45")
}

/// Every ownership probe for `s`, most generic first.
pub fn probes(s: &str) -> [String; 3] {
    [probe(s), year_probe(s), volume_probe(s)]
}

/// The kind of the rule in `reg` that owns all of `text`: a case rule whose
/// pattern matches it whole, or the one citation `text` is detected as.
pub fn owner(reg: &Registry, text: &str) -> Option<Kind> {
    let whole = |s: usize, e: usize| s == 0 && e == text.len();
    let case = reg.active().any(|(_, r)| {
        r.def.kind == Kind::Case && r.regex.find_iter(text).any(|m| whole(m.start(), m.end()))
    });
    if case {
        return Some(Kind::Case);
    }
    // The catch-all layer owns nothing: it only marks what no real rule took.
    match legal_citations::detect_with(reg, text).as_slice() {
        [c] if c.kind != Kind::Unrecognized && whole(c.core_span.start, c.core_span.end) => {
            Some(c.kind)
        }
        _ => None,
    }
}

/// A planned rule: one reporter family and the strings it owns.
#[derive(Clone, Debug)]
pub struct Family {
    /// reporters-db key.
    pub key: String,
    /// Rule id.
    pub id: String,
    /// Dotted jurisdiction.
    pub jurisdiction: String,
    /// The family's reporter objects.
    pub reporters: Vec<Reporter>,
    /// Strings this rule matches (editions first).
    pub strings: Vec<String>,
    /// Hyphenated citation format.
    pub hyphen: bool,
    /// Overlap priority: [`PRIORITY`], or [`ELEVATED_PRIORITY`] for a family
    /// that otherwise loses its own example to a non-case rule.
    pub priority: i32,
}

impl Family {
    fn name(&self) -> String {
        let names: Vec<&str> = self.reporters.iter().map(|r| r.name.as_str()).collect();
        let mut uniq: Vec<&str> = Vec::new();
        for n in names {
            if !uniq.contains(&n) {
                uniq.push(n);
            }
        }
        uniq.join("; ")
    }

    fn edition_of(&self, s: &str) -> Option<&Edition> {
        self.reporters.iter().find_map(|r| {
            r.editions.get(s).or_else(|| {
                r.variations
                    .get(s)
                    .and_then(|ed| r.editions.get(ed.as_str()))
            })
        })
    }

    fn year_of(&self, s: &str) -> String {
        self.edition_of(s)
            .and_then(|e| e.start.as_deref())
            .and_then(|d| d.get(0..4))
            .unwrap_or("1900")
            .to_string()
    }

    fn neutral(&self) -> bool {
        self.reporters.iter().any(|r| r.cite_type == "neutral")
    }

    fn date_ranges(&self) -> String {
        let mut parts = Vec::new();
        for r in &self.reporters {
            for (ed, range) in &r.editions {
                let year = |d: &Option<String>| {
                    d.as_deref()
                        .and_then(|d| d.get(0..4))
                        .unwrap_or("")
                        .to_string()
                };
                let (start, end) = (year(&range.start), year(&range.end));
                let end = if end.is_empty() {
                    "present".into()
                } else {
                    end
                };
                let start = if start.is_empty() { "?".into() } else { start };
                parts.push(format!("{ed} {start}-{end}"));
            }
        }
        parts.join("; ")
    }
}

/// What a generation run decided, for the report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// reporters-db keys (families).
    pub families: usize,
    /// Reporter objects.
    pub reporters: usize,
    /// Editions.
    pub editions: usize,
    /// Variations.
    pub variations: usize,
    /// Distinct strings (editions plus variations, per family).
    pub strings: usize,
    /// Strings a hand-written rule already owns, with its kind (left out).
    pub handwritten: Vec<(String, Kind)>,
    /// Strings owned by an earlier family.
    pub shared: usize,
    /// Variations that end in the short-form `at` (`T.C. at`), left to the
    /// short-form rule.
    pub short_form_variants: Vec<String>,
    /// Families dropped because a higher-priority rule (of any kind) claims
    /// every one of their strings, with those strings.
    pub conflicts: Vec<(String, Vec<String>)>,
    /// Families raised to [`ELEVATED_PRIORITY`].
    pub elevated: Vec<String>,
    /// Strings a generated rule owns but `12 <string> 345` is not detected as.
    pub unrecognized: Vec<String>,
    /// Strings in the generated short-form rule.
    pub short_strings: usize,
    /// Short forms left to the hand-written short-form rule.
    pub short_handwritten: usize,
    /// Rules emitted.
    pub rules: usize,
}

/// Plan the families against the hand-written registry.
pub fn plan(db: &Db, handwritten: &Registry) -> (Vec<Family>, Stats) {
    let mut stats = Stats {
        families: db.len(),
        ..Stats::default()
    };
    let mut owned: BTreeSet<String> = BTreeSet::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    let mut out = Vec::new();
    for (key, reps) in db {
        stats.reporters += reps.len();
        stats.editions += reps.iter().map(|r| r.editions.len()).sum::<usize>();
        stats.variations += reps.iter().map(|r| r.variations.len()).sum::<usize>();
        let mut strings = Vec::new();
        for s in family_strings(reps) {
            stats.strings += 1;
            if owned.contains(&s) {
                stats.shared += 1;
            } else if s.ends_with(" at") {
                // `T.C. at` is the short-form `at`, not a reporter: `12 T.C. at
                // 345` is the short form of `T.C.`.
                stats.short_form_variants.push(s);
            } else if let Some(kind) = probes(&s).iter().find_map(|t| owner(handwritten, t)) {
                stats.handwritten.push((s.clone(), kind));
                owned.insert(s);
            } else {
                owned.insert(s.clone());
                strings.push(s);
            }
        }
        if strings.is_empty() {
            continue;
        }
        let Some(first) = reps.first() else { continue };
        let jurisdiction = jurisdiction(first);
        let base = format!("{jurisdiction}.case.rdb.{}", slug(key));
        let mut id = base.clone();
        let mut n = 2;
        while ids.contains(&id) {
            id = format!("{base}_{n}");
            n += 1;
        }
        ids.insert(id.clone());
        out.push(Family {
            key: key.clone(),
            id,
            jurisdiction,
            reporters: reps.clone(),
            strings,
            hyphen: hyphenated(reps),
            priority: PRIORITY,
        });
    }
    stats.rules = out.len();
    (out, stats)
}

/// Generated rule file layout (field order matches the hand-written rules).
#[derive(Serialize)]
struct RuleOut {
    id: String,
    kind: &'static str,
    jurisdiction: String,
    pattern: String,
    priority: i32,
    normalize: &'static str,
    authority_key: &'static str,
    url_templates: Vec<&'static str>,
    url_decoders: Vec<&'static str>,
    base_confidence: f64,
    provenance: ProvenanceOut,
    examples: ExamplesOut,
    measured: MeasuredOut,
    note: String,
}

#[derive(Serialize)]
struct ProvenanceOut {
    encodes: String,
    opened: bool,
    source_url: String,
    how_to_verify: String,
    derived_from: Vec<String>,
    third_party: Vec<String>,
}

#[derive(Clone, Serialize)]
struct PositiveOut {
    text: String,
    spans: Vec<[usize; 2]>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    groups: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct ExamplesOut {
    positive: Vec<PositiveOut>,
    negative: Vec<String>,
}

#[derive(Serialize)]
struct MeasuredOut {
    precision: &'static str,
}

/// Rule URLs whose CourtListener `/c/` links were verified by browser fetch.
/// Each entry records the tested request and its final destination.
const VERIFIED_COURTLISTENER: &[(&str, &str, &str)] = &[
    ("us.ar.case.rdb.ark_app", "https://www.courtlistener.com/c/Ark.%20App./84/412/", "https://www.courtlistener.com/opinion/1397166/dooley-v-automated-conveyor-systems-inc/"),
    ("us.co.case.rdb.colo_n_p", "https://www.courtlistener.com/c/Colo.%20N.%20P./1/102/", "https://www.courtlistener.com/opinion/9332580/denver-union-water-co-v-board-of-co-comrs/"),
    ("us.ct.case.rdb.conn_super_ct", "https://www.courtlistener.com/c/Conn.%20Super.%20Ct./1999/13301/", "https://www.courtlistener.com/opinion/3350325/statewide-grievance-v-pile-strother-no-cv99-0590638s-oct-15-1999/"),
    ("us.dc.case.rdb.app_d_c", "https://www.courtlistener.com/c/App.%20D.C./48/606/", "https://www.courtlistener.com/opinion/7360259/cross-v-union-storage-transfer-co/"),
    ("us.dc.case.rdb.d_c", "https://www.courtlistener.com/c/D.C./1/1/", "https://www.courtlistener.com/c/dc/1/1/"),
    ("us.dc.case.rdb.hay_haz", "https://www.courtlistener.com/c/Hay.%20%26%20Haz./1/176/", "https://www.courtlistener.com/opinion/7360321/carter-v-lane/"),
    ("us.de.case.rdb.del", "https://www.courtlistener.com/c/Del./1/1/", "https://www.courtlistener.com/opinion/6675438/goslin-v-cannon/"),
    ("us.fed.case.rdb.a_f_t_r_ria", "https://www.courtlistener.com/c/A.F.T.R.2d%20%28RIA%29/42/6397/", "https://www.courtlistener.com/opinion/6329673/in-re-the-estate-of-webster/"),
    ("us.fed.case.rdb.am_disabilities_cas_bna", "https://www.courtlistener.com/c/A.D.%20Cases/18/651/", "https://www.courtlistener.com/opinion/202408/carmona-rivera-v-commonwealth-of-pr/"),
    ("us.fed.case.rdb.brunn_coll_cas", "https://www.courtlistener.com/c/Brunn.%20Coll.%20Cas./1/577/", "https://www.courtlistener.com/opinion/8655338/the-niphons-crew/"),
    ("us.fed.case.rdb.chi_leg_news", "https://www.courtlistener.com/c/Chi.Leg.N./10/41/", "https://www.courtlistener.com/opinion/9304932/scott-v-otis/"),
    ("us.fed.case.rdb.dnh", "https://www.courtlistener.com/c/DNH/2025/052/", "https://www.courtlistener.com/opinion/10697916/chayna-drapeau-et-al-v-bluestem-brands-inc-et-al/"),
    ("us.fed.case.rdb.f_app_x", "https://www.courtlistener.com/c/Fed.%20App%27x/409/277/", "https://www.courtlistener.com/opinion/181267/united-states-v-christopher-c-hayes/"),
    ("us.fed.case.rdb.f_cas", "https://www.courtlistener.com/c/F.%20Cas./1/1/", "https://www.courtlistener.com/opinion/6759394/the-aalesund/"),
    ("us.fed.case.rdb.int_rev_rec", "https://www.courtlistener.com/c/Int.%20Rev.%20Rec./24/164/", "https://www.courtlistener.com/opinion/6759559/in-re-ah-yup/"),
    ("us.fed.case.rdb.l_r_a", "https://www.courtlistener.com/c/L.R.A./40/437/", "https://www.courtlistener.com/opinion/6847348/cornwell-v-fraternal-accident-assn/"),
    ("us.fed.case.rdb.mccrary_s_cir_ct_rpts", "https://www.courtlistener.com/c/McCrary%27s%20Cir.%20Ct.%20Rpts/4/567/", "https://www.courtlistener.com/opinion/9308193/nichols-v-horton/"),
    ("us.fed.case.rdb.penn_law_j", "https://www.courtlistener.com/c/Pa.%20Law%20J./1/291/", "https://www.courtlistener.com/opinion/9303920/in-re-irwine/"),
    ("us.fed.case.rdb.tax_ct_memo_lexis", "https://www.courtlistener.com/c/Tax%20Ct.%20Memo%20LEXIS/1994/397/", "https://www.courtlistener.com/opinion/4813277/hansen-v-commissioner/"),
    ("us.fed.case.rdb.u_s_tax_ct_lexis", "https://www.courtlistener.com/c/U.S.%20Tax%20Ct.%20LEXIS/2002/28/", "https://www.courtlistener.com/opinion/4696627/baker-v-commr/"),
    ("us.fl.case.rdb.fla", "https://www.courtlistener.com/c/Fla./128/1/", "https://www.courtlistener.com/opinion/3390813/ryan-v-state/"),
    ("us.fl.case.rdb.fla_l_weekly", "https://www.courtlistener.com/c/Fla.%20L.%20Weekly/10/2563/", "https://www.courtlistener.com/c/fla-l-weekly/10/2563/"),
    ("us.ga.case.rdb.fulton_county_d_rep", "https://www.courtlistener.com/c/Fulton%20County%20D.%20Rep./96/1243/", "https://www.courtlistener.com/opinion/1201787/bunch-v-mathieson-drive-apartments-inc/"),
    ("us.ga.case.rdb.ga", "https://www.courtlistener.com/c/Ga./1/1/", "https://www.courtlistener.com/opinion/5701227/doe-ex-dem-truluck-v-peeples/"),
    ("us.il.case.rdb.il", "https://www.courtlistener.com/c/IL/2014/115811/", "https://www.courtlistener.com/c/il/2014/115811/"),
    ("us.il.case.rdb.ill_ct_cl", "https://www.courtlistener.com/c/Ill.%20Ct.%20Cl./49/42/", "https://www.courtlistener.com/opinion/8641003/pessin-v-state/"),
    ("us.ky.case.rdb.hughes", "https://www.courtlistener.com/c/Hughes/2/447/", "https://www.courtlistener.com/opinion/8649557/in-re-carter/"),
    ("us.ky.case.rdb.ky", "https://www.courtlistener.com/c/Ky./1/1/", "https://www.courtlistener.com/c/ky/1/1/"),
    ("us.ma.case.rdb.law_rep", "https://www.courtlistener.com/c/Law%20Rep./5/462/", "https://www.courtlistener.com/opinion/8652718/in-re-horton/"),
    ("us.ma.case.rdb.mass", "https://www.courtlistener.com/c/Mass./1/1/", "https://www.courtlistener.com/c/mass/1/1/"),
    ("us.ma.case.rdb.mass_l_rptr", "https://www.courtlistener.com/c/Mass.%20Law%20Rep./1/561/", "https://www.courtlistener.com/opinion/8351325/mceneaney-v-chestnut-hill-realty-corp/"),
    ("us.md.case.rdb.md", "https://www.courtlistener.com/c/Md./253/1/", "https://www.courtlistener.com/opinion/2393462/bituminous-construction-co-v-lewis/"),
    ("us.mi.case.rdb.mich", "https://www.courtlistener.com/c/Mich./1/1/", "https://www.courtlistener.com/opinion/6748640/may-v-rumney/"),
    ("us.mo.case.rdb.mo_app", "https://www.courtlistener.com/c/Mo.App./201/523/", "https://www.courtlistener.com/opinion/6753354/kansas-city-v-sells-floto-shows-co/"),
    ("us.ms.case.rdb.miss", "https://www.courtlistener.com/c/Miss./1/1/", "https://www.courtlistener.com/opinion/7463407/chew-v-randolph/"),
    ("us.nc.case.rdb.n_c", "https://www.courtlistener.com/c/N.C./1/1/", "https://www.courtlistener.com/c/nc/1/1/"),
    ("us.ne.case.rdb.neb_ct_app", "https://www.courtlistener.com/c/Neb.%20Ct.%20App./1/1/", "https://www.courtlistener.com/opinion/1599467/1733-estates-assn-inc-v-randolph/"),
    ("us.nm.case.rdb.nmca", "https://www.courtlistener.com/c/NMCA/2005/078/", "https://www.courtlistener.com/opinion/2590814/state-v-montoya/"),
    ("us.ny.case.rdb.cai_cas", "https://www.courtlistener.com/c/Cai./1/2/", "https://www.courtlistener.com/opinion/6271678/bergen-v-bennett/"),
    ("us.ny.case.rdb.dem_sur", "https://www.courtlistener.com/c/Dem.Surr./6/478/", "https://www.courtlistener.com/opinion/6274274/in-re-the-probate-of-the-will-of-crumb/"),
    ("us.ny.case.rdb.lock_rev_cas", "https://www.courtlistener.com/c/Lock.%20Rev.%20Cas./1/567/", "https://www.courtlistener.com/opinion/6277860/people-ex-rel-dyckman-v-trustees-of-brooklyn/"),
    ("us.ny.case.rdb.n_j_manumission", "https://www.courtlistener.com/c/N.J.%20%28Manumission%29/1/17/", "https://www.courtlistener.com/opinion/8105247/state-v-phillips/"),
    ("us.ny.case.rdb.n_y", "https://www.courtlistener.com/c/NY2d/78/364/", "https://www.courtlistener.com/opinion/5690492/prue-v-hunt/"),
    ("us.oh.case.rdb.ohio_c_c", "https://www.courtlistener.com/c/Ohio%20C.C./17/289/", "https://www.courtlistener.com/opinion/9294508/zinn-v-baxter/"),
    ("us.oh.case.rdb.ohio_st_n_s", "https://www.courtlistener.com/c/Ohio%20St.%20%28n.s.%29/61/494/", "https://www.courtlistener.com/opinion/8375433/finnical-v-village-of-cadiz/"),
    ("us.pa.case.rdb.am_law_reg", "https://www.courtlistener.com/c/Am.%20Law%20Reg./9/661/", "https://www.courtlistener.com/opinion/9325180/in-re-mcdonald/"),
    ("us.pa.case.rdb.pa_super", "https://www.courtlistener.com/c/Pa.%20Super./1/77/", "https://www.courtlistener.com/opinion/6400968/philadelphia-reading-terminal-railroad-cos-appeal/"),
    ("us.pa.case.rdb.serg_rawle", "https://www.courtlistener.com/c/Serg.%20%26%20Rawl./3/133/", "https://www.courtlistener.com/opinion/6481012/christmas-v-thompson/"),
    ("us.sc.case.rdb.s_c_eq", "https://www.courtlistener.com/c/S.C.%20Eq./1/1/", "https://www.courtlistener.com/c/sc-eq/1/1/"),
    ("us.sc.case.rdb.s_c_l", "https://www.courtlistener.com/c/S.C.L./1/1/", "https://www.courtlistener.com/opinion/7468391/state-v-lewis/"),
    ("us.state.case.rdb.am_ann_cas", "https://www.courtlistener.com/c/Am.%20Ann.%20Cas./11/198/", "https://www.courtlistener.com/opinion/7990738/rose-v-linderman/"),
    ("us.state.case.rdb.s_w", "https://www.courtlistener.com/c/SW%203d/47/1/", "https://www.courtlistener.com/opinion/1749546/colorado-interstate-gas-co-v-hunt-energy-corp/"),
    ("us.tn.case.rdb.tenn", "https://www.courtlistener.com/c/Tenn./1/1/", "https://www.courtlistener.com/opinion/4130010/murfree-v-leeper/"),
    ("us.tn.case.rdb.thompson", "https://www.courtlistener.com/c/Unrep.%20Tenn.%20Cas./1/141/", "https://www.courtlistener.com/opinion/8337159/graves-v-hyatt/"),
    ("us.tx.case.rdb.tex", "https://www.courtlistener.com/c/Tex./1/1/", "https://www.courtlistener.com/opinion/5071365/western-v-wood/"),
    ("us.tx.case.rdb.tex_bus", "https://www.courtlistener.com/c/Tex.%20Bus./2025/28/", "https://www.courtlistener.com/opinion/10661970/safelease-insurance-services-v-storable/"),
    ("us.va.case.rdb.va", "https://www.courtlistener.com/c/Va./1/1/", "https://www.courtlistener.com/c/va/1/1/"),
    ("us.wa.case.rdb.wash_app", "https://www.courtlistener.com/c/Wash.App.2d/13/185/", "https://www.courtlistener.com/opinion/4752917/state-of-washington-v-yeshak-k-bedada/"),
    ("us.wi.case.rdb.wis", "https://www.courtlistener.com/c/Wis.%202d/211/1/", "https://www.courtlistener.com/opinion/1624237/bergmann-v-mccaughtry/"),
    ("us.case.rdb.cal_wcc", "https://www.courtlistener.com/c/Cal.%20WCC/2/1205/", "https://www.courtlistener.com/opinion/1361358/travelers-property-casualty-co-of-america-v-conocophillips-co/"),
    ("us.case.rdb.cj_c_a_r", "https://www.courtlistener.com/c/CJ%20C.A.R./1999/4982/", "https://www.courtlistener.com/opinion/158341/osage-tribal-council-ex-rel-osage-tribe-of-indians-v-united-states/"),
    ("us.case.rdb.fcdr", "https://www.courtlistener.com/c/FCDR/12/3667/", "https://www.courtlistener.com/opinion/7928236/center-for-a-sustainable-coast-inc-v-georgia-department-of-natural/"),
    ("us.case.rdb.i_e_r_cas_bna", "https://www.courtlistener.com/c/IER%20Cases/21/1326/", "https://www.courtlistener.com/opinion/787844/william-fanslow-v-chicago-manufacturing-center-inc/"),
    ("us.case.rdb.jour_fr_inst", "https://www.courtlistener.com/c/Jour.%20Fr.%20Inst./6/132/", "https://www.courtlistener.com/opinion/8652837/hulls-truss/"),
    ("us.case.rdb.maca_pat_cas", "https://www.courtlistener.com/c/McA.%20Pat.%20Cas./1/123/", "https://www.courtlistener.com/opinion/7891147/in-re-winslow/"),
];

/// Rules whose CourtListener `/c/` link was verified from the agent pod with a
/// real citation read off CourtListener's reporter browse pages
/// (`scripts/courtlistener-browse-fetch.py`, dot-1ij4xz.11). Columns: rule id,
/// abbreviation, citation, url, status, final url, outcome; only `opinion`
/// rows count.
const BROWSE_LOG: &str =
    include_str!("../../../docs/fetch-logs/courtlistener-reporters-2026-10-26-browse.tsv");

/// The fetch-log clause for a rule whose CourtListener `/c/` link was
/// verified, or `None`.
fn courtlistener_fetch(id: &str) -> Option<String> {
    if let Some((_, url, final_url)) = VERIFIED_COURTLISTENER.iter().find(|(rid, _, _)| *rid == id)
    {
        return Some(format!("BrowserOS; {url} -> {final_url}"));
    }
    BROWSE_LOG.lines().skip(1).find_map(|line| {
        let cols: Vec<&str> = line.split('\t').collect();
        match cols.as_slice() {
            [rid, _, cite, url, _, final_url, "opinion"] if *rid == id => Some(format!(
                "agent pod, real citation {cite} from the CourtListener reporter browse pages; {url} -> {final_url}"
            )),
            _ => None,
        }
    })
}

fn rule_out(f: &Family, positive: Vec<PositiveOut>, negative: Vec<String>) -> RuleOut {
    let verified = courtlistener_fetch(&f.id);
    RuleOut {
        id: f.id.clone(),
        kind: "case",
        jurisdiction: f.jurisdiction.clone(),
        pattern: pattern(&f.strings, f.hyphen, f.neutral()),
        priority: f.priority,
        normalize: "{volume} {reporter} {page}",
        authority_key: "case/{volume}/{reporter}/{page}",
        url_templates: if verified.is_some() {
            vec!["https://www.courtlistener.com/c/{reporter}/{volume}/{page}/"]
        } else {
            vec![]
        },
        url_decoders: vec!["courtlistener.com/c/{reporter}/{volume}/{page}"],
        base_confidence: BASE_CONFIDENCE,
        provenance: ProvenanceOut {
            encodes: format!("reporters-db reporters.json entry {:?} ({})", f.key, f.name()),
            opened: false,
            source_url: format!(
                "https://github.com/freelawproject/reporters-db/blob/{UPSTREAM_SHA}/reporters_db/data/reporters.json"
            ),
            how_to_verify: format!(
                "Compare the reporter alternation with the {:?} entry of third_party/reporters-db/reporters.json (upstream {UPSTREAM_SHA}) and with Bluebook T1.",
                f.key
            ),
            derived_from: vec![],
            third_party: vec![format!(
                "reporters-db (Free Law Project, BSD-2-Clause) reporters.json @ {UPSTREAM_SHA}; see NOTICE"
            )],
        },
        examples: ExamplesOut { positive, negative },
        measured: MeasuredOut {
            precision: "unmeasured",
        },
        note: format!(
            "GENERATED by crates/xtask-rules from third_party/reporters-db; do not hand-edit (run `cargo run -p xtask-rules`). Editions: {}. {} reporter strings. {}",
            f.date_ranges(),
            f.strings.len(),
            if let Some(fetch) = verified {
                format!("url_templates verified by HTTP fetch on 2026-10-26 ({fetch}).")
            } else {
                "url_templates: none; no fetch of CourtListener /c/ is recorded for this reporter (the decoder stays).".to_string()
            }
        ),
    }
}

/// The short-form probe: `12 <string> at 345`.
pub fn short_probe(s: &str) -> String {
    format!("12 {s} at 345")
}

/// The `volume reporter at pin` pattern over `strings`, pin ranges as in
/// the hand-written short-form rule.
pub fn short_pattern(strings: &[String]) -> String {
    let mut alts: Vec<String> = strings.iter().map(|s| alternative(s)).collect();
    alts.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    alts.dedup();
    format!(
        "\\b(?P<volume>[0-9]{{1,4}})\\s+(?P<reporter>{})\\s+at\\s+(?P<pin>[0-9]{{1,7}}(?:\\s?[-–‒—]\\s?[0-9]{{1,7}})?)\\b",
        alts.join("|")
    )
}

fn short_rule_out(g: &Short, positive: Vec<PositiveOut>, negative: Vec<String>) -> RuleOut {
    let strings = &g.strings;
    RuleOut {
        id: g.id.clone(),
        kind: "case_short",
        jurisdiction: "us".into(),
        pattern: short_pattern(strings),
        priority: g.priority,
        normalize: "{volume} {reporter} at {pin}",
        authority_key: "",
        url_templates: vec![],
        url_decoders: vec![],
        base_confidence: BASE_CONFIDENCE,
        provenance: ProvenanceOut {
            encodes: "Bluebook R. 10.9 short form `volume reporter at page`, over every reporters-db reporter string the generated case rules own".into(),
            opened: false,
            source_url: format!(
                "https://github.com/freelawproject/reporters-db/blob/{UPSTREAM_SHA}/reporters_db/data/reporters.json"
            ),
            how_to_verify: "Open Bluebook R. 10.9 (short forms for cases); compare the reporter alternation with the generated case rules under rules/case/generated/.".into(),
            derived_from: vec!["us.short.case_at (hand-written short form for the major reporters)".into()],
            third_party: vec![format!(
                "reporters-db (Free Law Project, BSD-2-Clause) reporters.json @ {UPSTREAM_SHA}; see NOTICE"
            )],
        },
        examples: ExamplesOut { positive, negative },
        measured: MeasuredOut {
            precision: "unmeasured",
        },
        note: format!(
            "GENERATED by crates/xtask-rules from third_party/reporters-db; do not hand-edit (run `cargo run -p xtask-rules`). {} reporter strings; those the hand-written us.short.case_at matches are left to it. Resolves to the nearest earlier full case cite with the same volume and reporter.",
            strings.len()
        ),
    }
}

/// A planned short-form rule.
struct Short {
    id: String,
    priority: i32,
    strings: Vec<String>,
}

fn rule_index(reg: &Registry, id: &str) -> Result<usize, String> {
    reg.rules
        .iter()
        .position(|r| r.def.id == id)
        .ok_or_else(|| format!("{id} missing from the registry"))
}

/// Hand-written rules, the generated case rules and draft short rules.
fn short_registry(
    hand: &[(&str, &str)],
    cases: &[File],
    groups: &[Short],
) -> Result<Registry, String> {
    let mut texts: Vec<(String, String)> = cases
        .iter()
        .map(|f| (embedded_path(&f.rel), f.text.clone()))
        .collect();
    for g in groups {
        texts.push((
            embedded_path(&format!("{SHORT_GENERATED_DIR}/{}.json", g.id)),
            to_json(&short_rule_out(g, vec![], vec![]))?,
        ));
    }
    let mut all: Vec<(&str, &str)> = hand.to_vec();
    all.extend(texts.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    Ok(Registry::from_sources(&all))
}

/// Strings whose short probe is not detected as one citation of rule `id`.
fn short_unrecognized(reg: &Registry, id: &str, strings: &[String]) -> Vec<String> {
    strings
        .iter()
        .filter(|s| {
            let text = short_probe(s);
            !matches!(
                legal_citations::detect_with(reg, &text).as_slice(),
                [c] if c.rule_id == id && c.core_span.start == 0 && c.core_span.end == text.len()
            )
        })
        .cloned()
        .collect()
}

/// Short-form examples, or `None` when no string can carry positive[0].
fn choose_short_examples(
    reg: &Registry,
    idx: usize,
    strings: &[String],
) -> Option<(Vec<PositiveOut>, Vec<String>)> {
    let make = |s: &str, n: usize| {
        let pre = "Smith, ";
        let cite = format!("{} {s} at {}", 12 + 7 * n, 350 + 11 * n);
        PositiveOut {
            text: format!("{pre}{cite}."),
            spans: vec![[pre.len(), pre.len() + cite.len()]],
            groups: BTreeMap::new(),
        }
    };
    let ok = |p: &PositiveOut| own_spans(reg, idx, &p.text) == p.spans;
    let (first_i, mut first) = strings.iter().enumerate().find_map(|(i, s)| {
        let p = make(s, 0);
        (ok(&p) && wins_alone(reg, idx, &p)).then_some((i, p))
    })?;
    if let Some(caps) = reg
        .rules
        .get(idx)
        .and_then(|r| r.regex.captures(&first.text))
    {
        for name in ["volume", "reporter", "pin"] {
            if let Some(m) = caps.name(name) {
                first
                    .groups
                    .insert(name.to_string(), m.as_str().to_string());
            }
        }
    }
    let mut positives = vec![first];
    // A spread of other strings, then any string again (new numbers) until
    // there are three.
    let step = (strings.len() / MAX_POSITIVES).max(1);
    let spread = strings
        .iter()
        .enumerate()
        .step_by(step)
        .filter(|(i, _)| *i != first_i);
    let again = strings.iter().enumerate().cycle().take(3);
    for (pass, (_, s)) in spread.map(|x| (0, x)).chain(again.map(|x| (1, x))) {
        let n = positives.len();
        if n >= MAX_POSITIVES || (pass == 1 && n >= 3) {
            break;
        }
        let p = make(s, n);
        if ok(&p) {
            positives.push(p);
        }
    }
    let s = strings.get(first_i).map(String::as_str).unwrap_or("");
    let negatives: Vec<String> = [
        format!("Smith, 12 {s} 350, is the full cite."),
        format!("Volume 12 of {s}, at page 350."),
        format!("Smith, {s} at 350, has no volume."),
        "We met at 5 and left at 7.".to_string(),
    ]
    .into_iter()
    .filter(|t| {
        legal_citations::detect_with(reg, t)
            .iter()
            .all(|c| c.kind != Kind::CaseShort)
    })
    .collect();
    Some((positives, negatives))
}

/// Pretty JSON with spans kept on one line (`"spans": [[20, 32]]`), like the
/// hand-written rules.
fn to_json(rule: &RuleOut) -> Result<String, String> {
    let pretty = serde_json::to_string_pretty(rule).map_err(|e| e.to_string())?;
    let span = regex::Regex::new(r"\[\s+(\d+),\s+(\d+)\s+\]").map_err(|e| e.to_string())?;
    let pretty = span.replace_all(&pretty, "[$1, $2]");
    let spans = regex::Regex::new(r"\[\s+(\[\d+, \d+\])\s+\]").map_err(|e| e.to_string())?;
    Ok(spans.replace_all(&pretty, "[$1]").into_owned() + "\n")
}

/// Relative path (under `rules/`) of a family's file.
pub fn rel_path(f: &Family) -> String {
    format!("{GENERATED_DIR}/{}/{}.json", f.jurisdiction, f.id)
}

fn embedded_path(rel: &str) -> String {
    format!("rules/{rel}")
}

/// Positive text templates: `(prefix, suffix)` around `volume reporter page`.
const POSITIVE_TEMPLATES: [(&str, &str); 3] = [
    ("See Smith v. Jones, ", " ({year})."),
    (
        "The court relied on Doe v. Roe, ",
        ", {pin} ({year}) in reaching its holding.",
    ),
    ("Under Alpha v. Beta, ", " ({year}), the claim fails."),
];

/// Negative templates over `{v}`, `{s}`, `{p}`, `{year}`, `{name}`.
const NEGATIVE_TEMPLATES: [&str; 6] = [
    "Volume {v} of {s} begins at page {p}.",
    "The index lists {v} {s} without a page.",
    "The {s} series ran to {v} volumes by {year}.",
    "Its title is {name}, abbreviated {s}.",
    "Cite {s} by volume and page, not as {s} alone.",
    "Page {p} of {s} is missing from the copy.",
];

fn positive(f: &Family, s: &str, template: usize, n: usize) -> PositiveOut {
    let [t0, t1, t2] = POSITIVE_TEMPLATES;
    let (pre, post) = match template % 3 {
        0 => t0,
        1 => t1,
        _ => t2,
    };
    let year = f.year_of(s);
    let volume = if f.neutral() {
        year.clone()
    } else {
        (12 + 7 * n).to_string()
    };
    let page = 345 + 11 * n;
    let sep = if f.hyphen && f.neutral() { "-" } else { " " };
    let cite = format!("{volume}{sep}{s}{sep}{page}");
    let post = post
        .replace("{year}", &year)
        .replace("{pin}", &(page + 5).to_string());
    let start = pre.len();
    PositiveOut {
        text: format!("{pre}{cite}{post}"),
        spans: vec![[start, start + cite.len()]],
        groups: BTreeMap::new(),
    }
}

fn own_spans(reg: &Registry, idx: usize, text: &str) -> Vec<[usize; 2]> {
    reg.rules
        .get(idx)
        .map(|r| {
            r.regex
                .find_iter(text)
                .map(|m| [m.start(), m.end()])
                .collect()
        })
        .unwrap_or_default()
}

/// The self-example guard, as `registry::validate` applies it.
fn wins_alone(reg: &Registry, idx: usize, p: &PositiveOut) -> bool {
    let Some(want) = p.spans.first() else {
        return false;
    };
    let cites = legal_citations::detect_with(reg, &p.text);
    let own = cites
        .iter()
        .any(|c| c.rule_index == idx && c.core_span.start == want[0] && c.core_span.end == want[1]);
    let rivals = cites
        .iter()
        .any(|c| c.rule_index != idx && c.core_span.start < want[1] && want[0] < c.core_span.end);
    own && !rivals
}

fn no_case_match(reg: &Registry, text: &str) -> bool {
    legal_citations::detect_with(reg, text)
        .iter()
        .all(|c| c.kind != Kind::Case)
}

/// Examples for a family, or `None` when no string of it can carry the
/// documentation example (a higher-priority rule claims every one).
fn choose_examples(
    reg: &Registry,
    idx: usize,
    f: &Family,
) -> Option<(Vec<PositiveOut>, Vec<String>)> {
    let (first_i, mut first) = f.strings.iter().enumerate().find_map(|(i, s)| {
        let p = positive(f, s, 0, 0);
        (own_spans(reg, idx, &p.text) == p.spans && wins_alone(reg, idx, &p)).then_some((i, p))
    })?;
    if let Some(caps) = reg
        .rules
        .get(idx)
        .and_then(|r| r.regex.captures(&first.text))
    {
        for name in ["volume", "reporter", "page"] {
            if let Some(m) = caps.name(name) {
                first
                    .groups
                    .insert(name.to_string(), m.as_str().to_string());
            }
        }
    }
    let mut positives = vec![first];
    // Every other string once (editions first), up to MAX_POSITIVES; then
    // any string again with the next template until there are three.
    let others = (0..f.strings.len()).filter(|&i| i != first_i);
    let again = (0..f.strings.len())
        .cycle()
        .take(3 * POSITIVE_TEMPLATES.len());
    for (pass, i) in others.map(|i| (0, i)).chain(again.map(|i| (1, i))) {
        let n = positives.len();
        if n >= MAX_POSITIVES || (pass == 1 && n >= 3) {
            break;
        }
        let Some(s) = f.strings.get(i) else { continue };
        let p = positive(f, s, n, n);
        if own_spans(reg, idx, &p.text) == p.spans && !positives.iter().any(|q| q.text == p.text) {
            positives.push(p);
        }
    }
    let mut negatives: Vec<String> = Vec::new();
    'outer: for s in &f.strings {
        let year = f.year_of(s);
        for t in NEGATIVE_TEMPLATES {
            if negatives.len() >= 3 {
                break 'outer;
            }
            let text = t
                .replace("{v}", "12")
                .replace("{p}", "345")
                .replace("{year}", &year)
                .replace("{name}", &f.name())
                .replace("{s}", s);
            if !negatives.contains(&text) && no_case_match(reg, &text) {
                negatives.push(text);
            }
        }
    }
    Some((positives, negatives))
}

/// A generated file: path relative to `rules/` and its contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    /// Path relative to `rules/`, e.g. `case/generated/us.fed/us.fed.case.rdb.f.json`.
    pub rel: String,
    /// JSON text.
    pub text: String,
}

/// The result of a generation run.
#[derive(Debug)]
pub struct Output {
    /// Generated files, sorted by path.
    pub files: Vec<File>,
    /// Counts.
    pub stats: Stats,
    /// Contract violations of the full registry (must be empty).
    pub violations: Vec<Violation>,
}

/// Strings of `f` whose probe is not detected as exactly one citation of
/// rule `idx` covering all of it.
fn unrecognized(reg: &Registry, idx: usize, f: &Family) -> Vec<String> {
    f.strings
        .iter()
        .filter(|s| {
            let text = probe(s);
            !matches!(
                legal_citations::detect_with(reg, &text).as_slice(),
                [c] if c.rule_index == idx && c.core_span.start == 0 && c.core_span.end == text.len()
            )
        })
        .cloned()
        .collect()
}

/// Hand-written rules plus every family's pattern (examples left empty: they
/// do not affect detection).
fn full_registry(hand: &[(&str, &str)], families: &[Family]) -> Result<Registry, String> {
    let mut drafts = Vec::new();
    for f in families {
        drafts.push((
            embedded_path(&rel_path(f)),
            to_json(&rule_out(f, vec![], vec![]))?,
        ));
    }
    let mut all: Vec<(&str, &str)> = hand.to_vec();
    all.extend(drafts.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    let full = Registry::from_sources(&all);
    match full.load_errors.first() {
        Some(e) => Err(format!("{} [{}] {}", e.rule, e.check, e.detail)),
        None => Ok(full),
    }
}

/// Generate rules from `reporters.json` text and the hand-written rule
/// files (`(path relative to rules/, text)`).
pub fn generate(reporters_json: &str, handwritten: &[(String, String)]) -> Result<Output, String> {
    let db = parse_db(reporters_json)?;
    let hand: Vec<(String, String)> = handwritten
        .iter()
        .map(|(p, t)| (embedded_path(p), t.clone()))
        .collect();
    let hand_refs: Vec<(&str, &str)> = hand.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
    let hand_reg = Registry::from_sources(&hand_refs);
    let (mut families, mut stats) = plan(&db, &hand_reg);

    // A family that loses its example, or any of its strings, to a
    // higher-priority rule gets one retry at ELEVATED_PRIORITY. One that
    // still has no usable example is dropped and reported.
    let mut files = Vec::new();
    for attempt in 0..2 {
        let full = full_registry(&hand_refs, &families)?;
        files.clear();
        stats.unrecognized.clear();
        stats.conflicts.clear();
        let mut lost = Vec::new();
        for (i, f) in families.iter().enumerate() {
            let idx = full
                .rules
                .iter()
                .position(|r| r.def.id == f.id)
                .ok_or_else(|| format!("{} missing from the registry", f.id))?;
            let missed = unrecognized(&full, idx, f);
            let examples = choose_examples(&full, idx, f);
            if attempt == 0 && (examples.is_none() || !missed.is_empty()) {
                lost.push(i);
                continue;
            }
            stats.unrecognized.extend(missed);
            match examples {
                Some((pos, neg)) => files.push(File {
                    rel: rel_path(f),
                    text: to_json(&rule_out(f, pos, neg))?,
                }),
                None => {
                    stats.elevated.retain(|k| *k != f.key);
                    stats.conflicts.push((f.key.clone(), f.strings.clone()));
                }
            }
        }
        if lost.is_empty() {
            break;
        }
        for &i in &lost {
            if let Some(f) = families.get_mut(i) {
                f.priority = ELEVATED_PRIORITY;
                stats.elevated.push(f.key.clone());
            }
        }
    }
    stats.rules = files.len();

    // Short forms: one rule over every string a case rule owns (generated or
    // hand-written, e.g. `T.C.`, `M.J.`), minus those the hand-written
    // short-form rule matches. Strings that
    // lose `12 <s> at 345` to a higher-priority rule move to a second rule
    // at ELEVATED_PRIORITY.
    let dropped: BTreeSet<&str> = stats.conflicts.iter().map(|(k, _)| k.as_str()).collect();
    let mut short_strings = Vec::new();
    for s in families
        .iter()
        .filter(|f| !dropped.contains(f.key.as_str()))
        .flat_map(|f| f.strings.iter())
        .chain(
            stats
                .handwritten
                .iter()
                .filter(|(_, k)| *k == Kind::Case)
                .map(|(s, _)| s),
        )
    {
        if owner(&hand_reg, &short_probe(s)).is_some() {
            stats.short_handwritten += 1;
        } else {
            short_strings.push(s.clone());
        }
    }
    let mut groups = vec![Short {
        id: SHORT_ID.into(),
        priority: PRIORITY,
        strings: short_strings,
    }];
    for attempt in 0..2 {
        let reg = short_registry(&hand_refs, &files, &groups)?;
        let missed: Vec<Vec<String>> = groups
            .iter()
            .map(|g| short_unrecognized(&reg, &g.id, &g.strings))
            .collect();
        let losers = missed.first().cloned().unwrap_or_default();
        if attempt == 0 && !losers.is_empty() {
            if let Some(g) = groups.first_mut() {
                g.strings.retain(|s| !losers.contains(s));
            }
            groups.push(Short {
                id: SHORT_ELEVATED_ID.into(),
                priority: ELEVATED_PRIORITY,
                strings: losers,
            });
            continue;
        }
        stats
            .unrecognized
            .extend(missed.iter().flatten().map(|s| short_probe(s)));
        for g in groups.iter().filter(|g| !g.strings.is_empty()) {
            let idx = rule_index(&reg, &g.id)?;
            match choose_short_examples(&reg, idx, &g.strings) {
                Some((pos, neg)) => {
                    stats.short_strings += g.strings.len();
                    files.push(File {
                        rel: format!("{SHORT_GENERATED_DIR}/{}.json", g.id),
                        text: to_json(&short_rule_out(g, pos, neg))?,
                    });
                }
                None => stats.conflicts.push((g.id.clone(), g.strings.clone())),
            }
        }
        break;
    }
    files.sort_by(|a, b| a.rel.cmp(&b.rel));

    let finals: Vec<(String, String)> = files
        .iter()
        .map(|f| (embedded_path(&f.rel), f.text.clone()))
        .collect();
    let mut all: Vec<(&str, &str)> = hand_refs;
    all.extend(finals.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    let violations = validate(&Registry::from_sources(&all));
    Ok(Output {
        files,
        stats,
        violations,
    })
}

/// One-paragraph summary of a run.
pub fn report(out: &Output) -> String {
    let s = &out.stats;
    format!(
        "reporters-db {UPSTREAM_SHA}: {} families, {} reporters, {} editions, {} variations, {} strings; {} case rules generated, short form over {} strings ({} left to the hand-written one), {} strings left to hand-written rules ({} case, others {:?}), {} shared with an earlier family; {} short-form variants {:?}; {} families raised to priority {ELEVATED_PRIORITY} {:?}; {} families dropped for conflicts {:?}; {} strings unrecognized {:?}; {} violations",
        s.families,
        s.reporters,
        s.editions,
        s.variations,
        s.strings,
        s.rules,
        s.short_strings,
        s.short_handwritten,
        s.handwritten.len(),
        s.handwritten.iter().filter(|(_, k)| *k == Kind::Case).count(),
        s.handwritten
            .iter()
            .filter(|(_, k)| *k != Kind::Case)
            .collect::<Vec<_>>(),
        s.shared,
        s.short_form_variants.len(),
        s.short_form_variants,
        s.elevated.len(),
        s.elevated,
        s.conflicts.len(),
        s.conflicts,
        s.unrecognized.len(),
        s.unrecognized,
        out.violations.len()
    )
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
    Ok(())
}

/// Every `*.json` under `dir`, as `(path relative to dir, text)`, sorted.
pub fn read_tree(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let mut paths = Vec::new();
    if dir.exists() {
        walk(dir, &mut paths).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let rel = p
                .strip_prefix(dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            Ok((rel, text))
        })
        .collect()
}

/// Generate from the repo at `root`. With `check`, compare against the
/// committed files instead of writing; returns the differing paths.
pub fn run(root: &Path, check: bool) -> Result<(Output, Vec<String>), String> {
    let rules = root.join(RULES_DIR);
    let dirs = [GENERATED_DIR, SHORT_GENERATED_DIR];
    let generated = |rel: &str| dirs.iter().any(|d| rel.starts_with(&format!("{d}/")));
    let handwritten: Vec<(String, String)> = read_tree(&rules)?
        .into_iter()
        .filter(|(rel, _)| !generated(rel))
        .collect();
    let json = std::fs::read_to_string(root.join(REPORTERS_JSON))
        .map_err(|e| format!("{REPORTERS_JSON}: {e}"))?;
    let out = generate(&json, &handwritten)?;
    let committed: BTreeMap<String, String> = read_tree(&rules)?
        .into_iter()
        .filter(|(rel, _)| generated(rel))
        .collect();
    let fresh: BTreeMap<String, String> = out
        .files
        .iter()
        .map(|f| (f.rel.clone(), f.text.clone()))
        .collect();
    let mut diff: Vec<String> = fresh
        .iter()
        .filter(|(rel, text)| committed.get(*rel) != Some(*text))
        .map(|(rel, _)| rel.clone())
        .collect();
    diff.extend(
        committed
            .keys()
            .filter(|rel| !fresh.contains_key(*rel))
            .cloned(),
    );
    diff.sort();
    if !check {
        for dir in dirs.map(|d| rules.join(d)).iter().filter(|d| d.exists()) {
            std::fs::remove_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        for f in &out.files {
            let path = rules.join(&f.rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::write(&path, &f.text).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok((out, diff))
}

/// The `xtask-rules` command: `(success, stdout, stderr)`. Fails on any
/// contract violation and, with `check`, on stale committed files.
pub fn cli(root: &Path, check: bool) -> (bool, String, String) {
    let (out, diff) = match run(root, check) {
        Ok(r) => r,
        Err(e) => return (false, String::new(), format!("xtask-rules: {e}\n")),
    };
    let mut err = String::new();
    // The court table is optional input: a tree without the Indigo
    // inventory (the unit-test fixtures) skips it.
    let mut courts_line = String::new();
    if root.join(courts::INDIGO_JSONL).exists() {
        match courts::run(root, check) {
            Ok((n, stale)) => {
                courts_line = format!("{n} courts in {}\n", courts::COURTS_JSON);
                if stale && check {
                    err.push_str(&format!(
                        "stale {} (run `cargo run -p xtask-rules`)\n",
                        courts::COURTS_JSON
                    ));
                }
            }
            Err(e) => err.push_str(&format!("xtask-rules: {e}\n")),
        }
    }
    for v in &out.violations {
        err.push_str(&format!("{} [{}] {}\n", v.rule, v.check, v.detail));
    }
    let stale = check && !diff.is_empty();
    if stale {
        err.push_str("stale generated rules (run `cargo run -p xtask-rules`):\n");
        for d in &diff {
            err.push_str(&format!("  {d}\n"));
        }
    }
    let ok = !stale && out.violations.is_empty() && err.is_empty();
    (ok, format!("{}\n{courts_line}", report(&out)), err)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;

    fn rep(cite_type: &str, mlz: &[&str]) -> Reporter {
        Reporter {
            name: "Test Reports".into(),
            cite_type: cite_type.into(),
            editions: BTreeMap::new(),
            variations: BTreeMap::new(),
            mlz_jurisdiction: mlz.iter().map(|s| (*s).to_string()).collect(),
            cite_format: None,
        }
    }

    #[test]
    fn jurisdictions_follow_cite_type_and_mlz() {
        assert_eq!(
            jurisdiction(&rep("federal", &["us:c9;court.appeals"])),
            "us.fed"
        );
        assert_eq!(
            jurisdiction(&rep("scotus_early", &["us;supreme.court"])),
            "us.fed"
        );
        assert_eq!(
            jurisdiction(&rep("state_regional", &["us:pa;supreme.court"])),
            "us.state"
        );
        assert_eq!(
            jurisdiction(&rep(
                "state",
                &["us:nc;supreme.court", "us:sc;supreme.court"]
            )),
            "us.nc"
        );
        assert_eq!(
            jurisdiction(&rep("neutral", &["us:terr:dakota;supreme.court"])),
            "us.terr"
        );
        assert_eq!(jurisdiction(&rep("state", &["en:c;queens.bench"])), "gb");
        assert_eq!(
            jurisdiction(&rep("state", &["us:del;supreme.court"])),
            "us.de"
        );
        assert_eq!(jurisdiction(&rep("state", &["us;supreme.court"])), "us");
        assert_eq!(jurisdiction(&rep("state", &[])), "us");
        assert_eq!(
            jurisdiction(&rep(
                "specialty",
                &["us:c9:ca.nd;district.court", "us:fed;x", "us;y"]
            )),
            "us.fed"
        );
        assert_eq!(
            jurisdiction(&rep("specialty", &["us:ny;a", "us:ny;b"])),
            "us.ny"
        );
        assert_eq!(jurisdiction(&rep("specialty", &["gb:england;kb"])), "gb");
        assert_eq!(
            jurisdiction(&rep("specialty", &["us:ny;a", "us:c2;b"])),
            "us"
        );
        assert_eq!(jurisdiction(&rep("specialty", &[])), "us");
    }

    #[test]
    fn slugs_are_id_segments() {
        assert_eq!(slug("F. Supp. 2d"), "f_supp_2d");
        assert_eq!(slug("App’x"), "app_x");
        assert_eq!(slug("(So. 2nd)"), "so_2nd");
        assert_eq!(slug("§§"), "x");
    }

    #[test]
    fn patterns_put_longer_strings_first_and_widen_spaces() {
        let p = pattern(&["A.".into(), "A.2d".into(), "So. 2d".into()], false, false);
        assert!(p.ends_with("(?P<page>[0-9]{1,5})\\b"), "{p}");
        assert!(p.contains("(?P<reporter>So\\.\\s+2d|A\\.2d|A\\.)"), "{p}");
        let re = regex::Regex::new(&p).unwrap();
        assert_eq!(
            re.find("12 So.\n2d 34").map(|m| m.as_str()),
            Some("12 So.\n2d 34")
        );
        let h = pattern(&["NMSC".into()], true, true);
        let re = regex::Regex::new(&h).unwrap();
        assert!(re.is_match("2009-NMSC-001"));
        assert!(re.is_match("2009 NMSC 001"));
        assert!(re.is_match("2014 NMSC 1205830"));
    }

    #[test]
    fn family_strings_dedupe_and_trim() {
        let mut r = rep("state", &["us:ny;a"]);
        r.editions.insert("N.Y.".into(), Edition::default());
        r.variations.insert(" N.Y. ".into(), "N.Y.".into());
        r.variations.insert("NY".into(), "N.Y.".into());
        r.variations.insert(" ".into(), "N.Y.".into());
        r.cite_format = Some("{reporter} {page}".into());
        assert_eq!(family_strings(std::slice::from_ref(&r)), vec!["N.Y.", "NY"]);
        assert!(!hyphenated(std::slice::from_ref(&r)));
        r.cite_format = Some("{volume}-{reporter}-{page}".into());
        assert!(hyphenated(&[r]));
    }

    #[test]
    fn family_metadata_has_years_and_ranges() {
        let mut a = rep("neutral", &["us:nm;a"]);
        a.editions.insert(
            "NMSC".into(),
            Edition {
                start: Some("1998-01-01T00:00:00".into()),
                end: None,
            },
        );
        a.variations.insert("N.M.S.C.".into(), "NMSC".into());
        let mut b = a.clone();
        b.editions.insert("Old".into(), Edition::default());
        let f = Family {
            key: "NMSC".into(),
            id: "us.nm.case.rdb.nmsc".into(),
            jurisdiction: "us.nm".into(),
            reporters: vec![a, b],
            strings: vec!["NMSC".into(), "N.M.S.C.".into(), "Old".into()],
            hyphen: true,
            priority: PRIORITY,
        };
        assert_eq!(f.name(), "Test Reports");
        assert_eq!(f.year_of("N.M.S.C."), "1998");
        assert_eq!(f.year_of("Old"), "1900");
        assert_eq!(f.year_of("nope"), "1900");
        assert!(f.neutral());
        assert_eq!(
            f.date_ranges(),
            "NMSC 1998-present; NMSC 1998-present; Old ?-present"
        );
        let p = positive(&f, "NMSC", 0, 0);
        assert_eq!(p.text, "See Smith v. Jones, 1998-NMSC-345 (1998).");
        assert_eq!(p.spans, vec![[20, 33]]);
    }

    const DB: &str = r#"{
      "Zz. Rep.": [{"name": "Zed Reports", "cite_type": "state",
                    "editions": {"Zz. Rep.": {"start": "1850-01-01T00:00:00", "end": "1900-12-31T00:00:00"},
                                 "Zz. Rep. 2d": {"start": "1901-01-01T00:00:00", "end": null}},
                    "variations": {"Zz.Rep.": "Zz. Rep.", "F.3d": "Zz. Rep."},
                    "mlz_jurisdiction": ["us:zz;supreme.court"]}],
      "Zz.Rep.": [{"name": "Dup", "cite_type": "state", "editions": {"Zz.Rep.": {}},
                   "variations": {}, "mlz_jurisdiction": ["us:zz;supreme.court"]}],
      "Zz Rep": [{"name": "Slug twin", "cite_type": "state", "editions": {"Zz Rep": {}},
                  "variations": {}, "mlz_jurisdiction": ["us:zz;supreme.court"]}],
      "Zz-Rep": [{"name": "Slug triplet", "cite_type": "state", "editions": {"Zz-Rep": {}},
                  "variations": {}, "mlz_jurisdiction": ["us:zz;supreme.court"]}],
      "Empty": []
    }"#;

    const FEDERAL_REPORTER: &str =
        include_str!("../../../rules/case/us.case.federal_reporter.json");

    fn hand() -> Vec<(String, String)> {
        vec![(
            "case/us.case.federal_reporter.json".into(),
            FEDERAL_REPORTER.into(),
        )]
    }

    #[test]
    fn generation_skips_handwritten_and_shared_strings() {
        let out = generate(DB, &hand()).unwrap();
        let s = &out.stats;
        assert_eq!(
            (s.families, s.reporters, s.editions, s.variations),
            (5, 4, 5, 2)
        );
        assert_eq!(s.strings, 7);
        assert_eq!(s.handwritten, vec![("F.3d".to_string(), Kind::Case)]);
        assert_eq!(s.shared, 1);
        assert_eq!(s.rules, 3);
        assert_eq!((s.short_strings, s.short_handwritten), (6, 0));
        let rels: Vec<&str> = out.files.iter().map(|f| f.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec![
                "case/generated/us.zz/us.zz.case.rdb.zz_rep.json",
                "case/generated/us.zz/us.zz.case.rdb.zz_rep_2.json",
                "case/generated/us.zz/us.zz.case.rdb.zz_rep_3.json",
                "case_short/generated/us.short.rdb.case_at.json",
            ]
        );
        assert!(out.violations.is_empty(), "{:#?}", out.violations);
        let main: serde_json::Value = serde_json::from_str(&out.files[2].text).unwrap();
        assert_eq!(main["id"], "us.zz.case.rdb.zz_rep_3");
        assert_eq!(
            main["examples"]["positive"][0]["groups"]["reporter"],
            "Zz. Rep."
        );
        assert!(main["pattern"]
            .as_str()
            .unwrap()
            .contains("Zz\\.\\s+Rep\\.\\s+2d|"));
        assert!(
            out.files[2].text.contains("\"spans\": [[20, 35]]"),
            "{}",
            out.files[2].text
        );
        assert_eq!(main["provenance"]["opened"], false);
        assert!(main["note"]
            .as_str()
            .unwrap()
            .contains("Zz. Rep. 1850-1900"));
    }

    #[test]
    fn generation_reports_bad_input() {
        assert!(generate("{", &[]).unwrap_err().contains("reporters.json"));
        let bad = vec![("case/x.json".to_string(), "{".to_string())];
        assert!(generate(DB, &bad).unwrap_err().contains("[parse]"));
    }

    const LOSER: &str = r#"{"Q": [{"name": "Q", "cite_type": "federal", "editions": {"Xx 12 F.": {}},
                     "variations": {}, "mlz_jurisdiction": ["us;supreme.court"]}]}"#;

    #[test]
    fn families_that_lose_their_example_are_elevated() {
        // The only string ends in `12 F.`, so its example holds a hand-written
        // `12 F. 345` that outranks priority 30; at 65 the family wins.
        let out = generate(LOSER, &hand()).unwrap();
        assert_eq!(out.stats.rules, 1);
        assert_eq!(out.stats.elevated, vec!["Q".to_string()]);
        assert!(out.stats.conflicts.is_empty());
        assert!(out.violations.is_empty(), "{:#?}", out.violations);
        assert!(out.files[0].text.contains("\"priority\": 65,"));
        assert!(report(&out).contains("1 families raised to priority 65"));
    }

    #[test]
    fn families_that_lose_even_when_elevated_are_dropped() {
        let mut fed: serde_json::Value = serde_json::from_str(FEDERAL_REPORTER).unwrap();
        fed["priority"] = 99.into();
        let hand = vec![(
            "case/us.case.federal_reporter.json".to_string(),
            fed.to_string(),
        )];
        let out = generate(LOSER, &hand).unwrap();
        assert_eq!(out.stats.rules, 0);
        assert!(out.files.is_empty());
        assert!(out.stats.elevated.is_empty());
        assert_eq!(
            out.stats.conflicts,
            vec![("Q".to_string(), vec!["Xx 12 F.".to_string()])]
        );
        assert!(out.violations.is_empty(), "{:#?}", out.violations);
        assert!(report(&out).contains("1 families dropped"));
    }

    #[test]
    fn run_writes_then_checks_clean() {
        let root = std::env::temp_dir().join(format!("xtask-rules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rules = root.join(RULES_DIR);
        std::fs::create_dir_all(rules.join("case")).unwrap();
        std::fs::write(
            rules.join("case/us.case.federal_reporter.json"),
            FEDERAL_REPORTER,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("third_party/reporters-db")).unwrap();
        std::fs::write(root.join(REPORTERS_JSON), DB).unwrap();
        let stale = rules.join(GENERATED_DIR).join("old/stale.json");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "{}").unwrap();

        let (ok, _, err) = cli(&root, true);
        assert!(!ok);
        assert_eq!(err.matches("\n  ").count(), 5, "{err}");
        assert!(stale.exists(), "--check must not write");
        let (out, _) = run(&root, false).unwrap();
        assert!(!stale.exists());
        assert_eq!(out.files.len(), 4);
        let (ok, stdout, err) = cli(&root, true);
        assert!(ok, "{err}");
        assert!(
            stdout.contains("3 case rules generated, short form over 6 strings"),
            "{stdout}"
        );

        // A broken hand-written rule fails the run even when fresh.
        let mut bad: serde_json::Value = serde_json::from_str(FEDERAL_REPORTER).unwrap();
        bad["id"] = "us.case.broken".into();
        bad["examples"]["negative"] = serde_json::json!([]);
        std::fs::write(rules.join("case/broken.json"), bad.to_string()).unwrap();
        let (ok, _, err) = cli(&root, true);
        assert!(!ok);
        assert!(err.contains("us.case.broken [examples-count]"), "{err}");

        std::fs::remove_file(root.join(REPORTERS_JSON)).unwrap();
        let (ok, _, err) = cli(&root, true);
        assert!(!ok);
        assert!(err.contains("reporters.json"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn courtlistener_fetches_come_from_both_logs() {
        let browser = courtlistener_fetch("us.ar.case.rdb.ark_app").unwrap();
        assert!(browser.starts_with("BrowserOS; "), "{browser}");
        let pod = courtlistener_fetch("us.ar.case.rdb.ark").unwrap();
        assert!(pod.contains("real citation 200 Ark. 671"), "{pod}");
        assert!(
            pod.ends_with("/opinion/3272223/kitchens-v-wheeler/"),
            "{pod}"
        );
        // Logged but unverified (CourtListener has no volumes for it).
        assert!(BROWSE_LOG.contains("us.fed.case.rdb.a_e_c\t"));
        assert_eq!(courtlistener_fetch("us.fed.case.rdb.a_e_c"), None);
        assert_eq!(courtlistener_fetch("no.such.rule"), None);
    }

    #[test]
    fn read_tree_of_a_missing_dir_is_empty() {
        assert!(read_tree(Path::new("/nonexistent/xtask-rules"))
            .unwrap()
            .is_empty());
    }
}
