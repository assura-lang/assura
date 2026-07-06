#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8
    if let Ok(source) = std::str::from_utf8(data) {
        // Parse -> resolve -> type_check pipeline.
        // None of these should ever panic, only return errors.
        let (file, _errors) = assura_parser::parse(source);
        let file = match file {
            Some(f) => f,
            None => return,
        };
        let resolved = match assura_resolve::resolve(&file) {
            Ok(r) => r,
            Err(_) => return,
        };
        // The type checker must never panic on any resolved input
        let _ = assura_types::type_check(resolved);
    }
});
