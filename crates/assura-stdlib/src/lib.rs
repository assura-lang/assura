//! Assura standard library contracts and prelude types.
//!
//! This crate provides:
//! - Standard library `.assura` contract files (math, string, collections)
//! - The prelude: types and contracts auto-imported into every Assura file
//! - A stable API for querying stdlib definitions

use assura_parser::ast::{Decl, SourceFile};

/// Embedded standard library contract source files.
pub struct StdlibSources;

impl StdlibSources {
    /// Return the math module source.
    pub fn math() -> &'static str {
        include_str!("../std/math.assura")
    }

    /// Return the string module source.
    pub fn string() -> &'static str {
        include_str!("../std/string.assura")
    }

    /// Return the collections module source.
    pub fn collections() -> &'static str {
        include_str!("../std/collections.assura")
    }

    /// Return all standard library sources with their module names.
    pub fn all() -> Vec<(&'static str, &'static str)> {
        vec![
            ("std.math", Self::math()),
            ("std.string", Self::string()),
            ("std.collections", Self::collections()),
        ]
    }
}

/// A parsed standard library contract.
#[derive(Debug, Clone)]
pub struct StdlibContract {
    /// The contract name (e.g., "abs", "min", "list_get").
    pub name: String,
    /// The module it belongs to (e.g., "std.math").
    pub module: String,
}

/// Parse all standard library modules and extract contract names.
pub fn stdlib_contracts() -> Vec<StdlibContract> {
    let mut contracts = Vec::new();
    for (module_name, source) in StdlibSources::all() {
        let (parsed, _errors) = assura_parser::parse(source);
        if let Some(file) = parsed {
            for decl in &file.decls {
                if let Decl::Contract(c) = &decl.node {
                    contracts.push(StdlibContract {
                        name: c.name.clone(),
                        module: module_name.to_string(),
                    });
                }
            }
        }
    }
    contracts
}

/// Parse all standard library modules and return their ASTs.
pub fn parse_stdlib() -> Vec<(String, SourceFile)> {
    let mut modules = Vec::new();
    for (module_name, source) in StdlibSources::all() {
        let (parsed, _errors) = assura_parser::parse(source);
        if let Some(file) = parsed {
            modules.push((module_name.to_string(), file));
        }
    }
    modules
}

/// Prelude type names that are available without explicit import.
///
/// These types are injected into the type environment at the start
/// of type checking, so users can write `List<Int>` without
/// `import std.collections`.
pub fn prelude_type_names() -> Vec<&'static str> {
    vec![
        "Int",
        "Nat",
        "Float",
        "Bool",
        "String",
        "Bytes",
        "Unit",
        "List",
        "Map",
        "Set",
        "Option",
        "Result",
        // Refinement types from the existing stdlib
        "Pos",
        "NonNeg",
        "Email",
        "Uuid",
        "Port",
        "Percentage",
    ]
}

/// Prelude contract names that are available without explicit import.
pub fn prelude_contract_names() -> Vec<&'static str> {
    vec!["abs", "min", "max", "clamp"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_source_parses() {
        let source = StdlibSources::math();
        let (parsed, errors) = assura_parser::parse(source);
        assert!(errors.is_empty(), "math.assura parse errors: {errors:?}");
        let file = parsed.expect("math.assura should parse");
        assert!(file.module.is_some(), "should have module declaration");
        assert!(!file.decls.is_empty(), "should have declarations");
    }

    #[test]
    fn string_source_parses() {
        let source = StdlibSources::string();
        let (parsed, errors) = assura_parser::parse(source);
        assert!(errors.is_empty(), "string.assura parse errors: {errors:?}");
        let file = parsed.expect("string.assura should parse");
        assert!(file.module.is_some());
        assert!(!file.decls.is_empty());
    }

    #[test]
    fn collections_source_parses() {
        let source = StdlibSources::collections();
        let (parsed, errors) = assura_parser::parse(source);
        assert!(
            errors.is_empty(),
            "collections.assura parse errors: {errors:?}"
        );
        let file = parsed.expect("collections.assura should parse");
        assert!(file.module.is_some());
        assert!(!file.decls.is_empty());
    }

    #[test]
    fn stdlib_has_at_least_ten_contracts() {
        let contracts = stdlib_contracts();
        assert!(
            contracts.len() >= 10,
            "stdlib should have at least 10 contracts, got {}",
            contracts.len()
        );
    }

    #[test]
    fn stdlib_contracts_have_expected_names() {
        let contracts = stdlib_contracts();
        let names: Vec<&str> = contracts.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"abs"), "missing abs");
        assert!(names.contains(&"min"), "missing min");
        assert!(names.contains(&"max"), "missing max");
        assert!(names.contains(&"clamp"), "missing clamp");
        assert!(names.contains(&"string_length"), "missing string_length");
        assert!(names.contains(&"contains"), "missing contains");
        assert!(names.contains(&"list_get"), "missing list_get");
        assert!(names.contains(&"list_push"), "missing list_push");
        assert!(names.contains(&"map_get"), "missing map_get");
        assert!(names.contains(&"set_contains"), "missing set_contains");
    }

    #[test]
    fn stdlib_contracts_have_modules() {
        let contracts = stdlib_contracts();
        for c in &contracts {
            assert!(
                c.module.starts_with("std."),
                "contract {} should be in std.* module, got {}",
                c.name,
                c.module
            );
        }
    }

    #[test]
    fn parse_stdlib_returns_all_modules() {
        let modules = parse_stdlib();
        assert_eq!(modules.len(), 3, "should have 3 stdlib modules");
        let names: Vec<&str> = modules.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"std.math"));
        assert!(names.contains(&"std.string"));
        assert!(names.contains(&"std.collections"));
    }

    #[test]
    fn prelude_includes_core_types() {
        let types = prelude_type_names();
        assert!(types.contains(&"Int"));
        assert!(types.contains(&"Bool"));
        assert!(types.contains(&"List"));
        assert!(types.contains(&"Map"));
        assert!(types.contains(&"Set"));
        assert!(types.contains(&"Nat"));
    }

    #[test]
    fn prelude_includes_refinement_types() {
        let types = prelude_type_names();
        assert!(types.contains(&"Pos"));
        assert!(types.contains(&"NonNeg"));
        assert!(types.contains(&"Port"));
    }

    #[test]
    fn all_sources_returns_three_modules() {
        let all = StdlibSources::all();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn math_has_four_contracts() {
        let (parsed, _) = assura_parser::parse(StdlibSources::math());
        let file = parsed.unwrap();
        let count = file
            .decls
            .iter()
            .filter(|d| matches!(&d.node, Decl::Contract(_)))
            .count();
        assert_eq!(
            count, 4,
            "math should have 4 contracts (abs, min, max, clamp)"
        );
    }

    #[test]
    fn string_has_three_contracts() {
        let (parsed, _) = assura_parser::parse(StdlibSources::string());
        let file = parsed.unwrap();
        let count = file
            .decls
            .iter()
            .filter(|d| matches!(&d.node, Decl::Contract(_)))
            .count();
        assert_eq!(
            count, 3,
            "string should have 3 contracts (string_length, substring, contains)"
        );
    }

    #[test]
    fn collections_has_six_contracts() {
        let (parsed, _) = assura_parser::parse(StdlibSources::collections());
        let file = parsed.unwrap();
        let count = file
            .decls
            .iter()
            .filter(|d| matches!(&d.node, Decl::Contract(_)))
            .count();
        assert_eq!(count, 6, "collections should have 6 contracts");
    }
}
