# legal-citations-rs

Find legal citations in text or markdown. For each one it reports exact spans
into the original text, the authority it names, and whether the document links
it to that authority. It ships as a library (`legal-citations`) and a CLI
(`cite-detect`).

The tool detects citations. It does **not** check that a citation supports the
proposition it is cited for, or that the authority is still good law.

## Install

```sh
cargo install --git https://github.com/davidawad/legal-citations-rs.git --tag v0.1.0 --locked cite-detect
cite-detect --version   # cite-detect 0.1.0 (cc8dd9fe0348)
```

This installs the `cite-detect` binary; the library is the `legal-citations` crate in the same
repository.

## Quickstart

```sh
$ printf 'Jurisdiction lies under 28 U.S.C. § 1331. See Brown v. Board of Education, 347 U.S. 483 (1954).\n' > brief.md

$ cite-detect brief.md                      # inventory, human format; exit 0
brief.md:1:25: CITE010 detected [Info] statute citation (us.fed.statute.usc)
brief.md:1:47: CITE010 detected [Info] case citation (us.case.us_reports)
2 citation(s), 0 linked, 2 finding(s), 0 suppressed: PASS

$ cite-detect --require-links brief.md      # lint; exit 1
brief.md:1:25: CITE001 unlinked [Error] statute citation "28 U.S.C. § 1331" is not linked to its source
    fix: https://www.law.cornell.edu/uscode/text/28/1331
brief.md:1:47: CITE001 unlinked [Error] case citation "Brown v. Board of Education, 347 U.S. 483 (1954)" is not linked to its source
    fix: https://www.courtlistener.com/c/U.S./347/483/
2 citation(s), 0 linked, 2 finding(s), 0 suppressed: FAIL

$ cite-detect brief.md --format jsonl       # one header line, then one JSON object per citation
{"type":"header","schema":"cite-detect.report/1","version":"0.1.0","registry_hash":"...","policy_hash":"...","mode":"detect"}
{"type":"citation","file":"brief.md","index":0,"linked":false,"kind":"statute","rule_id":"us.fed.statute.usc","span":{"start":24,"end":41,...},"text":"28 U.S.C. § 1331",...}
...

$ cite-detect fix brief.md --include-unverified   # JSON plan of edits; --apply writes them
[{"span":{...},"replacement":"[Brown v. Board of Education, 347 U.S. 483 (1954)](https://www.courtlistener.com/c/U.S./347/483/)","url":"...","safe":false}, ...]

$ cite-detect explain CITE001               # JSON: code, name, severity, deterministic, ...
```

## Use

```sh
# Inventory: every citation, with spans
cite-detect brief.md --format jsonl

# Lint: every citation must be a markdown link to its source (exit 1 if not)
cite-detect --require-links brief.md

# Same, with a shared policy file (allowed hosts, kinds, link schemes)
cite-detect --policy cite-detect.json brief.md

# Wrap unlinked citations using rule URL templates (prints the plan; --apply writes)
cite-detect fix brief.md --include-unverified

# Lint only Ohio citations, or only two courts, or use a named profile
cite-detect --jurisdiction us.oh --require-links brief.md
cite-detect --jurisdiction us.fed.dist.txed --jurisdiction 'us.fed.app.ca*' brief.md
cite-detect --profile ptab --exclude kind:record brief.md

# Table of authorities: every authority, its occurrences, short forms and pages cited
cite-detect authorities brief.md --format md
```

```rust
let cites = legal_citations::detect("Jurisdiction lies under 28 U.S.C. § 1331.");
assert_eq!(cites[0].authority.as_deref(), Some("us.usc/28/1331"));

let doc = legal_citations::detect_markdown("[28 U.S.C. § 1331](https://www.law.cornell.edu/uscode/text/28/1331)");
assert!(doc.is_linked(0));
```

**Exit codes:**

| Code | Meaning |
| --- | --- |
| 0 | pass |
| 1 | a finding at or above `--fail-on` |
| 2 | usage |
| 3 | policy or config |
| 4 | input |
| 5 | runtime |

**Output formats:** `human`, `json`, `jsonl` and `md-report`. The JSON shapes
have schemas in `schema/` (`cite-detect schema report|finding|citation|rule|policy|authorities`).

## CLI reference

From `cite-detect --help` (each subcommand takes `--help`):

| Command | What it does |
| --- | --- |
| `cite-detect [FILES]` | Detect citations and lint (no subcommand); `-` or nothing reads stdin |
| `cite-detect rules list\|show\|validate\|table\|export\|vectors` | List, show or validate the rule registry; `table` is the README coverage table, `export` the MIT rule bundle for other languages, `vectors` test vectors from the rule examples |
| `cite-detect explain CODE\|RULE` | Explain a finding code (CITE001) or a rule id (us.fed.statute.usc) |
| `cite-detect inspect FILE --stage S` | Dump one pipeline layer for a file as JSON |
| `cite-detect schema [WHICH]` | Print JSON Schema for an output or input format |
| `cite-detect authorities [FILES]` | Table of authorities: citations grouped by authority, with every occurrence, short-form count and pages cited |
| `cite-detect profiles list\|show` | List or show the named rule profiles (`--profile NAME`) |
| `cite-detect courts [TEXT]` | The court table: each court abbreviation and its court-level jurisdiction id |
| `cite-detect coverage FILE` | Run detection over a citation-format inventory and report which formats are found |
| `cite-detect fix FILE` | Wrap unlinked citations in links built from rule URL templates |
| `cite-detect --version` | Version and the git commit it was built from |

## Selecting rules: collections and profiles

Every rule belongs to collections derived from its file: `kind:<kind>`,
`family:<family>` (case, statute, regulation, court_rule, agency,
legislative, record, secondary, international, short_form, unrecognized),
`jurisdiction:<prefix>` for every dotted prefix of its jurisdiction (`us`,
`us.fed`, `us.fed.uspto`), `tag:<tag>` from the optional `tags` field, and
`rule:<id>`. Values take `*` and `?` globs; a bare pattern is a rule-id glob
(`us.fed.uspto.*`).

### Courts

A case citation's jurisdiction comes from its court parenthetical, down to
the court, and `jurisdiction:` patterns match it by prefix. The ids:

| Court | Id |
| --- | --- |
| Supreme Court | `us.scotus` |
| courts of appeals | `us.fed.app.ca1` .. `us.fed.app.ca11`, `us.fed.app.cadc`, `us.fed.app.cafc` |
| district courts | `us.fed.dist.<state><district>`: `us.fed.dist.txed` (E.D. Tex.), `us.fed.dist.nysd`, `us.fed.dist.mad` (D. Mass.) |
| bankruptcy courts, B.A.P.s | `us.fed.bankr.nysd`, `us.fed.bankr.bap.ca9` |
| other federal courts | `us.fed.cit`, `us.fed.jpml`, `us.fed.military.caaf`, ... |
| state courts | `us.<st>.sup` (court of last resort), `us.<st>.app` (intermediate appellate), `us.<st>.trial` |
| foreign courts | `foreign.<iso2>.<court>` (`foreign.il.sup`), from the rule |

So `us.fed.dist` is every district court, `us.tx` every Texas state court
and `us.fed.app.ca*` every circuit. `100 N.E.3d 5 (Ohio 2018)` is
`us.oh.sup`, `(Ohio Ct. App. 2010)` `us.oh.app`, `(E.D. Tex. 2020)`
`us.fed.dist.txed`; a citation without a known court keeps its rule's
jurisdiction, and each citation also carries `rule_jurisdiction`.
`court:` is an alias of `jurisdiction:` that also takes a court as cited
(`court:E.D. Tex.`). `cite-detect courts [TEXT]` lists the table (1337
courts, generated from the Indigo Book's T1 and T3 tables by
`cargo run -p xtask-rules`).

| Flag | Policy field | Meaning |
| --- | --- | --- |
| `--include P` (alias `--select`) | `include` | report only rules in these collections |
| `--exclude P` | `exclude` | never report these; exclude wins over everything |
| `--profile NAME` | `profiles` | add a named profile's rules to the includes |
| `--jurisdiction J` | | shorthand for `--include jurisdiction:J` |

Every flag repeats (or takes a comma list), and the includes OR together.

A custom mixed selection, for one brief that cites an Israeli district court
and the Eastern District of Texas, plus every statute (short forms follow
the citation they refer to):

```json
{
  "include": [
    "jurisdiction:us.fed.dist.txed",
    "jurisdiction:foreign.il.dist*",
    "kind:statute"
  ],
  "exclude": ["kind:record"]
}
```

Pass it as `cite-detect --policy cite-detect.json brief.md` (the CLI flags
add to it), or save it with a `name` and `description` as
`crates/legal-citations/profiles/<name>.json` to make it a profile
(`--profile <name>`). A pattern for a jurisdiction no rule or court has yet
exits 3.

Foreign courts are court-level where the rules split them: Israeli district
courts are `foreign.il.dist.tlv`, `.jer`, `.hfa`, `.bsh`, `.naz` and `.ctr`
(English `CC (TA) 1234/05` and Hebrew `ת"א (מחוזי ת"א) 1234/05`), Israeli
Supreme Court dockets and P.D. reports `foreign.il.sup`, DIFC judgments
`foreign.ae.difc.ca`, `.cfi`, `.sct`, `.arb`, `.tcd` and `.enf`, ADGM
judgments `foreign.ae.adgm.cfi` and `.ca`.

Profiles are JSON files in `crates/legal-citations/profiles/`:
federal-litigation, ohio, california, new-york, texas, ptab, ttab,
uspto-prosecution, tax, immigration, labor, agency-all, uae, difc-adgm, gcc
(the GCC states other than the UAE), israel, mena and everything
(`cite-detect profiles list`, `cite-detect profiles show ptab`). See which
rules a selector picks with `cite-detect rules list --select family:agency`.
An unknown namespace, kind, family, court or profile, or an include/exclude
pattern that matches no rule (or, for `jurisdiction:`, no rule and no court),
exits 3 with the reason (the library reports CITE008).

Every rule still runs; the selection only filters what is reported. So
overlaps resolve as in a full run, and short forms follow the citation they
refer to: with `--jurisdiction us.oh`, an `Id.` after an Ohio cite is kept and
an `Id.` after a federal cite is dropped. Parallel cites follow their first
cite the same way.

## Table of authorities

Every citation carries typed `details`: for cases the case name, parties
(`v.`, `In re`, `Ex parte`), volume, reporter as written and with Bluebook
spacing, page, pincites, court, year, date, parallel cites and subsequent
history (`aff'd`, `rev'd`, `cert. denied`); for codes the code, title,
sections, subsection path, year and edition; for agency documents the agency,
document type, number and date. `urls` lists candidate source URLs from the
rule's verified templates.

`cite-detect authorities FILE --format md|json|jsonl` (library:
`legal_citations::authorities(&citations)`) groups citations by authority
key. Parallel cites join their first cite, short forms join their
antecedent, and a subsequent-history cite is listed under the decision it
follows. Each authority lists every occurrence with spans, the short-form
count and the pages cited (pincites into the authority, not pages of your
brief; the markdown table also gives the lines of the document).

## What "linked" means

Markdown is parsed with pulldown-cmark. A citation is LINKED when:

- an inline, reference, HTML `<a>` or autolink's text overlaps it (a link on the
  case name alone counts), or
- a footnote reference placed right after it points to a footnote definition
  that contains a link.

Front matter, code, HTML tags and comments, link destinations and reference
definitions are masked to spaces before detection. That keeps the offsets
identical to the original file. Text after a directive comment such as
`<!--u-->` is still scanned.

## Finding codes

| Code | Name | Default | Deterministic |
| --- | --- | --- | --- |
| CITE001 | unlinked | error | yes |
| CITE002 | bad-url | error | yes |
| CITE003 | host-not-allowed | warn | yes |
| CITE004 | incongruent (the link points to a different authority) | error | yes |
| CITE005 | undecodable (allowed host, target cannot be confirmed offline) | info | yes |
| CITE006 | scheme-accepted (`toa:` and other indirect schemes) | info | yes |
| CITE008 | bad-selector (policy include/exclude/profiles cannot be applied; library only, the CLI exits 3) | error | yes |
| CITE009 | bad-suppression (an ignore comment with no reason) | error | yes |
| CITE010 | detected (inventory) | info | yes |
| CITE011 | unrecognized-citation (citation-shaped text no rule recognizes) | warn | yes |
| CITE020 | low-confidence | info | no (cue only) |

Record cites (ER-139, JA 16, Ex. 1001, TTABVUE) and docket numbers are exempt
from the link requirement by default. So is page furniture: a slip-opinion
running head (`Cite as: 606 U. S. 656 (2025)`) or a citation alone on a line
that repeats verbatim (a PTAB caption on every page). These are still
detected, with `furniture` set to `running_head` or `repeated_caption`; set
`link_furniture: true` in the policy to require links on them too. Suppress a single finding with
`<!-- cite-detect: ignore CITE001 reason="..." -->` on the line before it.
A reason is mandatory.

## Coverage (2026-10-23)

There are 2087 active rules plus 4 retired rules (a legacy rule, the split
`intl.treaty.ust_tias`, and two duplicates merged when branches met:
`us.fed.federal_register.fr_abbreviated` and `us.record.record_at`): 1069 generated from reporters-db
(`cargo run -p xtask-rules`), 163 authored from the Indigo Book inventory
(`cargo run -p xtask-rules -- indigo`: state session laws, registers,
nominative reporters, fallback code forms), and the hand-written rest. The table below is generated by
`cite-detect rules table` (a test fails when it is stale); `cite-detect rules list` is
authoritative. "Verified" counts rules whose encoded authority was opened (`opened: true`).

Indigo Book format coverage (`cite-detect coverage corpus/formats/indigo.jsonl`,
[report](docs/measurements/indigo-coverage.md)): 1221 of 1225 formats (99.6%), 2541 of
2550 examples; the count is a regression gate (`docs/measurements/indigo-baseline.json`).

<!-- rules-table:start (cite-detect rules table) -->
| Kind | Family | Rules | Generated | Verified | Jurisdictions | Representative forms |
| --- | --- | ---: | ---: | ---: | ---: | --- |
| agency_decision | agency | 142 | 0 | 29 | 91 | `Alaska Op. Att'y Gen. No. 2019-001`, `43 Op. Att'y Gen. 293`, `acq., 1974-2 C.B. 4`, `Exchange Act Release No. 75592, 80 Fed. Reg 47829`, `Guam Op. Att'y Gen. No. 2019-001`, `Nev. Op. Att'y Gen. No. 2019-001` |
| agency_manual | agency | 21 | 0 | 7 | 13 | `Medicare Benefit Policy Manual, ch. 15, § 50.2`, `Institute of Education Sciences, National Board for Education Sciences Annual Report 12 (2014)`, `Deleg. Order 25-7 (Rev. 1)`, `HALLEX I-2-7-30`, `9 FAM 302.1-2(B)(3)`, `USCIS Policy Manual, Vol. 6, Pt. F, Ch. 5` |
| case | case | 1134 | 1068 | 22 | 71 | `2009 Ark. App. 318`, `2020 U.S. Dist. LEXIS 12345`, `100 USPQ2d 1234`, `6 Ill. (1 Gilm.) 145`, `62 N.C. (Phil. Eq.) 441`, `24 S.C. Eq. (3 Rich. Eq.) 569` |
| case_short | case | 2 | 1 | 0 | 1 | `449 U.S. at 389`, `12 A .2d at 350` |
| constitution | statute | 7 | 0 | 5 | 6 | `U.S. Const. art. I, § 8, cl. 3`, `U.S. Const. amend. XVIII (repealed 1933)`, `La. Const. Ann. art. 57`, `N. Mar. I. Const. art. I, § 3`, `P.R. Const. art. I, § 7`, `R.I. Const. art. VII, § 13` |
| court_rule | court_rule | 151 | 0 | 1 | 63 | `Alaska R. App. P. 28(a)(7)`, `3d Cir. I.O.P. 9.1`, `Guam R. App. P. 28(a)(7)`, `Md. Rule 2-322(b)`, `N.H. Sup. Ct. R. 16(3)(b)`, `Tenn. R. App. P. 28(a)(7)` |
| docket | record | 22 | 0 | 2 | 17 | `R.20-01-007`, `No. 12 Civ. 6065`, `WC Docket No. 17-108`, `EEOC Charge No. 520-2019-01234`, `MDL No. 2741`, `Admin. Proc. File No. 3-17950` |
| executive | agency | 9 | 0 | 3 | 3 | `3 C.F.R. 638 (1994)`, `Exec. Order No. 14,028`, `Exec. Order 13,136, 3 C.F.R. 216`, `Exec. Order 12,900, 59 FR 9061`, `NSPM-33`, `37 Weekly Comp. Pres. Doc. 1310` |
| federal_register | agency | 3 | 0 | 1 | 1 | `85 Fed. Reg. 12,345`, `59 FR 9061`, `FR Doc. 2021-12345` |
| foreign | international | 191 | 0 | 39 | 85 | `Abu Dhabi Court of Cassation, Appeal No. 1 of 2018`, `Constitution Act, 1867 (UK), 30 & 31 Vict, c 3, s 91(2)`, `CC (Hai) 1234/05`, `Iraqi Official Gazette No. 3789`, `[2007] 4 SLR(R) 100`, `1979 Common Mkt. Rep. (CCH) ¶ 8543` |
| id | short_form | 2 | 0 | 1 | 1 | `Id. at 495`, `Id. § 7102(26)` |
| legislative | legislative | 17 | 0 | 6 | 7 | `H.R. 1, 117th Cong.`, `Cong. Rsch. Serv., R45678`, `H.R. Con. Res. 27, 114th Cong.`, `H.R. Res. 431, 114th Cong.`, `S. Exec. Res. 1, 112th Cong.`, `LB 123` |
| record | record | 30 | 0 | 5 | 5 | `ROA.123`, `AR 123`, `ABC0001234`, `Resp. to Interrog. No. 5`, `Opp’n at 4–6`, `Answer ¶ 3` |
| regulation | regulation | 145 | 0 | 1 | 58 | `Alaska Admin. Code tit. 18, § 70.020`, `38 D.C. Reg. 1524`, `Ill. Admin. Code tit. 35, § 302.102`, `22 Minn. Reg. 130`, `N.M. Code R. § 20.6.2.3103`, `Tenn. Comp. R. & Regs. 0400-40-03-.01` |
| secondary | secondary | 35 | 0 | 10 | 10 | `L.A. Cnty. Bar Ass'n Formal Op. 525`, `ABA Comm. on Ethics & Pro. Resp., Formal Op. 512`, `N.Y.C. Bar Ass'n Comm. on Pro. Ethics, Formal Op. 2017-5`, `4 A.L.R. Fed. 3d 1`, `Mass Media Law Cases and Materials 472 (8th ed. 2011)`, `133 Harv. L. Rev. F. 1, 4 (2019)` |
| session_law | legislative | 66 | 0 | 1 | 55 | `2015 Alaska Sess. Laws 312`, `Pub. L. No. 111-148`, `2015 Kan. Sess. Laws 1371`, `2015 N. Mar. I. Pub. L. 30-55`, `2015 N.Y. Laws 1161`, `2015 Tenn. Pub. Acts 1818` |
| statute | statute | 76 | 0 | 4 | 62 | `Alaska Stat. § 11.41.100`, `42 U.S.C.U. § 1983`, `Kan. Stat. Ann. § 21-5401`, `49 N. Mar. I. Code § 57`, `N.Y. Fam. Ct. Act § 45`, `See Guam Code Ann. § 16.30` |
| statute_short | statute | 1 | 0 | 1 | 1 | `§ 1983` |
| supra | short_form | 1 | 0 | 0 | 1 | `Upjohn, supra, at 389` |
| treaty | international | 28 | 0 | 20 | 11 | `OAU Doc. CAB/LEG/24.9/49`, `Feb. 10, 1763, 42 Consol. T.S. 279`, `Nov. 22, 1969, O.A.S.T.S. No. 36`, `U.N. Charter art. 2, ¶ 4`, `A/RES/61/295`, `June 30, 1990, Hein’s No. KAV 2615` |
| unrecognized | unrecognized | 4 | 0 | 0 | 1 | `FTC File No. 123-4567`, `Index No. 654321/2019`, `45 Haw. Trib. 112`, `Guam Code Ann. tit. 7, § 25104` |
| **all** | | **2087** | **1069** | **158** | | |
<!-- rules-table:end -->

## Measured accuracy (gold set, 2026-10-23)

`cite-eval` (crate `crates/cite-eval`) scores the tool against hand-labeled gold citations in
`corpus/gold/`. The corpus has 23 public documents and 64,091 words: 2 U.S. Supreme Court
opinions, 4 circuit and 2 district opinions, the supreme courts of California, New York, Texas,
Florida, Illinois, Ohio and Delaware, and a PTAB, an NLRB, a BIA, an IRS and a Federal
Register document. It also has a law review excerpt, a UK Supreme Court judgment and a DIFC
Court of Appeal judgment. Every citation is labeled: 1,089 counted, plus 338 borderline or
page-furniture references marked optional.

**The labels are by an AI agent (Claude), not a lawyer.** A second pass by the same model
re-read each document. That is a single rater, so inter-rater agreement is unmeasured.

Run at `d5c2ba8`, plain-text detection, Wilson 95% intervals:

| Measure | Value | n |
| --- | --- | --- |
| Recall (gold citation overlapped by a detection) | 0.794 (0.769–0.817) | 865 / 1,089 |
| Precision (detection overlaps a gold citation) | 1.000 (0.996–1.000) | 873 / 873 |
| Exact span (byte-identical, among detected) | 0.708 (0.676–0.737) | 612 / 865 |

Precision is optimistic. The labeler saw every unmatched detection and labeled the real ones.
The 81 detections that fall only on optional labels, such as running heads, are not scored.

- **Strong:** case recall is 0.98 (n = 256), `Id.` 1.00 (157), `§` short forms 0.96 (55),
  agency decisions 0.97 (29). Circuit, district and agency documents are at 0.90–0.94.
- **Weak recall:** court rules 0.15 (n = 26: DIFC `RDC 45.2`, `Crim.R. 3(A)`,
  `TEX. R. APP. P. 53.1`, `DC Civil Rule 4(c)(4)`), constitutions 0.17 (23: `Cal. Const., art.
  V, § 13`, `Art. II, § 2, cl. 2`), secondary 0.38 (26: small-caps law reviews, dictionaries),
  foreign 0.45 (67: UK and NI statute sections, DIFC law articles), record 0.58 (98: `App. 39`,
  Delaware `A8 ¶ 12`) and docket 0.60 (42: `No. 20–1668`, `2024-1094`). By document, the
  foreign judgments score 0.38 and California 0.38.
- **Weak spans:** agency decisions (6 of 28 exact: `Matter of X,` and NLRB case names are not
  in the span), Federal Register (1 of 14: the date parenthetical), foreign (5 of 30: case name
  before `[1980] AC 598`, `para 27` pins), `Id.` with pin lists (`id. at 7, 11`, `id. ¶ 36`),
  `Dkt. 1 at 15` pins, statute year parentheticals and en-dash sections (`§ 299b–4(a)(1)`).

Reports: [`2026-10-23-gold.md`](docs/measurements/2026-10-23-gold.md) (by kind, jurisdiction and
document, plus false positives) and [`misses.md`](docs/measurements/misses.md), which lists
every missed or mis-spanned citation grouped by form and is the input for the next coverage
round. Conventions: [`corpus/gold/README.md`](corpus/gold/README.md).

```sh
cargo run --release -p cite-eval -- fetch    # texts into corpus/cache/gold (gitignored), sha256 checked
cargo run --release -p cite-eval -- --date 2026-10-23 --revision d5c2ba8   # rewrite the reports
cargo run --release -p cite-eval -- --gate   # exit 1 below corpus/gold/gate.json
```

The gate needs the fetched texts, so it is not part of `just check`. The crate's own tests use a
small committed fixture corpus and need no network.

## Limits (read before relying on it)

- **Almost every rule is `opened: false`.** Each rule says which Bluebook or court rule
  it encodes. Only 15 public-domain case rules (lcr-uc3) were checked against the
  court's own rule or style manual, and each of those cites the URL that was read.
- **Every rule is `precision: unmeasured`.** No rule's `measured` block has been
  written yet. The gold-set run below measures 30 rules (n >= 5) in
  `docs/measurements/2026-10-23-per-rule.json`, for a later step to write back.
- **What is checked today:** the per-rule examples, a clean-prose check, and
  property tests (no panics, and spans always slice the original text).
- **Case names** are found by a backwards scan. Unusual party names, such as
  ones that start lowercase or have more than 10 tokens before `v.`, can be
  truncated or missed. The citation itself is still detected.
- **Short forms** resolve only backwards, and only within one document. The
  `at` short form does not yet know the specialty-court reporters
  (`145 T.C. at 95` is not detected).
- **Loose shapes.** A bare `S. 567`, `Notice 2020-23`, `B-412581` or
  `Slip Op. 21-12` can name something else, so those rules start at 0.85
  confidence. State executive orders numbered `2020-01` match the federal
  executive order rule.
- **Short forms** resolve only backwards, and only within one document.
- **Generated reporter rules** match volume, reporter, page only, with each
  reporters-db string as listed, including very short ones (`2012 IL 112345`,
  `5 NY 10`). They run at priority 30, below every hand-written rule. Twelve
  families run at 65, because otherwise a court-rule or record pattern would
  split the cite (`12 A.L.R.3d 345` read as local rule `L.R.3`). Their short
  forms come from one generated rule at 30 and, for the 10 strings another
  rule would split, a second one at 65.
  Edition date ranges are in each rule's `note`; nothing uses them yet.
- **Spans.** Pin ranges with a hyphen, en dash or figure dash (`at 31–32`,
  `879–880`), Westlaw tails (`, at *3 (E.D. La. Sept. 10, 2002)`) and case
  names after `quoting`/`citing` inside a parenthetical are in the span. A
  short case form starts at its short name (`Bruen, 597 U.S. at 17`), so a
  link on `Bruen` counts. Section numbers that contain en dashes
  (`Ala. Code § 8–19G–3`) are still cut at the first dash.
- **Unrecognized citations (CITE011)** come from a catch-all layer that runs
  after every real rule and never inside a span a real rule matched or a real
  citation covers (case name through parenthetical). It finds
  shapes, not authorities: no authority key, no URL, confidence 0.4. It
  skips months and common non-legal abbreviations (`5 Jan. 2020`, `3 Fig. 4`,
  `No.`, `St.`, `Co.`) and a bare `§ 1983` (a short form of an earlier cite).
  It does not see a case name before an unknown reporter, citations without
  a period abbreviation (`45 Haw 112`), or unknown docket labels. A hit means
  "add a rule", not "this is wrong".
- **Court selection needs a court parenthetical.** A case citation's
  court-level jurisdiction comes from its parenthetical; without one (`347
  U.S. 483`, `100 Ohio St.3d 1 (2003)`) it is the rule's (`us.scotus`,
  `us.oh`), so `--jurisdiction us.oh.sup` misses the second. The court
  table holds the Indigo Book's courts (T1 and T3); a state court it lacks
  resolves to the state (`(Ohio Ct. Com. Pl. 2001)` is `us.oh`). District
  ids exist for every state and every `N.|S.|E.|W.|M.|C.D.` prefix,
  including districts that do not exist. Court levels for state courts are
  read from the name (`App` is intermediate appellate) with a short override
  list (`Tex. Crim. App.` is a court of last resort). Non-case citations
  (statutes, rules) are selected by their rule's jurisdiction only.
- **UAE, GCC, Israel and MENA rules** (lcr-i4l) were drafted without a
  source in hand; only the DIFC CA, CFI and SCT neutral citations were checked
  (DIFC Courts Practice Direction No. 4 of 2016). Shapes shared across the
  region are kept narrow or low: a UAE Federal Supreme Court cite needs a UAE
  marker; Bahraini, Kuwaiti and Qatari laws and cassation cites need a country
  marker (bare `Amiri Decree` is either Kuwait or Qatar); Omani royal decrees
  need Oman context, and the bare `Royal Decree 6/2020` rule
  (`foreign.om.royal_decree.bare`) starts at 0.5 because Spain, Morocco and
  Jordan use the shape; Arabic `قانون رقم 5 لسنة 1985` is read as Egyptian at
  0.6. The Israeli Supreme Court English forms (`CA`, `CrimA`, `LCA`, `HCJ`,
  `FH`, `DNH`, `AAA` with n/yy) are `foreign.il.sup`; the new-style
  `CA 12345-05-19` without a district stays `foreign.il`.
- **Typed fields** are read from rule groups and nearby text. Parallel cites
  are recognized only when separated by a bare comma with no case name in
  between; two unrelated cases written `1 F.3d 1, 2 F.3d 2` are treated as
  parallel. Code names are the literal text before the section, as written.
- **URL fix templates** exist only where the rule note records a successful
  HTTP fetch (2026-10-22 and 2026-10-23): CourtListener `/c/` for the
  hand-written reporter rules (U.S., S. Ct., L. Ed., F., F. Supp., F.R.D., Fed.
  Appx, B.R., Fed. Cl., regional, California, New York, Ohio St., USPQ, T.C.,
  Ct. Int'l Trade, Vet. App., M.J.) and the generated Abb. Adm. rule; Cornell
  LII uscode/cfr/rules; govinfo plaw, statute, Federal Register and bills;
  supremecourt.gov docket pages; legislation.gov.uk UK SIs; wto.org dispute
  pages. The other generated reporters-db rules and the Indigo nominative
  reporters have the CourtListener decoder but no template. eCFR,
  federalregister.gov, congress.gov, SSRN, EUR-Lex and Justia block scripted
  fetches and undocs.org answers 200 for any symbol, so they have no templates.

## TNET

TNET (english-text-eval) plugins are declarative JSON packs. Metrics are
built-in scorers, so a pack cannot ship code. TNET integration is therefore
two pieces:

1. a feature-gated adapter in TNET that maps `cite-detect` findings 1:1 to
   TNET findings (the spans already use TNET's byte, char and UTF-16 contract);
2. a `plugins/legal-citations` pack with presets and fixtures.

Until the adapter lands, TNET users can run `cite-detect --format jsonl` beside
`text-eval`.

## Develop

Rules are data: edit `rules/<kind>/<id>.json` and run
`just rules`. `just check` is the gate.
`just dist` writes the release artifact for other languages to `dist/`:
`legal-citation-rules.json` (every rule, with an ECMAScript/.NET form of each pattern and how
to apply `reject`/`reject_before`), `test-vectors.json` (the rule-level spans and groups of
every rule example, UTF-8 byte offsets), their JSON Schemas, `rule.schema.json`, and the
rules' `LICENSE` (MIT) and `NOTICE`. See `AGENTS.md` and `docs/DESIGN.md`.
`just indigo-coverage` re-measures Indigo Book coverage (the test suite fails if it
drops below the baseline); after adding rules, paste `cite-detect rules table` into
the coverage section above.
`just release VERSION` tags a release: it needs a clean tree on `main` and a `## [VERSION]`
section in `CHANGELOG.md`, runs `just check`, and creates the annotated tag `vVERSION`
(it does not push).

## Acknowledgements

- [The Indigo Book: A Manual of Legal Citation](https://law.resource.org/pub/us/code/blue/IndigoBook.html)
  (Sprigman et al., Public Resource, CC0). Its tables are the checklist of citation
  formats this tool aims to cover (`corpus/formats/indigo.jsonl`).

Full third-party notices are in `NOTICE`.

## License

- **Code** (everything outside `rules/`): GPL-3.0-or-later (`LICENSE`).
- **Rule data** (`rules/`, the regex JSON rules): MIT (`rules/LICENSE`). The third-party
  notices that travel with it are in `rules/NOTICE`.

To use the rules from another language without the GPL code, run `just dist`. It writes the
bundled rules JSON `dist/legal-citation-rules.json` (every rule, with an ECMAScript/.NET form of
each pattern), `dist/test-vectors.json` for checking a port, their JSON Schemas, and the MIT
`LICENSE` and `NOTICE`. `cite-detect rules export` prints the same bundle.

Source at https://github.com/davidawad/legal-citations-rs; not
published to crates.io (`publish = false`). Changes: [`CHANGELOG.md`](CHANGELOG.md).
