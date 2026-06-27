use super::*;

// -- parse_doc_clauses tests --

#[test]
fn parse_doc_contracts_single_requires() {
    let lines = vec![(" @requires x > 0".to_string(), 10)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.requires.len(), 1);
    assert_eq!(contract.requires[0].body, "x > 0");
    assert_eq!(contract.requires[0].kind, InlineClauseKind::Requires);
}

#[test]
fn parse_doc_contracts_multiple_clauses() {
    let lines = vec![
        (" @requires x > 0".to_string(), 10),
        (" @requires y > 0".to_string(), 30),
        (" @ensures result > 0".to_string(), 50),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.requires.len(), 2);
    assert_eq!(contract.ensures.len(), 1);
    assert_eq!(contract.ensures[0].body, "result > 0");
}

#[test]
fn parse_doc_contracts_multiline_ensures() {
    let lines = vec![
        (" @ensures".to_string(), 10),
        ("   result > 0 &&".to_string(), 30),
        ("   result < 100".to_string(), 50),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.ensures.len(), 1);
    assert_eq!(contract.ensures[0].body, "result > 0 && result < 100");
}

#[test]
fn parse_doc_contracts_struct_invariant() {
    let lines = vec![
        (" @invariant self.len <= self.capacity".to_string(), 10),
        (
            " @invariant self.capacity <= isize::MAX as usize".to_string(),
            50,
        ),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.invariants.len(), 2);
    assert_eq!(contract.invariants[0].body, "self.len <= self.capacity");
}

#[test]
fn parse_doc_contracts_effects_clause() {
    let lines = vec![(" @effects io.read, net.connect".to_string(), 10)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.effects.len(), 1);
    assert_eq!(contract.effects[0].body, "io.read, net.connect");
}

#[test]
fn parse_doc_contracts_decreases_clause() {
    let lines = vec![(" @decreases n".to_string(), 10)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.decreases.len(), 1);
    assert_eq!(contract.decreases[0].body, "n");
}

#[test]
fn parse_doc_contracts_mixed_with_regular_docs() {
    let lines = vec![
        (" Divides two integers safely.".to_string(), 0),
        ("".to_string(), 30),
        (" @requires divisor != 0".to_string(), 35),
        (" @ensures result == dividend / divisor".to_string(), 60),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.requires.len(), 1);
    assert_eq!(contract.ensures.len(), 1);
    assert_eq!(contract.requires[0].body, "divisor != 0");
}

#[test]
fn parse_doc_contracts_empty_input() {
    let lines: Vec<(String, usize)> = vec![];
    let contract = parse_doc_clauses(&lines);
    assert!(contract.is_empty());
    assert_eq!(contract.clause_count(), 0);
}

#[test]
fn parse_doc_contracts_no_annotations() {
    let lines = vec![
        (" This is a regular doc comment.".to_string(), 0),
        (" It has no contract clauses.".to_string(), 35),
    ];
    let contract = parse_doc_clauses(&lines);
    assert!(contract.is_empty());
}

#[test]
fn parse_doc_contracts_unknown_keyword_ignored() {
    let lines = vec![
        (" @unknown something".to_string(), 0),
        (" @requires x > 0".to_string(), 25),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.requires.len(), 1);
    // Unknown keyword should not produce any clause
    assert_eq!(contract.clause_count(), 1);
}

// -- parse_rust_source tests --

#[test]
fn parse_doc_contracts_from_rust_function() {
    let source = r#"
/// Divides two integers.
///
/// @requires divisor != 0
/// @ensures result == dividend / divisor
fn safe_divide(dividend: i64, divisor: i64) -> i64 {
dividend / divisor
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);

    let item = &items[0];
    assert_eq!(item.contract.requires.len(), 1);
    assert_eq!(item.contract.ensures.len(), 1);
    assert_eq!(item.contract.requires[0].body, "divisor != 0");

    match &item.kind {
        AnnotatedItemKind::Function {
            name,
            params,
            return_type,
            ..
        } => {
            assert_eq!(name, "safe_divide");
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "dividend");
            return_type.as_ref().unwrap();
        }
        _ => panic!("expected Function"),
    }
}

#[test]
fn parse_doc_contracts_struct_invariant_from_source() {
    let source = r#"
/// @invariant self.len <= self.capacity
/// @invariant self.head < self.capacity || self.len == 0
struct RingBuffer {
head: usize,
len: usize,
capacity: usize,
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.invariants.len(), 2);

    match &items[0].kind {
        AnnotatedItemKind::Struct { name, fields } => {
            assert_eq!(name, "RingBuffer");
            assert_eq!(fields.len(), 3);
        }
        _ => panic!("expected Struct"),
    }
}

#[test]
fn parse_doc_contracts_impl_block() {
    let source = r#"
/// @invariant self.balance >= 0
impl BankAccount {
/// @requires amount > 0
/// @ensures self.balance == old(self.balance) + amount
fn deposit(&mut self, amount: u64) {
    self.balance += amount;
}
}
"#;
    let items = parse_rust_source(source).unwrap();
    // Should have 2 items: the impl block invariant and the method
    assert_eq!(items.len(), 2);

    // First: impl block with invariant
    match &items[0].kind {
        AnnotatedItemKind::ImplBlock { self_type, .. } => {
            assert_eq!(self_type, "BankAccount");
        }
        _ => panic!("expected ImplBlock, got {:?}", items[0].kind),
    }
    assert_eq!(items[0].contract.invariants.len(), 1);

    // Second: method with requires/ensures
    match &items[1].kind {
        AnnotatedItemKind::Function { name, .. } => {
            assert_eq!(name, "deposit");
        }
        _ => panic!("expected Function"),
    }
    assert_eq!(items[1].contract.requires.len(), 1);
    assert_eq!(items[1].contract.ensures.len(), 1);
}

#[test]
fn parse_doc_contracts_unannotated_skipped() {
    let source = r#"
/// Regular documentation, no contracts.
fn no_contracts(x: i32) -> i32 {
x + 1
}

/// @requires x > 0
fn annotated(x: i32) -> i32 {
x
}

fn no_docs(y: i32) -> i32 {
y
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0].kind {
        AnnotatedItemKind::Function { name, .. } => assert_eq!(name, "annotated"),
        _ => panic!("expected Function"),
    }
}

#[test]
fn roundtrip_clause_kind() {
    let all_kinds = [
        InlineClauseKind::Requires,
        InlineClauseKind::Ensures,
        InlineClauseKind::Invariant,
        InlineClauseKind::Effects,
        InlineClauseKind::Decreases,
        InlineClauseKind::FfiBoundary,
        InlineClauseKind::Trust,
        InlineClauseKind::Ghost,
        InlineClauseKind::Lemma,
        InlineClauseKind::Modifies,
        InlineClauseKind::Opaque,
        InlineClauseKind::Eventually,
        InlineClauseKind::Taint,
        InlineClauseKind::ConstantTime,
        InlineClauseKind::Zeroize,
        InlineClauseKind::Region,
        InlineClauseKind::Width,
        InlineClauseKind::Allocator,
        InlineClauseKind::Circular,
        InlineClauseKind::Interface,
        InlineClauseKind::Errors,
        InlineClauseKind::Shared,
        InlineClauseKind::NoReentrant,
        InlineClauseKind::Deterministic,
        InlineClauseKind::LockOrder,
        InlineClauseKind::Deadline,
        InlineClauseKind::MemoryOrdering,
        InlineClauseKind::Format,
        InlineClauseKind::Bits,
        InlineClauseKind::Encoding,
        InlineClauseKind::Checksum,
        InlineClauseKind::Platform,
        InlineClauseKind::Feature,
        InlineClauseKind::Resource,
        InlineClauseKind::UnsafeEscape,
        InlineClauseKind::Complexity,
        InlineClauseKind::Precision,
        InlineClauseKind::Monotonic,
        InlineClauseKind::SuspendInvariant,
    ];
    // Verify all 39 variants round-trip
    assert_eq!(all_kinds.len(), 39);
    for kind in all_kinds {
        let s = kind.as_str();
        let parsed = InlineClauseKind::from_keyword(s).unwrap();
        assert_eq!(parsed, kind, "round-trip failed for keyword '{s}'");
    }
}

#[test]
fn edge_cases_malformed_clause() {
    // @ with no keyword after it
    let lines = vec![(" @".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert!(contract.is_empty());
}

#[test]
fn edge_cases_clause_no_body() {
    // @requires with nothing after it and no continuation
    let lines = vec![(" @requires".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    // Empty body should not produce a clause
    assert!(contract.is_empty());
}

#[test]
fn edge_cases_multiline_terminated_by_new_clause() {
    let lines = vec![
        (" @requires".to_string(), 0),
        ("   x > 0 &&".to_string(), 20),
        ("   y > 0".to_string(), 35),
        (" @ensures result > 0".to_string(), 50),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.requires.len(), 1);
    assert_eq!(contract.requires[0].body, "x > 0 && y > 0");
    assert_eq!(contract.ensures.len(), 1);
}

#[test]
fn parse_doc_contracts_async_unsafe_function() {
    let source = r#"
/// @requires buf.len() >= 4
async unsafe fn read_data(buf: &[u8]) -> u32 {
0
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0].kind {
        AnnotatedItemKind::Function {
            name,
            is_unsafe,
            is_async,
            ..
        } => {
            assert_eq!(name, "read_data");
            assert!(is_unsafe);
            assert!(is_async);
        }
        _ => panic!("expected Function"),
    }
}

#[test]
fn inline_contract_clause_count() {
    let mut c = InlineContract::default();
    assert!(c.is_empty());
    assert_eq!(c.clause_count(), 0);

    c.push(ContractClause {
        kind: InlineClauseKind::Requires,
        body: "x > 0".to_string(),
        offset: 0,
    });
    c.push(ContractClause {
        kind: InlineClauseKind::Ensures,
        body: "result > 0".to_string(),
        offset: 10,
    });
    c.push(ContractClause {
        kind: InlineClauseKind::Invariant,
        body: "self.ok".to_string(),
        offset: 20,
    });

    assert!(!c.is_empty());
    assert_eq!(c.clause_count(), 3);
}

// -- dual-source merge tests --

#[test]
fn dual_source_merge_external_only() {
    let external = vec![
        (InlineClauseKind::Requires, "x > 0".to_string()),
        (InlineClauseKind::Ensures, "result > 0".to_string()),
    ];
    let inline = InlineContract::default();
    let merged = merge_contracts(&external, &inline);

    assert_eq!(merged.clause_count(), 2);
    assert_eq!(merged.external_clauses().len(), 2);
    assert_eq!(merged.inline_clauses().len(), 0);
    assert!(merged.warnings.is_empty());
}

#[test]
fn dual_source_merge_inline_only() {
    let external: Vec<(InlineClauseKind, String)> = vec![];
    let mut inline = InlineContract::default();
    inline.push(ContractClause {
        kind: InlineClauseKind::Requires,
        body: "x > 0".to_string(),
        offset: 0,
    });
    let merged = merge_contracts(&external, &inline);

    assert_eq!(merged.clause_count(), 1);
    assert_eq!(merged.external_clauses().len(), 0);
    assert_eq!(merged.inline_clauses().len(), 1);
    assert!(merged.warnings.is_empty());
}

#[test]
fn dual_source_merge_both_sources() {
    let external = vec![(InlineClauseKind::Requires, "x > 0".to_string())];
    let mut inline = InlineContract::default();
    inline.push(ContractClause {
        kind: InlineClauseKind::Ensures,
        body: "result > 0".to_string(),
        offset: 0,
    });
    let merged = merge_contracts(&external, &inline);

    assert_eq!(merged.clause_count(), 2);
    assert_eq!(merged.external_clauses().len(), 1);
    assert_eq!(merged.inline_clauses().len(), 1);
    assert!(merged.warnings.is_empty());
}

#[test]
fn dual_source_merge_duplicate_detection() {
    let external = vec![(InlineClauseKind::Requires, "x > 0".to_string())];
    let mut inline = InlineContract::default();
    inline.push(ContractClause {
        kind: InlineClauseKind::Requires,
        body: "x > 0".to_string(),
        offset: 0,
    });
    let merged = merge_contracts(&external, &inline);

    // Duplicate inline clause should be detected
    assert_eq!(merged.clause_count(), 1); // Only the external one kept
    assert_eq!(merged.warnings.len(), 1);
    assert!(merged.warnings[0].contains("duplicate"));
}

#[test]
fn dual_source_merge_whitespace_normalization() {
    let external = vec![(InlineClauseKind::Requires, "x  >  0".to_string())];
    let mut inline = InlineContract::default();
    inline.push(ContractClause {
        kind: InlineClauseKind::Requires,
        body: "x > 0".to_string(),
        offset: 0,
    });
    let merged = merge_contracts(&external, &inline);

    // Should detect as duplicate even with different whitespace
    assert_eq!(merged.clause_count(), 1);
    assert_eq!(merged.warnings.len(), 1);
}

#[test]
fn dual_source_merge_different_kinds_not_duplicate() {
    let external = vec![(InlineClauseKind::Requires, "x > 0".to_string())];
    let mut inline = InlineContract::default();
    inline.push(ContractClause {
        kind: InlineClauseKind::Ensures, // Different kind
        body: "x > 0".to_string(),       // Same body
        offset: 0,
    });
    let merged = merge_contracts(&external, &inline);

    // Different clause kinds should not be considered duplicates
    assert_eq!(merged.clause_count(), 2);
    assert!(merged.warnings.is_empty());
}

#[test]
fn dual_source_merge_empty_contract() {
    let merged = MergedContract::default();
    assert!(merged.is_empty());
    assert_eq!(merged.clause_count(), 0);
}

// -- language adapter tests --

#[test]
fn language_adapter_rust_id() {
    let adapter = RustAdapter;
    assert_eq!(adapter.language_id(), "rust");
    assert_eq!(adapter.file_extensions(), &["rs"]);
}

#[test]
fn language_adapter_rust_type_mapping() {
    let adapter = RustAdapter;
    assert_eq!(adapter.map_type("i32"), Some("Int".to_string()));
    assert_eq!(adapter.map_type("u64"), Some("Nat".to_string()));
    assert_eq!(adapter.map_type("f64"), Some("Float".to_string()));
    assert_eq!(adapter.map_type("bool"), Some("Bool".to_string()));
    assert_eq!(adapter.map_type("String"), Some("String".to_string()));
    assert_eq!(adapter.map_type("()"), Some("Unit".to_string()));
    assert_eq!(adapter.map_type("CustomType"), None);
}

#[test]
fn language_adapter_python_id() {
    let adapter = PythonAdapter;
    assert_eq!(adapter.language_id(), "python");
    assert_eq!(adapter.file_extensions(), &["py"]);
}

#[test]
fn language_adapter_python_type_mapping() {
    let adapter = PythonAdapter;
    assert_eq!(adapter.map_type("int"), Some("Int".to_string()));
    assert_eq!(adapter.map_type("float"), Some("Float".to_string()));
    assert_eq!(adapter.map_type("bool"), Some("Bool".to_string()));
    assert_eq!(adapter.map_type("str"), Some("String".to_string()));
    assert_eq!(adapter.map_type("bytes"), Some("Bytes".to_string()));
    assert_eq!(adapter.map_type("None"), Some("Unit".to_string()));
    assert_eq!(adapter.map_type("list"), Some("List".to_string()));
    assert_eq!(adapter.map_type("dict"), Some("Map".to_string()));
    assert_eq!(adapter.map_type("CustomClass"), None);
}

// -- python adapter parsing tests --

#[test]
fn python_adapter_function_with_requires() {
    let source = r#"
# @requires x > 0
def double(x: int) -> int:
return x * 2
"#;
    let adapter = PythonAdapter;
    let items = adapter.parse_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.requires.len(), 1);
    assert_eq!(items[0].contract.requires[0].body, "x > 0");
    match &items[0].kind {
        AnnotatedItemKind::Function {
            name, return_type, ..
        } => {
            assert_eq!(name, "double");
            assert_eq!(return_type.as_deref(), Some("int"));
        }
        _ => panic!("expected Function"),
    }
}

#[test]
fn python_adapter_class_with_invariant() {
    let source = r#"
# @invariant self.count >= 0
class Counter:
pass
"#;
    let adapter = PythonAdapter;
    let items = adapter.parse_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.invariants.len(), 1);
    assert_eq!(items[0].contract.invariants[0].body, "self.count >= 0");
    match &items[0].kind {
        AnnotatedItemKind::Struct { name, .. } => {
            assert_eq!(name, "Counter");
        }
        _ => panic!("expected Struct"),
    }
}

#[test]
fn python_adapter_async_function() {
    let source = r#"
# @requires timeout > 0
# @ensures result != None
async def fetch_data(url: str, timeout: int) -> str:
pass
"#;
    let adapter = PythonAdapter;
    let items = adapter.parse_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.requires.len(), 1);
    assert_eq!(items[0].contract.ensures.len(), 1);
    match &items[0].kind {
        AnnotatedItemKind::Function {
            name,
            is_async,
            params,
            ..
        } => {
            assert_eq!(name, "fetch_data");
            assert!(is_async);
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].name, "url");
            assert_eq!(params[0].ty, "str");
        }
        _ => panic!("expected Function"),
    }
}

#[test]
fn python_adapter_no_annotations() {
    let source = r#"
def plain_function(x):
return x + 1

class PlainClass:
pass
"#;
    let adapter = PythonAdapter;
    let items = adapter.parse_source(source).unwrap();
    assert!(items.is_empty());
}

#[test]
fn python_adapter_multiple_functions() {
    let source = r#"
# @requires a > 0
def first(a: int) -> int:
return a

# @ensures result >= 0
def second(b: int) -> int:
return abs(b)
"#;
    let adapter = PythonAdapter;
    let items = adapter.parse_source(source).unwrap();
    assert_eq!(items.len(), 2);
}

#[test]
fn python_adapter_function_no_type_hints() {
    let source = r#"
# @requires x > 0
def untyped(x):
return x * 2
"#;
    let adapter = PythonAdapter;
    let items = adapter.parse_source(source).unwrap();
    assert_eq!(items.len(), 1);
    match &items[0].kind {
        AnnotatedItemKind::Function {
            params,
            return_type,
            ..
        } => {
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].ty, "Any");
            assert!(return_type.is_none());
        }
        _ => panic!("expected Function"),
    }
}

#[test]
fn adapter_for_extension_rust() {
    let adapter = adapter_for_extension("rs").unwrap();
    assert_eq!(adapter.language_id(), "rust");
}

#[test]
fn adapter_for_extension_python() {
    let adapter = adapter_for_extension("py").unwrap();
    assert_eq!(adapter.language_id(), "python");
}

#[test]
fn adapter_for_extension_unknown() {
    assert!(adapter_for_extension("java").is_none());
    assert!(adapter_for_extension("go").is_none());
}

// -- SEC.2 FFI boundary inline annotation tests --

#[test]
fn ffi_boundary_clause_parsed() {
    let lines = vec![(" @ffi_boundary untrusted".to_string(), 10)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.ffi_boundary.len(), 1);
    assert_eq!(contract.ffi_boundary[0].body, "untrusted");
    assert_eq!(contract.ffi_boundary[0].kind, InlineClauseKind::FfiBoundary);
}

#[test]
fn trust_clause_parsed() {
    let lines = vec![(" @trust audited".to_string(), 10)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.ffi_boundary.len(), 1);
    assert_eq!(contract.ffi_boundary[0].body, "audited");
    assert_eq!(contract.ffi_boundary[0].kind, InlineClauseKind::Trust);
}

#[test]
fn ffi_boundary_with_requires_ensures() {
    let lines = vec![
        (" @ffi_boundary untrusted".to_string(), 10),
        (" @requires buf.len() >= 4".to_string(), 40),
        (" @ensures result >= 0".to_string(), 70),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.ffi_boundary.len(), 1);
    assert_eq!(contract.requires.len(), 1);
    assert_eq!(contract.ensures.len(), 1);
}

#[test]
fn ffi_boundary_on_unsafe_extern_fn() {
    let source = r#"
/// @ffi_boundary untrusted
/// @requires size > 0
/// @ensures result != 0
unsafe fn malloc(size: usize) -> *mut u8 {
std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(size, 1))
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.ffi_boundary.len(), 1);
    assert_eq!(items[0].contract.ffi_boundary[0].body, "untrusted");
    assert_eq!(items[0].contract.requires.len(), 1);
    assert_eq!(items[0].contract.ensures.len(), 1);
    match &items[0].kind {
        AnnotatedItemKind::Function {
            name, is_unsafe, ..
        } => {
            assert_eq!(name, "malloc");
            assert!(is_unsafe);
        }
        _ => panic!("expected Function"),
    }
}

#[test]
fn ffi_boundary_roundtrip_clause_kind() {
    for kind in [InlineClauseKind::FfiBoundary, InlineClauseKind::Trust] {
        let s = kind.as_str();
        let parsed = InlineClauseKind::from_keyword(s).unwrap();
        assert_eq!(parsed, kind);
    }
}

// -- Feature annotation tests (Batches 1A-1D) --

#[test]
fn annotation_ghost_parsed() {
    let lines = vec![(" @ghost helper_var".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Ghost);
    assert_eq!(contract.annotations[0].body, "helper_var");
}

#[test]
fn annotation_lemma_parsed() {
    let lines = vec![(" @lemma sum_positive".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Lemma);
    assert_eq!(contract.annotations[0].body, "sum_positive");
}

#[test]
fn annotation_modifies_parsed() {
    let lines = vec![(" @modifies self.buffer, self.len".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Modifies);
    assert_eq!(contract.annotations[0].body, "self.buffer, self.len");
}

#[test]
fn annotation_opaque_parsed() {
    let lines = vec![(" @opaque".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    // @opaque with no body produces no clause (empty body is skipped)
    assert_eq!(contract.annotations.len(), 0);
}

#[test]
fn annotation_opaque_with_reason() {
    let lines = vec![(" @opaque implementation_detail".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Opaque);
    assert_eq!(contract.annotations[0].body, "implementation_detail");
}

#[test]
fn annotation_eventually_parsed() {
    let lines = vec![(" @eventually lock_released".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Eventually);
}

#[test]
fn annotation_taint_parsed() {
    let lines = vec![(" @taint source=user_input, sink=sql_query".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Taint);
    assert!(contract.annotations[0].body.contains("user_input"));
}

#[test]
fn annotation_constant_time_parsed() {
    let lines = vec![(" @constant_time".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 0); // no body
}

#[test]
fn annotation_constant_time_with_note() {
    let lines = vec![(" @constant_time crypto_compare".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::ConstantTime);
}

#[test]
fn annotation_zeroize_parsed() {
    let lines = vec![(" @zeroize on_drop".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Zeroize);
    assert_eq!(contract.annotations[0].body, "on_drop");
}

#[test]
fn annotation_region_parsed() {
    let lines = vec![(" @region stack".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Region);
}

#[test]
fn annotation_width_parsed() {
    let lines = vec![(" @width 32".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Width);
    assert_eq!(contract.annotations[0].body, "32");
}

#[test]
fn annotation_allocator_parsed() {
    let lines = vec![(" @allocator bump".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Allocator);
}

#[test]
fn annotation_circular_parsed() {
    let lines = vec![(" @circular capacity=1024".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Circular);
}

#[test]
fn annotation_interface_parsed() {
    let lines = vec![(" @interface Readable".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Interface);
}

#[test]
fn annotation_errors_parsed() {
    let lines = vec![(" @errors IoError, ParseError".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Errors);
    assert_eq!(contract.annotations[0].body, "IoError, ParseError");
}

#[test]
fn annotation_shared_parsed() {
    let lines = vec![(" @shared mutex".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Shared);
}

#[test]
fn annotation_no_reentrant_parsed() {
    let lines = vec![(" @no_reentrant callback".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::NoReentrant);
}

#[test]
fn annotation_deterministic_parsed() {
    let lines = vec![(" @deterministic pure_computation".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(
        contract.annotations[0].kind,
        InlineClauseKind::Deterministic
    );
}

#[test]
fn annotation_lock_order_parsed() {
    let lines = vec![(" @lock_order A < B < C".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::LockOrder);
    assert_eq!(contract.annotations[0].body, "A < B < C");
}

#[test]
fn annotation_deadline_parsed() {
    let lines = vec![(" @deadline 100ms".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Deadline);
    assert_eq!(contract.annotations[0].body, "100ms");
}

#[test]
fn annotation_ordering_parsed() {
    let lines = vec![(" @ordering seq_cst".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(
        contract.annotations[0].kind,
        InlineClauseKind::MemoryOrdering
    );
}

#[test]
fn annotation_format_parsed() {
    let lines = vec![(" @format big_endian, packed".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Format);
}

#[test]
fn annotation_bits_parsed() {
    let lines = vec![(" @bits flags[0..3], tag[4..7]".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Bits);
}

#[test]
fn annotation_encoding_parsed() {
    let lines = vec![(" @encoding utf8".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Encoding);
}

#[test]
fn annotation_checksum_parsed() {
    let lines = vec![(" @checksum crc32 0..1024".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Checksum);
}

#[test]
fn annotation_platform_parsed() {
    let lines = vec![(" @platform linux, macos".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Platform);
}

#[test]
fn annotation_feature_parsed() {
    let lines = vec![(" @feature simd".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Feature);
}

#[test]
fn annotation_resource_parsed() {
    let lines = vec![(" @resource max_memory=1GB, max_fds=1024".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Resource);
}

#[test]
fn annotation_unsafe_escape_parsed() {
    let lines = vec![(
        " @unsafe_escape reason=\"performance critical inner loop\"".to_string(),
        0,
    )];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::UnsafeEscape);
}

#[test]
fn annotation_complexity_parsed() {
    let lines = vec![(" @complexity O(n log n)".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Complexity);
    assert_eq!(contract.annotations[0].body, "O(n log n)");
}

#[test]
fn annotation_precision_parsed() {
    let lines = vec![(" @precision epsilon=1e-6".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Precision);
}

#[test]
fn annotation_monotonic_parsed() {
    let lines = vec![(" @monotonic counter".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(contract.annotations[0].kind, InlineClauseKind::Monotonic);
}

#[test]
fn annotation_suspend_invariant_parsed() {
    let lines = vec![(" @suspend_invariant during_resize".to_string(), 0)];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.annotations.len(), 1);
    assert_eq!(
        contract.annotations[0].kind,
        InlineClauseKind::SuspendInvariant
    );
}

#[test]
fn annotation_mixed_with_core_clauses() {
    let lines = vec![
        (" @ghost helper".to_string(), 0),
        (" @requires x > 0".to_string(), 20),
        (" @taint source=network".to_string(), 40),
        (" @ensures result > 0".to_string(), 60),
        (" @complexity O(1)".to_string(), 80),
    ];
    let contract = parse_doc_clauses(&lines);
    assert_eq!(contract.requires.len(), 1);
    assert_eq!(contract.ensures.len(), 1);
    assert_eq!(contract.annotations.len(), 3);
    assert_eq!(contract.clause_count(), 5);
}

#[test]
fn annotation_on_rust_function() {
    let source = r#"
/// @taint source=user_input
/// @constant_time crypto
/// @requires buf.len() >= 32
fn verify_hmac(buf: &[u8]) -> bool {
true
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.annotations.len(), 2);
    assert_eq!(items[0].contract.requires.len(), 1);
    let taint_annotations = items[0].contract.annotations_of(InlineClauseKind::Taint);
    assert_eq!(taint_annotations.len(), 1);
    let ct_annotations = items[0]
        .contract
        .annotations_of(InlineClauseKind::ConstantTime);
    assert_eq!(ct_annotations.len(), 1);
}

#[test]
fn annotation_is_annotation_vs_core() {
    assert!(!InlineClauseKind::Requires.is_annotation());
    assert!(!InlineClauseKind::Ensures.is_annotation());
    assert!(!InlineClauseKind::FfiBoundary.is_annotation());
    assert!(!InlineClauseKind::Trust.is_annotation());
    assert!(InlineClauseKind::Ghost.is_annotation());
    assert!(InlineClauseKind::Taint.is_annotation());
    assert!(InlineClauseKind::Complexity.is_annotation());
    assert!(InlineClauseKind::Platform.is_annotation());
}

#[test]
fn annotation_annotations_of_filters_correctly() {
    let mut c = InlineContract::default();
    c.push(ContractClause {
        kind: InlineClauseKind::Ghost,
        body: "x".to_string(),
        offset: 0,
    });
    c.push(ContractClause {
        kind: InlineClauseKind::Taint,
        body: "source=net".to_string(),
        offset: 10,
    });
    c.push(ContractClause {
        kind: InlineClauseKind::Ghost,
        body: "y".to_string(),
        offset: 20,
    });
    assert_eq!(c.annotations_of(InlineClauseKind::Ghost).len(), 2);
    assert_eq!(c.annotations_of(InlineClauseKind::Taint).len(), 1);
    assert_eq!(c.annotations_of(InlineClauseKind::Region).len(), 0);
}

#[test]
fn annotation_merge_includes_annotations() {
    let external = vec![(InlineClauseKind::Requires, "x > 0".to_string())];
    let mut inline = InlineContract::default();
    inline.push(ContractClause {
        kind: InlineClauseKind::Ghost,
        body: "helper".to_string(),
        offset: 0,
    });
    inline.push(ContractClause {
        kind: InlineClauseKind::Taint,
        body: "source=net".to_string(),
        offset: 10,
    });
    let merged = merge_contracts(&external, &inline);
    assert_eq!(merged.clause_count(), 3);
    assert_eq!(merged.external_clauses().len(), 1);
    assert_eq!(merged.inline_clauses().len(), 2);
}

// -- Attribute extraction tests (#659) --

#[test]
fn parse_requires_attribute_syntax() {
    let source = r#"
#[requires(x > 0)]
fn positive(x: i32) -> i32 {
    x
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.requires.len(), 1);
    assert_eq!(items[0].contract.requires[0].body, "x > 0");
}

#[test]
fn parse_ensures_attribute_syntax() {
    let source = r#"
#[ensures(result >= 0)]
fn absolute(x: i32) -> i32 {
    if x < 0 { -x } else { x }
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.ensures.len(), 1);
    assert_eq!(items[0].contract.ensures[0].body, "result >= 0");
}

#[test]
fn parse_invariant_attribute_syntax() {
    let source = r#"
#[invariant(self.len <= self.capacity)]
fn push(x: i32) -> i32 {
    x
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.invariants.len(), 1);
    assert_eq!(
        items[0].contract.invariants[0].body,
        "self . len <= self . capacity"
    );
}

#[test]
fn parse_mixed_doc_and_attr_clauses() {
    let source = r#"
/// @requires y > 0
#[requires(x > 0)]
#[ensures(result > 0)]
fn multiply(x: i32, y: i32) -> i32 {
    x * y
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    // One from doc comment, one from attribute
    assert_eq!(items[0].contract.requires.len(), 2);
    assert_eq!(items[0].contract.ensures.len(), 1);
}

#[test]
fn parse_multiple_requires_attributes() {
    let source = r#"
#[requires(a > 0)]
#[requires(b > 0)]
#[ensures(result > 0)]
fn add_positive(a: i32, b: i32) -> i32 {
    a + b
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.requires.len(), 2);
    assert_eq!(items[0].contract.ensures.len(), 1);
}

#[test]
fn parse_attr_clauses_on_method() {
    let source = r#"
struct Counter { value: i32 }

impl Counter {
    #[requires(self.value < i32::MAX)]
    #[ensures(result > 0)]
    fn increment(&mut self) -> i32 {
        self.value += 1;
        self.value
    }
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.requires.len(), 1);
    assert_eq!(items[0].contract.ensures.len(), 1);
}

#[test]
fn parse_no_attr_clauses_passes_through() {
    let source = r#"
fn plain(x: i32) -> i32 {
    x + 1
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert!(items.is_empty());
}

#[test]
fn parse_contradictory_requires_attributes() {
    // check-rust should extract these for Z3 to find contradiction
    let source = r#"
#[requires(x > 10)]
#[requires(x < 5)]
fn impossible(x: i32) -> i32 {
    x
}
"#;
    let items = parse_rust_source(source).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].contract.requires.len(), 2);
    assert_eq!(items[0].contract.requires[0].body, "x > 10");
    assert_eq!(items[0].contract.requires[1].body, "x < 5");
}
