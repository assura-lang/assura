#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 strings (Assura source is always text).
    if let Ok(source) = std::str::from_utf8(data) {
        // The parser must never panic on any input.
        // Errors are expected and fine; panics are bugs.
        let _ = assura_parser::parse(source);
    }
});