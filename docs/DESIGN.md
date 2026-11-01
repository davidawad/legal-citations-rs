# legal-citations-rs: system design (dot-1ij4xz)

Status: design, not built. Date: 2026-10-22. Owner bead: dot-1ij4xz.

## 0. What this is, in one paragraph

A standalone Rust library and CLI (`cite-detect`) that finds legal citations
of every kind in text or markdown, says exactly where each one is in the
original bytes, what authority it names, and whether the document links it to
that authority. It has no dotfiles, TNET or formbook dependency. TNET, formbook
and the dotfiles `legal-document` crate are optional consumers at the edge.
It detects citations; it never claims a citation supports a proposition.

## 1. Design principle: one source of truth, everything else is a projection

The system is built so that an agent (or David) changing anything touches
exactly one place, and every consequence is regenerated and checked
mechanically. The single source of truth is the **rule registry**: one data
file per recognizer rule. Code, docs, coverage tables, fixtures, the TNET
preset pack and measured precision are all projections of it.

```
rules/<kind>/<rule-id>.json   <- the only thing a human/agent edits to add coverage
        |  build.rs compiles + validates (fails the build on any contract breach)
        v
  L0 Source        original bytes, line index, byte<->char<->utf16 offset maps
  L1 Recognize     rules -> raw matches (rule id, span, named groups)
  L2 Assemble      raw matches -> Citation (pincite, parenthetical, signal, string cite, overlap resolution)
  L3 Resolve       short forms (id., ibid., supra, short case) -> antecedent; every Citation gets an AuthorityKey
  L4 Document      markdown model: link spans + targets, code/front-matter exclusion zones, footnotes
  L5 Policy        which kinds must be linked, which link targets are acceptable, suppressions
  L6 Findings      deterministic, stable rule-coded findings with fix hints
        |
        +-- cite-detect CLI (json | jsonl | md-report | sarif later)
        +-- TNET adapter (optional cargo feature in TNET + plugins/legal-citations pack)
        +-- formbook pre-render check (optional link-scheme profile, see 8.3)
        +-- dotfiles legal-document (L1 raw mode, parity)
```

Each layer has a typed, serializable output and a CLI stage to dump it
(`cite-detect inspect --stage recognize|assemble|resolve|document|policy`).
An agent debugging a wrong result reads the layer where it went wrong rather
than guessing from the final finding.

## 2. The rule registry (L1 source of truth)

One JSON file per rule (David's preference, 2026-10-22). JSON costs doubled
regex backslashes and has no comments; it matches TNET's preset format, and
`schema/rule.schema.json` (also `cite-detect schema --rule`) gives editor
validation. Notes go in `note`. Example:

```json
{
  "id": "us.fed.statute.usc",
  "kind": "statute",
  "jurisdiction": "us.fed",
  "pattern": "(?P<title>\\d{1,2})\\s+U\\.S\\.C\\.?\\s*(?:§§?|[Ss]ec(?:tion)?\\.?)\\s*(?P<section>[\\w.\\-]+(?:\\(\\w+\\))*)",
  "normalize": "{title} U.S.C. § {section}",
  "authority_key": "us.usc/{title}/{section}",
  "priority": 50,
  "url_templates": ["https://www.law.cornell.edu/uscode/text/{title}/{section}"],
  "url_decoders": ["law.cornell.edu/uscode/text/{title}/{section}"],
  "provenance": {
    "encodes": "Bluebook R. 12.3 (21st ed.)",
    "opened": false,
    "source_url": "",
    "how_to_verify": "Open Bluebook R. 12.3 and Table T1 U.S.C. entry",
    "derived_from": ["dotfiles legal_citation_patterns.py FED_STAT_RE"],
    "third_party": []
  },
  "examples": {
    "positive": [
      {"text": "jurisdiction under 28 U.S.C. § 1331.", "spans": [[19, 36]],
       "groups": {"title": "28", "section": "1331"}}
    ],
    "negative": ["the 28 U.S. citizens in section 1331 of the report"]
  },
  "measured": {"precision": "unmeasured"},
  "note": "positive[0] is the documentation example (self-example guard). Spans are UTF-8 bytes (§ is 2 bytes). measured is written only by cite-detect eval. opened is true only if the rule text was actually read."
}
```

The registry contract (`registry::validate`, run by the `registry_contract` test and `cite-detect rules validate`) enforces the following; build.rs only embeds the files, so the checks are tested, covered and callable:

- regex compiles; every named group used by `normalize`, `authority_key`,
  `url_templates` exists.
- **self-example guard**: positive example 0 is matched by exactly this rule
  (after overlap resolution) with exactly the stated span. This kills the
  previous failure mode where rule wording was copied into a regex.
- every positive is matched with its exact spans; no negative matches any rule
  of that kind.
- `provenance.opened = false` forces `confidence <= 0.8` ceiling and marks the
  rule "unverified" in every projection.
- id uniqueness; id never deleted, only `retired = true` with reason.

### 2.1 Kind taxonomy (closed enum, extensible by PR only)

`case`, `case_short`, `id`, `supra`, `statute`, `session_law`, `regulation`
(CFR, state admin codes), `federal_register`, `constitution`, `court_rule`
(FRCP, FRAP, FRE, Fed. Cir. R., local rules), `agency_manual` (MPEP, TBMP,
TMEP), `agency_decision` (PTAB/TTAB decisions, Rev. Rul.), `record` (ER-139,
JA 16, Ex. 1001, TTABVUE, Pet. App. 23a, ECF No.), `secondary` (Restatement,
law reviews, treatises), `treaty`, `foreign`, `docket`, `legislative`,
`executive`, `statute_short` (`§ 1983` after `42 U.S.C. § 1983`: resolved to
the latest statute or regulation with that section, a low-confidence cue when
none precedes it), and `unrecognized` (the catch-all layer, 3.2).

`record` is a separate kind because record cites usually cannot be hyperlinked
to a public source; the default policy (5) does not require them to be linked.

### 2.2 Seeding the registry

1. Generate one JSON rule per existing pattern from `legal-citation-patterns`
   (57 rules), with `derived_from` set and `opened = false`. Keep the 67-case
   `parity.json` as the regression baseline.
2. Generate reporter case rules from reporters-db `reporters.json` and
   `journals.json` (BSD-2-Clause, verified; vendored data with NOTICE). One rule
   per reporter family. Each rule also carries reporter date ranges so L2 can
   lower confidence when the parenthetical year falls outside them.
3. Write state statute, court rule and agency rules by hand, using citeurl
   templates (MIT, verified) as reference. Copy wording only with attribution
   in `third_party`.
4. Write short forms (id., ibid., supra, `449 U.S. at 389`) by hand, following
   eyecite's model (BSD-2-Clause): token rules plus backward-only resolution.

### 2.3 Collections, selectors and profiles (lcr-z18)

Selection never edits rule files. `RuleDef::collections()` derives, per rule:
`kind:<kind>`, `family:<family>` (`Kind::family()`, overridable by an optional
`"family"` field), `jurisdiction:<p>` for every dotted prefix of the
jurisdiction, `tag:<t>` for each optional `"tags"` entry (lowercase
`[a-z0-9_-]`, checked by `validate`), and `rule:<id>`. A `Selector`
(`include`, `exclude`, `profiles`) of glob patterns becomes a `Selection`
via `Registry::select`: one flag per rule, no regex recompiled. The selected
set is (include ∪ profiles, or everything when both are empty) − exclude.

`detect_with`, `detect_markdown_with` and `lint_markdown_with` take any
`RuleSet` (a `Registry` or a `Selection`). Detection always runs every
active rule and the selection filters the result, so overlap resolution and
short-form antecedents are identical to a full run. A short form, parallel
cite or subsequent-history cite is kept when its own rule is selected, or
when the citation it refers to is kept and its rule is not explicitly
excluded. Indices (`antecedent`, parallel and history links) are renumbered.

Profiles are data: `crates/legal-citations/profiles/<name>.json`
(`name`, `description`, `include`, `exclude`, `note`), embedded by build.rs
like rules and checked by `validate_profiles` (also run by `rules validate`).
Profile patterns are syntax-checked and the profile must select a rule, but a
pattern may name a tag or agency jurisdiction no rule uses yet, so later rules
join by tagging themselves (`ptab`, `ttab`, `uspto`, `tax`, `immigration`,
`labor`, `agency`) or by jurisdiction (`us.fed.eoir`, `us.fed.nlrb`, ...).
User patterns are strict: an include or exclude that matches no rule is an
error (CLI exit 3; library CITE008 and a full-registry lint).

## 3. Citation model (L2/L3 output, the public API)

```rust
pub struct Citation {
    pub kind: Kind,
    pub rule_id: RuleId,                 // provenance pointer, `cite-detect explain <rule_id>`
    pub span: Span,                      // core: byte + char + utf16 + line/col, half-open, ORIGINAL text
    pub full_span: Span,                 // includes signal, pincite, parenthetical
    pub text: String,
    pub fields: BTreeMap<String, String>, // named groups
    pub jurisdiction: Jurisdiction,
    pub normalized: String,              // Bluebook-ish display form
    pub authority: Option<AuthorityKey>, // identity; short forms inherit from antecedent
    pub antecedent: Option<usize>,       // index of the full cite a short form resolves to
    pub confidence: f32,                 // documented formula, see 3.1
    pub signal: Option<String>,
    pub parenthetical: Option<Span>,
}
pub fn detect(text: &str) -> Vec<Citation>;              // plain text
pub fn detect_markdown(md: &str) -> Document;            // L4: citations + links + zones
pub fn detect_raw(text: &str) -> Vec<RawMatch>;          // L1 only, legacy keep-all semantics
```

`span`, the half-open byte span, is the single source of truth. Char and UTF-16
offsets are derived from it by one offset map, and proptest checks that
`&text[span]` always equals `citation.text`. This matches TNET's span contract,
so findings map across without conversion.

The `AuthorityKey` is what makes this a system and not a pile of regexes.
Every layer above L3 reasons about authorities (`us.scotus.reporter/U.S./347/483`,
`us.usc/28/1331`), not strings:

- Short-form handling. `Id.` is linked correctly when its link target decodes
  to the same key as its antecedent.
- Link congruence. A link is correct when the URL decodes to the key the text
  cites (section 5).
- Fix hints. A citation with a key plus a `url_template` yields a suggested URL.
- Cross-document indexes. Downstream consumers (a table of authorities, the
  dotfiles validator) group by key.

### 3.0 Typed fields and authorities (lcr-bq9)

`Citation.details` is a tagged union: `case` (case name, parties, volume,
reporter as written and canonical per Bluebook R. 6.1(a) spacing, page,
database number, pincites, court, year, date, `parallel_of`/`parallels`,
`subsequent_history`/`history_of`), `code` (code, title, sections,
subsection path, year, edition), `agency` (agency, document type, number,
volume, page, date), `short` (pincites) or `other`. `Citation.urls` holds
every URL the rule's `url_templates` build (verified hosts only); a resolved
short form inherits its antecedent's. Both are added fields: existing
consumers see the same report shape plus two keys.

Parallel cites are consecutive case citations separated by a bare comma, the
later ones without a case name; the group shares name, court and date. A
subsequent-history cite follows a case's full span after `, aff'd,`,
`, cert. denied,` and similar phrases, and is attached to the first decision
of the chain. `Citation.authority` is unchanged for every cite (so link
congruence still compares each reporter's own key).

`authority::authorities(&[Citation])` groups by authority key: parallel
cites join their first cite (and register their own keys as aliases, so a
later bare cite of the parallel reporter joins too), short forms join their
antecedent, history cites are listed under the decision they follow. Keyless
citations group by kind and display text. `cite-detect authorities` renders
md, json or jsonl (`schema/authorities.schema.json`).

### 3.0.1 Court jurisdiction (lcr-k5o, court-level ids)

`Citation.jurisdiction` is the court's when the date parenthetical names a
court the court table knows, else the rule's; `Citation.rule_jurisdiction`
is always the rule's. Ids are hierarchical down to the court:
`us.scotus`; `us.fed.app.ca1`..`ca11`, `cadc`, `cafc`;
`us.fed.dist.<state><district>` (`us.fed.dist.txed`, PACER's codes);
`us.fed.bankr.<district>` and `us.fed.bankr.bap.ca9`; `us.fed.cc.<district>`
(old circuit courts); `us.fed.cit`, `us.fed.military.caaf`, ...; and
`us.<st>.sup` / `.app` / `.trial` for state courts (`100 N.E.2d 1 (Ohio
2010)` is `us.oh.sup`, `(Ohio Ct. App. 2010)` `us.oh.app`). Foreign rules
use `foreign.<iso2>.<court>`. A rule jurisdiction already inside the
court's is kept; a court string that only starts with a state abbreviation
(`Ohio Ct. Com. Pl.`) gets the state (`Court.partial`, `us.oh`). Resolved
short forms take their antecedent's.

The table, `crates/legal-citations/courts/courts.json`, is generated by
`cargo run -p xtask-rules` from `corpus/formats/indigo.jsonl` (Indigo Book,
CC0): state abbreviations from R23.3, state court names from T3 (level from
the name, `App` is intermediate, plus an override list), federal courts from
T1, with district, bankruptcy and old circuit court names generated over
the state abbreviations and B.A.P.s over the circuits. `cite-detect courts`
lists it. Matching ignores whitespace; the longest entry that starts the
court string on a token or period boundary wins.

Selection applies `jurisdiction:` patterns to a full cite's resolved
jurisdiction by prefix and glob (the rule's other collections unchanged), so
`--jurisdiction us.oh` keeps the N.E.2d and Westlaw Ohio cites and drops the
Ninth Circuit, `us.fed.dist` keeps every district court and
`--jurisdiction us.fed.dist.txed --jurisdiction 'foreign.il.dist*'` keeps
exactly those courts. A `jurisdiction:` pattern is valid when it matches a
rule or a court-table id; `court:` is an alias that also takes a court as
cited (`court:E.D. Tex.`). Short forms still follow their antecedent.

### 3.1 Confidence

Confidence is a small documented formula, not vibes. It starts from a per-rule
base. Corroborating features raise it: a parenthetical year inside the
reporter's date range, a known reporter, a resolved antecedent. Contradicting
features lower it: the year is out of range, or the match falls on a generic
fallback rule. Two caps apply: `opened = false` caps it at 0.8, and an
`unmeasured` precision caps it at 0.9. `cite-detect explain --citation N` prints
each term of the calculation.

### 3.2 Overlap resolution and the catch-all layer

Overlap resolution follows eyecite's approach: sort by start ascending, then end
descending, drop matches contained in a longer one, and break ties by `priority`.
`detect_raw` keeps every match, which preserves the current `legal-document`
behavior (keep-all, deduplicated on `(type, start_char)`). That gives a
zero-behavior-change migration path.

The catch-all layer (lcr-xvl) is kind `unrecognized`, rules in
`rules/unrecognized/`. It does not take part in overlap resolution with real
rules: real rules are recognized and assembled first, then catch-all matches
that overlap any raw real match or any real citation's span or full span are
dropped, the rest are overlap-resolved among themselves, assembled and merged
in text order. Real citations are therefore byte-for-byte what they were
without the layer. Catch-all rules use `base_confidence` 0.4 and produce
CITE011 (warn) instead of link findings. Rules may carry `reject` patterns: a
match whose text a reject pattern finds is dropped and the search resumes one
character after its start (the `regex` crate has no look-around). Any rule may also carry `reject_before` patterns, run on the up to 24 bytes before the match (anchored with `$`): the stand-in for look-behind that keeps `L.R.` out of `A.L.R.3d`, `Appx 345` out of `Fed. Appx 345` and `Sup. Ct. R.` out of `Hall's Sup. Ct. R.`.

## 4. Document model (L4): markdown, generic

The parser is pulldown-cmark with offset iterators, with tables, footnotes and
YAML/TOML front-matter blocks enabled. It produces:

- **Exclusion zones**: front matter, code spans, code blocks, HTML comments,
  and link destinations. Citations are not reported inside these zones.
  `--include-front-matter` overrides this.
- **Link objects**: `{text_span, dest, dest_span, style: inline|reference|autolink|footnote|html_a}`.
  Reference definitions are resolved. A footnote definition counts as linking
  its reference only if the definition itself contains a link to the authority.
- **Citation-to-link binding**: a citation is LINKED when its `span` falls
  inside a link's text span. A string cite that spans several links binds each
  citation separately.

Non-markdown input (`--input text`) has no link objects, so every citation is
UNLINKED. That is the right answer for plain text.

## 5. Policy (L5) and findings (L6)

Policy is one JSON file (`cite-detect.json`, or `--policy`) and is the same for every
consumer. TNET, formbook and the CLI therefore can never disagree about what
"compliant" means.

```json
{
  "require_link": ["case", "case_short", "id", "supra", "statute", "session_law", "regulation",
                   "federal_register", "constitution", "court_rule", "agency_manual",
                   "agency_decision", "secondary", "treaty", "foreign"],
  "allowed_hosts": ["courtlistener.com", "law.cornell.edu", "govinfo.gov", "ecfr.gov", "uspto.gov",
                    "supremecourt.gov", "justia.com", "congress.gov", "federalregister.gov"],
  "link_schemes": {},
  "check_congruence": true
}
```

`record` and `docket` are absent from `require_link` by default. `link_schemes`
maps an indirect scheme to a resolver, e.g. `{"toa": "front-matter:authorities.{key}.url"}`
(8.3). `check_congruence` requires the URL to decode to the cited AuthorityKey
when a decoder exists.

Findings use closed, stable codes. Every finding carries the citation, the
policy clause that fired, and a machine-applicable fix where one exists.

| Code | Meaning | Deterministic | Default severity |
| --- | --- | --- | --- |
| `CITE001 unlinked` | citation of a require_link kind is not inside a link | yes | error |
| `CITE002 bad-url` | link destination does not parse as a URL or known scheme | yes | error |
| `CITE003 host-not-allowed` | URL host not on allowlist | yes | warn |
| `CITE004 incongruent` | URL decodes to a different AuthorityKey than the text cites | yes | error |
| `CITE005 undecodable` | host allowed but no decoder: cannot confirm target | yes | info |
| `CITE006 scheme-unresolved` | indirect scheme (toa:) key has no URL | yes | error |
| `CITE008 bad-selector` | policy include/exclude/profiles cannot be applied | yes | error |
| `CITE010 detected` | inventory record (not a failure) | yes | info |
| `CITE011 unrecognized-citation` | citation-shaped text no specific rule matched | yes | warn |
| `CITE020 low-confidence` | match below threshold, possible false positive | heuristic | info |

Fix hints: CITE001 suggests `[text](url_template(fields))` when the rule has a
template. `cite-detect fix --apply` writes only fixes whose URL comes from a
template for an `opened = true` rule, and never guesses. Every other fix stays
a suggestion.

Suppression is `<!-- cite-detect: ignore CITE001 reason="..." -->` on the
preceding line. The reason is mandatory, and suppressions are counted in the
summary so they can't hide.

The online verifier (`cite-detect verify-links`, behind a separate cargo feature
`online`) HEADs or GETs each URL and calls the CourtListener citation-lookup API.
It is never part of the default gate and never runs in CI by default.

## 6. CLI surface

```
cite-detect [FILE|-] [--input md|text] [--format json|jsonl|md-report] [--kinds K,..]
            [--require-links] [--policy FILE] [--fail-on info|warn|error]
            [--profile NAME] [--include P] [--exclude P] [--jurisdiction J]
cite-detect rules [list|show ID] [--json]      # registry, coverage, provenance, measured precision
cite-detect rules list --select P              # rules a selector picks
cite-detect profiles [list|show NAME]          # named rule profiles
cite-detect authorities FILE [--format md|json|jsonl]  # table of authorities
cite-detect explain CODE|RULE_ID               # what it means, the rule it encodes, verified or not
cite-detect inspect FILE --stage S             # dump any layer
cite-detect fix FILE [--apply]                 # template-backed fixes only
cite-detect eval --corpus DIR                  # precision/recall per rule -> writes [measured]
cite-detect schema [--finding|--citation]      # JSON Schema for every output
cite-detect verify-links FILE                  # online, feature-gated, opt-in
```

Exit codes follow TNET's codes so callers learn one scheme: 0 pass, 1 findings
at or above `--fail-on`, 2 usage, 3 policy/config, 4 input/parse, 5 runtime.
Output ordering is deterministic (span, then code). JSONL begins with a
`header` (schema id `cite-detect.finding/1`, tool version, registry hash,
policy hash) and ends with a `summary` (counts by code, kind and rule;
suppressed; passed). An agent can stop reading after the summary line, and the
two hashes make any result reproducible.

## 7. Measurement: honesty by construction

- **Corpus registry**: `corpus/manifest.json` lists public briefs and opinions
  (CourtListener / govinfo URLs, license or public-domain status, sha256). Text
  is fetched by `just corpus-fetch` and is not committed if the license is
  unclear.
- **Gold labels**: a stratified sample is hand-labeled with spans. Sample sizes
  are stated per kind and per jurisdiction.
- `cite-detect eval` writes the `[measured]` block per rule (precision, recall
  where gold exists, n, corpus hash, date). The README coverage table is
  generated from it. A rule with no measurement shows `unmeasured`, never a
  blank.
- **Clean-text false-positive gate**: a corpus of non-legal prose (public-domain
  books, news) must produce zero findings above `info`.
- **Snapshot tests** (insta) over the corpus: any rule edit shows its full blast
  radius as a diff in review.

## 8. Consumers (edges, all optional)

### 8.1 TNET

TNET plugins are declarative JSON packs that reference built-in scorers
(`english-text-eval-presets/src/loader.rs`). A plugin cannot ship code. The
integration therefore has two parts:

1. A feature-gated adapter inside TNET, following the Harper precedent
   (`crates/english-text-eval/src/harper.rs`, exit 6 under `--strict-adapters`).
   It adds metrics `citation.linked`, `citation.link_target` and
   `citation.detected`, which map `cite-detect` findings 1:1 onto TNET findings.
   The spans are already TNET-shaped. This touches TNET shared files, so it
   needs text-pm's agreement before any edit.
2. `plugins/legal-citations/` (pack): presets that set policy params (kinds,
   allowlist, severity), `requires_scorers`, a test-crate with compliant and
   violating fixtures, and provenance records. The regex-only rules in
   `citations-federal` and `citations-ptab-ttab` are re-pointed to
   `citation.detected` with their authority blocks kept. Those rules are
   `pattern.required`/`pattern.count` today and never flag a bad citation.
   `CITE001`/`CITE004` are deterministic and may be warn/error. `CITE020` is a
   cue: `cue_only: true`, info (per `docs/presets/CUE-SCORERS.md` on
   origin/main).

### 8.2 dotfiles

`legal-document` switches to `legacy::detect_legacy` and must pass
`parity.json` byte-for-byte. Correction (2026-10-22, verified while seeding):
`parity.json` spans are UTF-8 byte offsets, not Python character offsets
(slicing by character drifts one position per preceding `§`), so the existing
byte comparison was right and there is no latent mismatch. `legal-citation-validator` does NOT use
`legal-citation-patterns` today; it has its own `fancy_regex` table in
`core.rs`. Migrating it means a behavior change, so it is a separate bead with
its own parity fixture, not a "no behavior change" swap.

### 8.3 formbook (optional profile, not a design driver)

formbook links citations as `[cite](toa:key)` to a front-matter `authorities`
map that has no URL field today. This is handled only as a generic
`link_schemes` entry in policy. No formbook code lives in the library. To make
formbook files pass, `authorities.<key>` would need a `url:` field; that is
David's call, and formbook is not modified. The command is
`cite-detect INPUT.md --policy formbook.json`, with a proposed justfile target
`cite-check DIR`.

## 9. Repo and quality bar

- Repo: gitlab.com/davidawad/legal-citations-rs (created private 2026-10-22; made public the same day at David's direction so dotfiles and TNET can take an anonymous HTTPS git dependency).
  GPL-3.0-or-later, `publish = false`.
  Crates: `legal-citations` (lib; features `markdown`, `online`) and
  `cite-detect` (bin).
- Quality gates: nextest; proptest (span slicing, no panic, idempotent normalize,
  markdown/plain parity on link-free text); cargo-fuzz targets (`detect`,
  `detect_markdown`, `policy_parse`); cargo-mutants per file under
  `ulimit -v`; clippy with `unwrap_used`/`expect_used`/`panic` denied;
  cargo-deny; gitleaks; llvm-cov at 97 percent lines; no `#[ignore]`.
  All heavy runs go in a box pod.
- `NOTICE` carries the reporters-db/eyecite (Free Law Project, BSD-2-Clause) and
  citeurl (MIT) notices for any vendored data.
- `AGENTS.md` in the repo tells an agent the four facts it needs:
  1. Edit `rules/`, never generated code.
  2. Run `just check`.
  3. Read `cite-detect inspect` to debug.
  4. `[measured]` is written only by `eval`.

## 10. Build order (each a bead, each independently landable)

1. Registry schema + build.rs validator + L0/L1 + the 57 seeded rules + parity.
2. L2/L3 citation model, overlap, short forms, AuthorityKey.
3. L4 markdown model + L5 policy + L6 findings + CLI (json/jsonl, exit codes, schema, explain, inspect).
4. reporters-db generated case rules + NOTICE; statutes/rules/agency expansion.
5. URL templates/decoders (congruence + fix) for the allowlisted hosts.
6. Corpus + eval + generated README coverage table; clean-text FP gate.
7. dotfiles `legal-document` migration (detect_raw parity).
8. TNET adapter (after text-pm agrees) + `plugins/legal-citations` pack.
9. Optional: formbook profile doc; online verifier; validator migration.

## 11. Open decisions for David

- Should `Id.` and short forms require their own link (default: yes, strict), or
  is a linked antecedent enough?
- Should `record` cites be exempt from links by default (default: exempt)?

## 12. Build log (2026-10-22)

- Repo: gitlab.com/davidawad/legal-citations-rs (public), main + branch `dot-1ij4xz`, own br store (prefix `lcr`).
- 111 rule files: 57 legacy-derived (frozen legacy pattern kept per rule for parity mode; forward pattern uses ASCII digits) plus 54 new ones (reporters, Ohio R.C./webcite/Ohio St., short forms, session laws, Fed. Reg., constitutions, court rules, USPTO, record, docket, secondary, treaty, foreign). One retired: `us.case.legacy_full`.
- Legacy defects found while seeding: the case pattern's reporter class has no digits (misses F.2d/F.3d/F.4th, splits `F. Supp. 2d 200` into reporter `F. Supp.` page `2`) and swallows signals into case names (`See Brown versus Board of Education`); nine state patterns (Pa., Va., W. Va., Fla., La., Mo., Ky., Me., Vt.) never match the standard Bluebook form (lost escape: `Fla(?:\.|\s+Stat)\.` requires `Fla..`); the 67-case parity fixture could not catch it because it records whatever Python matched. Forward rules fix these; legacy parity mode keeps them byte for byte.
- URL fix templates only for hosts verified by HTTP fetch (CourtListener `/c/`, Cornell LII uscode/cfr/rules, govinfo plaw/statute); eCFR, federalregister.gov and Justia block scripted fetches and get decoders only.
- Markdown: HTML blocks (e.g. a paragraph starting with `<!--u-->`) keep their prose; only comments and tags are masked.
- `markdown` is not a cargo feature: pulldown-cmark is small and always on.
- Box verification is blocked for command-only pods by dot-83ad50 (sandbox controller rejects the empty credential); verification runs as an agent box instead.
- 2026-10-23 combined branch (lcr-g): the five parallel jobs did not build green together. The catch-all layer made the reporters-db generator think it owned every reporter string (owner() now ignores `unrecognized`); generated duplicates of B3's hand-written specialty reporters (T.C., M.J., Vet. App., Ct. Int'l Trade, agency reporters) were dropped; three B2 negatives were real reporter cites. `reject_before` (look-behind stand-in) stops `L.R.`, `Appx` and `Sup. Ct. R.` splitting `A.L.R.3d`, `Fed. Appx` and `Hall's Sup. Ct. R.`, which removed B1's priority-65 workaround for those families. Court parentheticals now set `Citation.jurisdiction` (3.0.1). `cite-detect coverage` measures the Indigo inventory: 658/1225 formats at first run, 1070 after `scripts/indigo_rules.py` (state session laws, registers, nominative reporters, fallback code forms) and two criterion fixes; the baseline is a test gate. Regex crates build at opt-level 3 in dev (the debug suite went from about 175 s to 50 s).
- 2026-10-23 lcr-i0p: `scripts/indigo_rules.py` is ported to Rust as `cargo run -p xtask-rules -- indigo` (`just indigo-rules`). Run against the same inputs, it writes the 163 files the Python wrote byte for byte except one intended difference: each note now names the Rust command instead of the script. The freshness test `committed_indigo_rules_are_fresh` fails when a committed file differs from a fresh run.
- 2026-10-23 UAE, GCC, Israel and MENA (lcr-i4l): about 100 foreign rules drafted outside cargo were validated and fixed (16 contract violations). Rules have no court-group mapping, so a rule whose citations belong to several courts is split one rule per court (the DIFC and ADGM neutral citations) to give court-level jurisdictions; Israeli district courts are one rule per district and language. Shapes shared by several states require a country marker (a `reject` drops the bare form) or carry low confidence. UAE rules are tagged `uae`, not `gcc`, so the `gcc` profile is the other five states; `mena` is every rule tagged `mena`.
- 2026-10-23 URL templates (dot-1ij4xz.5): a template stays only when its rule note records a successful HTTP fetch. The generated reporters-db rules keep CourtListener `/c/` only for Abb. Adm. (`VERIFIED_FAMILIES` in xtask-rules) and the Indigo nominative rules have none; the unfinished CourtListener probe (sample URLs plus a results file) was dropped when CourtListener rate-limited it (HTTP 429). Templates removed for undocs.org (200 for any symbol), EUR-Lex (202 challenge), SSRN and congress.gov (403), the IRM index page and the `{url}` web cites; Cornell U.S.C.S. and Treas. Reg., legislation.gov.uk and wto.org were fetched and kept.
- 2026-10-23 rule data split out (lcr-otx): rules moved from `crates/legal-citations/rules/` to top-level `rules/`, licensed MIT (`rules/LICENSE`; `rules/NOTICE` carries the reporters-db BSD-2-Clause and Indigo CC0 notices); the code stays GPL-3.0-or-later. `just dist` writes `dist/legal-citation-rules.json` (every rule plus an ECMAScript/.NET form of each pattern), `dist/test-vectors.json` (rule-level spans and groups of every rule example) and their schemas. `rules validate` fails on a pattern the bundle cannot carry in both dialects (`portable`) and warns on one only Go RE2 rejects (`portable-re2`). Checked by hand when this landed: Python `re` and Node (the JS form, flag `u`) reproduce all 14,095 vectors; Go RE2 compiles every pattern and misses 40 vectors, the Arabic and Hebrew rules whose `\b`/`\d` RE2 reads as ASCII.
