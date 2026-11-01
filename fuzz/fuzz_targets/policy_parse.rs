#![no_main]
//! Policy JSON parsing never panics; a parsed policy is usable.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(policy) = serde_json::from_slice::<legal_citations::Policy>(data) {
        let _ = legal_citations::lint_markdown("See 42 U.S.C. § 1983 and Id. at 5.", &policy);
    }
});
