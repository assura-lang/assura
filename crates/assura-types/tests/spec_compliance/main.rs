//! Spec compliance test suite: Section 13 type interaction cases.
//!
//! Each module corresponds to a test case from Section 13 of SPECIFICATION.md.
//! These validate that the compiler handles pairwise and multi-way feature
//! interactions correctly (parse, resolve, type-check).

mod tc01_refinement_linear;
mod tc02_refinement_typestate;
mod tc03_refinement_dependent;
mod tc04_linear_effect;
mod tc05_typestate_info_flow;
mod tc06_dependent_effect;
mod tc07_linear_info_flow;
mod tc08_three_way;
mod tc09_full_stack;
mod tc10_conditional_typestate;
mod tc11_effect_info_flow;

/// Parse source, resolve, and type-check. Returns Ok(()) on success or
/// the list of type error codes on failure.
pub fn pipeline(source: &str) -> Result<(), Vec<String>> {
    let (ast, parse_errs) = assura_parser::parse(source);
    if !parse_errs.is_empty() {
        return Err(parse_errs
            .iter()
            .map(|e| format!("PARSE: {}", e.message))
            .collect());
    }
    let ast = ast.expect("parse returned None without errors");
    let resolved = assura_resolve::resolve(&ast).map_err(|errs| {
        errs.iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
    })?;
    assura_types::type_check(&resolved).map_err(|errs| {
        errs.iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
    })?;
    Ok(())
}

/// Assert that source compiles (parse + resolve + typecheck succeed).
pub fn must_compile(source: &str) {
    if let Err(errs) = pipeline(source) {
        panic!(
            "Expected compilation to succeed, got errors:\n{}",
            errs.join("\n")
        );
    }
}

/// Assert that source fails with at least one error containing the given code.
pub fn must_reject(source: &str, expected_code: &str) {
    match pipeline(source) {
        Ok(()) => panic!(
            "Expected compilation to fail with {}, but it succeeded",
            expected_code
        ),
        Err(errs) => {
            let has_code = errs.iter().any(|e| e.contains(expected_code));
            if !has_code {
                panic!(
                    "Expected error code {}, got:\n{}",
                    expected_code,
                    errs.join("\n")
                );
            }
        }
    }
}
