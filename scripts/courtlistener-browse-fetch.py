#!/usr/bin/env python3
"""Find a real citation per reporters-db case rule and fetch its CourtListener /c/ URL.

dot-1ij4xz.11: the reporters-db examples are mostly placeholders (1/1 cites, or
no example at all), so fetching them proves nothing. CourtListener's own
reporter browse pages list real citations without an API token:

  /c/<abbr>/          the volumes CourtListener holds for that reporter
  /c/<abbr>/<vol>/    the cases in one volume, each with its citations

For every generated case rule that has no verified template, this script tries
the family's reporters-db edition abbreviations in order. For the first one
CourtListener knows, it reads a citation `<vol> <abbr> <page>` off a volume
page, checks that the rule's own pattern matches that citation with exactly
that reporter string (so the rule's template renders the fetched URL), and
fetches `/c/<abbr>/<vol>/<page>/`. Only a fetch that is redirected to an
/opinion/ page counts as verified.

Output: one TSV row per rule (stdout or --out). Rows are written as they
finish, so a rerun with --resume skips rules already logged.

Usage: scripts/courtlistener-browse-fetch.py --out docs/fetch-logs/X.tsv [--resume] [--shard I/N] [--only RULE_ID ...]
"""

from __future__ import annotations

import argparse
import html
import json
import pathlib
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASE = "https://www.courtlistener.com"
UA = "legal-citations-rs reporter check (gitlab.com/davidawad/legal-citations-rs)"
DELAY = 0.4
HEADER = ["rule_id", "abbr", "citation", "url", "status", "final_url", "outcome"]


def get(url: str) -> tuple[int, str, str]:
    """GET url following redirects; return (status, final url, body)."""
    for attempt in range(3):
        time.sleep(DELAY)
        req = urllib.request.Request(url, headers={"User-Agent": UA})
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                status, final, body = resp.status, resp.geturl(), resp.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            status, final, body = e.code, e.geturl() or url, ""
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            status, final, body = 0, url, ""
        resolved = urllib.parse.urlparse(final).path.startswith("/opinion/")
        # Retry a bot challenge, rate limit or dropped connection; a 5xx is
        # CourtListener failing on that citation and is logged, not retried.
        if status in (0, 202, 429) and not resolved:
            time.sleep(5 * (attempt + 1))
            continue
        return status, final, body
    return status, final, body


def c_url(*parts: str) -> str:
    return BASE + "/c/" + "".join(urllib.parse.quote(p, safe="") + "/" for p in parts)


def generated_rules() -> list[dict]:
    rules = []
    for path in sorted((ROOT / "rules/case/generated").rglob("*.json")):
        rule = json.loads(path.read_text())
        if rule["url_templates"]:
            continue
        m = re.search(r'reporters\.json entry "(.*)" \(', rule["provenance"]["encodes"])
        if m:
            rules.append({"id": rule["id"], "key": m.group(1), "pattern": rule["pattern"]})
    return rules


def variations(db: dict, key: str, abbr: str) -> list[str]:
    """reporters-db variant spellings of edition abbr, in file order."""
    out: list[str] = []
    for reporter in db.get(key, []):
        for variant, edition in reporter.get("variations", {}).items():
            if edition == abbr and variant not in out:
                out.append(variant)
    return out


def candidates(db: dict, key: str) -> list[str]:
    out: list[str] = []
    for reporter in db.get(key, []):
        for edition in reporter["editions"]:
            if edition not in out:
                out.append(edition)
    if key not in out:
        out.insert(0, key)
    return out


VOLUME_RE = re.compile(r'href="(/c/[^"/]+/([0-9]+)/)"')
CITES_RE = re.compile(r'Citations:</span>\s*<span class="meta-data-value">([^<]*)</span>')


def volumes(abbr: str) -> list[str]:
    status, _, body = get(c_url(abbr))
    if status != 200:
        return []
    vols: list[str] = []
    for _, vol in VOLUME_RE.findall(body):
        if vol not in vols:
            vols.append(vol)
    return vols


def citations(abbr: str, vol: str) -> list[tuple[str, str]]:
    """(volume, page) pairs printed on the volume page for this reporter."""
    status, _, body = get(c_url(abbr, vol))
    if status != 200:
        return []
    # The list is comma-separated, but some abbreviations contain a comma.
    want = re.compile(r"(?:^|, )([0-9]+) " + re.escape(abbr) + r" ([0-9]+)(?=,|$)")
    found = []
    for block in CITES_RE.findall(body):
        for m in want.finditer(html.unescape(block).strip()):
            if (m.group(1), m.group(2)) not in found:
                found.append((m.group(1), m.group(2)))
    return found


def matches_rule(pattern: str, abbr: str, vol: str, page: str) -> bool:
    text = f"{vol} {abbr} {page}"
    try:
        m = re.compile(pattern).search(f"See {text} (x).")
    except re.error:
        return False
    return bool(m and m.group(0) == text and m.group("reporter") == abbr
                and m.group("volume") == vol and m.group("page") == page)


def check(rule: dict, db: dict) -> list[str]:
    tried = []
    for abbr in candidates(db, rule["key"]):
        vols = volumes(abbr)
        if not vols:
            tried.append(f"{abbr}:no-volumes")
            continue
        # A middle volume is less likely to be a sparse first or last one.
        order = [vols[len(vols) // 2], vols[0], vols[-1]]
        for vol in dict.fromkeys(order):
            cites = citations(abbr, vol)
            if not cites:
                tried.append(f"{abbr}:{vol}:no-citation")
            for cvol, page in cites[:3]:
                # The rule's template renders whatever reporter string it
                # matched, so fetch the citation spelled the way the rule
                # matches it: the edition itself, else a reporters-db variant
                # (some generated rules match only variants; hand-written
                # rules own the canonical form).
                spelled = [a for a in [abbr, *variations(db, rule["key"], abbr)]
                           if matches_rule(rule["pattern"], a, cvol, page)]
                if not spelled:
                    tried.append(f"{abbr}:pattern-miss")
                    break
                url = c_url(spelled[0], cvol, page)
                status, final, _ = get(url)
                # The opinion page itself may answer 202 (bot challenge); the
                # redirect to /opinion/<id>/ is what proves the citation resolved.
                if status in (200, 202) and urllib.parse.urlparse(final).path.startswith("/opinion/"):
                    cite = f"{cvol} {spelled[0]} {page}"
                    return [rule["id"], spelled[0], cite, url, str(status), final, "opinion"]
                tried.append(f"{spelled[0]}:{cvol}/{page}:{status}")
            else:
                continue
            break
    return [rule["id"], "", "", "", "", "", "unverified " + ";".join(tried)]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", type=pathlib.Path)
    ap.add_argument("--resume", action="store_true")
    ap.add_argument("--only", nargs="*")
    ap.add_argument("--shard", default="0/1", help="I/N: take every Nth rule from offset I")
    args = ap.parse_args()
    db = json.loads((ROOT / "third_party/reporters-db/reporters.json").read_text())
    rules = generated_rules()
    if args.only:
        rules = [r for r in rules if r["id"] in args.only]
    shard, shards = map(int, args.shard.split("/"))
    rules = rules[shard::shards]
    done = set()
    if args.out and args.resume and args.out.exists():
        done = {line.split("\t", 1)[0] for line in args.out.read_text().splitlines()[1:]}
    out = args.out.open("a" if done else "w") if args.out else sys.stdout
    if not done:
        print("\t".join(HEADER), file=out, flush=True)
    for i, rule in enumerate(rules):
        if rule["id"] in done:
            continue
        row = check(rule, db)
        print("\t".join(row), file=out, flush=True)
        print(f"{i + 1}/{len(rules)} {row[0]} {row[-1][:80]}", file=sys.stderr, flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
