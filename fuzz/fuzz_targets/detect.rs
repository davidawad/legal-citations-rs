#![no_main]
//! Plain-text detection never panics and every span slices the input.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        for c in legal_citations::detect(text) {
            assert_eq!(text.get(c.span.start..c.span.end), Some(c.text.as_str()));
        }
        let _ = legal_citations::legacy::detect_legacy(text);
    }
});
