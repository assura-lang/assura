use super::*;

/// Parse + resolve + type-check using published workspace deps only.
fn typecheck_ok(source: &str) -> assura_types::TypedFile {
    let file = assura_parser::parse_unwrap(source);
    let resolved = assura_resolve::resolve(&file).expect("resolve should succeed");
    assura_types::type_check(resolved).expect("type check should succeed")
}

/// Type-check then run local codegen.
fn codegen_ok(source: &str) -> GeneratedProject {
    let typed = typecheck_ok(source);
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
