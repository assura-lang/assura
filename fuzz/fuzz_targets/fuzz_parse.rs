#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8; the parser takes &str
    if let Ok(source) = std::str::from_utf8(data) {
        // The parser must never panic, only return errors
        let _ = assura_parser::parse(source);
    }
});
