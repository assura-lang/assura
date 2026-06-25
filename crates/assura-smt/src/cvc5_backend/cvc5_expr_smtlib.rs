//! CVC5 shell SMT-LIB2 expression encoder.
//!
//! Prefer [`crate::encode_*_policy`] for solver-neutral shapes; `cvc5_*_encode`
//! modules remain for term/orchestration that is CVC5-specific.

use assura_ast::{Expr, SpExpr};
use std::cell::RefCell;
use std::sync::OnceLock;

use crate::cvc5_adt::{Cvc5AdtDef, adt_is_constructor_smt, define_adt_cvc5};
use crate::cvc5_atom_encode::{encode_apply_smtlib, encode_ident_smtlib, encode_literal_smtlib};
use crate::cvc5_call_encode::{encode_call_smtlib, encode_method_call_smtlib};
use crate::cvc5_list_encode::encode_list_smtlib;
use crate::cvc5_match_encode::encode_match_smtlib;
use crate::cvc5_old_access::encode_old_smtlib;
use crate::cvc5_raw_encode::encode_raw_expr_smtlib;
use crate::cvc5_tuple_encode::encode_tuple_smtlib;
use crate::cvc5_wrapper_encode::encode_wrapper_smtlib;
use crate::encode_atom_policy::index_access_smtlib;
use crate::encode_binop_policy::{encode_ast_binop_smtlib, encode_ast_unary_smtlib};
use crate::encode_field_policy::{FieldAccessPlan, plan_field_access, shallow_field_smtlib};
use crate::encode_if_policy::encode_if_smtlib;
use crate::encode_let_policy::{encode_block_smtlib, encode_let_smtlib};
use crate::encode_quantifier_policy::encode_ast_quantifier_smtlib;

/// Baseline Option ADT for shell-out match encoding (#263).
static SHELL_MATCH_ADT: OnceLock<Cvc5AdtDef> = OnceLock::new();

fn shell_match_adt_def() -> &'static Cvc5AdtDef {
    SHELL_MATCH_ADT.get_or_init(|| {
        let (def, _) = define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);
        assert_eq!(def.name, "Option");
        def
    })
}

// ---------------------------------------------------------------------------
// Thread-local side-effect context for expr_to_smtlib (#462).
//
// Tuple and list encoding need to emit declarations and axioms as side effects
// (fresh constants, accessor UF declarations, element equality assertions).
// expr_to_smtlib is passed as a function pointer (fn(&SpExpr) -> Option<String>)
// in many call sites, so its signature cannot change. We use a thread-local to
// accumulate these side effects.
// ---------------------------------------------------------------------------

/// Side-effect context accumulated during `expr_to_smtlib` calls.
#[derive(Default)]
pub struct SmtlibSideEffects {
    /// SMT-LIB declarations to prepend (declare-const, declare-fun).
    pub declarations: Vec<String>,
    /// SMT-LIB assertions to inject (element axioms, length axioms).
    pub assertions: Vec<String>,
    /// Fresh name counter for unique tuple/list constants.
    pub fresh_counter: usize,
}

thread_local! {
    static SMTLIB_CTX: RefCell<Option<SmtlibSideEffects>> = const { RefCell::new(None) };
}

/// Install a fresh side-effect context, run `f`, then return the context.
///
/// Callers should inject `ctx.declarations` and `ctx.assertions` into the
/// SMT-LIB script after the variable declarations section.
pub fn with_smtlib_side_effects<R>(f: impl FnOnce() -> R) -> (R, SmtlibSideEffects) {
    SMTLIB_CTX.with(|cell| {
        let prev = cell.borrow_mut().replace(SmtlibSideEffects::default());
        let result = f();
        let ctx = cell.borrow_mut().take().unwrap_or_default();
        // Restore previous context (supports nesting, though unlikely).
        *cell.borrow_mut() = prev;
        (result, ctx)
    })
}

/// Push a declaration into the active side-effect context (no-op if none).
fn push_decl(decl: String) {
    SMTLIB_CTX.with(|cell| {
        if let Some(ctx) = cell.borrow_mut().as_mut() {
            ctx.declarations.push(decl);
        }
    });
}

/// Push an assertion into the active side-effect context (no-op if none).
fn push_axiom(assertion: String) {
    SMTLIB_CTX.with(|cell| {
        if let Some(ctx) = cell.borrow_mut().as_mut() {
            ctx.assertions.push(assertion);
        }
    });
}

/// Allocate a fresh counter value from the active context (returns 0 if none).
fn alloc_fresh() -> usize {
    SMTLIB_CTX.with(|cell| {
        if let Some(ctx) = cell.borrow_mut().as_mut() {
            let n = ctx.fresh_counter;
            ctx.fresh_counter += 1;
            n
        } else {
            0
        }
    })
}

/// Convert an AST expression to an SMT-LIB2 string representation.
pub fn expr_to_smtlib(expr: &SpExpr) -> Option<String> {
    match &expr.node {
        Expr::Literal(lit) => encode_literal_smtlib(lit),
        Expr::Ident(name) => Some(encode_ident_smtlib(name)),
        Expr::BinOp { op, lhs, rhs } => {
            // Comparison chaining: a < b < c  =>  (and (< a b) (< b c))
            // Parity with Z3 encode_binop (uses shared is_comparison_ast_binop).
            if crate::encode_binop_policy::is_comparison_ast_binop(op)
                && let Expr::BinOp {
                    lhs: inner_lhs,
                    op: inner_op,
                    rhs: inner_rhs,
                } = &lhs.node
                && crate::encode_binop_policy::is_comparison_ast_binop(inner_op)
            {
                let il = expr_to_smtlib(inner_lhs)?;
                let mid = expr_to_smtlib(inner_rhs)?;
                let r_str = expr_to_smtlib(rhs)?;
                let left_cmp = encode_ast_binop_smtlib(inner_op, &il, &mid)?;
                let right_cmp = encode_ast_binop_smtlib(op, &mid, &r_str)?;
                return Some(format!("(and {left_cmp} {right_cmp})"));
            }
            let l = expr_to_smtlib(lhs)?;
            let r = expr_to_smtlib(rhs)?;
            encode_ast_binop_smtlib(op, &l, &r)
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = expr_to_smtlib(inner)?;
            Some(encode_ast_unary_smtlib(op, &e))
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = expr_to_smtlib(cond)?;
            let t = expr_to_smtlib(then_branch)?;
            let e = else_branch.as_ref().and_then(|eb| expr_to_smtlib(eb));
            Some(encode_if_smtlib(&c, &t, e.as_deref()))
        }
        Expr::Forall { var, domain, body } => {
            let b = expr_to_smtlib(body)?;
            encode_ast_quantifier_smtlib(true, var, domain, &b, expr_to_smtlib)
        }
        Expr::Exists { var, domain, body } => {
            let b = expr_to_smtlib(body)?;
            encode_ast_quantifier_smtlib(false, var, domain, &b, expr_to_smtlib)
        }
        Expr::Call { func, args } => encode_call_smtlib(func, args, expr_to_smtlib),
        Expr::Old(inner) => encode_old_smtlib(inner.as_ref(), expr_to_smtlib),
        Expr::Ghost(inner) => encode_wrapper_smtlib(inner, expr_to_smtlib),
        Expr::Cast { expr: inner, .. } => encode_wrapper_smtlib(inner, expr_to_smtlib),
        Expr::Let {
            name, value, body, ..
        } => encode_let_smtlib(name, value, body, expr_to_smtlib),
        Expr::Match {
            scrutinee, arms, ..
        } => encode_match_smtlib(scrutinee, arms, expr_to_smtlib, |name, s| {
            adt_is_constructor_smt("Option", name, s, shell_match_adt_def())
        }),
        Expr::Field(obj, field) => match plan_field_access(obj, field) {
            FieldAccessPlan::CanonicalLength { obj_name } => Some(
                crate::encode_field_policy::canonical_length_field_smtlib(&obj_name),
            ),
            FieldAccessPlan::Flatten(name) => Some(name),
            FieldAccessPlan::ShallowUf { field: f } => {
                let o = expr_to_smtlib(obj)?;
                Some(shallow_field_smtlib(&f, &o))
            }
        },
        Expr::Index { expr: coll, index } => {
            let c = expr_to_smtlib(coll)?;
            let i = expr_to_smtlib(index)?;
            Some(index_access_smtlib(&c, &i))
        }
        Expr::Block(body) => encode_block_smtlib(body, expr_to_smtlib),
        Expr::Raw(tokens) => encode_raw_expr_smtlib(tokens),
        Expr::Tuple(elems) => Some(encode_tuple_smtlib_with_ctx(elems)),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => encode_method_call_smtlib(receiver, method, args, expr_to_smtlib),
        Expr::List(elems) => Some(encode_list_smtlib_with_ctx(elems)),
        Expr::Apply { lemma_name, .. } => Some(encode_apply_smtlib(lemma_name)),
    }
}

/// Encode a tuple literal with proper element accessor axioms via side-effect context.
///
/// When a `SmtlibSideEffects` context is active (via `with_smtlib_side_effects`),
/// emits `declare-const`, `declare-fun`, and element equality axioms matching
/// the Z3 and CVC5 native tuple encoding. Without a context, falls back to
/// the static placeholder.
fn encode_tuple_smtlib_with_ctx(elems: &[SpExpr]) -> String {
    use crate::encode_tuple_policy::{tuple_accessor_uf_name, tuple_value_fresh_name};

    // Check if we have an active side-effect context.
    let has_ctx = SMTLIB_CTX.with(|cell| cell.borrow().is_some());
    if !has_ctx || elems.is_empty() {
        return encode_tuple_smtlib();
    }

    let arity = elems.len();
    let counter = alloc_fresh();
    let tuple_name = tuple_value_fresh_name(counter);

    // Declare the fresh tuple constant.
    push_decl(format!("(declare-const {tuple_name} Int)"));

    // Declare accessor UFs and assert element equalities.
    for (i, elem) in elems.iter().enumerate() {
        let accessor_name = tuple_accessor_uf_name(arity, i);
        push_decl(format!("(declare-fun {accessor_name} (Int) Int)"));
        if let Some(elem_smt) = expr_to_smtlib(elem) {
            push_axiom(format!(
                "(assert (= ({accessor_name} {tuple_name}) {elem_smt}))"
            ));
        }
    }

    tuple_name
}

/// Encode a list literal with proper element accessor and length axioms via side-effect context.
fn encode_list_smtlib_with_ctx(elems: &[SpExpr]) -> String {
    use crate::encode_atom_policy::FIELD_LEN_UF_NAME;
    use crate::encode_list_policy::{list_get_uf_name, list_value_fresh_name};

    let has_ctx = SMTLIB_CTX.with(|cell| cell.borrow().is_some());
    if !has_ctx || elems.is_empty() {
        return encode_list_smtlib();
    }

    let counter = alloc_fresh();
    let list_name = list_value_fresh_name(counter);
    let get_name = list_get_uf_name();

    // Declare the fresh list constant and the get UF.
    push_decl(format!("(declare-const {list_name} Int)"));
    push_decl(format!("(declare-fun {get_name} (Int Int) Int)"));

    // Assert element equalities.
    for (i, elem) in elems.iter().enumerate() {
        if let Some(elem_smt) = expr_to_smtlib(elem) {
            push_axiom(format!(
                "(assert (= ({get_name} {list_name} {i}) {elem_smt}))"
            ));
        }
    }

    // Assert length.
    push_decl(format!("(declare-fun {FIELD_LEN_UF_NAME} (Int) Int)"));
    push_axiom(format!(
        "(assert (= ({FIELD_LEN_UF_NAME} {list_name}) {}))",
        elems.len()
    ));

    list_name
}
