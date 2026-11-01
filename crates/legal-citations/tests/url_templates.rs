//! Every URL template verified by fetch (dot-1ij4xz.5) builds the expected
//! URL for a citation, and its decoder turns that URL back into the same
//! authority, so CITE004 congruence holds for the host; a link to another
//! authority on the host is incongruent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use legal_citations::link_target::{candidates, decode, decode_with};
use legal_citations::policy::{evaluate, Mode, Policy};
use legal_citations::rule::{fill_template, template_filters, template_names};
use legal_citations::{detect, detect_markdown, registry};
use std::collections::BTreeMap;

/// `(text, rule id, first URL, a URL on the host for another authority)`.
const PAIRS: &[(&str, &str, &str, &str)] = &[
    (
        "85 Fed. Reg. 12,345",
        "us.fed.federal_register",
        "https://www.govinfo.gov/link/fr/85/12345",
        "https://www.govinfo.gov/link/fr/85/12346",
    ),
    (
        "60 Fed. Reg. 5000",
        "us.fed.federal_register",
        "https://www.govinfo.gov/link/fr/60/5000",
        "https://www.govinfo.gov/link/fr/61/5000",
    ),
    (
        "88 FR 1,000",
        "us.fed.federal_register.fr_abbrev",
        "https://www.govinfo.gov/link/fr/88/1000",
        "https://www.govinfo.gov/link/fr/88/100",
    ),
    (
        "H.R. 3684, 117th Cong.",
        "us.fed.legislative.bill",
        "https://www.govinfo.gov/link/bills/117/hr/3684",
        "https://www.govinfo.gov/link/bills/117/s/3684",
    ),
    (
        "S. 567, 117th Cong.",
        "us.fed.legislative.bill",
        "https://www.govinfo.gov/link/bills/117/s/567",
        "https://www.govinfo.gov/link/bills/116/s/567",
    ),
    (
        "H.J. Res. 64, 107th Cong.",
        "us.fed.legislative.bill",
        "https://www.govinfo.gov/link/bills/107/hjres/64",
        "https://www.govinfo.gov/link/bills/107/sjres/64",
    ),
    (
        "H.R. Res. 431, 114th Cong.",
        "us.fed.legislative.house_resolution",
        "https://www.govinfo.gov/link/bills/114/hres/431",
        "https://www.govinfo.gov/link/bills/114/hres/432",
    ),
    (
        "H.R.J. Res. 64, 107th Cong.",
        "us.fed.legislative.house_joint_resolution",
        "https://www.govinfo.gov/link/bills/107/hjres/64",
        "https://www.govinfo.gov/link/bills/107/hr/64",
    ),
    (
        "H.R. Con. Res. 27, 114th Cong.",
        "us.fed.legislative.house_concurrent_resolution",
        "https://www.govinfo.gov/link/bills/114/hconres/27",
        "https://www.govinfo.gov/link/bills/114/sconres/27",
    ),
    (
        "Pub. L. No. 117-58, 135 Stat. 429 (2021)",
        "us.fed.session_law.public_law_stat",
        "https://www.govinfo.gov/link/plaw/117/public/58",
        "https://www.govinfo.gov/link/plaw/117/public/59",
    ),
    (
        "Fed. R. Bankr. P. 7001",
        "us.fed.rules.bankruptcy_procedure",
        "https://www.law.cornell.edu/rules/frbp/rule_7001",
        "https://www.law.cornell.edu/rules/frbp/rule_7002",
    ),
    (
        "Sup. Ct. R. 14.1(g)",
        "us.fed.rules.supreme_court",
        "https://www.law.cornell.edu/rules/supct/rule_14",
        "https://www.law.cornell.edu/rules/supct/rule_15",
    ),
    (
        "No. 21-476, slip op. at 5 (U.S. June 30, 2023)",
        "us.scotus.docket.slip_op",
        "https://www.supremecourt.gov/docket/docketfiles/html/public/21-476.html",
        "https://www.supremecourt.gov/docket/docketfiles/html/public/21-477.html",
    ),
    (
        "50 Ohio St.3d 58",
        "us.oh.case.ohio_state",
        "https://www.courtlistener.com/c/Ohio%20St.3d/50/58/",
        "https://www.courtlistener.com/c/Ohio%20St.3d/50/59/",
    ),
];

#[test]
fn templates_and_decoders_agree() {
    let reg = registry();
    for (text, rule, url, other) in PAIRS {
        let cites = detect(text);
        let [c] = cites.as_slice() else {
            panic!("{text}: {cites:?}");
        };
        assert_eq!(c.rule_id, *rule, "{text}");
        let urls = candidates(reg, c.rule_index, &c.fields);
        assert_eq!(urls.first().map(String::as_str), Some(*url), "{text}");
        let authority = c.authority.as_deref().expect(text);
        let decoded = decode(reg, &url::Url::parse(url).unwrap()).expect(url);
        assert_eq!(decoded.authority, authority, "{text} -> {url}");
        let other = decode(reg, &url::Url::parse(other).unwrap()).expect(other);
        assert_ne!(other.authority, authority, "{text} vs {other:?}");
    }
}

/// The same pairs through the linter: the right link is clean, the wrong
/// one is CITE004.
#[test]
fn linked_cites_are_congruent_and_wrong_links_are_not() {
    for (text, _, url, other) in PAIRS {
        let codes = |link: &str| -> Vec<String> {
            let md = format!("See [{text}]({link}).\n");
            let doc = detect_markdown(&md);
            evaluate(registry(), &doc, &Policy::default(), Mode::Lint)
                .iter()
                .map(|f| f.code.to_string())
                .filter(|c| c.starts_with("CITE00"))
                .collect()
        };
        assert!(codes(url).is_empty(), "{text}: {:?}", codes(url));
        assert_eq!(codes(other), ["CITE004"], "{text}");
    }
}

/// The Federal Register key ignores comma grouping, so a cite with and one
/// without the comma are one authority; the bill key is the govinfo type.
#[test]
fn filtered_keys_unify_spellings() {
    let a = detect("85 Fed. Reg. 12,345");
    let b = detect("85 FR 12345");
    assert_eq!(a[0].authority, b[0].authority);
    assert_eq!(a[0].authority.as_deref(), Some("us.fr/85/12345"));
    let h = detect("H. Res. 755, 116th Cong.");
    let r = detect("H.R. Res. 755, 116th Cong.");
    assert_eq!(h[0].authority.as_deref(), Some("us.bill/116/hres755"));
    assert_eq!(h[0].authority, r[0].authority);
    // Without the Congress there is no key, and no URL.
    let bare = detect("S. 567");
    assert_eq!(bare[0].authority, None);
    assert!(candidates(registry(), bare[0].rule_index, &bare[0].fields).is_empty());
}

#[test]
fn template_filters_fill_and_validate() {
    let g: BTreeMap<String, String> = [("p", "12,345"), ("c", "H.J. Res. ")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(
        fill_template("{p|digits}/{c|slug}/{p}", &g).as_deref(),
        Some("12345/hjres/12,345")
    );
    assert_eq!(fill_template("{p|nope}", &g), None);
    assert_eq!(template_names("{p|digits}/{c}"), ["p", "c"]);
    assert_eq!(template_filters("{p|digits}/{c}/{x|y}"), ["digits", "y"]);
    assert!(template_filters("{p").is_empty());
}

/// Decoder segments may hold several placeholders around literals.
#[test]
fn decoder_segments_with_several_placeholders() {
    let u = |s: &str| url::Url::parse(s).unwrap();
    let g = decode_with(
        "federalregister.gov/citation/{volume}-FR-{page}",
        &u("https://www.federalregister.gov/citation/85-fr-12345"),
    )
    .unwrap();
    assert_eq!(g["volume"], "85");
    assert_eq!(g["page"], "12345");
    let d = decode(
        registry(),
        &u("https://www.federalregister.gov/citation/85-FR-12345"),
    )
    .unwrap();
    assert_eq!(d.authority, "us.fr/85/12345");
    for (decoder, url) in [
        // Literal missing, prefix wrong, empty value.
        ("h.org/{a}-x-{b}", "https://h.org/1-y-2"),
        ("h.org/p{a}", "https://h.org/q1"),
        ("h.org/{a}-{b}", "https://h.org/1-"),
        ("h.org/{a}.html", "https://h.org/.html"),
        ("h.org/{a", "https://h.org/1"),
    ] {
        assert!(decode_with(decoder, &u(url)).is_none(), "{decoder} {url}");
    }
    // Adjacent placeholders: the first takes one character.
    let g = decode_with("h.org/{a}{b}", &u("https://h.org/123")).unwrap();
    assert_eq!((g["a"].as_str(), g["b"].as_str()), ("1", "23"));
    let g = decode_with("h.org/{a}.html", &u("https://h.org/21-476.html")).unwrap();
    assert_eq!(g["a"], "21-476");
}
