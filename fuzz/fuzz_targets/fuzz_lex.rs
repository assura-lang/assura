#![no_main]

use libfuzzer_sys::fuzz_target;
use logos::Logos;

fuzz_target!(|data: &[u8]| {
    // Only process valid UTF-8; the lexer takes &str
    if let Ok(source) = std::str::from_utf8(data) {
        // The lexer must never panic, only return tokens or errors
        let lex = assura_parser::lexer::Token::lexer(source);
        for result in lex {
            let _ = result;
        }
    }
});