# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- README install command: cargo needs the `ssh://` URL form for `--git`.
- Flaky property test `link_free_markdown_matches_plain_text`: input starting with four
  spaces is an indented code block, which markdown masks on purpose. The property now
  skips it and a fixed test covers it.

## [0.1.0] - 2026-10-24

First release. Built from history 2026-10-22 to 2026-10-24.

### Added

- Library `legal-citations` and CLI `cite-detect`. They detect citations in text and
  markdown, report exact byte spans into the original text, and lint markdown so every
  citation links to its source. Findings CITE001-CITE020, exit codes 0-5, `human`, `json`,
  `jsonl` and `md-report` output with committed JSON Schemas, and `inspect` and `explain`
  for debugging (c3a83e9).
- Page furniture: running heads and repeated captions are marked and exempt from the link
  requirement by default (45b328e).
- Rule collections, selectors and named profiles (`--include`, `--exclude`, `--profile`,
  `--jurisdiction`) (aa87b0d). Court-level jurisdictions resolved from the court
  parenthetical (99fa15b, f750a36). `israel` and `mena` profiles (d2efc36).
- Typed citation details and a table of authorities (`cite-detect authorities`) (bdb3c45).
- Catch-all layer for citation-shaped text no rule recognizes (CITE011) (e5d1eef).
- Short forms: `statute_short` kind, `Id. §` and bare `§` short forms, re-sectioned
  `Id. §` cites, `sub nom.` history links and footnote pincites (55460f6, 23ff7ba, 2b9f12d).
- `cite-detect fix` URL templates with offline decoders for govinfo, Cornell LII and
  supremecourt.gov (e27e44b).
- Indigo Book format coverage measurement with a regression gate (`just indigo-coverage`)
  (cf8cd2d).
- `cite-eval` crate: gold-corpus scoring of recall, precision and exact spans (07c82b7). It
  ships with 23 gold-labeled documents (3ed9fda) and a measurement report at d5c2ba8
  (79e07fd).
- `xtask-rules`: generates case rules from reporters-db (53fb478) and their short forms
  (2acdf5b), and ports the Indigo rule generator to Rust (6c5bd73).
- `just dist`: a language-neutral release bundle with every rule, test vectors from the
  rule examples and their JSON Schemas. It also adds a pattern portability check for
  Python `re`, Go RE2, PCRE and ECMAScript/.NET (1005d49).
- Landing gate: `scripts/box-verify.sh` records the tested tree, and `just land-check`
  refuses landings without a PASS (500af30). Sharded, memory-capped `cargo-mutants` runs
  (1d75ec6).
- `cite-detect --version` prints the version and the git commit it was built from.
- `just release VERSION` checks the tree, the branch and the CHANGELOG, runs `just check`
  and creates the annotated tag.

### Data / rules

2087 active rules at release (`cite-detect rules table`), including:

- 1069 case rules generated from reporters-db (e6523fd, 2acdf5b). Public-domain
  case citations for 16 states and 4 territories (47aafc6).
- Ohio R.C. sections, webcites and Ohio State Reports (9401cf1).
- Federal agency adjudications, IRS and Treasury rulings, executive materials and
  legislative materials (0c7df75, ba1d04d, 22ea503, b5e4760). Indigo T2 agency forms
  (9cc5dae). Federal agency materials outside the Indigo Book (306b95b).
- State court rules, administrative codes, session laws, registers, attorney general
  opinions and constitutions (ed358b5, 3ca954f, 78147d0, e23df3a, b4bd7fb).
- Specialty courts, uniform and model laws, district local rules and standing orders
  (b08576b, eab612a, de34cab).
- Treaties, U.N. documents, international courts, EU sources and WTO (9715f9b,
  45c5d6a, 3f2a8e4). Foreign national law for common-law jurisdictions (93f4f3d). Draft
  UAE, GCC, Israel and MENA rules, validated against the rule contract but mostly
  unverified against sources (d5c2ba8, c887237).
- Record cites, Bates numbers, ECF pins and bare docket numbers (c12c08f, 276873b, 3457a96).
- Secondary sources: A.L.R., encyclopedias, law reviews and The Federalist (c9eb131,
  ff026ca, f5ea4da).
- Indigo Book format inventory, with 1221 of 1225 formats covered (04f2dc9).
- Rule data moved to top-level `rules/` (1005d49).

Almost every rule is `opened: false` and `precision: unmeasured`; see the README's Limits.

### Fixed

- Spans in markdown use original-text coordinates (4ca447f). Spans that stopped short on
  real text are extended (31e020e). A pin list ends before masked markup that comes ahead
  of a parallel cite (bd96eab).
- The case-name scan stops at a number that ends a sentence (40ff5dc).
- Hand-written, record, bill and secondary rules no longer split reporter cites (e7cd8c3,
  afb16e5, 170c4b1). State executive orders stay out of the federal rule (1fcaf98).
- `us.ak.statute.code` no longer matches `AS` plus any word (6d8864c).
- `us.record.bates` rejects USPTO ESTTA/TEAS/TTABVUE ids and tokens after
  `Tracking number:` (a9a3665).
- URL fix templates are kept only where a successful fetch was recorded (805c1b4).

### License

- Code is GPL-3.0-or-later (`LICENSE`).
- Rule data in `rules/` and the `just dist` bundle are MIT (`rules/LICENSE`). The
  third-party notices that travel with them are in `rules/NOTICE` (1005d49).
- Vendored reporters-db `reporters.json` (BSD-2-Clause) (6422e48). Indigo Book inventory
  (CC0) (04f2dc9). Notices are in `NOTICE`.

[Unreleased]: https://github.com/davidawad/legal-citations-rs/compare/v0.1.0...main
[0.1.0]: https://github.com/davidawad/legal-citations-rs/releases/tag/v0.1.0
