# Gold citations

Hand labels for `cite-eval` (crate `crates/cite-eval`). Every citation in each document of
`manifest.json` is one line of `<id>.jsonl`. The texts are not committed: `cargo run -p
cite-eval -- fetch` downloads, extracts and excerpts them into `corpus/cache/gold/` and checks
each sha256, so the byte spans below always index the pinned text.

> **Who labeled this.** Every label was made by an AI agent (Claude), reading each document in
> full and then cross-checking against a broad regex candidate list and the tool's detections.
> A second pass by the same model re-read every document for missed citations and convention
> errors; with one consistency fix of ours it touched 7 of the 23 files (12 label lines added,
> 9 removed). That is one rater, not two independent ones:
> inter-rater agreement is unmeasured.

## Label fields

```json
{"start": 1234, "end": 1260, "kind": "case", "jurisdiction": "us.fed.app.ca5",
 "text": "Smith v. Jones, 5 F.4th 1, 3 (5th Cir. 2021)", "normalized": "5 F.4th 1",
 "form": "F.4th", "optional": false, "note": ""}
```

- `start`, `end`: UTF-8 byte offsets into the text, half-open; `text` must equal the slice.
- `kind`: a `legal_citations::Kind` name (see below). Never `unrecognized`.
- `jurisdiction`: dotted id in the tool's scheme. Reports group by the first two segments.
- `normalized`: the citation's core in standard form (`347 U.S. 483`, `42 U.S.C. § 1983`, `Id. at 5`).
- `form`: the citation form, from a shared vocabulary, so misses group by form.
- `optional`: a borderline reference. Not counted when missed; a detection on it is neither a
  true nor a false positive.

## What is a citation

Counted (`optional: false`):

| Kind | What |
| --- | --- |
| `case` | Court decisions: any reporter, public-domain (`2025-Ohio-2425`, `2022 IL 127258`), Westlaw/Lexis, slip opinions cited by docket and date. |
| `case_short` | `Roe, 410 U.S. at 116`, `410 U.S., at 116`, `Bruen, 597 U.S. at 17`. |
| `id` | `Id.`, `Id. at 5`, `id. § 2`, `Ibid.` |
| `supra` | `Upjohn, supra, at 389`, `Smith, supra note 3, at 4`, `supra note 12`. |
| `statute` | Codes: U.S.C., state codes (`R.C. 2921.31(A)`, `735 ILCS 5/13-116`, `Gov. Code, § 12560`, `General Business Law § 394-ccc`, `I.R.C. § 1014`, `§ 1014 of the Code`). |
| `statute_short` | A bare `§ 1983` / `§§ 1014(a), (b)` referring to an already cited code. |
| `session_law` | `Pub. L. No. 111-148`, `124 Stat. 119`, `L 2022, ch 204`, Ohio `Am.Sub.H.B. No. 137`. |
| `regulation` | C.F.R. (`29 CFR 2510.3-21`), `Treas. Reg. § 1.1014-1`, state admin codes. |
| `federal_register` | `89 Fed. Reg. 32122`, `81 FR 20946`. |
| `constitution` | `U.S. Const. art. I, § 8`, `Cal. Const., art. V, § 13`, `Ohio Const. art. I, § 14`. |
| `court_rule` | `Fed. R. Civ. P. 12(b)(6)`, `Tex. R. App. P. 53.1`, `Crim.R. 3(A)`, DIFC `RDC 45.2`. |
| `agency_decision` | Decisions and rulings of agencies: `342 NLRB 418 (2004)`, `Matter of X, 29 I&N Dec. 780 (BIA 2026)`, PTAB decisions, `Rev. Rul. 84-139, 1984-2 C.B. 168`, `Rev. Proc.`, IRS Notices. |
| `agency_manual` | MPEP, TMEP, PTAB Trial Practice Guide, agency handbooks. |
| `record` | `ECF No. 12`, `Dkt. 5 at 3`, `J.A. 16`, `A8`, `Ex. 1001, 4:5-9`, `Paper 37, 6`, `Tr. 45`, `Compl. ¶ 6`. |
| `docket` | Docket numbers used as identifiers (`No. 24-354`, `IPR2025-00687`, `Case 29-CA-24484`), once per caption, and in text. |
| `secondary` | Law reviews, books and treatises, dictionaries, Restatements, A.L.R., reports and articles cited as sources. |
| `legislative` | `H.R. Rep. No. 93-1280`, `S. Rep.`, `Cong. Rec.`, bills, hearings. |
| `executive` | Executive orders, presidential documents. |
| `treaty` | Treaties and international agreements. |
| `foreign` | Non-US authority: `[2026] UKSC 7`, `[1980] AC 598`, `s 142 of the Magistrates' Courts Act 1980`, `[2014] DIFC CA 003`, `Article 15 of the DIFC Courts Law`. |

Optional (`optional: true`), with `form` saying why:

- **page furniture**: running heads (`Cite as: 606 U. S. ___ (2025)`), page headers
  (`Case: 24-10 Document: 51 Page: 3`), repeated captions;
- **prose section reference**: a section, rule or article named without its code (`section
  13-116`, `Rule 8`, `article V, section 13`, `section 1014 of the Code`) and statutes named
  only by title without a section (`the Administrative Procedure Act`); a UK or DIFC statute
  named with its year but no section (`the Magistrates' Courts Act 1980`) is counted as `foreign`;
- **patent**: `U.S. Patent No. 9,123,456`, `the '456 patent`;
- anything else the labeler judged borderline, with a `note`.

Not labeled at all: bare names of acts without a year or section (`Title VII`), `the First
Amendment`, URLs on their own, internal cross-references (`Part II.B above`).

## Spans

- **Start** at the first character of the citation: the case name (including `In re`,
  `Ex parte`, `Matter of`, `State ex rel.`), the author of a secondary source, otherwise the first
  element (volume, code name, `Id.`). Never a signal (`See`, `Cf.`, `see also`, `quoting`,
  `citing`, `aff'd`, `cert. denied`), never an opening parenthesis or bracket.
- **End** at the last character of the last element: page, pincites (with ranges and `n.5`),
  the court/date parenthetical (closing parenthesis included), and a code's year or publisher
  parenthetical (`(West 2022)`, `(2018)`). Excluded: explanatory parentheticals (`(holding ...)`,
  `(quoting ...)`, `(cleaned up)`, `(emphasis added)`, `(citation omitted)`), subsequent history
  (labeled as its own citation, without the phrase), trailing sentence punctuation, footnote
  numbers glued to the end, and URLs.
- **Act names** attached to their cite are in the span (`Indian Reorganization Act, ch. 576,
  48 Stat. 984 (1934)`).
- **Case name**: included when it is attached to the cite (`Name, 1 F.4th 1`, California `Name
  (2004) 32 Cal.4th 820`). A name that is separated from the cite by other words is not included.
- **Parallel cites**: one label per reporter. The first runs from the case name through its own
  page and pin, middle ones from volume through page and pin, the last one through the closing
  parenthetical.
- **Lists**: each citation in a string cite is its own label. One code with several sections
  (`42 U.S.C. §§ 1983, 1988`) is one label; `28 U.S.C. § 1331 and § 1343` is two (the second is
  `statute_short`).
- Spans cover the raw bytes, line breaks and PDF artifacts included. When page furniture (a page
  number, running head or page header) falls inside a citation, the span runs through it. When a
  footnote's text falls between a case name and its cite, the span starts at the volume.
- Weight-of-authority parentheticals (`(per curiam)`, `(en banc)`, `(plurality opinion)`) that
  follow the date parenthetical are excluded, like explanatory ones.

## Jurisdictions

Cases: the court (`us.scotus`; `us.fed.app.ca1`..`ca11`, `cadc`, `cafc`; `us.fed.dist.<pacer>`
like `us.fed.dist.dcd`; `us.<st>.sup`, `us.<st>.app`, `us.<st>.trial`; `foreign.gb`,
`foreign.ae`). Codes, rules, regulations: the enacting jurisdiction (`us.fed`, `us.oh`). Agency
decisions: `us.fed.nlrb`, `us.fed.eoir`, `us.fed.uspto`, `us.fed.irs`. Records and dockets: the
forum's court. US secondary sources: `us`.
