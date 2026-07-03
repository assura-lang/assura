//! In-crate test helpers (no assura-test-support / pipeline path deps).
#![cfg(test)]

use std::path::{Path, PathBuf};

use assura_types::TypedFile;

pub fn parse_ok(source: &str) -> assura_parser::ast::SourceFile {
    assura_parser::parse_unwrap(source)
}

pub fn typecheck_ok(source: &str) -> TypedFile {
    let file = parse_ok(source);
    let resolved = assura_resolve::resolve(&file).expect("resolve should succeed");
    assura_types::type_check(resolved).expect("type check should succeed")
}

pub fn fixture_path(relative: impl AsRef<Path>) -> PathBuf {
    let relative = relative.as_ref();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // assura-smt is crates/assura-smt; fixtures live at workspace root.
    let root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root");
    root.join(relative)
}

pub fn load_fixture(relative: impl AsRef<Path>) -> String {
    let path = fixture_path(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}
