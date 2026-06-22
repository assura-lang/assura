//! SMT-based verification for Assura contracts.
//!
//! Supports multiple solver backends:
//! - **Z3** (default): via the z3 Rust crate, compiled in with the `z3-verify` feature
//! - **CVC5**: via the `cvc5` command-line binary, using SMT-LIB2 format
//! - **Portfolio**: tries Z3 first, falls back to CVC5 on timeout/unknown
//!
//! For each contract in a `TypedFile`, encodes requires/ensures/invariant
//! clauses as SMT formulas and checks their validity:
//!
//! - **ensures with requires**: Check `P => Q` validity by asserting P,
//!   asserting NOT Q, and checking satisfiability. UNSAT = verified.
//! - **invariant**: Check satisfiability (not always false).
//! - **requires**: Recorded as assumptions (checked at call sites).
//!
//! The default timeout is 1 second (Layer 1).

#![cfg_attr(feature = "cvc5-verify", allow(dead_code, unused_imports))]

// ---------------------------------------------------------------------------
// Solver backend selection (defined in assura-config, re-exported here)
// ---------------------------------------------------------------------------

pub use assura_config::SolverChoice;

// Re-export parser/types that sub-modules import via `use super::*;`
#[cfg(any(feature = "z3-verify", test))]
pub(crate) use assura_ast::Expr;
#[cfg(feature = "z3-verify")]
pub(crate) use assura_ast::ServiceItem;
#[cfg(test)]
pub(crate) use assura_ast::Spanned;
pub(crate) use assura_ast::{ClauseKind, Decl};
pub(crate) use assura_types::TypedFile;

// ---------------------------------------------------------------------------
// Extracted modules
// ---------------------------------------------------------------------------

/// Measure definitions (T054): sorts, axioms, and built-in measures.
pub mod measures;
pub use measures::{
    MeasureAxiom, MeasureAxiomTag, MeasureDefinition, MeasureSort, register_builtin_measures,
};

/// Verification result types: `VerificationResult` and `CounterexampleModel`.
pub mod result;
pub use result::{CounterexampleModel, VerificationResult, VerificationSummary};

/// Shared IR expression encoding helpers.
mod ir_encode;
mod ir_generate;
mod ir_lower;
#[cfg(test)]
mod ir_parity;
mod ir_templates;
mod ir_type_ctx;
mod verify_context;

/// Public entry point functions for SMT verification.
mod entry;
mod ir_loader;
pub use entry::{
    EvolutionResult, Verifier, VerifyFileExtras, check_refinement_subtype,
    check_refinement_subtype_with_context, has_verifiable_clauses, verify, verify_buffer_bounds,
    verify_contract, verify_contract_with_solver, verify_decrease, verify_evolution,
    verify_file_evolution, verify_region_containment, verify_taint_safety, verify_with_measures,
};
pub use feature_max::{collect_feature_max_constants, derive_narrowings};
pub use ir_generate::{EnsuresShape, classify_ensures_shape, generate_ir_sidecar_text};
pub use ir_loader::{
    LoadedVerifyExtras, collect_verification_job_names, ir_search_dirs_for_source,
    load_ir_bodies_for_contracts, load_ir_bodies_for_typed, stub_ir_sidecars_for_typed,
};
pub use ir_templates::{
    IrPromptContext, IrPromptPattern, ir_prompt_contexts_for_typed, render_ir_prompt,
    resolve_ir_pattern, suggest_ir_pattern,
};
pub use verify_context::{ContractVerifyContext, LoadedIrContext};

/// SMT-LIB2 dump and quantifier bound validation.
pub mod smt_dump;
pub use smt_dump::{
    SmtQuery, UnboundedQuantifierWarning, dump_smt_queries, validate_quantifier_bounds,
};

// ---------------------------------------------------------------------------
// Display and formatting
// ---------------------------------------------------------------------------

/// Human-readable display formatting for verification results.
pub mod display;

// ---------------------------------------------------------------------------
// No-Z3 fallback
// ---------------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
mod no_z3;

// ---------------------------------------------------------------------------
// CVC5 backend (organized in cvc5_backend/ subdirectory)
// ---------------------------------------------------------------------------

pub(crate) mod cvc5_backend;

// Re-export CVC5 sub-modules at crate root so existing `use crate::cvc5_*`
// paths throughout the crate continue to resolve without changes.
pub(crate) use cvc5_backend::cvc5_adt;
pub(crate) use cvc5_backend::cvc5_atom_encode;
pub(crate) use cvc5_backend::cvc5_binop_encode;
pub(crate) use cvc5_backend::cvc5_builtins;
pub(crate) use cvc5_backend::cvc5_call_encode;
pub(crate) use cvc5_backend::cvc5_collect;
pub(crate) use cvc5_backend::cvc5_common;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_encoder_state;
pub(crate) use cvc5_backend::cvc5_expr_smtlib;
pub(crate) use cvc5_backend::cvc5_field_access;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_havoc_assume_smtlib;
pub(crate) use cvc5_backend::cvc5_if_encode;
pub(crate) use cvc5_backend::cvc5_index_access;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_ir_native;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_ir_smtlib;
pub(crate) use cvc5_backend::cvc5_let_block_encode;
pub(crate) use cvc5_backend::cvc5_list_encode;
pub(crate) use cvc5_backend::cvc5_match_encode;
pub(crate) use cvc5_backend::cvc5_model;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_native_binops;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_native_builtins;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_native_encoder;
pub(crate) use cvc5_backend::cvc5_old_access;
pub(crate) use cvc5_backend::cvc5_quantifier_encode;
pub(crate) use cvc5_backend::cvc5_raw_encode;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_raw_native;
pub(crate) use cvc5_backend::cvc5_raw_ops;
pub(crate) use cvc5_backend::cvc5_raw_smtlib;
pub(crate) use cvc5_backend::cvc5_tuple_encode;
#[allow(unused_imports)]
pub(crate) use cvc5_backend::cvc5_verify_dispatch;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_checks;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_clause;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_contract;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_features;
#[cfg(feature = "cvc5-verify")]
pub(crate) use cvc5_backend::cvc5_verify_native_solver;
pub(crate) use cvc5_backend::cvc5_verify_shared;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_clause;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_contract;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_runner;
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) use cvc5_backend::cvc5_verify_shell_script;
pub(crate) use cvc5_backend::cvc5_wrapper_encode;
mod feature_max;

// ---------------------------------------------------------------------------
// Z3 backend
// ---------------------------------------------------------------------------

#[cfg(feature = "z3-verify")]
mod z3_backend;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_z3.rs"]
mod tests_z3;

#[cfg(all(test, feature = "z3-verify"))]
#[path = "tests_havoc_assume.rs"]
mod tests_havoc_assume;

// ---------------------------------------------------------------------------
// Additional verification modules
// ---------------------------------------------------------------------------

/// Advanced verification: prophecy variables, triggers, quantifier strategies.
pub mod advanced;
/// Verification result caching (content-addressed by clause hash).
pub mod cache;
/// Incremental verification: skip re-checking unchanged clauses.
pub mod incremental;
/// Layer 2 verification: cross-contract and module-level properties.
pub mod layer2;
/// Feature-specific SMT verification for all 50 features.
pub mod smt_features;
// Re-export key types from submodules so callers and tests can use them
// without qualifying the module path.
pub use advanced::{
    CodecDispatcher, CodecEntry, DispatchResult, LivenessChecker, LivenessKind, LivenessObligation,
    MemoryAccess, MemoryOrdering, ProphecyError, ProphecyManager, ProphecyVariable, TriggerManager,
    TriggerPattern, WeakMemoryChecker,
};
pub use cache::{SessionCache, SessionCacheEntry, VerificationCache};
pub use incremental::{IncrementalCompiler, ModuleState};
pub use layer2::{
    Layer2Config, Layer2Result, Layer2Verifier, QuantifiedInvariant, RoundtripObligation,
    TerminationObligation, verify_quantified_expr,
};

/// Havoc+assume helpers for result-field verification (#267).
pub mod havoc_assume;

/// Implementation IR (Section 4): parser, codegen, and `assura ir` CLI command.
pub mod ir;
pub use ir::{
    IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrInstr, IrLiteral, IrModule, IrNode, IrParser,
    IrPred, IrPredArg, IrSlotDecl, IrValidation, ir_to_rust, parse_ir_module, stub_ir_sidecar_text,
    validate_ir_against_contract,
};
#[cfg(test)]
pub(crate) use ir::{ir_type_to_rust, parse_arith_op, parse_cmp_op, parse_ir_pred_str};

#[cfg(test)]
#[path = "tests_measures.rs"]
mod tests_measures;

#[cfg(test)]
mod quantifier_bound_tests {
    use super::*;

    fn type_check_source(source: &str) -> assura_types::TypedFile {
        let out = assura_pipeline::compile(
            source,
            "test.assura",
            &assura_config::CompilerConfig::default(),
        );
        out.typed.expect("type_check failed")
    }

    #[test]
    fn forall_over_int_is_unbounded() {
        let typed = type_check_source(
            r#"
contract Bad {
    input(x: Int)
    requires { forall n in Int: n >= 0 }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            !warnings.is_empty(),
            "forall over Int should produce a warning"
        );
        assert!(warnings[0].reason.contains("infinite domain"));
    }

    #[test]
    fn exists_over_nat_is_unbounded() {
        let typed = type_check_source(
            r#"
contract Bad {
    input(x: Int)
    requires { exists n in Nat: n > x }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            !warnings.is_empty(),
            "exists over Nat should produce a warning"
        );
    }

    #[test]
    fn forall_over_collection_is_bounded() {
        let typed = type_check_source(
            r#"
contract Good {
    input(items: List<Int>)
    requires { forall v in items: v > 0 }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            warnings.is_empty(),
            "forall over a collection variable should NOT warn: {warnings:?}"
        );
    }

    #[test]
    fn forall_over_range_is_bounded() {
        let typed = type_check_source(
            r#"
contract Good {
    input(n: Nat)
    requires { forall i in 0 .. n: i >= 0 }
}
"#,
        );
        let warnings = validate_quantifier_bounds(&typed);
        assert!(
            warnings.is_empty(),
            "forall over a range should NOT warn: {warnings:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// S001: Termination checking via verify_decrease
// ---------------------------------------------------------------------------

#[cfg(test)]
mod decrease_tests {
    use super::*;
    use assura_ast::{BinOp, Expr, Literal, SpExpr};

    /// Helper: verify_decrease with trivial preconditions.
    fn check_decrease(measure: &SpExpr, call_arg: &SpExpr, desc: &str) -> VerificationResult {
        verify_decrease(&[], measure, call_arg, desc.to_string())
    }

    /// Helper: verify_decrease with preconditions.
    fn check_decrease_with_pre(
        preconditions: &[SpExpr],
        measure: &SpExpr,
        call_arg: &SpExpr,
        desc: &str,
    ) -> VerificationResult {
        verify_decrease(preconditions, measure, call_arg, desc.to_string())
    }

    // -- Factorial: decreases n, calls with n-1, with requires n > 0 --

    #[test]
    fn factorial_terminates() {
        // decreases n, call arg = n - 1, precondition: n > 0
        let measure = Spanned::no_span(Expr::Ident("n".into()));
        let call_arg = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Sub,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let pre = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "factorial::decreases");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "factorial should verify: {result:?}"
        );
    }

    // -- Fibonacci: decreases n, calls with n-1 and n-2 --

    #[test]
    fn fibonacci_n_minus_1_terminates() {
        let measure = Spanned::no_span(Expr::Ident("n".into()));
        let call_arg = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Sub,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let pre = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "fib::decreases(n-1)");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "fib(n-1) should verify: {result:?}"
        );
    }

    #[test]
    fn fibonacci_n_minus_2_terminates() {
        let measure = Spanned::no_span(Expr::Ident("n".into()));
        let call_arg = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Sub,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("2".into())))),
        });
        let pre = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "fib::decreases(n-2)");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "fib(n-2) should verify: {result:?}"
        );
    }

    // -- Non-decreasing: spin(n) calling spin(n) should NOT verify --

    #[test]
    fn spin_same_arg_does_not_terminate() {
        // decreases n, call arg = n (same, not decreasing)
        let measure = Spanned::no_span(Expr::Ident("n".into()));
        let call_arg = Spanned::no_span(Expr::Ident("n".into()));
        let result = check_decrease(&measure, &call_arg, "spin::decreases");
        assert!(
            !matches!(result, VerificationResult::Verified { .. }),
            "spin(n) calling spin(n) should NOT verify: {result:?}"
        );
    }

    // -- Increasing: bad(n) calling bad(n+1) should NOT verify --

    #[test]
    fn increasing_arg_does_not_terminate() {
        let measure = Spanned::no_span(Expr::Ident("n".into()));
        let call_arg = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Add,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let result = check_decrease(&measure, &call_arg, "bad::decreases");
        assert!(
            !matches!(result, VerificationResult::Verified { .. }),
            "bad(n+1) should NOT verify: {result:?}"
        );
    }

    // -- With precondition ensuring non-negativity --

    #[test]
    fn decrease_with_nat_precondition() {
        // decreases n, call arg = n - 1, precondition: n >= 1
        let measure = Spanned::no_span(Expr::Ident("n".into()));
        let call_arg = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Sub,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let pre = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Gte,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        let result = check_decrease_with_pre(&[pre], &measure, &call_arg, "countdown::decreases");
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "countdown with n >= 1 should verify: {result:?}"
        );
    }
}

#[cfg(test)]
mod verify_contract_tests {
    use super::*;
    use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal};

    #[test]
    fn verify_contract_single_ensures_verified() {
        // requires x > 0 ensures x > 0 (trivially true)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract("TestContract", &clauses);
        assert_eq!(results.len(), 1, "one ensures clause: {results:?}");
        assert!(
            matches!(&results[0], VerificationResult::Verified { clause_desc, .. } if clause_desc.contains("TestContract")),
            "should verify: {results:?}"
        );
    }

    #[test]
    fn verify_contract_counterexample() {
        // No requires, ensures x > 0 (counterexample: x = 0)
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract("NoPrecondition", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { clause_desc, .. } if clause_desc.contains("NoPrecondition")),
            "should have counterexample: {results:?}"
        );
    }

    #[test]
    fn verify_contract_multiple_ensures() {
        // requires x > 10
        // ensures x > 5  (verified)
        // ensures x > 20 (counterexample: x = 11)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("20".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract("MultiClause", &clauses);
        assert_eq!(results.len(), 2, "two ensures clauses: {results:?}");
        // First ensures (x > 5) should verify
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x > 10 => x > 5 should verify: {:?}",
            results[0]
        );
        // Second ensures (x > 20) should have counterexample
        assert!(
            matches!(&results[1], VerificationResult::Counterexample { .. }),
            "x > 10 => x > 20 should fail: {:?}",
            results[1]
        );
    }

    #[test]
    fn verify_contract_no_verifiable_clauses() {
        // Only requires, no ensures/invariant
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
            effect_variables: vec![],
        }];
        let results = verify_contract("OnlyRequires", &clauses);
        assert!(results.is_empty(), "no verifiable clauses: {results:?}");
    }

    // ===================================================================
    // #264: Incremental solving (push/pop) tests
    // ===================================================================

    #[test]
    fn incremental_push_pop_three_clauses() {
        // Contract with 3 ensures clauses sharing the same requires.
        // Tests that incremental push/pop produces correct results for all 3.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract("IncrementalPushPop", &clauses);
        assert_eq!(results.len(), 3, "expected 3 results, got {results:?}");
        // x > 0 => x > 0 (verified)
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x > 0 => x > 0 should verify: {:?}",
            results[0]
        );
        // x > 0 => x >= 1 (verified for integers)
        assert!(
            matches!(&results[1], VerificationResult::Verified { .. }),
            "x > 0 => x >= 1 should verify: {:?}",
            results[1]
        );
        // x > 0 => x > 100 (counterexample)
        assert!(
            matches!(&results[2], VerificationResult::Counterexample { .. }),
            "x > 0 => x > 100 should have counterexample: {:?}",
            results[2]
        );
    }

    #[test]
    fn incremental_correctness_verified_and_counterexample() {
        // Two clauses: requires { x > 0 }
        //   ensures { x > 0 }     -> verified
        //   ensures { x > 5 }     -> counterexample
        // The push/pop must isolate clause checks so the negation
        // of one does not leak into the next.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract("IncrementalCorrectness", &clauses);
        assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
        assert!(
            matches!(&results[0], VerificationResult::Verified { .. }),
            "x > 0 => x > 0 should verify: {:?}",
            results[0]
        );
        assert!(
            matches!(&results[1], VerificationResult::Counterexample { .. }),
            "x > 0 => x > 5 should have counterexample: {:?}",
            results[1]
        );
    }

    #[test]
    fn incremental_no_cross_contamination() {
        // Verify that a counterexample clause does not contaminate
        // the solver state for subsequent clauses.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            // This will have a counterexample (not implied by x > 0)
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
                }),
                effect_variables: vec![],
            },
            // This MUST still verify (pop must remove negation of x > 10)
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                    op: BinOp::Gt,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract("NoCrossContamination", &clauses);
        assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
        assert!(
            matches!(&results[0], VerificationResult::Counterexample { .. }),
            "x > 0 => x > 10 should have counterexample: {:?}",
            results[0]
        );
        assert!(
            matches!(&results[1], VerificationResult::Verified { .. }),
            "x > 0 => x > 0 should verify after pop: {:?}",
            results[1]
        );
    }
}

#[cfg(test)]
mod quantified_verification_tests {
    use super::*;
    use assura_ast::{BinOp, Expr, Literal};

    #[test]
    fn forall_trivially_true() {
        // forall x in 0..10: x == x (always true)
        let body = Spanned::no_span(Expr::Forall {
            var: "x".into(),
            domain: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                op: BinOp::Range,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
            })),
            body: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Eq,
                rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            })),
        });
        let result = verify_quantified_expr("trivial_forall", &[], &body);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "forall x in 0..10: x == x should verify: {result:?}"
        );
    }

    #[test]
    fn forall_with_counterexample() {
        // forall x in 0..10: x > 0 (false: x = 0 is a counterexample)
        let body = Spanned::no_span(Expr::Forall {
            var: "x".into(),
            domain: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                op: BinOp::Range,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
            })),
            body: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            })),
        });
        let result = verify_quantified_expr("nonpositive_forall", &[], &body);
        assert!(
            matches!(result, VerificationResult::Counterexample { .. }),
            "forall x in 0..10: x > 0 should have counterexample: {result:?}"
        );
    }

    #[test]
    fn exists_trivially_satisfiable() {
        // exists x in 0..100: x > 5 (true: e.g. x = 6)
        let body = Spanned::no_span(Expr::Exists {
            var: "x".into(),
            domain: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                op: BinOp::Range,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("100".into())))),
            })),
            body: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("5".into())))),
            })),
        });
        let result = verify_quantified_expr("trivial_exists", &[], &body);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "exists x in 0..100: x > 5 should verify: {result:?}"
        );
    }

    #[test]
    fn forall_with_assumption() {
        // Assumption: n > 0
        // Check: forall x in 0..10: n + x >= x (always true when n > 0)
        let assumption = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        let body = Spanned::no_span(Expr::Forall {
            var: "x".into(),
            domain: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                op: BinOp::Range,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("10".into())))),
            })),
            body: Box::new(Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
                    op: BinOp::Add,
                    rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                })),
                op: BinOp::Gte,
                rhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            })),
        });
        let result = verify_quantified_expr("forall_with_pre", &[assumption], &body);
        assert!(
            matches!(result, VerificationResult::Verified { .. }),
            "forall x in 0..10: n + x >= x with n > 0 should verify: {result:?}"
        );
    }

    #[test]
    fn layer2_verifier_verify_method() {
        // Test the Layer2Verifier.verify() method
        let config = Layer2Config::default();
        let verifier = Layer2Verifier::new(config);
        let results = verifier.verify();
        assert!(results.is_empty(), "empty verifier returns no results");
    }

    #[test]
    fn layer2_verifier_with_invariant() {
        let config = Layer2Config::new().with_timeout(5000);
        let mut verifier = Layer2Verifier::new(config);
        verifier.add_invariant(QuantifiedInvariant {
            name: "sorted_invariant".into(),
            bound_vars: vec![("i".into(), "Int".into())],
            body: "i >= 0".into(),
            triggers: Vec::new(),
        });
        let results = verifier.verify();
        assert_eq!(results.len(), 1);
        // "i >= 0" is NOT universally true (i = -1 is a counterexample)
        assert!(matches!(results[0], Layer2Result::Counterexample { .. }));
    }

    // =======================================================================
    // P005: IR parser tests
    // =======================================================================

    #[test]
    fn ir_parse_safe_division() {
        let source = r#"
module safe_division {
  fn #0 : ($0: Int @omega, $1: Int @omega) -> Int ! pure
    pre: cmp ne $1 (const 0)
    post: cmp eq (arith add (arith mul $result $1) (arith mod $0 $1)) $0
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        assert_eq!(module.name, "safe_division");
        assert_eq!(module.functions.len(), 1);
        let func = &module.functions[0];
        assert_eq!(func.id, "#0");
        assert_eq!(func.params.len(), 2);
        assert_eq!(func.params[0].slot, 0);
        assert_eq!(func.params[0].ty, "Int");
        assert_eq!(func.params[1].slot, 1);
        assert_eq!(func.return_type, "Int");
        assert_eq!(func.effects, "pure");
        assert!(func.pre.is_some());
        assert!(func.post.is_some());
        assert_eq!(func.body.len(), 2);
        // First instruction: $2 = arith div $0 $1 : Int
        assert_eq!(func.body[0].target, 2);
        assert_eq!(func.body[0].ty, "Int");
        assert!(matches!(
            func.body[0].expr,
            IrExprKind::Arith {
                op: IrArithOp::Div,
                lhs: 0,
                rhs: 1,
            }
        ));
        // Second instruction: $result = load $2 : Int
        assert_eq!(func.body[1].target, usize::MAX);
        assert!(matches!(func.body[1].expr, IrExprKind::Load(2)));
    }

    #[test]
    fn ir_parse_const_and_call() {
        let source = r#"
module test {
  fn #0 : ($0: Int) -> Bool ! pure
  {
    $1 = const 42 : Int
    $2 = call is_valid ($0, $1) : Bool
    $result = load $2 : Bool
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        assert_eq!(module.functions.len(), 1);
        let body = &module.functions[0].body;
        assert_eq!(body.len(), 3);
        assert!(matches!(
            &body[0].expr,
            IrExprKind::Const(IrLiteral::Int(42))
        ));
        assert!(matches!(
            &body[1].expr,
            IrExprKind::Call { func, args } if func == "is_valid" && args == &[0, 1]
        ));
    }

    #[test]
    fn ir_parse_field_and_construct() {
        let source = r#"
module test {
  fn #0 : ($0: Point) -> Point ! pure
  {
    $1 = field $0 .0 : Int
    $2 = field $0 .1 : Int
    $3 = construct Point { .0 = $2, .1 = $1 } : Point
    $result = load $3 : Point
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        let body = &module.functions[0].body;
        assert!(matches!(
            &body[0].expr,
            IrExprKind::Field { slot: 0, index: 0 }
        ));
        assert!(matches!(
            &body[2].expr,
            IrExprKind::Construct { type_id, fields }
            if type_id == "Point" && fields == &[(0, 2), (1, 1)]
        ));
    }

    #[test]
    fn ir_parse_cmp_and_cast() {
        let source = r#"
module test {
  fn #0 : ($0: Int, $1: Int) -> Bool ! pure
  {
    $2 = cmp lt $0 $1 : Bool
    $3 = cast $0 as Float : Float
    $result = load $2 : Bool
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        let body = &module.functions[0].body;
        assert!(matches!(
            &body[0].expr,
            IrExprKind::Cmp {
                op: IrCmpOp::Lt,
                lhs: 0,
                rhs: 1,
            }
        ));
        assert!(matches!(&body[1].expr, IrExprKind::Cast { slot: 0, .. }));
    }

    #[test]
    fn ir_parse_if_and_transition() {
        let source = r#"
module test {
  fn #0 : ($0: Bool, $1: Connection) -> Unit ! io
  {
    $2 = if $0 then #0 else #1 : Unit
    $3 = transition $1 to Connected : Connection
    $result = load $3 : Connection
  }
}
"#;
        let module = parse_ir_module(source).expect("parse should succeed");
        let body = &module.functions[0].body;
        assert!(matches!(
            &body[0].expr,
            IrExprKind::If {
                cond: 0,
                then_block: 0,
                else_block: 1,
            }
        ));
        assert!(matches!(
            &body[1].expr,
            IrExprKind::Transition { slot: 1, .. }
        ));
    }

    #[test]
    fn ir_parse_empty_module() {
        let source = "module empty {\n}\n";
        let module = parse_ir_module(source).expect("parse should succeed");
        assert_eq!(module.name, "empty");
        assert!(module.functions.is_empty());
    }

    #[test]
    fn ir_parse_error_no_module() {
        let source = "fn #0 : () -> Unit ! pure {}";
        let result = parse_ir_module(source);
        assert!(result.is_err());
    }

    #[test]
    fn ir_to_rust_safe_division() {
        let source = r#"
module safe_division {
  fn #0 : ($0: Int, $1: Int) -> Int ! pure
    pre: cmp ne $1 (const 0)
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
        let module = parse_ir_module(source).unwrap();
        let rust = ir_to_rust(&module);
        assert!(rust.contains("fn ir_0("));
        assert!(rust.contains("slot_0: i64"));
        assert!(rust.contains("slot_1: i64"));
        assert!(rust.contains("-> i64"));
        assert!(rust.contains("debug_assert!"));
        assert!(rust.contains("(slot_0 / slot_1)"));
        assert!(rust.contains("__result"));
    }

    #[test]
    fn ir_validate_slot_gap() {
        let module = IrModule {
            name: "test".into(),
            functions: vec![IrFunction {
                id: "#0".into(),
                params: vec![IrSlotDecl {
                    slot: 0,
                    ty: "Int".into(),
                }],
                return_type: "Int".into(),
                effects: "pure".into(),
                pre: None,
                post: None,
                body: vec![IrInstr {
                    target: 5, // gap: skips $1-$4
                    expr: IrExprKind::Load(0),
                    ty: "Int".into(),
                }],
            }],
        };
        let contract = assura_ast::ContractDecl {
            name: "Test".into(),
            type_params: vec![],
            clauses: vec![],
            fn_params: vec![],
        };
        let validation = validate_ir_against_contract(&module, &contract);
        assert!(!validation.valid);
        assert!(validation.errors[0].contains("skips slot"));
    }

    #[test]
    fn ir_arith_ops() {
        for (s, expected) in [
            ("add", IrArithOp::Add),
            ("sub", IrArithOp::Sub),
            ("mul", IrArithOp::Mul),
            ("div", IrArithOp::Div),
            ("mod", IrArithOp::Mod),
        ] {
            assert_eq!(parse_arith_op(s).unwrap(), expected);
        }
        assert!(parse_arith_op("xor").is_err());
    }

    #[test]
    fn ir_cmp_ops() {
        for (s, expected) in [
            ("eq", IrCmpOp::Eq),
            ("ne", IrCmpOp::Ne),
            ("lt", IrCmpOp::Lt),
            ("le", IrCmpOp::Le),
            ("gt", IrCmpOp::Gt),
            ("ge", IrCmpOp::Ge),
        ] {
            assert_eq!(parse_cmp_op(s).unwrap(), expected);
        }
        assert!(parse_cmp_op("in").is_err());
    }

    #[test]
    fn ir_pred_true_false() {
        assert_eq!(parse_ir_pred_str("true"), Some(IrPred::True));
        assert_eq!(parse_ir_pred_str("false"), Some(IrPred::False));
        assert_eq!(parse_ir_pred_str(""), None);
    }

    #[test]
    fn ir_pred_not() {
        let pred = parse_ir_pred_str("not true");
        assert!(matches!(pred, Some(IrPred::Not(_))));
    }

    #[test]
    fn ir_type_to_rust_mapping() {
        assert_eq!(ir_type_to_rust("Int"), "i64");
        assert_eq!(ir_type_to_rust("Nat"), "u64");
        assert_eq!(ir_type_to_rust("Float"), "f64");
        assert_eq!(ir_type_to_rust("Bool"), "bool");
        assert_eq!(ir_type_to_rust("String"), "String");
        assert_eq!(ir_type_to_rust("Unit"), "()");
        assert_eq!(ir_type_to_rust("CustomType"), "CustomType");
    }
}

// ---------------------------------------------------------------------------
// CVC5 backend unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod cvc5_tests {
    use super::*;

    #[test]
    fn solver_choice_from_str() {
        assert_eq!(SolverChoice::from_str_loose("z3"), Some(SolverChoice::Z3));
        assert_eq!(SolverChoice::from_str_loose("Z3"), Some(SolverChoice::Z3));
        assert_eq!(
            SolverChoice::from_str_loose("cvc5"),
            Some(SolverChoice::Cvc5)
        );
        assert_eq!(
            SolverChoice::from_str_loose("CVC5"),
            Some(SolverChoice::Cvc5)
        );
        assert_eq!(
            SolverChoice::from_str_loose("portfolio"),
            Some(SolverChoice::Portfolio)
        );
        assert_eq!(SolverChoice::from_str_loose("invalid"), None);
    }

    #[test]
    fn cvc5_expr_to_smtlib_literal() {
        use assura_ast::Literal;
        let e = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("42".to_string()));

        let e = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("true".to_string()));

        let e = Spanned::no_span(Expr::Literal(Literal::Int("-5".into())));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("(- 5)".to_string()));
    }

    #[test]
    fn cvc5_expr_to_smtlib_ident() {
        let e = Spanned::no_span(Expr::Ident("x".to_string()));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("x".to_string()));
    }

    #[test]
    fn cvc5_expr_to_smtlib_binop() {
        use assura_ast::{BinOp, Literal};
        let e = Spanned::no_span(Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        });
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(+ x 1)".to_string())
        );

        let e = Spanned::no_span(Expr::BinOp {
            op: BinOp::Neq,
            lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(not (= a 0))".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_unary() {
        use assura_ast::UnaryOp;
        let e = Spanned::no_span(Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Spanned::no_span(Expr::Ident("p".into()))),
        });
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(not p)".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_ite() {
        use assura_ast::Literal;
        let e = Spanned::no_span(Expr::If {
            cond: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
            then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
            else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
                "0".into(),
            ))))),
        });
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(ite c 1 0)".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_forall() {
        let e = Spanned::no_span(Expr::Forall {
            var: "i".to_string(),
            domain: Box::new(Spanned::no_span(Expr::Ident("S".into()))),
            body: Box::new(Spanned::no_span(Expr::BinOp {
                op: assura_ast::BinOp::Gt,
                lhs: Box::new(Spanned::no_span(Expr::Ident("i".into()))),
                rhs: Box::new(Spanned::no_span(Expr::Literal(assura_ast::Literal::Int(
                    "0".into(),
                )))),
            })),
        });
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("(forall ((i Int)) (=> (__domain_contains S i) (> i 0)))".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_result() {
        let e = Spanned::no_span(Expr::Ident("result".to_string()));
        assert_eq!(
            cvc5_backend::expr_to_smtlib(&e),
            Some("__result".to_string())
        );
    }

    #[test]
    fn cvc5_expr_to_smtlib_old() {
        let e = Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(Expr::Ident(
            "x".into(),
        )))));
        assert_eq!(cvc5_backend::expr_to_smtlib(&e), Some("x__old".to_string()));
    }

    #[test]
    fn cvc5_collect_vars() {
        use std::collections::HashSet;
        let e = Spanned::no_span(Expr::BinOp {
            op: assura_ast::BinOp::Add,
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            rhs: Box::new(Spanned::no_span(Expr::Ident("y".into()))),
        });
        let mut vars = HashSet::new();
        cvc5_backend::collect_vars(&e, &mut vars);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
    }

    #[test]
    fn cvc5_parse_model() {
        let model = "(define-fun x () Int 42)\n(define-fun y () Int (- 1))";
        let cm = cvc5_backend::parse_smtlib_model(model).expect("model should parse");
        assert_eq!(cm.variables.len(), 2);
        assert!(cm.variables.iter().any(|(n, v)| n == "x" && v == "42"));
        assert!(cm.variables.iter().any(|(n, v)| n == "y" && v == "(- 1)"));
    }

    #[test]
    fn cvc5_parse_empty_model() {
        let parsed = cvc5_backend::parse_smtlib_model("");
        assert!(parsed.is_none());
    }

    #[test]
    fn cvc5_verify_without_binary() {
        // If cvc5 is not installed, verify_contract_cvc5 returns Error results
        use assura_ast::{Clause, ClauseKind, Literal};
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Spanned::no_span(Expr::BinOp {
                    op: assura_ast::BinOp::Neq,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Spanned::no_span(Expr::BinOp {
                    op: assura_ast::BinOp::Gt,
                    lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
                effect_variables: vec![],
            },
        ];
        let results = cvc5_backend::verify_contract_cvc5("TestContract", &clauses);
        // Should return 1 result (for ensures). May be Unknown if cvc5 not installed.
        assert_eq!(results.len(), 1);
    }
}
