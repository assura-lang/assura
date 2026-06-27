use super::*;

// =======================================================================
// T070: BinaryFormatChecker tests
// =======================================================================

#[test]
fn binary_fmt_bounds_ok() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "magic".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    assert!(checker.check_bounds(100).is_empty());
}

#[test]
fn binary_fmt_bounds_overflow() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "data".into(),
        offset: 96,
        size: 8,
        endianness: Some(Endianness::Little),
        span: 0..1,
    });
    let errors = checker.check_bounds(100);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26001");
}

#[test]
fn binary_fmt_no_endianness() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "len".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1,
    });
    let errors = checker.check_endianness();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26003");
}

#[test]
fn binary_fmt_single_byte_no_endianness_ok() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "flags".into(),
        offset: 0,
        size: 1,
        endianness: None,
        span: 0..1,
    });
    assert!(checker.check_endianness().is_empty());
}

#[test]
fn binary_fmt_overlap() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "a".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    checker.add_field(BinaryField {
        name: "b".into(),
        offset: 2,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    let errors = checker.check_overlaps();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A26004");
}

#[test]
fn binary_fmt_no_overlap() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "a".into(),
        offset: 0,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    checker.add_field(BinaryField {
        name: "b".into(),
        offset: 4,
        size: 4,
        endianness: Some(Endianness::Big),
        span: 0..1,
    });
    assert!(checker.check_overlaps().is_empty());
}

#[test]
fn binary_fmt_check_all() {
    let mut checker = BinaryFormatChecker::new();
    checker.add_field(BinaryField {
        name: "header".into(),
        offset: 0,
        size: 4,
        endianness: None,
        span: 0..1, // missing endianness
    });
    let errors = checker.check_all(100);
    assert_eq!(errors.len(), 1); // endianness only
}

#[test]
fn binary_fmt_default() {
    let checker = BinaryFormatChecker::default();
    assert!(checker.check_all(0).is_empty());
}

// =======================================================================
// T071: BitLevelChecker tests
// =======================================================================

#[test]
fn bit_level_bounds_ok() {
    let mut checker = BitLevelChecker::new(32);
    checker.add_field(BitField {
        name: "version".into(),
        bit_offset: 0,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_bounds().is_empty());
}

#[test]
fn bit_level_bounds_overflow() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "big".into(),
        bit_offset: 4,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: true,
    });
    let errors = checker.check_bounds();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A27001");
}

#[test]
fn bit_level_byte_crossing() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "cross".into(),
        bit_offset: 6,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    let errors = checker.check_byte_crossing();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A27002");
}

#[test]
fn bit_level_byte_crossing_allowed() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "cross".into(),
        bit_offset: 6,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: true,
    });
    assert!(checker.check_byte_crossing().is_empty());
}

#[test]
fn bit_level_total_width_match() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    checker.add_field(BitField {
        name: "b".into(),
        bit_offset: 4,
        bit_width: 4,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_total_width(8).is_none());
}

#[test]
fn bit_level_total_width_mismatch() {
    let mut checker = BitLevelChecker::new(8);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 3,
        span: 0..1,
        cross_byte_ok: false,
    });
    let err = checker.check_total_width(8);
    assert_eq!(err.unwrap().code, "A27003");
}

#[test]
fn bit_level_check_all() {
    let mut checker = BitLevelChecker::new(16);
    checker.add_field(BitField {
        name: "a".into(),
        bit_offset: 0,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: false,
    });
    checker.add_field(BitField {
        name: "b".into(),
        bit_offset: 8,
        bit_width: 8,
        span: 0..1,
        cross_byte_ok: false,
    });
    assert!(checker.check_all(16).is_empty());
}

// =======================================================================
// T072: StringEncodingChecker tests
// =======================================================================

#[test]
fn string_encoding_raw_bytes_error() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("data".into(), StringEncoding::RawBytes);
    let err = checker.check_use_as_string("data", &(0..1));
    assert_eq!(err.unwrap().code, "A28001");
}

#[test]
fn string_encoding_utf8_ok() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("text".into(), StringEncoding::Utf8);
    assert!(checker.check_use_as_string("text", &(0..1)).is_none());
}

#[test]
fn string_encoding_mismatch() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Le);
    let err = checker.check_encoding_compat("wide", &StringEncoding::Utf8, &(0..1));
    assert_eq!(err.unwrap().code, "A28002");
}

#[test]
fn string_encoding_ascii_compat() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("ascii_str".into(), StringEncoding::Ascii);
    // ASCII is compatible with everything
    assert!(
        checker
            .check_encoding_compat("ascii_str", &StringEncoding::Utf8, &(0..1))
            .is_none()
    );
}

#[test]
fn string_encoding_truncation_utf16() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Le);
    let err = checker.check_truncation("wide", 5, &(0..1)); // 5 bytes, not aligned to 2
    assert_eq!(err.unwrap().code, "A28003");
}

#[test]
fn string_encoding_truncation_ok() {
    let mut checker = StringEncodingChecker::new();
    checker.declare("wide".into(), StringEncoding::Utf16Be);
    assert!(checker.check_truncation("wide", 4, &(0..1)).is_none()); // 4 bytes, aligned
}

#[test]
fn string_encoding_unknown_var() {
    let checker = StringEncodingChecker::new();
    let err = checker.check_use_as_string("unknown", &(0..1));
    assert_eq!(err.unwrap().code, "A28001");
}

#[test]
fn string_encoding_default() {
    let checker = StringEncodingChecker::default();
    checker.check_use_as_string("x", &(0..1)).unwrap();
}

// =======================================================================
// T074: ChecksumChecker tests
// =======================================================================

#[test]
fn checksum_use_before_verify() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
    let err = checker.check_use_before_verify("payload", &(0..1));
    assert_eq!(err.unwrap().code, "A29001");
}

#[test]
fn checksum_use_after_verify_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("payload".into(), ChecksumAlgorithm::Crc32, 0, 100);
    checker.mark_verified("payload");
    assert!(
        checker
            .check_use_before_verify("payload", &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_algorithm_mismatch() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
    let err = checker.check_algorithm_match("data", &ChecksumAlgorithm::Crc32, &(0..1));
    assert_eq!(err.unwrap().code, "A29002");
}

#[test]
fn checksum_algorithm_match_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Sha256, 0, 100);
    assert!(
        checker
            .check_algorithm_match("data", &ChecksumAlgorithm::Sha256, &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_range_coverage() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 10, 50);
    let err = checker.check_range_coverage("data", 0, 60, &(0..1));
    assert_eq!(err.unwrap().code, "A29003");
}

#[test]
fn checksum_range_covered_ok() {
    let mut checker = ChecksumChecker::new();
    checker.declare_region("data".into(), ChecksumAlgorithm::Adler32, 0, 100);
    assert!(
        checker
            .check_range_coverage("data", 10, 50, &(0..1))
            .is_none()
    );
}

#[test]
fn checksum_default() {
    let checker = ChecksumChecker::default();
    assert!(checker.check_use_before_verify("x", &(0..1)).is_none());
}

// =======================================================================
// T075: ProtocolGrammarChecker tests
// =======================================================================

#[test]
fn protocol_valid_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    assert!(checker.check_send("CONNECT", &(0..1)).is_none());
    assert!(checker.transition("CONNECT", &(0..1)).is_none());
}

#[test]
fn protocol_invalid_send() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    let err = checker.check_send("DISCONNECT", &(0..1));
    assert_eq!(err.unwrap().code, "A30002");
}

#[test]
fn protocol_invalid_transition() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    let err = checker.transition("DATA", &(0..1));
    assert_eq!(err.unwrap().code, "A30001");
}

#[test]
fn protocol_required_fields() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields("CONNECT".into(), vec!["host".into(), "port".into()]);
    let errors = checker.check_required_fields("CONNECT", &["host"], &(0..1));
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, "A30003");
    assert!(errors[0].message.contains("port"));
}

#[test]
fn protocol_required_fields_ok() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_required_fields("CONNECT".into(), vec!["host".into()]);
    let errors = checker.check_required_fields("CONNECT", &["host", "port"], &(0..1));
    assert!(errors.is_empty());
}

#[test]
fn protocol_multi_state() {
    let mut checker = ProtocolGrammarChecker::new("idle".into());
    checker.add_state("connected".into());
    checker.add_state("ready".into());
    checker.add_transition("idle".into(), "connected".into(), "CONNECT".into());
    checker.add_transition("connected".into(), "ready".into(), "AUTH".into());
    checker.add_transition("ready".into(), "idle".into(), "CLOSE".into());

    assert!(checker.transition("CONNECT", &(0..1)).is_none());
    assert!(checker.transition("AUTH", &(0..1)).is_none());
    assert!(checker.transition("CLOSE", &(0..1)).is_none());
}
