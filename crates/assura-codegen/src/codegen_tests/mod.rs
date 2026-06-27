use super::*;

/// Helper: parse + resolve + type-check via shared test support, then local codegen.
///
/// Uses `typecheck_ok` (not `codegen_ok`) so `GeneratedProject` is always
/// produced by *this* crate instance (avoiding duplicate assura-codegen types
/// when assura-test-support also depends on assura-codegen).
fn codegen_ok(source: &str) -> GeneratedProject {
    let typed = assura_test_support::typecheck_ok(source);
    codegen(&typed)
}

mod basic;
mod contract;
mod ghost_lemma;
mod project;
mod remaining;
mod safe_division;
mod struct_enum;
mod type_mapping;
