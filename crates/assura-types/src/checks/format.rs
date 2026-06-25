//! Format-related checks.
//!
//! Binary format, bit-level, string encoding, checksum,
//! protocol grammar, opaque function, codec registry.

use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::domain::*;
use crate::types::*;

pub(crate) fn run_binary_format_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = BinaryFormatChecker::new();
    let mut found = false;
    let mut buffer_len: usize = 0;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn_block(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "binary_format" || k == "byte_layout" {
                    found = true;
                    // Extract buffer length from call syntax: binary_format(len)
                    if let Some((_, args)) = extract_call(&clause.body) {
                        if let Some(len) = args.first().and_then(extract_int_literal) {
                            buffer_len = len as usize;
                        }
                    } else if let Some(len) = extract_int_literal(&clause.body) {
                        buffer_len = len as usize;
                    }
                }
                if k == "field" {
                    found = true;
                    // Extract field from call syntax: field(name, offset, size)
                    // or from kv pairs: name = x, offset = y, size = z
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let offset = args
                                    .first()
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO)
                                    as usize;
                                let size = args
                                    .get(1)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ONE)
                                    as usize;
                                let endianness =
                                    args.get(2).and_then(extract_ident).map(|e| match e {
                                        "big" | "be" => Endianness::Big,
                                        "little" | "le" => Endianness::Little,
                                        _ => Endianness::Native,
                                    });
                                checker.add_field(BinaryField {
                                    name: name.clone(),
                                    offset,
                                    size,
                                    endianness,
                                    span: decl.span.clone(),
                                });
                            }
                        }
                        Expr::Ident(name) => {
                            checker.add_field(BinaryField {
                                name: name.clone(),
                                offset: 0,
                                size: 1,
                                endianness: None,
                                span: decl.span.clone(),
                            });
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let offset = kvs
                                .iter()
                                .find(|(k, _)| *k == "offset")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as usize;
                            let size = kvs
                                .iter()
                                .find(|(k, _)| *k == "size")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as usize;
                            let endianness = kvs
                                .iter()
                                .find(|(k, _)| *k == "endian" || *k == "endianness")
                                .and_then(|(_, v)| extract_ident(v))
                                .map(|e| match e {
                                    "big" | "be" => Endianness::Big,
                                    "little" | "le" => Endianness::Little,
                                    _ => Endianness::Native,
                                });
                            checker.add_field(BinaryField {
                                name,
                                offset,
                                size,
                                endianness,
                                span: decl.span.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    checker.check_all(buffer_len)
}

/// Scan for bit-level format annotations and validate bit fields.
pub(crate) fn run_bit_level_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut container_bits: usize = 0;
    let mut checker: Option<BitLevelChecker> = None;
    let mut found = false;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "bit_layout" || k == "bit_level" {
                    found = true;
                    // Extract container size: bit_layout(bits)
                    let bits = match &clause.body.node {
                        Expr::Call { func: _, args } => args
                            .first()
                            .and_then(extract_int_literal)
                            .unwrap_or(DEFAULT_BIT_CONTAINER_BITS)
                            as usize,
                        Expr::Literal(_) => extract_int_literal(&clause.body)
                            .unwrap_or(DEFAULT_BIT_CONTAINER_BITS)
                            as usize,
                        _ => 64,
                    };
                    container_bits = bits;
                    checker = Some(BitLevelChecker::new(bits));
                }
                if k == "bit_field" {
                    found = true;
                    // Extract bit field: bit_field(name, offset, width) or bit_field(name, offset, width, cross_byte_ok)
                    if let Some(ref mut ch) = checker {
                        match &clause.body.node {
                            Expr::Call { func, args } => {
                                if let Expr::Ident(name) = &func.as_ref().node {
                                    let bit_offset = args
                                        .first()
                                        .and_then(extract_int_literal)
                                        .unwrap_or(DEFAULT_PARAM_ZERO)
                                        as usize;
                                    let bit_width = args
                                        .get(1)
                                        .and_then(extract_int_literal)
                                        .unwrap_or(DEFAULT_PARAM_ONE)
                                        as usize;
                                    let cross_byte_ok = args
                                        .get(2)
                                        .and_then(extract_ident)
                                        .is_some_and(|v| v == "true");
                                    ch.add_field(BitField {
                                        name: name.clone(),
                                        bit_offset,
                                        bit_width,
                                        span: decl.span.clone(),
                                        cross_byte_ok,
                                    });
                                }
                            }
                            Expr::Ident(name) => {
                                ch.add_field(BitField {
                                    name: name.clone(),
                                    bit_offset: 0,
                                    bit_width: 1,
                                    span: decl.span.clone(),
                                    cross_byte_ok: false,
                                });
                            }
                            _ => {}
                        }
                    } else {
                        // No container declared yet, create default 64-bit
                        container_bits = 64;
                        let mut ch = BitLevelChecker::new(64);
                        if let Expr::Ident(name) = &clause.body.node {
                            ch.add_field(BitField {
                                name: name.clone(),
                                bit_offset: 0,
                                bit_width: 1,
                                span: decl.span.clone(),
                                cross_byte_ok: false,
                            });
                        }
                        checker = Some(ch);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    match checker {
        Some(ch) => ch.check_all(container_bits),
        None => Vec::new(),
    }
}

/// Scan for string encoding annotations and validate.
pub(crate) fn run_string_encoding_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = StringEncodingChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "encoding" || k == "string_encoding" || k == "charset")
            {
                found = true;
                // Extract encoding from call syntax: encoding(name, enc_type)
                match &clause.body.node {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = &func.as_ref().node {
                            let enc = args
                                .first()
                                .and_then(extract_ident)
                                .map(parse_encoding)
                                .unwrap_or(StringEncoding::RawBytes);
                            checker.declare(name.clone(), enc);
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(name.clone(), StringEncoding::RawBytes);
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "var")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let enc = kvs
                            .iter()
                            .find(|(k, _)| *k == "encoding" || *k == "enc")
                            .and_then(|(_, v)| extract_ident(v))
                            .map(parse_encoding)
                            .unwrap_or(StringEncoding::RawBytes);
                        checker.declare(name, enc);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check for raw bytes used as strings, encoding compatibility, and truncation
    let mut errors = Vec::new();
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_use_as_string(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check encoding compatibility (target UTF-8 by default)
                    if let Some(err) =
                        checker.check_encoding_compat(name, &StringEncoding::Utf8, &decl.span)
                    {
                        errors.push(err);
                    }
                    // Check truncation at common byte boundaries
                    if let Some(err) = checker.check_truncation(name, 1, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Parse a string encoding name to the enum.
fn parse_encoding(name: &str) -> StringEncoding {
    match name {
        "utf8" | "UTF8" | "utf-8" | "UTF-8" => StringEncoding::Utf8,
        "utf16le" | "UTF16LE" | "utf-16le" => StringEncoding::Utf16Le,
        "utf16be" | "UTF16BE" | "utf-16be" => StringEncoding::Utf16Be,
        "ascii" | "ASCII" => StringEncoding::Ascii,
        "latin1" | "LATIN1" | "iso-8859-1" => StringEncoding::Latin1,
        _ => StringEncoding::RawBytes,
    }
}

/// Scan for checksum annotations and validate verification order.
pub(crate) fn run_checksum_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = ChecksumChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "checksum" || k == "crc" || k == "hash" {
                    found = true;
                    // Extract checksum params: checksum(name, algorithm, start, end)
                    match &clause.body.node {
                        Expr::Call { func, args } => {
                            if let Expr::Ident(name) = &func.as_ref().node {
                                let algo = args
                                    .first()
                                    .and_then(extract_ident)
                                    .map(parse_checksum_algorithm)
                                    .unwrap_or(ChecksumAlgorithm::Crc32);
                                let start = args
                                    .get(1)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_PARAM_ZERO)
                                    as usize;
                                let end = args
                                    .get(2)
                                    .and_then(extract_int_literal)
                                    .unwrap_or(DEFAULT_REGION_SIZE)
                                    as usize;
                                checker.declare_region(name.clone(), algo, start, end);
                            }
                        }
                        Expr::Ident(name) => {
                            checker.declare_region(name.clone(), ChecksumAlgorithm::Crc32, 0, 1024);
                        }
                        _ => {
                            let kvs = extract_kv_pairs(&clause.body);
                            let name = kvs
                                .iter()
                                .find(|(k, _)| *k == "name" || *k == "region")
                                .and_then(|(_, v)| extract_ident(v))
                                .unwrap_or("unnamed")
                                .to_string();
                            let algo = kvs
                                .iter()
                                .find(|(k, _)| *k == "algorithm" || *k == "algo")
                                .and_then(|(_, v)| extract_ident(v))
                                .map(parse_checksum_algorithm)
                                .unwrap_or(ChecksumAlgorithm::Crc32);
                            let start = kvs
                                .iter()
                                .find(|(k, _)| *k == "start")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as usize;
                            let end = kvs
                                .iter()
                                .find(|(k, _)| *k == "end")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_REGION_SIZE)
                                as usize;
                            checker.declare_region(name, algo, start, end);
                        }
                    }
                }
                if (k == "verify_checksum" || k == "verified")
                    && let Expr::Ident(name) = &clause.body.node
                {
                    checker.mark_verified(name);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check for use before verification, algorithm match, and range coverage
    let mut errors = Vec::new();
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_use_before_verify(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check algorithm consistency (verify declared matches expected)
                    if let Some(err) =
                        checker.check_algorithm_match(name, &ChecksumAlgorithm::Crc32, &decl.span)
                    {
                        errors.push(err);
                    }
                    // Check range coverage (verify checksum covers data range)
                    if let Some(err) = checker.check_range_coverage(name, 0, 1024, &decl.span) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Parse a checksum algorithm name to the enum.
fn parse_checksum_algorithm(name: &str) -> ChecksumAlgorithm {
    match name {
        "crc32" | "CRC32" | "crc" => ChecksumAlgorithm::Crc32,
        "adler32" | "ADLER32" | "adler" => ChecksumAlgorithm::Adler32,
        "sha256" | "SHA256" | "sha-256" => ChecksumAlgorithm::Sha256,
        "sha512" | "SHA512" | "sha-512" => ChecksumAlgorithm::Sha512,
        "md5" | "MD5" => ChecksumAlgorithm::Md5,
        _ => ChecksumAlgorithm::Custom(name.to_string()),
    }
}

/// Scan for protocol grammar/state machine annotations and validate transitions.
pub(crate) fn run_protocol_grammar_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker: Option<ProtocolGrammarChecker> = None;
    let mut found = false;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn_block(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "protocol" || k == "state_machine" || k == "rfc" {
                    found = true;
                    // Extract initial state from expression
                    let initial = extract_ident(&clause.body).unwrap_or("init").to_string();
                    if checker.is_none() {
                        checker = Some(ProtocolGrammarChecker::new(initial));
                    }
                }
                // Register states
                if (k == "state" || k == "protocol_state")
                    && let Some(name) = extract_ident(&clause.body)
                    && let Some(ref mut ch) = checker
                {
                    ch.add_state(name.to_string());
                }
                // Register transitions: transition(from, msg, to)
                if k == "transition"
                    && let Some((from, args)) = extract_call(&clause.body)
                    && args.len() >= 2
                    && let Some(ref mut ch) = checker
                {
                    let msg = extract_ident(&args[0]).unwrap_or("unknown").to_string();
                    let to = extract_ident(&args[1]).unwrap_or("unknown").to_string();
                    ch.add_transition(from.to_string(), to, msg);
                }
                // Register required fields: required_fields(msg, [field1, field2])
                if (k == "required_fields" || k == "required")
                    && let Some((msg, args)) = extract_call(&clause.body)
                    && let Some(ref mut ch) = checker
                {
                    let field_names: Vec<String> = args
                        .iter()
                        .filter_map(|a| extract_ident(a).map(String::from))
                        .collect();
                    ch.add_required_fields(msg.to_string(), field_names);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let checker = match checker {
        Some(c) => c,
        None => return Vec::new(),
    };
    // Validate message sends, transitions, and required fields
    let mut checker = checker;
    let mut errors = Vec::new();
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "send" || k == "message")
                && let Some(msg) = extract_ident(&clause.body)
            {
                if let Some(err) = checker.check_send(msg, &decl.span) {
                    errors.push(err);
                }
                // Perform state transition for the message
                if let Some(err) = checker.transition(msg, &decl.span) {
                    errors.push(err);
                }
                // Check required fields for the message (none provided by default)
                let field_errs = checker.check_required_fields(msg, &[], &decl.span);
                errors.extend(field_errs);
            }
        }
    }
    errors
}

/// Scan for opaque function declarations and check contracts.
pub(crate) fn run_opaque_function_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = OpaqueFunctionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node {
            for clause in &f.clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "opaque"
                {
                    found = true;
                    let has_contract = f
                        .clauses
                        .iter()
                        .any(|c| matches!(c.kind, ClauseKind::Requires | ClauseKind::Ensures));
                    checker.declare_opaque(f.name.clone(), has_contract, decl.span.clone());
                }
            }
        } else if let Decl::Contract(c) = &decl.node {
            for clause in &c.clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "opaque"
                {
                    found = true;
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check that opaque functions called without contracts are flagged
    let mut errors = Vec::new();
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            // Handle proof context and reveal annotations
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "proof" || k == "proof_context" {
                    checker.enter_proof();
                }
                if k == "end_proof" {
                    checker.exit_proof();
                }
                if k == "reveal"
                    && let Expr::Ident(fn_name) = &clause.body.node
                    && let Some(err) = checker.reveal(fn_name, &decl.span)
                {
                    errors.push(err);
                }
            }
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if let Some(err) = checker.check_call(name, &decl.span) {
                        errors.push(err);
                    }
                    // Check body access for opaque functions
                    if checker.is_opaque(name)
                        && let Some(mut err) = checker.check_body_access(name, &decl.span)
                    {
                        err.secondary = checker.opaque_span(name).map(|s| {
                            (s.clone(), format!("opaque function `{name}` declared here"))
                        });
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// G008: Codec registry validation (FMT.4)
// ---------------------------------------------------------------------------

/// Check codec registry declarations for:
/// - A52001: Overlapping magic byte patterns between codecs
/// - A52002: Empty decoder function name
pub(crate) fn run_codec_registry_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    use assura_parser::ast::MagicPattern;
    let mut errors = Vec::new();

    for decl in &source.decls {
        let Decl::CodecRegistry(cr) = &decl.node else {
            continue;
        };

        // A52001: Check for overlapping magic byte prefixes
        let byte_patterns: Vec<(usize, &[u8])> = cr
            .codecs
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match &c.magic {
                MagicPattern::Bytes { bytes, .. } if !bytes.is_empty() => {
                    Some((i, bytes.as_slice()))
                }
                _ => None,
            })
            .collect();

        for (i, (idx_a, bytes_a)) in byte_patterns.iter().enumerate() {
            for (idx_b, bytes_b) in byte_patterns.iter().skip(i + 1) {
                let min_len = bytes_a.len().min(bytes_b.len());
                if bytes_a[..min_len] == bytes_b[..min_len] {
                    errors.push(TypeError {
                        code: "A52001".into(),
                        message: format!(
                            "overlapping magic byte patterns in codec registry `{}`: \
                             codec `{}` and codec `{}` share a common prefix",
                            cr.name, cr.codecs[*idx_a].name, cr.codecs[*idx_b].name,
                        ),
                        span: decl.span.clone(),
                        secondary: None,
                    });
                }
            }
        }

        // A52002: Check for empty decoder names
        for codec in &cr.codecs {
            if codec.decoder.is_empty() {
                errors.push(TypeError {
                    code: "A52002".into(),
                    message: format!(
                        "codec `{}` in registry `{}` has no decoder function",
                        codec.name, cr.name,
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    // --- binary format checks ---

    #[test]
    fn binary_format_no_annotation_produces_no_errors() {
        let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
        let sf = parse_source(src);
        let errors = run_binary_format_checks(&sf);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn binary_format_field_exceeds_buffer_length() {
        let src = r#"contract Header { binary_format buf field length }"#;
        let sf = parse_source(src);
        let errors = run_binary_format_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A26001"),
            "expected A26001 for field exceeding buffer length, got: {errors:?}"
        );
    }

    // --- bit level checks ---

    #[test]
    fn bit_level_no_annotation_produces_no_errors() {
        let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
        let sf = parse_source(src);
        let errors = run_bit_level_checks(&sf);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn bit_level_width_mismatch() {
        let src = r#"contract Flags { bit_layout flags bit_field status }"#;
        let sf = parse_source(src);
        let errors = run_bit_level_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A27003"),
            "expected A27003 for bit width mismatch, got: {errors:?}"
        );
    }

    // --- string encoding checks ---

    #[test]
    fn string_encoding_no_annotation_produces_no_errors() {
        let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
        let sf = parse_source(src);
        let errors = run_string_encoding_checks(&sf);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn string_encoding_raw_bytes_as_string() {
        let src = r#"contract Decode { encoding data ensures { data > 0 } }"#;
        let sf = parse_source(src);
        let errors = run_string_encoding_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A28001"),
            "expected A28001 for raw bytes used as string, got: {errors:?}"
        );
    }

    // --- opaque function checks ---

    #[test]
    fn opaque_function_no_annotation_produces_no_errors() {
        let src = r#"contract Simple { input(x: Int) requires { x > 0 } ensures { x > 0 } }"#;
        let sf = parse_source(src);
        let errors = run_opaque_function_checks(&sf);
        assert!(errors.is_empty(), "expected no errors: {errors:?}");
    }

    #[test]
    fn opaque_function_body_access_without_reveal() {
        let src = "fn helper(x: Int) -> Int\n    opaque marker\n    ensures { helper > 0 }";
        let sf = parse_source(src);
        let errors = run_opaque_function_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A32002"),
            "expected A32002 for opaque function body access, got: {errors:?}"
        );
    }
}
