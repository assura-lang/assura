//! Shell-out CVC5 verification tests (no native feature required).

use crate::VerificationResult;
use crate::cache::SessionCache;
use crate::cvc5_backend::expr_to_smtlib;
use crate::cvc5_backend::verify_contract_cvc5_with_lemmas;
use crate::cvc5_expr_smtlib::with_smtlib_side_effects;
use crate::cvc5_havoc_assume_smtlib::append_havoc_assume_smtlib;
use crate::cvc5_verify_shared::{Cvc5TypeConstraint, collect_cvc5_type_constraints};
use crate::encode_atom_policy::canonical_length_name;
use crate::havoc_assume::{HavocAssumeInput, HavocAssumeSmtlibTarget};
use crate::verify_context::{ContractVerifyContext, LoadedIrContext};
use assura_ast::{BinOp, Clause, ClauseKind, Expr, Literal, Param, Spanned};
use std::collections::HashSet;
use std::process::Command;

fn cvc5_on_path() -> bool {
    Command::new("cvc5")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn shell_method_call_length_uses_canonical_len() {
    let expr = Spanned::no_span(Expr::MethodCall {
        receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
        method: "length".into(),
        args: vec![],
    });
    assert_eq!(expr_to_smtlib(&expr), Some(canonical_length_name("raw")));
}

#[test]
fn shell_ident_field_length_uses_canonical_len() {
    let expr = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
        "length".into(),
    ));
    assert_eq!(expr_to_smtlib(&expr), Some(canonical_length_name("raw")));
}

#[test]
fn shell_shared_type_constraints_match_shell_script_rules() {
    let mut vars = HashSet::new();
    vars.insert("__result".into());
    vars.insert("max_size".into());
    let constraints =
        collect_cvc5_type_constraints(&vars, &[], &["Nat".into()], &[("max_size".into(), 42)], &[]);
    assert!(constraints.contains(&Cvc5TypeConstraint::NatNonNegative("__result".into())));
    assert!(constraints.contains(&Cvc5TypeConstraint::ConstantEq("max_size".into(), 42)));
}

#[test]
fn shell_havoc_assume_script_emits_length_link_axiom() {
    let n = Spanned::no_span(Expr::Literal(Literal::Int("50".into())));
    let requires = vec![Clause {
        kind: ClauseKind::Requires,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident("buf".into()))),
                method: "length".into(),
                args: vec![],
            })),
            op: BinOp::Lte,
            rhs: Box::new(n.clone()),
        }),
        effect_variables: vec![],
    }];
    let ensures = vec![Clause {
        kind: ClauseKind::Ensures,
        body: Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::MethodCall {
                receiver: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                method: "length".into(),
                args: vec![],
            })),
            op: BinOp::Lte,
            rhs: Box::new(n),
        }),
        effect_variables: vec![],
    }];
    let mut script = String::new();
    let mut vars = HashSet::new();
    let mut target = HavocAssumeSmtlibTarget {
        script: &mut script,
        vars: &mut vars,
    };
    let req_refs: Vec<_> = requires.iter().collect();
    let ens_refs: Vec<_> = ensures.iter().collect();
    let input = HavocAssumeInput {
        requires: &req_refs,
        ensures: &ens_refs,
        return_ty: &["Bytes".into()],
        param_names: &["buf".into()],
        ir: None,
        enc_ctx: crate::ir_encode::IrEncodeContext::default(),
    };
    append_havoc_assume_smtlib(&mut target, &input);
    assert!(script.contains("(assert (<= __canonical_len_result __canonical_len_buf))"));
}

#[test]
fn shell_havoc_assume_with_ir_emits_load_length_identity() {
    use crate::ir::parse_ir_module;

    let ir_source = r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#;
    let func = parse_ir_module(ir_source).unwrap().functions[0].clone();
    let mut script = String::new();
    let mut vars = HashSet::new();
    let mut target = HavocAssumeSmtlibTarget {
        script: &mut script,
        vars: &mut vars,
    };
    let input = HavocAssumeInput {
        requires: &[],
        ensures: &[],
        return_ty: &["Bytes".into()],
        param_names: &["raw".into()],
        ir: Some(&func),
        enc_ctx: crate::ir_encode::IrEncodeContext::default(),
    };
    append_havoc_assume_smtlib(&mut target, &input);
    assert!(
        script.contains("(assert (= __canonical_len_result __canonical_len_raw))"),
        "IR load should tie result length to input, got:\n{script}"
    );
}

#[test]
fn shell_ir_body_verifies_copy_length_contract() {
    if !cvc5_on_path() {
        return;
    }

    use crate::ir::{IrFunction, parse_ir_module};

    let ir_source = r#"
module copy {
  fn #0 : ($0: Bytes) -> Bytes ! pure
  {
    $result = load $0 : Bytes
  }
}
"#;
    let ir: IrFunction = parse_ir_module(ir_source).unwrap().functions[0].clone();

    let raw_len_gt_zero = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
            method: "length".into(),
            args: vec![],
        })),
        op: BinOp::Gt,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
    });
    let result_len_le_raw = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
            method: "length".into(),
            args: vec![],
        })),
        op: BinOp::Lte,
        rhs: Box::new(Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(Spanned::no_span(Expr::Ident("raw".into()))),
            method: "length".into(),
            args: vec![],
        })),
    });

    let clauses = vec![
        Clause {
            kind: ClauseKind::Requires,
            body: raw_len_gt_zero,
            effect_variables: vec![],
        },
        Clause {
            kind: ClauseKind::Ensures,
            body: result_len_le_raw,
            effect_variables: vec![],
        },
    ];
    let params = vec![Param {
        name: "raw".into(),
        ty: Some(assura_ast::TypeExpr::Named("Bytes".into())),
    }];

    let mut cache = SessionCache::new();
    let ctx = ContractVerifyContext {
        contract_name: "CopyBytes",
        clauses: &clauses,
        params: &params,
        return_ty: &["Bytes".into()],
        constants: &[],
        ir: Some(LoadedIrContext::with_body(&ir)),
    };
    let results = verify_contract_cvc5_with_lemmas(&ctx, None, &mut cache);

    assert_eq!(results.len(), 1);
    assert!(
        matches!(&results[0], VerificationResult::Verified { .. }),
        "IR load should make result.length <= raw.length() verifiable, got: {:?}",
        results[0]
    );
}

// ---------------------------------------------------------------------------
// Tuple / List element axiom tests (#462)
// ---------------------------------------------------------------------------

#[test]
fn shell_tuple_encoding_emits_element_axioms() {
    let tuple_expr = Spanned::no_span(Expr::Tuple(vec![
        Spanned::no_span(Expr::Literal(Literal::Int("10".into()))),
        Spanned::no_span(Expr::Literal(Literal::Int("20".into()))),
        Spanned::no_span(Expr::Literal(Literal::Int("30".into()))),
    ]));

    let (result, effects) = with_smtlib_side_effects(|| expr_to_smtlib(&tuple_expr));

    // Result should be a fresh name, not the static placeholder.
    let smt = result.expect("tuple should encode");
    assert!(
        smt.starts_with("__tuple_"),
        "expected fresh tuple name, got: {smt}"
    );
    assert_ne!(smt, "__tuple_fresh", "should not be the static placeholder");

    // Declarations: 1 fresh constant + 3 accessor UFs.
    assert!(
        effects.declarations.len() >= 4,
        "expected >= 4 declarations (1 const + 3 UFs), got {}: {:?}",
        effects.declarations.len(),
        effects.declarations
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("declare-const") && d.contains(&smt))
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("__tuple_3_0"))
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("__tuple_3_1"))
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("__tuple_3_2"))
    );

    // Axioms: 3 element equalities.
    assert_eq!(
        effects.assertions.len(),
        3,
        "expected 3 element axioms, got: {:?}",
        effects.assertions
    );
    assert!(effects.assertions[0].contains("__tuple_3_0"));
    assert!(effects.assertions[0].contains("10"));
    assert!(effects.assertions[1].contains("__tuple_3_1"));
    assert!(effects.assertions[1].contains("20"));
    assert!(effects.assertions[2].contains("__tuple_3_2"));
    assert!(effects.assertions[2].contains("30"));
}

#[test]
fn shell_list_encoding_emits_element_and_length_axioms() {
    let list_expr = Spanned::no_span(Expr::List(vec![
        Spanned::no_span(Expr::Literal(Literal::Int("1".into()))),
        Spanned::no_span(Expr::Literal(Literal::Int("2".into()))),
    ]));

    let (result, effects) = with_smtlib_side_effects(|| expr_to_smtlib(&list_expr));

    let smt = result.expect("list should encode");
    assert!(
        smt.starts_with("__list_"),
        "expected fresh list name, got: {smt}"
    );
    assert_ne!(smt, "__list_fresh", "should not be the static placeholder");

    // Declarations: 1 fresh constant + __list_get UF + __field_len UF.
    assert!(
        effects.declarations.len() >= 3,
        "expected >= 3 declarations, got {}: {:?}",
        effects.declarations.len(),
        effects.declarations
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("declare-const") && d.contains(&smt))
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("__list_get"))
    );
    assert!(
        effects
            .declarations
            .iter()
            .any(|d| d.contains("__field_len"))
    );

    // Axioms: 2 element equalities + 1 length axiom = 3.
    assert_eq!(
        effects.assertions.len(),
        3,
        "expected 3 axioms (2 elements + 1 length), got: {:?}",
        effects.assertions
    );
    assert!(effects.assertions[0].contains("__list_get"));
    assert!(effects.assertions[0].contains("1")); // elem value
    assert!(effects.assertions[1].contains("__list_get"));
    assert!(effects.assertions[1].contains("2")); // elem value
    assert!(effects.assertions[2].contains("__field_len"));
    assert!(effects.assertions[2].contains("2")); // length
}

#[test]
fn shell_empty_tuple_falls_back_to_placeholder() {
    let tuple_expr = Spanned::no_span(Expr::Tuple(vec![]));
    let (result, effects) = with_smtlib_side_effects(|| expr_to_smtlib(&tuple_expr));
    assert_eq!(result, Some("__tuple_fresh".into()));
    assert!(effects.declarations.is_empty());
    assert!(effects.assertions.is_empty());
}

#[test]
fn shell_empty_list_falls_back_to_placeholder() {
    let list_expr = Spanned::no_span(Expr::List(vec![]));
    let (result, effects) = with_smtlib_side_effects(|| expr_to_smtlib(&list_expr));
    assert_eq!(result, Some("__list_fresh".into()));
    assert!(effects.declarations.is_empty());
    assert!(effects.assertions.is_empty());
}

#[test]
fn shell_tuple_without_context_returns_placeholder() {
    // Without with_smtlib_side_effects, tuple falls back to placeholder.
    let tuple_expr = Spanned::no_span(Expr::Tuple(vec![Spanned::no_span(Expr::Literal(
        Literal::Int("1".into()),
    ))]));
    let result = expr_to_smtlib(&tuple_expr);
    assert_eq!(result, Some("__tuple_fresh".into()));
}

#[test]
fn shell_list_without_context_returns_placeholder() {
    let list_expr = Spanned::no_span(Expr::List(vec![Spanned::no_span(Expr::Literal(
        Literal::Int("1".into()),
    ))]));
    let result = expr_to_smtlib(&list_expr);
    assert_eq!(result, Some("__list_fresh".into()));
}

#[test]
fn shell_multiple_tuples_get_unique_names() {
    let t1 = Spanned::no_span(Expr::Tuple(vec![Spanned::no_span(Expr::Literal(
        Literal::Int("1".into()),
    ))]));
    let t2 = Spanned::no_span(Expr::Tuple(vec![Spanned::no_span(Expr::Literal(
        Literal::Int("2".into()),
    ))]));
    let (results, _effects) = with_smtlib_side_effects(|| {
        let a = expr_to_smtlib(&t1);
        let b = expr_to_smtlib(&t2);
        (a, b)
    });
    let (a, b) = results;
    assert_ne!(a, b, "two tuples should get different fresh names");
}

// ---------------------------------------------------------------------------
// Shell encoding parity tests (#468)
// ---------------------------------------------------------------------------

#[test]
fn shell_literal_int_encoding() {
    let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
    assert_eq!(expr_to_smtlib(&expr), Some("42".into()));

    // Negative literal encoded via int SMT-LIB
    let neg = Spanned::no_span(Expr::Literal(Literal::Int("-7".into())));
    let smt = expr_to_smtlib(&neg).unwrap();
    assert!(smt.contains("7"), "negative int should encode, got: {smt}");
}

#[test]
fn shell_literal_bool_encoding() {
    let t = Spanned::no_span(Expr::Literal(Literal::Bool(true)));
    assert_eq!(expr_to_smtlib(&t), Some("true".into()));
    let f = Spanned::no_span(Expr::Literal(Literal::Bool(false)));
    assert_eq!(expr_to_smtlib(&f), Some("false".into()));
}

#[test]
fn shell_ident_result_renamed() {
    // `result` -> `__result` in SMT-LIB
    let expr = Spanned::no_span(Expr::Ident("result".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("__result".into()));
}

#[test]
fn shell_ident_regular_unchanged() {
    let expr = Spanned::no_span(Expr::Ident("x".into()));
    assert_eq!(expr_to_smtlib(&expr), Some("x".into()));
}

#[test]
fn shell_binop_arithmetic() {
    let add = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        op: BinOp::Add,
        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
    });
    assert_eq!(expr_to_smtlib(&add), Some("(+ x 1)".into()));
}

#[test]
fn shell_binop_comparison() {
    let lt = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Lt,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_smtlib(&lt), Some("(< a b)".into()));
}

#[test]
fn shell_binop_chained_comparison() {
    // a < b < c  =>  (and (< a b) (< b c))
    let inner = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Lt,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    let chained = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(inner),
        op: BinOp::Lt,
        rhs: Box::new(Spanned::no_span(Expr::Ident("c".into()))),
    });
    let smt = expr_to_smtlib(&chained).unwrap();
    assert!(
        smt.contains("and") && smt.contains("(< a b)") && smt.contains("(< b c)"),
        "chained comparison should produce (and (< a b) (< b c)), got: {smt}"
    );
}

#[test]
fn shell_if_then_else_encoding() {
    let ite = Spanned::no_span(Expr::If {
        cond: Box::new(Spanned::no_span(Expr::Ident("p".into()))),
        then_branch: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
        else_branch: Some(Box::new(Spanned::no_span(Expr::Literal(Literal::Int(
            "0".into(),
        ))))),
    });
    let smt = expr_to_smtlib(&ite).unwrap();
    assert!(
        smt.contains("ite") && smt.contains("p") && smt.contains("1") && smt.contains("0"),
        "if/then/else should produce (ite p 1 0), got: {smt}"
    );
}

#[test]
fn shell_old_ident_encoding() {
    use crate::cvc5_backend::cvc5_old_access::encode_old_smtlib;
    let inner = Spanned::no_span(Expr::Ident("x".into()));
    let result = encode_old_smtlib(&inner, expr_to_smtlib);
    // old(x) for an ident should produce the old snapshot name
    assert!(result.is_some(), "old(x) should encode");
    let smt = result.unwrap();
    assert!(
        smt.contains("old"),
        "old(x) should contain 'old' in the name, got: {smt}"
    );
}

#[test]
fn shell_unary_not_encoding() {
    let not_expr = Spanned::no_span(Expr::UnaryOp {
        op: assura_ast::UnaryOp::Not,
        expr: Box::new(Spanned::no_span(Expr::Ident("flag".into()))),
    });
    assert_eq!(expr_to_smtlib(&not_expr), Some("(not flag)".into()));
}

#[test]
fn shell_unary_neg_encoding() {
    let neg = Spanned::no_span(Expr::UnaryOp {
        op: assura_ast::UnaryOp::Neg,
        expr: Box::new(Spanned::no_span(Expr::Ident("n".into()))),
    });
    assert_eq!(expr_to_smtlib(&neg), Some("(- n)".into()));
}

#[test]
fn shell_block_empty_is_true() {
    let block = Spanned::no_span(Expr::Block(vec![]));
    assert_eq!(expr_to_smtlib(&block), Some("true".into()));
}

#[test]
fn shell_block_single_unwraps() {
    let block = Spanned::no_span(Expr::Block(vec![Spanned::no_span(Expr::Ident("x".into()))]));
    assert_eq!(expr_to_smtlib(&block), Some("x".into()));
}

#[test]
fn shell_index_access_encoding() {
    let idx = Spanned::no_span(Expr::Index {
        expr: Box::new(Spanned::no_span(Expr::Ident("buf".into()))),
        index: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("3".into())))),
    });
    let smt = expr_to_smtlib(&idx).unwrap();
    assert!(
        smt.contains("__index") && smt.contains("buf") && smt.contains("3"),
        "index should produce (__index buf 3), got: {smt}"
    );
}

#[test]
fn shell_field_access_shallow_uf() {
    // Non-length field access: obj.status => (__field_status obj)
    let field = Spanned::no_span(Expr::Field(
        Box::new(Spanned::no_span(Expr::Ident("obj".into()))),
        "status".into(),
    ));
    let smt = expr_to_smtlib(&field).unwrap();
    assert!(
        smt.contains("__field_status") && smt.contains("obj"),
        "field access should use UF, got: {smt}"
    );
}

#[test]
fn shell_logical_and_or_implies() {
    let and_expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::And,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_smtlib(&and_expr), Some("(and a b)".into()));

    let or_expr = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Or,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_smtlib(&or_expr), Some("(or a b)".into()));

    let implies = Spanned::no_span(Expr::BinOp {
        lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
        op: BinOp::Implies,
        rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
    });
    assert_eq!(expr_to_smtlib(&implies), Some("(=> a b)".into()));
}
