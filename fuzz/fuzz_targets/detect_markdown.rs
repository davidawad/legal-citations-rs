#![no_main]
//! Markdown detection and linting never panic and spans slice the input.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let (doc, findings) = legal_citations::lint_markdown(text, &legal_citations::Policy::default());
        for c in &doc.citations {
            assert_eq!(text.get(c.span.start..c.span.end), Some(c.text.as_str()));
        }
        for f in &findings {
            assert!(text.get(f.span.start..f.span.end).is_some());
        }
    }
});
