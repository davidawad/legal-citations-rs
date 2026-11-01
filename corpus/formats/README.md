# Citation format inventory: The Indigo Book

`indigo.jsonl` lists the citation formats described in *The Indigo Book: A
Manual of Legal Citation*. It has one line per distinct citation format, not one
per abbreviation word. It is a coverage checklist for rule authors and for
recall evaluation. It is not a rule file: nothing in `src/` or `rules/` reads
it.

## Source and license

- Source: <https://law.resource.org/pub/us/code/blue/IndigoBook.html>
  (Sprigman et al., *The Indigo Book: A Manual of Legal Citation*, Public
  Resource (2016); beta release, last modified May 2, 2016).
- License: CC0. The manifest says the work is "published under a CC-0
  public domain dedication—'No Rights Reserved'". Public Resource waives
  all copyright and related rights. A NOTICE entry records the source anyway.
- Read in full on 2026-10-23: rules R1–R40 and tables T1–T20.

## Line format

```json
{"section": "...", "jurisdiction": "...", "kind": "...", "form": "...", "examples": ["..."]}
```

- `section`: the Indigo rule or table the format comes from, such as
  `R16.1.5`, `T2 Securities and Exchange Commission ...` or
  `T3 Alabama: Supreme Court — Southern Reporter (1886–date)`. For reporter
  rows, the section also gives Indigo's date range.
- `jurisdiction`: one of the following:
  - `us.<postal code>` for states, D.C. and territories. The Canal Zone is
    `us.cz`.
  - `us.fed` for federal courts and statutes.
  - `us.fed.<agency>` for agencies listed in T2 (41 keys), such as `sec`,
    `nlrb`, `uspto` and `irs`.
  - `intl` for treaties, arbitral reporters and intergovernmental bodies.

  Three values go beyond the requested set:
  - `us` covers formats with no single jurisdiction: Restatements, U.C.C.,
    U.L.A., Uniform Rules of Evidence, books, periodicals, internet sources,
    and the generic state-bill and state-hearing forms.
  - `us.tribal.navajo` is the Navajo Nation.
  - `us.tribal.ok` is Oklahoma tribal courts.
- `kind`: the requested enum. State administrative registers use
  `regulation`, because `federal_register` is reserved for the Federal
  Register itself. Arbitral, ICJ, PCIJ, EU and human-rights tribunal
  reporters use `foreign`. Treaty series and U.N. documents use `treaty`.
- `form`: the pattern as Indigo prints it. Where Indigo gives an explicit
  pattern (T1 statutory rows, T3 code, session-law, register and
  administrative-code rows), `form` copies it verbatim, including Indigo's
  own placeholders such as `x-x-x`, `<year>` and `<page no.>`. Indigo's few
  typos are kept as printed, for example `Del Gov’t Reg.` and
  `Del. Code. Ann. Adv. Legis. Serv.`. Case-reporter forms are built from the
  T3 and T1 reporter rows in the shape `<case name>, <vol> <reporter> <page>,
  <pin> (<court> <year>)`. The notation is:
  - `a | b` or `a|b` marks alternative reporter series or variants;
  - `[...]` marks an optional part;
  - `<...>` marks a slot.

  The court abbreviation comes from the T3 court heading. When the reporter
  name already names the state, the jurisdiction is dropped from the court,
  as in R12.2: `(Ct. App. 1998)` and `(1975)`.
- `examples`: one to three citations in that form, written the way they
  would appear in a brief, without a trailing period. Where Indigo prints an
  example, it is used verbatim, including its quirks: `So.2d` without a
  space in Louisiana, and `80 Fed. Reg 47829`. Otherwise volumes, pages,
  sections and years are invented but plausible. Years fall inside the
  reporter's or register's date range and the series' span. Case names
  come from a fixed list of generic names. Invented examples are not real
  authorities; do not cite them.

## Counts

1225 lines.

By kind:

| kind | lines |
|---|---|
| case | 592 |
| session_law | 144 |
| statute | 142 |
| regulation | 123 |
| agency_decision | 61 |
| constitution | 57 |
| foreign | 29 |
| treaty | 20 |
| legislative | 17 |
| secondary | 11 |
| agency_manual | 8 |
| court_rule | 6 |
| record | 6 |
| executive | 5 |
| federal_register | 3 |
| docket | 1 |

By jurisdiction:

| jurisdiction | lines | jurisdiction | lines | jurisdiction | lines |
|---|---|---|---|---|---|
| us.fed | 117 | us.ky | 33 | us.ok | 17 |
| us.fed.<agency> (41) | 80 | us.nc | 33 | us.vt | 16 |
| us.ny | 52 | us.pa | 33 | us.al | 16 |
| intl | 47 | us.tn | 28 | us.ar | 16 |
| us.sc | 40 | us.la | 25 | us.fl | 16 |
| us.oh | 38 | us.ma | 25 | us.in | 16 |
| us.co | 23 | us.tx | 25 | us.ms | 16 |
| us.ct | 23 | us.ca | 18 | us | 16 |
| us.de | 23 | us.nj | 18 | us.nm | 15 |
| us.il | 23 | us.md | 17 | us.hi | 14 |
| us.va | 23 | us.mi | 17 | us.ks | 14 |
| us.wi | 17 | us.or | 14 | us.az | 13 |
| us.ia | 13 | us.nd | 13 | us.ut | 13 |
| us.wa | 13 | us.ga | 12 | us.id | 12 |
| us.nh | 12 | us.mo | 11 | us.ne | 11 |
| us.ri | 11 | us.wv | 11 | us.dc | 10 |
| us.me | 10 | us.mn | 10 | us.mt | 10 |
| us.nv | 10 | us.pr | 10 | us.sd | 10 |
| us.wy | 10 | us.ak | 9 | us.mp | 7 |
| us.vi | 6 | us.as | 4 | us.gu | 4 |
| us.tribal.navajo | 4 | us.cz | 1 | us.tribal.ok | 1 |

The inventory covers all 50 states, D.C., American Samoa, the Canal Zone,
Guam, the Northern Mariana Islands, Puerto Rico, the Virgin Islands, the
Navajo Nation and Oklahoma tribal courts. It includes every court and
reporter row in T3, every federal court in T1 and every agency in T2.
The T2 agencies use 41 `us.fed.<agency>` keys, because Indigo's predecessor
bodies (AEC, ICC) and sub-agencies (IRS/Treasury, CBP, OMB) get keys of
their own. Agencies with more than one citation form in T2 (IRS/Treasury,
USPTO, SEC, EOP) have one line per form. Each state and territory with a
constitution also has an R23.3 constitution line.

## What Indigo does not cover

These gaps are why this inventory alone is not a complete target list:

- State court rules. T3 has no rules-of-court rows. R18 covers only federal
  and uniform rules. The only state rule form here is N.Y. C.P.L.R., which
  T3 lists as a code.
- State attorney general opinions, state agency adjudications and state
  executive orders. R19 says only to cite state agency material
  "using approximately the same form" as the federal rules. N.J. Admin. is
  the only state agency reporter listed.
- Federal agencies beyond T2. The list is short. For example, it has no
  DOE, HHS/CMS, HUD, VA (other than the court), DOT/FMCSA, FDIC, OCC, PTAB
  or IRS Chief Counsel Advice forms. Only the reporter is named for
  FLRA, FMSHRC, FTC, MSPB, NLRB, NMB, NTSB, NRC and STB; the
  examples for those use invented numbers in the usual shape.
- Federal Register and Congressional Record. R19 does not give a separate
  Federal Register rule; it appears only inside T2 examples, recorded here as
  `federal_register`. Indigo has no Congressional Record form at all.
- Individual federal district and bankruptcy courts. T1 describes court
  types, such as "District Courts (e.g., D. Mass., S.D.N.Y.)", not the 94
  districts, and T9 gives only the abbreviation pieces (N.D., E.D., Bankr.).
- Docket and medium-neutral forms other than those in R12.4 and the T3
  "public domain citation format" blocks. Indigo has nothing for PACER,
  Bloomberg Law, Lexis/Westlaw database identifiers beyond the generic
  `<year> WL <no.>` and `<year> U.S. LEXIS <no.>`, CourtListener or
  vendor-neutral state forms adopted after 2016.
- Foreign national law. T19 sends readers to other guides instead of
  covering foreign courts and statutes (the Bluebook T2 equivalent). That is
  why no line uses `foreign.<cc>`. The `foreign` kind is used only for
  international and supranational tribunals (T7, T8).
- Looseleaf services. T4.2 lists about 140 service abbreviations; they are
  abbreviation words for one shared form, so there is a single generic T4.2
  line rather than one line per service.
- Abbreviation-only tables. T5 and T9–T18 cover legislative words, court
  names, judge titles, case-name words, geography, subdivisions, explanatory
  phrases, institutions, publishing terms, months and periodical words. They
  add no citation formats and are used only to build examples.
- Short forms for most kinds. Indigo gives short forms for cases (R15),
  statutes and regulations (R22), court documents (R26) and secondary
  sources (R29, R31, R34) only.
- Some T3 rows are split across cells in the source HTML, and the parser
  could not read them as rows. They are entered by hand: Harris & McHenry
  (Md.), Murphey (N.C.), Mackey, MacArthur and Cranch (D.C.), and pre-1941
  Texas session laws. Four Texas rows are left out. Robards and Dallam have
  "no vol. number". The 65 Tex. L. Rev. reprint of 1845 cases does not fit
  the form. The White & Willson and Commission-era Texas Reports rows are
  split across cells.
