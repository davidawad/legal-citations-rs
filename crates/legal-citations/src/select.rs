//! Rule collections, selectors and named profiles.
//!
//! Every rule belongs to collections derived from its file without editing
//! it ([`RuleDef::collections`]): `kind:<kind>`, `family:<family>`,
//! `jurisdiction:<prefix>` for every dotted prefix of its jurisdiction,
//! `tag:<tag>` and `rule:<id>`. A [`Selector`] names collections with
//! patterns (`*` and `?` globs over the value; a pattern without a namespace
//! is a rule-id glob) and [`Registry::select`] turns it into a [`Selection`]:
//! a cheap view over the compiled registry, no regex recompiled.
//!
//! Detection always runs every active rule, so overlap resolution and
//! short-form antecedents are the same as an unfiltered run; the selection
//! then decides which citations are reported. A short form (and a parallel
//! cite) is reported when its rule is selected, or when the citation it
//! refers to is reported and its rule is not explicitly excluded. So
//! `--jurisdiction us.oh` keeps an `Id.` that follows an Ohio cite and drops
//! one that follows a federal cite.
//!
//! A case citation's `jurisdiction` comes from its court parenthetical when
//! the court table knows the court ([`crate::court`]); `jurisdiction:`
//! patterns then apply to that, not the rule's, so `--jurisdiction us.oh`
//! keeps `100 N.E.2d 1 (Ohio 2010)` and drops `(9th Cir. 2001)`.
//!
//! Precedence: the selected set is the union of `include` and every profile
//! (or every rule when both are empty), minus every `exclude`. Exclude wins.

use crate::citation::Citation;
use crate::registry::{Registry, Rule, Violation};
use crate::rule::{Family, Kind, RuleDef};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/profile_files.rs"));

/// Include/exclude patterns plus named profiles. Also embedded in
/// [`crate::Policy`] (`include`, `exclude`, `profiles`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Selector {
    /// Patterns to include, e.g. `jurisdiction:us.oh*`,
    /// `jurisdiction:us.fed.dist.txed`, `court:E.D. Tex.`, `kind:case`,
    /// `family:agency`, `tag:ptab`, `rule:us.fed.statute.usc`, `us.fed.*`.
    pub include: Vec<String>,
    /// Patterns to exclude; exclusion wins over every include and profile.
    pub exclude: Vec<String>,
    /// Named profiles (`cite-detect profiles list`) unioned with `include`.
    pub profiles: Vec<String>,
}

impl Selector {
    /// True when the selector selects every rule.
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty() && self.profiles.is_empty()
    }

    /// Append another selector's lists to this one.
    pub fn extend(&mut self, other: &Selector) {
        self.include.extend(other.include.iter().cloned());
        self.exclude.extend(other.exclude.iter().cloned());
        self.profiles.extend(other.profiles.iter().cloned());
    }
}

/// A selector that cannot be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectError {
    /// The offending pattern or profile name.
    pub pattern: String,
    /// Why it was rejected.
    pub reason: String,
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "selector {:?}: {}", self.pattern, self.reason)
    }
}

impl std::error::Error for SelectError {}

/// Selector namespaces. `court:` is an alias of `jurisdiction:` that also
/// takes a court as cited (`court:E.D. Tex.` is `jurisdiction:us.fed.dist.txed`).
const NAMESPACES: [&str; 6] = ["kind", "family", "jurisdiction", "court", "tag", "rule"];

/// True when `glob` is written as a jurisdiction id (`us.fed.dist.txed`,
/// `foreign.il.dist*`) rather than a court abbreviation.
fn is_id_glob(glob: &str) -> bool {
    glob.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '*' | '?'))
}

/// One parsed pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern {
    /// Namespace (`kind`, `family`, `jurisdiction`, `tag`, `rule`).
    pub namespace: &'static str,
    /// Glob over the collection value.
    pub glob: String,
}

impl Pattern {
    /// Parse `ns:glob` or a bare rule-id glob. Unknown namespaces, and
    /// `kind:`/`family:` values that match no kind or family, are errors.
    pub fn parse(raw: &str) -> Result<Pattern, SelectError> {
        let err = |reason: String| SelectError {
            pattern: raw.to_string(),
            reason,
        };
        let text = raw.trim();
        let (ns, glob) = match text.split_once(':') {
            Some((ns, glob)) => (ns.trim(), glob.trim()),
            None => ("rule", text),
        };
        if glob.is_empty() {
            return Err(err("empty pattern".into()));
        }
        let namespace = NAMESPACES.into_iter().find(|n| *n == ns).ok_or_else(|| {
            err(format!(
                "unknown namespace {ns:?}; use one of {}",
                NAMESPACES.join(", ")
            ))
        })?;
        if namespace == "court" {
            let glob = if is_id_glob(glob) {
                glob.to_string()
            } else {
                crate::court::jurisdiction_of(glob)
                    .ok_or_else(|| {
                        err(format!(
                            "unknown court {glob:?}; give a court as cited (`E.D. Tex.`) or a jurisdiction id (`us.fed.dist.txed`)"
                        ))
                    })?
                    .to_string()
            };
            return Ok(Pattern {
                namespace: "jurisdiction",
                glob,
            });
        }
        let pat = Pattern {
            namespace,
            glob: glob.to_string(),
        };
        let known = match namespace {
            "kind" => Some(Kind::ALL.iter().map(|k| k.as_str()).collect::<Vec<_>>()),
            "family" => Some(Family::ALL.iter().map(|f| f.as_str()).collect()),
            _ => None,
        };
        if let Some(names) = known {
            if !names.iter().any(|n| glob_match(&pat.glob, n)) {
                return Err(err(format!(
                    "no {namespace} matches {glob:?}; {namespace}s: {}",
                    names.join(", ")
                )));
            }
        }
        Ok(pat)
    }

    /// True when the rule is in a collection this pattern names.
    pub fn matches(&self, def: &RuleDef) -> bool {
        self.matches_collections(&def.collections())
    }

    /// True when one of `collections` (`kind:case`, ...) is named.
    pub fn matches_collections(&self, collections: &[String]) -> bool {
        let prefix = format!("{}:", self.namespace);
        collections
            .iter()
            .filter_map(|c| c.strip_prefix(&prefix))
            .any(|v| glob_match(&self.glob, v))
    }
}

/// `*` (any run, including dots) and `?` (one char) glob, case-sensitive.
pub fn glob_match(glob: &str, text: &str) -> bool {
    let g: Vec<char> = glob.chars().collect();
    let t: Vec<char> = text.chars().collect();
    // Iterative wildcard match with single-star backtracking.
    let (mut gi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        match g.get(gi) {
            Some('*') => {
                star = Some((gi, ti));
                gi += 1;
            }
            Some(c) if *c == '?' || Some(c) == t.get(ti) => {
                gi += 1;
                ti += 1;
            }
            _ => match star {
                Some((sg, st)) => {
                    gi = sg + 1;
                    ti = st + 1;
                    star = Some((sg, st + 1));
                }
                None => return false,
            },
        }
    }
    g.get(gi..)
        .is_some_and(|rest| rest.iter().all(|c| *c == '*'))
}

/// A named profile: `profiles/<name>.json`, embedded at build time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Name, equal to the file stem.
    pub name: String,
    /// What the profile is for.
    pub description: String,
    /// Patterns to include.
    #[serde(default)]
    pub include: Vec<String>,
    /// Patterns removed from this profile's own includes.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Free-form notes (limits, what to combine it with).
    #[serde(default)]
    pub note: String,
}

/// Every embedded profile plus the files that failed to parse.
#[derive(Debug, Default)]
pub struct Profiles {
    /// Parsed profiles in file-path order.
    pub profiles: Vec<Profile>,
    /// Files that failed to parse, or whose name is not their file stem.
    pub load_errors: Vec<Violation>,
}

impl Profiles {
    /// Parse `(path, json)` pairs.
    pub fn from_sources(files: &[(&str, &str)]) -> Profiles {
        let mut out = Profiles::default();
        for (path, text) in files {
            match serde_json::from_str::<Profile>(text) {
                Ok(p) => {
                    let stem = path
                        .rsplit('/')
                        .next()
                        .and_then(|f| f.strip_suffix(".json"))
                        .unwrap_or("");
                    if p.name != stem {
                        out.load_errors.push(Violation {
                            rule: (*path).to_string(),
                            check: "profile-name",
                            detail: format!("name {:?} must equal the file stem {stem:?}", p.name),
                        });
                    }
                    out.profiles.push(p);
                }
                Err(e) => out.load_errors.push(Violation {
                    rule: (*path).to_string(),
                    check: "profile-parse",
                    detail: e.to_string(),
                }),
            }
        }
        out
    }

    /// Look a profile up by name.
    pub fn get(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

/// The embedded profiles, parsed once.
pub fn profiles() -> &'static Profiles {
    static CELL: OnceLock<Profiles> = OnceLock::new();
    CELL.get_or_init(|| Profiles::from_sources(PROFILE_FILES))
}

/// Check every profile: it parses, its name is its file stem, every pattern
/// parses, and it selects at least one rule. Profile patterns may name a
/// `tag:` or jurisdiction no rule uses yet, so later rules can join them.
pub fn validate_profiles(reg: &Registry, set: &Profiles) -> Vec<Violation> {
    let mut out = set.load_errors.clone();
    for p in &set.profiles {
        let mut fail = |detail: String| {
            out.push(Violation {
                rule: format!("profile:{}", p.name),
                check: "profile",
                detail,
            });
        };
        if p.description.trim().is_empty() {
            fail("description is empty".into());
        }
        match profile_mask(reg, p) {
            Ok(mask) if !mask.iter().any(|m| *m) && !names_a_court(&p.include) => {
                fail("selects no rule".into())
            }
            Ok(_) => {}
            Err(e) => fail(e.to_string()),
        }
    }
    out
}

/// True when a `jurisdiction:` (or `court:`) pattern names a court in the
/// court table: citations from that court are selected even though no rule
/// has its jurisdiction.
fn names_court(pat: &Pattern) -> bool {
    pat.namespace == "jurisdiction"
        && crate::court::court_jurisdictions()
            .iter()
            .any(|j| glob_match(&pat.glob, j))
}

fn names_a_court(patterns: &[String]) -> bool {
    patterns
        .iter()
        .filter_map(|p| Pattern::parse(p).ok())
        .any(|p| names_court(&p))
}

fn parse_all(patterns: &[String]) -> Result<Vec<Pattern>, SelectError> {
    patterns.iter().map(|p| Pattern::parse(p)).collect()
}

/// Rules matched by any pattern. With `strict`, a pattern matching no rule
/// is an error (it is almost always a typo), unless it is a `jurisdiction:`
/// pattern naming a court in the court table (`us.fed.dist.txed`): no rule
/// has that jurisdiction, but citations whose court parenthetical names it do.
fn mask_of(reg: &Registry, patterns: &[String], strict: bool) -> Result<Vec<bool>, SelectError> {
    let mut mask = vec![false; reg.rules.len()];
    for (raw, pat) in patterns.iter().zip(parse_all(patterns)?) {
        let mut hit = false;
        for (m, rule) in mask.iter_mut().zip(&reg.rules) {
            if pat.matches(&rule.def) {
                *m = true;
                hit = true;
            }
        }
        if strict && !hit && !names_court(&pat) {
            return Err(SelectError {
                pattern: raw.clone(),
                reason: if pat.namespace == "jurisdiction" {
                    "matches no rule or court; see `cite-detect rules list` and `cite-detect courts`".into()
                } else {
                    "matches no rule; see `cite-detect rules list`".into()
                },
            });
        }
    }
    Ok(mask)
}

fn profile_mask(reg: &Registry, p: &Profile) -> Result<Vec<bool>, SelectError> {
    let inc = mask_of(reg, &p.include, false)?;
    let exc = mask_of(reg, &p.exclude, false)?;
    Ok(inc.iter().zip(&exc).map(|(i, e)| *i && !*e).collect())
}

/// A filtered view of a registry. Cheap: one flag per rule, plus the parsed
/// patterns for citations whose court moved them to another jurisdiction.
#[derive(Clone, Debug)]
pub struct Selection<'r> {
    reg: &'r Registry,
    selected: Vec<bool>,
    excluded: Vec<bool>,
    patterns: Patterns,
}

/// A selector's parsed patterns: `include`, each profile's
/// `(include, exclude)`, and `exclude`. Empty `include` and no profiles
/// means everything.
#[derive(Clone, Debug, Default)]
struct Patterns {
    include: Vec<Pattern>,
    profiles: Vec<(Vec<Pattern>, Vec<Pattern>)>,
    exclude: Vec<Pattern>,
}

impl Patterns {
    /// `(selected, excluded)` for something in `collections`, by the same
    /// precedence as the rule masks.
    fn flags(&self, collections: &[String]) -> (bool, bool) {
        let any = |ps: &[Pattern]| ps.iter().any(|p| p.matches_collections(collections));
        let everything = self.include.is_empty() && self.profiles.is_empty();
        let included = everything
            || any(&self.include)
            || self.profiles.iter().any(|(i, e)| any(i) && !any(e));
        let excluded = any(&self.exclude);
        (included && !excluded, excluded)
    }
}

impl<'r> Selection<'r> {
    /// Every active rule of `reg`.
    pub fn all(reg: &'r Registry) -> Selection<'r> {
        Selection {
            reg,
            selected: reg.rules.iter().map(|r| r.def.retired.is_none()).collect(),
            excluded: vec![false; reg.rules.len()],
            patterns: Patterns::default(),
        }
    }

    /// Apply `sel` to `reg`, resolving profiles from `set`.
    pub fn new(reg: &'r Registry, sel: &Selector, set: &Profiles) -> Result<Self, SelectError> {
        let mut include = mask_of(reg, &sel.include, true)?;
        let excluded = mask_of(reg, &sel.exclude, true)?;
        let mut patterns = Patterns {
            include: parse_all(&sel.include)?,
            profiles: Vec::new(),
            exclude: parse_all(&sel.exclude)?,
        };
        for name in &sel.profiles {
            let p = set.get(name).ok_or_else(|| {
                let names: Vec<&str> = set.profiles.iter().map(|p| p.name.as_str()).collect();
                SelectError {
                    pattern: format!("profile:{name}"),
                    reason: format!("unknown profile; profiles: {}", names.join(", ")),
                }
            })?;
            for (i, m) in include.iter_mut().zip(profile_mask(reg, p)?) {
                *i |= m;
            }
            patterns
                .profiles
                .push((parse_all(&p.include)?, parse_all(&p.exclude)?));
        }
        let everything = sel.include.is_empty() && sel.profiles.is_empty();
        let selected = reg
            .rules
            .iter()
            .zip(include.iter().zip(&excluded))
            .map(|(r, (inc, exc))| r.def.retired.is_none() && (everything || *inc) && !*exc)
            .collect();
        Ok(Selection {
            reg,
            selected,
            excluded,
            patterns,
        })
    }

    /// The underlying registry.
    pub fn registry(&self) -> &'r Registry {
        self.reg
    }

    /// True when rule `index` is selected.
    pub fn contains(&self, index: usize) -> bool {
        self.selected.get(index).copied().unwrap_or(false)
    }

    /// Selected rules with their registry index.
    pub fn rules(&self) -> impl Iterator<Item = (usize, &'r Rule)> + '_ {
        let reg = self.reg;
        reg.rules
            .iter()
            .enumerate()
            .filter(|(i, _)| self.contains(*i))
    }

    /// Number of selected rules.
    pub fn len(&self) -> usize {
        self.selected.iter().filter(|s| **s).count()
    }

    /// True when no rule is selected.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `(selected, excluded)` for a citation: its rule's flags, or, when its
    /// court resolved it to another jurisdiction, the patterns applied to the
    /// rule's collections in that jurisdiction. A short form's jurisdiction
    /// is inherited, so it is reported by following its antecedent instead.
    fn flags(&self, c: &Citation) -> (bool, bool) {
        let rule = self.reg.rules.get(c.rule_index);
        match rule {
            Some(r) if c.jurisdiction != r.def.jurisdiction && !c.kind.is_short_form() => {
                let (selected, excluded) =
                    self.patterns.flags(&r.def.collections_in(&c.jurisdiction));
                (r.def.retired.is_none() && selected, excluded)
            }
            _ => (
                self.contains(c.rule_index),
                self.excluded.get(c.rule_index).copied().unwrap_or(false),
            ),
        }
    }

    /// Keep the citations this selection reports (see the module docs) and
    /// renumber every citation index they hold.
    pub fn retain(&self, cites: Vec<Citation>) -> Vec<Citation> {
        let mut keep = vec![false; cites.len()];
        for (i, c) in cites.iter().enumerate() {
            let follows = c
                .antecedent
                .or(c.details.parallel_of())
                .or(c.details.history_of());
            let via = follows
                .filter(|j| *j < i)
                .is_some_and(|j| keep.get(j).copied().unwrap_or(false));
            let (selected, excluded) = self.flags(c);
            if let Some(k) = keep.get_mut(i) {
                *k = selected || (via && !excluded);
            }
        }
        crate::citation::retain(cites, &keep)
    }
}

/// Something detection can run against: a whole [`Registry`] or a
/// [`Selection`] of it.
pub trait RuleSet {
    /// The registry whose rules run.
    fn registry(&self) -> &Registry;
    /// The filter applied to the result, if any.
    fn selection(&self) -> Option<&Selection<'_>>;
}

impl RuleSet for Registry {
    fn registry(&self) -> &Registry {
        self
    }
    fn selection(&self) -> Option<&Selection<'_>> {
        None
    }
}

impl RuleSet for Selection<'_> {
    fn registry(&self) -> &Registry {
        self.reg
    }
    fn selection(&self) -> Option<&Selection<'_>> {
        Some(self)
    }
}

impl Registry {
    /// A filtered view of this registry; profiles come from the embedded set.
    /// Unknown namespaces, kinds, families or profiles, and include/exclude
    /// patterns that match no rule, are errors.
    pub fn select(&self, sel: &Selector) -> Result<Selection<'_>, SelectError> {
        Selection::new(self, sel, profiles())
    }
}
