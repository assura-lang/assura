use super::*;
use crate::cache::SessionCache;
use assura_parser::ast::{BinOp, BlockKind, Clause, ClauseKind, Decl, Literal, Pattern, UnaryOp};
use std::collections::HashSet;

// =========================================================================
// Unmodelable feature detection (mirrors z3_backend::encoder logic)
//
// These functions detect features in clause bodies that cannot be encoded
// to SMT formulas (currently only typestate `@` annotations). Without
// this pre-check, the encoder either returns `None` (generic error) or
// partially encodes the expression, producing false counterexamples.
//
// Duplicated here because z3_backend is behind `#[cfg(feature = "z3-verify")]`
// and the CVC5 backend must work without Z3.
// =========================================================================

/// Returns true if the expression contains features not yet encodable in SMT.
///
/// Currently only typestate `@` annotations in `Expr::Raw` tokens are
/// unmodelable. All other constructs (field access, method calls, ghost
/// expressions, etc.) have SMT encodings.
fn expr_has_unmodelable_features_cvc5(expr: &Expr) -> bool {
    match expr {
        Expr::Field(obj, _) => expr_has_unmodelable_features_cvc5(obj),
        Expr::MethodCall {
            receiver,
            method: _,
            args,
        } => {
            expr_has_unmodelable_features_cvc5(receiver)
                || args.iter().any(expr_has_unmodelable_features_cvc5)
        }
        Expr::Raw(tokens) => tokens.iter().any(|t| t == "@"),
        Expr::BinOp { lhs, rhs, .. } => {
            expr_has_unmodelable_features_cvc5(lhs) || expr_has_unmodelable_features_cvc5(rhs)
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. } => expr_has_unmodelable_features_cvc5(inner),
        Expr::Call { func, args } => {
            expr_has_unmodelable_features_cvc5(func)
                || args.iter().any(expr_has_unmodelable_features_cvc5)
        }
        Expr::Index { expr: e, index } => {
            expr_has_unmodelable_features_cvc5(e) || expr_has_unmodelable_features_cvc5(index)
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            expr_has_unmodelable_features_cvc5(domain) || expr_has_unmodelable_features_cvc5(body)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_has_unmodelable_features_cvc5(cond)
                || expr_has_unmodelable_features_cvc5(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_has_unmodelable_features_cvc5(e))
        }
        Expr::Let { value, body, .. } => {
            expr_has_unmodelable_features_cvc5(value) || expr_has_unmodelable_features_cvc5(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_has_unmodelable_features_cvc5(scrutinee)
                || arms
                    .iter()
                    .any(|a| expr_has_unmodelable_features_cvc5(&a.body))
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            items.iter().any(expr_has_unmodelable_features_cvc5)
        }
        Expr::Apply { args, .. } => args.iter().any(expr_has_unmodelable_features_cvc5),
        Expr::Literal(_) | Expr::Ident(_) => false,
    }
}

/// Collect human-readable reasons for why an expression is unmodelable.
fn collect_unmodelable_reasons_cvc5(expr: &Expr) -> Vec<String> {
    let mut reasons = Vec::new();
    collect_unmodelable_reasons_cvc5_inner(expr, &mut reasons);
    reasons.sort();
    reasons.dedup();
    reasons
}

fn collect_unmodelable_reasons_cvc5_inner(expr: &Expr, reasons: &mut Vec<String>) {
    if let Expr::Raw(tokens) = expr {
        for t in tokens {
            if t == "@" {
                reasons.push("typestate annotation".into());
            }
        }
    }
    match expr {
        Expr::BinOp { lhs, rhs, .. } => {
            collect_unmodelable_reasons_cvc5_inner(lhs, reasons);
            collect_unmodelable_reasons_cvc5_inner(rhs, reasons);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Cast { expr: inner, .. }
        | Expr::Field(inner, _) => {
            collect_unmodelable_reasons_cvc5_inner(inner, reasons);
        }
        Expr::Call { func, args } => {
            collect_unmodelable_reasons_cvc5_inner(func, reasons);
            for a in args {
                collect_unmodelable_reasons_cvc5_inner(a, reasons);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_unmodelable_reasons_cvc5_inner(receiver, reasons);
            for a in args {
                collect_unmodelable_reasons_cvc5_inner(a, reasons);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_unmodelable_reasons_cvc5_inner(e, reasons);
            collect_unmodelable_reasons_cvc5_inner(index, reasons);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_unmodelable_reasons_cvc5_inner(domain, reasons);
            collect_unmodelable_reasons_cvc5_inner(body, reasons);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_unmodelable_reasons_cvc5_inner(cond, reasons);
            collect_unmodelable_reasons_cvc5_inner(then_branch, reasons);
            if let Some(eb) = else_branch {
                collect_unmodelable_reasons_cvc5_inner(eb, reasons);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_unmodelable_reasons_cvc5_inner(value, reasons);
            collect_unmodelable_reasons_cvc5_inner(body, reasons);
        }
        Expr::Match { scrutinee, arms } => {
            collect_unmodelable_reasons_cvc5_inner(scrutinee, reasons);
            for a in arms {
                collect_unmodelable_reasons_cvc5_inner(&a.body, reasons);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_unmodelable_reasons_cvc5_inner(item, reasons);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                collect_unmodelable_reasons_cvc5_inner(a, reasons);
            }
        }
        _ => {}
    }
}

// =========================================================================
// Lemma apply-ref collection (duplicated from z3_backend because that
// module is behind `#[cfg(feature = "z3-verify")]`)
// =========================================================================

/// Collect lemma names referenced by `apply lemma_name(args)` in a single expression.
fn collect_apply_refs_from_expr(expr: &Expr) -> Vec<String> {
    let mut refs = Vec::new();
    collect_apply_refs_inner(expr, &mut refs);
    refs
}

fn collect_apply_refs_inner(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::Apply { lemma_name, args } => {
            refs.push(lemma_name.clone());
            for arg in args {
                collect_apply_refs_inner(arg, refs);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_apply_refs_inner(lhs, refs);
            collect_apply_refs_inner(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            collect_apply_refs_inner(inner, refs);
        }
        Expr::Call { func, args } => {
            collect_apply_refs_inner(func, refs);
            for a in args {
                collect_apply_refs_inner(a, refs);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_apply_refs_inner(receiver, refs);
            for a in args {
                collect_apply_refs_inner(a, refs);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_apply_refs_inner(e, refs);
            collect_apply_refs_inner(index, refs);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_apply_refs_inner(domain, refs);
            collect_apply_refs_inner(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_apply_refs_inner(cond, refs);
            collect_apply_refs_inner(then_branch, refs);
            if let Some(eb) = else_branch {
                collect_apply_refs_inner(eb, refs);
            }
        }
        Expr::Let { value, body, .. } => {
            collect_apply_refs_inner(value, refs);
            collect_apply_refs_inner(body, refs);
        }
        Expr::Match { scrutinee, arms } => {
            collect_apply_refs_inner(scrutinee, refs);
            for a in arms {
                collect_apply_refs_inner(&a.body, refs);
            }
        }
        Expr::List(items) | Expr::Block(items) | Expr::Tuple(items) => {
            for item in items {
                collect_apply_refs_inner(item, refs);
            }
        }
        _ => {}
    }
}

/// Collect lemma definitions from a typed file's declarations.
///
/// Maps each lemma name to its ensures clause bodies. This mirrors
/// `z3_backend::collect_lemma_defs` but is available without the
/// `z3-verify` feature.
pub(crate) fn collect_lemma_defs_for_cvc5(
    typed: &assura_types::TypedFile,
) -> std::collections::HashMap<String, Vec<&Expr>> {
    let mut lemmas = std::collections::HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&Expr> = f
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .map(|c| &c.body)
                .collect();
            lemmas.insert(f.name.clone(), ensures);
        }
    }
    lemmas
}

// =========================================================================
// Feature-max constant collection and refinement narrowing (CVC5)
// =========================================================================

/// Collect `feature_max` constants from a `TypedFile`'s declarations.
///
/// Each `feature_max NAME: Nat = VALUE` declaration is returned as
/// `(NAME, VALUE)`. The CVC5 backend binds these as concrete integer
/// constants instead of free Z3/CVC5 variables (matching the Z3
/// backend's behavior from #180).
pub(crate) fn collect_feature_max_constants_cvc5(typed: &crate::TypedFile) -> Vec<(String, i64)> {
    let mut constants = Vec::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::Block {
            kind,
            name,
            value: Some(tokens),
            ..
        } = &decl.node
            && *kind == BlockKind::FeatureMax
            && let Some(eq_pos) = tokens.iter().position(|t| t == "=")
            && let Some(val_str) = tokens.get(eq_pos + 1)
            && let Ok(v) = val_str.parse::<i64>()
        {
            constants.push((name.clone(), v));
        }
    }
    constants
}

/// Derive refinement narrowings from `feature_max` constants.
///
/// For a constant named `max_X` or `MAX_X`, derives a narrowing
/// `(X, value)` that asserts `X <= value` in the solver. This
/// mirrors the Z3 backend's `derive_narrowings`.
pub(crate) fn derive_narrowings_cvc5(constants: &[(String, i64)]) -> Vec<(String, i64)> {
    let mut narrowings = Vec::new();
    for (name, value) in constants {
        let narrowed = name
            .strip_prefix("max_")
            .or_else(|| name.strip_prefix("MAX_"));
        if let Some(narrowed) = narrowed.filter(|s| !s.is_empty()) {
            narrowings.push((narrowed.to_string(), *value));
            let lower = narrowed.to_lowercase();
            if lower != narrowed {
                narrowings.push((lower, *value));
            }
        }
    }
    narrowings
}

// =========================================================================
// Deep field chain helpers (pure AST, no CVC5 dependency)
//
// These mirror the Z3 backend's helpers in encoder.rs. They detect and
// flatten deep field chains (e.g., `state.head.extra.extra_max`) into a
// single variable name (`state__head__extra__extra_max`) to avoid nested
// uninterpreted function calls that produce spurious counterexamples.
// =========================================================================

/// Check if an expression is rooted at `self`.
fn is_self_rooted_cvc5(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "self",
        Expr::Field(obj, _) => is_self_rooted_cvc5(obj),
        Expr::Paren(inner) => is_self_rooted_cvc5(inner),
        _ => false,
    }
}

fn field_chain_depth_cvc5(expr: &Expr) -> usize {
    match expr {
        Expr::Field(obj, _) => 1 + field_chain_depth_cvc5(obj),
        Expr::Paren(inner) => field_chain_depth_cvc5(inner),
        _ => 0,
    }
}

fn has_deep_field_chain_cvc5(expr: &Expr) -> bool {
    field_chain_depth_cvc5(expr) >= 2
}

/// Flatten a field chain like `a.b.c` into `"a__b__c"`.
fn flatten_field_chain_cvc5(expr: &Expr) -> String {
    match expr {
        Expr::Field(obj, field) => {
            let prefix = flatten_field_chain_cvc5(obj);
            format!("{prefix}__{field}")
        }
        Expr::Ident(name) => name.clone(),
        Expr::Paren(inner) => flatten_field_chain_cvc5(inner),
        _ => format!("__obj_{:p}", expr as *const _),
    }
}

// =========================================================================
// Internal variable filtering (matches Z3 backend's extract_counter_model)
//
// The CVC5 encoder creates many internal variables (__str_*, __field_*,
// __fresh_*, etc.) that should not appear in user-facing counterexample
// models. This mirrors the Z3 backend's filtering in solver.rs.
// =========================================================================

/// Check if a variable name is an internal encoder artifact.
///
/// Internal variables are created by the SMT encoder for string constants,
/// field access UFs, fresh temporaries, etc. They should be filtered out
/// of counterexample models shown to users. The only `__`-prefixed variable
/// kept is `__result` (the return value).
fn is_internal_cvc5_var(name: &str) -> bool {
    name.starts_with("__str_")
        || name.starts_with("__tuple_")
        || name.starts_with("__list_")
        || name.starts_with("__fresh_")
        || name.starts_with("__field_")
        || name.starts_with("__index")
        || name.starts_with("__len")
        || name.starts_with("__arr_")
        || name.starts_with("__domain_contains")
        || name.starts_with("__apply_")
        || name.starts_with("__coerce")
        || name.starts_with("__trigger_")
        || name.starts_with("__list_get")
        || name.starts_with("__result")
        || name.starts_with("__contains")
        || name.starts_with("__obj_")
}

// =========================================================================
// Native CVC5 API backend (feature = "cvc5-verify")
// =========================================================================

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

#[cfg(feature = "cvc5-verify")]
use assura_types::checkers::expr_references_var;

/// Encoder state for the native CVC5 backend.
/// Tracks background axioms, string constants, and fresh variable counter.
#[cfg(feature = "cvc5-verify")]
struct Cvc5EncoderState<'a> {
    axioms: Vec<cvc5::Term<'a>>,
    string_constants: Vec<String>,
    fresh_counter: usize,
}

/// Verify a single contract's clauses using CVC5.
///
/// When the `cvc5-verify` feature is enabled, uses the native Rust cvc5
/// crate (direct API calls, no process spawning). Otherwise falls back to
/// generating SMT-LIB2 text and invoking the `cvc5` binary.
///
/// This variant extracts params from `input()` clauses. For function
/// definitions whose params live in `FnDef.params`, use
/// `verify_contract_cvc5_with_types` instead.
pub(crate) fn verify_contract_cvc5(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    let params = crate::entry::extract_input_params(clauses);
    let return_ty = crate::entry::extract_output_return_type(clauses);
    let mut cache = SessionCache::new();
    verify_contract_cvc5_with_types(contract_name, clauses, &params, &return_ty, &mut cache)
}

/// Verify a single contract's clauses using CVC5 with explicit type info.
///
/// `params` and `return_ty` supply Nat constraints that cannot be extracted
/// from clauses alone (e.g., function parameters declared outside the clause
/// list). This fixes the parity gap where the Z3 backend received Nat >= 0
/// constraints via `verify_contract_impl_with_types` but the CVC5 backend
/// only extracted them from `input()` clauses.
pub(crate) fn verify_contract_cvc5_with_types(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    verify_contract_cvc5_with_full_context(contract_name, clauses, params, return_ty, &[], cache)
}

/// Verify a single contract's clauses using CVC5 with full context.
///
/// Like `verify_contract_cvc5_with_types` but also takes `feature_max`
/// constants that are bound to concrete integer values in the solver
/// (matching the Z3 backend's behavior from #180). Refinement narrowings
/// are derived from constants with `max_`/`MAX_` prefixes.
pub(crate) fn verify_contract_cvc5_with_full_context(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    verify_contract_cvc5_with_lemmas(
        contract_name,
        clauses,
        params,
        return_ty,
        None,
        constants,
        cache,
    )
}

/// Verify a single contract's clauses using CVC5, with optional lemma defs.
///
/// When `lemma_defs` is `Some`, `apply lemma_name(args)` expressions will
/// have the referenced lemma's ensures clauses injected as solver
/// assumptions (matching the Z3 backend's behavior).
///
/// `constants` binds `feature_max` names to concrete values instead of
/// leaving them as free solver variables.
pub(crate) fn verify_contract_cvc5_with_lemmas(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        verify_contract_cvc5_native(
            contract_name,
            clauses,
            params,
            return_ty,
            lemma_defs,
            constants,
            cache,
        )
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        verify_contract_cvc5_shellout(
            contract_name,
            clauses,
            params,
            return_ty,
            lemma_defs,
            constants,
            cache,
        )
    }
}

// -------------------------------------------------------------------------
// Native CVC5 implementation
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
fn verify_contract_cvc5_native(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Derive refinement narrowings from feature_max constants
    let narrowings = derive_narrowings_cvc5(constants);

    let requires_exprs: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    // Build frame checker from modifies clauses
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        assura_types::FrameChecker::new(&modifies_bodies)
    };

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases => {
                let desc = format!("{contract_name}::{:?}", clause.kind);
                let result = check_clause_cvc5_native(
                    &desc,
                    &requires_exprs,
                    &clause.body,
                    clause.kind.clone(),
                    params,
                    return_ty,
                    constants,
                    &narrowings,
                    &frame_checker,
                    lemma_defs,
                    cache,
                );
                results.push(result);
            }
            ClauseKind::Other(kind) => {
                // Dispatch to feature-specific verifier
                let feature_results = crate::smt_features::verify_feature_clause(
                    kind,
                    contract_name,
                    &clause.body,
                    clauses,
                );
                results.extend(feature_results);
            }
            _ => {}
        }
    }

    results
}

#[cfg(feature = "cvc5-verify")]
#[expect(clippy::too_many_arguments)]
fn check_clause_cvc5_native(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    frame_checker: &assura_types::FrameChecker,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    cache: &mut SessionCache,
) -> VerificationResult {
    // Check cache first (#253)
    let cache_key = format!("{desc}::{kind:?}:{ensures_body:?}");
    if let Some(entry) = cache.lookup(&cache_key) {
        return match entry.result.as_str() {
            "verified" => VerificationResult::Verified {
                clause_desc: desc.to_string(),
            },
            other => VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: format!("cached: {other}"),
            },
        };
    }

    // Pre-check for unmodelable features (matching Z3 backend behavior).
    // Skip clauses with typestate annotations etc. before attempting encoding,
    // preventing false counterexamples from partial encoding.
    if expr_has_unmodelable_features_cvc5(ensures_body) {
        let reasons = collect_unmodelable_reasons_cvc5(ensures_body);
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: format!(
                "clause uses features not yet encoded in SMT ({})",
                reasons.join(", ")
            ),
        };
    }

    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    // Collect all variable names
    let mut var_names = HashSet::new();
    for req in requires {
        collect_vars(req, &mut var_names);
    }
    collect_vars(ensures_body, &mut var_names);

    // Create CVC5 constants for each variable
    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    for name in &var_names {
        let term = tm.mk_const(tm.integer_sort(), name);
        var_map.insert(name.clone(), term);
    }

    // Bind feature_max constants to concrete values (#257)
    for (name, value) in constants {
        let key = sanitize_smtlib_name(name);
        var_map.insert(key, tm.mk_integer(*value));
    }

    // Assert type-level constraints (Nat params get >= 0)
    let zero = tm.mk_integer(0);
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if let Some(term) = var_map.get(&name) {
                let geq = tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]);
                solver.assert_formula(geq);
            }
        }
    }
    // Nat return type constrains result >= 0
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if let Some(term) = var_map.get("__result") {
            let geq = tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]);
            solver.assert_formula(geq);
        }
        // Also constrain "result" (different encoding paths use different names)
        if let Some(term) = var_map.get("result") {
            let geq = tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero]);
            solver.assert_formula(geq);
        }
    }

    // Assert refinement narrowings: for each (name, max_value), assert name <= max_value (#257)
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if let Some(var) = var_map.get(&key) {
            let upper = tm.mk_integer(*value);
            solver.assert_formula(tm.mk_term(cvc5::Kind::Leq, &[var.clone(), upper]));
        }
    }

    let mut enc_state = Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
    };

    // Assert requires as assumptions
    for req in requires {
        if let Some(term) = encode_expr_cvc5(&tm, req, &var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    // Inject lemma postconditions for any `apply lemma_name(args)` references
    // in the ensures body. This mirrors the Z3 backend behavior (verify.rs
    // lines 214-224): each referenced lemma's ensures clauses are asserted
    // as assumptions so the solver can use them during verification.
    if let Some(defs) = lemma_defs {
        let apply_refs = collect_apply_refs_from_expr(ensures_body);
        for lemma_name in &apply_refs {
            if let Some(ensures_bodies) = defs.get(lemma_name) {
                for ens_body in ensures_bodies {
                    if let Some(term) = encode_expr_cvc5(&tm, ens_body, &var_map, &mut enc_state) {
                        solver.assert_formula(term);
                    }
                }
            }
        }
    }

    // Encode the clause body
    let body_term = match encode_expr_cvc5(&tm, ensures_body, &var_map, &mut enc_state) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

    // Assert background axioms collected during encoding
    for axiom in &enc_state.axioms {
        solver.assert_formula(axiom.clone());
    }

    // Frame axioms: for ensures with modifies, assert var == old_var for unmodified vars
    if kind == ClauseKind::Ensures && frame_checker.has_modifies() {
        let frame_vars = frame_checker.frame_axiom_vars(ensures_body);
        for var_name in &frame_vars {
            let current_key = sanitize_smtlib_name(var_name);
            let old_key = sanitize_smtlib_name(&format!("{var_name}__old"));
            let current = var_map
                .get(&current_key)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &current_key));
            let old_var = var_map
                .get(&old_key)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_key));
            let axiom = tm.mk_term(cvc5::Kind::Equal, &[current, old_var]);
            solver.assert_formula(axiom);
        }
    }

    // Assert clause according to verification semantics
    match kind {
        ClauseKind::Invariant => {
            // Invariant: check satisfiability (not always false)
            solver.assert_formula(body_term);
        }
        ClauseKind::MustNot => {
            // MustNot P: assert P directly; UNSAT means P is impossible
            solver.assert_formula(body_term);
        }
        _ => {
            // Ensures/rule/decreases: check validity via negation
            let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
            solver.assert_formula(negated);
        }
    }

    let sat_result = solver.check_sat();

    let result = if sat_result.is_unsat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: "invariant is unsatisfiable".to_string(),
                counter_model: None,
            }
        } else {
            VerificationResult::Verified {
                clause_desc: desc.to_string(),
            }
        }
    } else if sat_result.is_sat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::Verified {
                clause_desc: desc.to_string(),
            }
        } else {
            // Extract counterexample model, filtering internal variables
            // and sorting alphabetically (matching Z3 backend behavior)
            let mut variables: Vec<(String, String)> = var_map
                .iter()
                .filter(|(name, _)| !is_internal_cvc5_var(name))
                .map(|(name, term)| {
                    let val = solver.get_value(term.clone());
                    (name.clone(), val.to_string())
                })
                .collect();
            variables.sort_by(|(a, _), (b, _)| a.cmp(b));
            let model_str = variables
                .iter()
                .map(|(n, v)| format!("{n} = {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            let counter_model = if variables.is_empty() {
                None
            } else {
                Some(CounterexampleModel { variables })
            };
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: model_str,
                counter_model,
            }
        }
    } else {
        // Unknown/timeout
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    };

    // Insert result into session cache (#253)
    let result_str = match &result {
        VerificationResult::Verified { .. } => "verified",
        VerificationResult::Counterexample { .. } => "counterexample",
        VerificationResult::Timeout { .. } => "timeout",
        VerificationResult::Unknown { .. } => "unknown",
    };
    cache.insert(cache_key, result_str.to_string(), 0);

    result
}

/// Hash a pattern name to a stable i64 for CVC5 match encoding.
///
/// Uses FNV-1a (same algorithm as the Z3 backend) for determinism across
/// Rust versions. Constructor variant names are hashed to integer tags so
/// that `match` arms can be encoded as ITE chains comparing the scrutinee
/// against the tag value.
#[cfg(feature = "cvc5-verify")]
fn pattern_hash_cvc5(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325; // FNV offset basis
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3); // FNV prime
    }
    hash as i64
}

/// Bind pattern variables as fresh CVC5 integer constants so they are
/// available when encoding the match arm body.
///
/// Recursively walks `Constructor` and `Tuple` sub-patterns. Wildcard
/// and literal patterns introduce no new bindings.
#[cfg(feature = "cvc5-verify")]
fn bind_pattern_vars_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    pattern: &Pattern,
    vars: &mut HashMap<String, cvc5::Term<'a>>,
) {
    match pattern {
        Pattern::Ident(name) => {
            if !vars.contains_key(name) {
                let v = tm.mk_const(tm.integer_sort(), name);
                vars.insert(name.clone(), v);
            }
        }
        Pattern::Constructor { fields, .. } => {
            for field in fields {
                bind_pattern_vars_cvc5(tm, field, vars);
            }
        }
        Pattern::Tuple(pats) => {
            for pat in pats {
                bind_pattern_vars_cvc5(tm, pat, vars);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) => {}
    }
}

/// Encode an AST expression as a CVC5 Term using the native API.
///
/// `state` collects background axioms and tracks string constants
/// so that `check_clause_cvc5_native` can assert them before check_sat.
#[cfg(feature = "cvc5-verify")]
fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            let val: i64 = n.parse().ok()?;
            Some(tm.mk_integer(val))
        }
        Expr::Literal(Literal::Bool(b)) => Some(tm.mk_boolean(*b)),
        Expr::Literal(Literal::Float(f_str)) => {
            // Rational approximation matching Z3 backend (Real sort)
            let f: f64 = f_str.parse().unwrap_or(0.0);
            let denom = 1_000_000i64;
            let numer = (f * denom as f64) as i64;
            Some(tm.mk_real(numer, denom))
        }
        Expr::Literal(Literal::Str(s)) => {
            // Named integer constant matching Z3 pattern
            let const_name = format!("__str_{s}");
            let str_val = tm.mk_const(tm.integer_sort(), &const_name);
            // Pairwise distinctness from previously seen string constants
            if !state.string_constants.contains(&const_name) {
                for prev in &state.string_constants {
                    let prev_val = tm.mk_const(tm.integer_sort(), prev);
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[str_val.clone(), prev_val]);
                    let neq = tm.mk_term(cvc5::Kind::Not, &[eq]);
                    state.axioms.push(neq);
                }
                state.string_constants.push(const_name);
            }
            // String length axiom: len("hello") == 5
            let len_name = "__field_len";
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, len_name);
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
            let str_len = tm.mk_integer(s.len() as i64);
            let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result, str_len]);
            state.axioms.push(len_eq);
            Some(str_val)
        }
        Expr::Ident(name) => {
            let key = if name == "result" {
                "__result".to_string()
            } else {
                sanitize_smtlib_name(name)
            };
            vars.get(&key)
                .cloned()
                .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)))
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = encode_expr_cvc5(tm, lhs, vars, state)?;
            let r = encode_expr_cvc5(tm, rhs, vars, state)?;
            let kind = match op {
                BinOp::Add => cvc5::Kind::Add,
                BinOp::Sub => cvc5::Kind::Sub,
                BinOp::Mul => cvc5::Kind::Mult,
                BinOp::Div => cvc5::Kind::IntsDivision,
                BinOp::Mod => cvc5::Kind::IntsModulus,
                BinOp::Eq => cvc5::Kind::Equal,
                BinOp::Neq => {
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[l, r]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[eq]));
                }
                BinOp::Lt => cvc5::Kind::Lt,
                BinOp::Lte => cvc5::Kind::Leq,
                BinOp::Gt => cvc5::Kind::Gt,
                BinOp::Gte => cvc5::Kind::Geq,
                BinOp::And => cvc5::Kind::And,
                BinOp::Or => cvc5::Kind::Or,
                BinOp::Implies => cvc5::Kind::Implies,
                BinOp::Range => {
                    // Range (a..b): create a fresh Int constrained to [lhs, rhs)
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let fresh = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[fresh.clone(), l]);
                    let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[fresh.clone(), r]);
                    let in_range = tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
                    state.axioms.push(in_range);
                    return Some(fresh);
                }
                BinOp::In => {
                    // In (elem in collection): UF __contains(collection, elem) -> Bool
                    let func_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
                    let contains = tm.mk_const(func_sort, "__contains");
                    return Some(tm.mk_term(cvc5::Kind::ApplyUf, &[contains, r, l]));
                }
                BinOp::NotIn => {
                    // NotIn: negation of In
                    let func_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
                    let contains = tm.mk_const(func_sort, "__contains");
                    let in_result = tm.mk_term(cvc5::Kind::ApplyUf, &[contains, r, l]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[in_result]));
                }
                BinOp::Concat => {
                    // Concat (a ++ b): fresh value with length axiom
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), l]);
                    let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), r]);
                    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let sum = tm.mk_term(cvc5::Kind::Add, &[len_l.clone(), len_r.clone()]);
                    let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]);
                    state.axioms.push(len_eq);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_l, zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_r, zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                    return Some(result);
                }
            };
            Some(tm.mk_term(kind, &[l, r]))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, vars, state)?;
            match op {
                UnaryOp::Not => Some(tm.mk_term(cvc5::Kind::Not, &[e])),
                UnaryOp::Neg => Some(tm.mk_term(cvc5::Kind::Neg, &[e])),
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = encode_expr_cvc5(tm, cond, vars, state)?;
            let t = encode_expr_cvc5(tm, then_branch, vars, state)?;
            if let Some(eb) = else_branch {
                let e = encode_expr_cvc5(tm, eb, vars, state)?;
                // Sort promotion: if one branch is Real and the other Integer, promote
                let t_sort = t.get_sort();
                let e_sort = e.get_sort();
                let (t_final, e_final) = if t_sort == tm.real_sort() && e_sort == tm.integer_sort()
                {
                    (t, tm.mk_term(cvc5::Kind::ToReal, &[e]))
                } else if t_sort == tm.integer_sort() && e_sort == tm.real_sort() {
                    (tm.mk_term(cvc5::Kind::ToReal, &[t]), e)
                } else {
                    (t, e)
                };
                Some(tm.mk_term(cvc5::Kind::Ite, &[c, t_final, e_final]))
            } else {
                Some(tm.mk_term(cvc5::Kind::Implies, &[c, t]))
            }
        }
        Expr::Forall { var, domain, body } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name.clone(), bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &local_vars, state)?;
            let guarded = guard_quantifier_body_cvc5(tm, domain, &bound_var, b, true, vars, state);
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var.clone()]);
            let trigger_terms = infer_quantifier_patterns_cvc5(tm, body, &v_name, &bound_var);
            if trigger_terms.is_empty() {
                Some(tm.mk_term(cvc5::Kind::Forall, &[bound_list, guarded]))
            } else {
                let inst_pattern = tm.mk_term(cvc5::Kind::InstPattern, &trigger_terms);
                Some(tm.mk_term(cvc5::Kind::Forall, &[bound_list, guarded, inst_pattern]))
            }
        }
        Expr::Exists { var, domain, body } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name.clone(), bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &local_vars, state)?;
            let guarded = guard_quantifier_body_cvc5(tm, domain, &bound_var, b, false, vars, state);
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var.clone()]);
            let trigger_terms = infer_quantifier_patterns_cvc5(tm, body, &v_name, &bound_var);
            if trigger_terms.is_empty() {
                Some(tm.mk_term(cvc5::Kind::Exists, &[bound_list, guarded]))
            } else {
                let inst_pattern = tm.mk_term(cvc5::Kind::InstPattern, &trigger_terms);
                Some(tm.mk_term(cvc5::Kind::Exists, &[bound_list, guarded, inst_pattern]))
            }
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                let f_name = sanitize_smtlib_name(name);
                if args.is_empty() {
                    return vars
                        .get(&f_name)
                        .cloned()
                        .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)));
                }
                let encoded_args: Option<Vec<cvc5::Term>> = args
                    .iter()
                    .map(|a| encode_expr_cvc5(tm, a, vars, state))
                    .collect();
                let encoded_args = encoded_args?;
                // Built-in functions with known semantics
                match f_name.as_str() {
                    // abs(x) => ite(x >= 0, x, -x)
                    "abs" if encoded_args.len() == 1 => {
                        let x = &encoded_args[0];
                        let zero = tm.mk_integer(0);
                        let neg = tm.mk_term(cvc5::Kind::Neg, &[x.clone()]);
                        let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
                        return Some(tm.mk_term(cvc5::Kind::Ite, &[cond, x.clone(), neg]));
                    }
                    // min(a, b) => ite(a <= b, a, b)
                    "min" if encoded_args.len() == 2 => {
                        let (a, b) = (&encoded_args[0], &encoded_args[1]);
                        let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
                        return Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]));
                    }
                    // max(a, b) => ite(a >= b, a, b)
                    "max" if encoded_args.len() == 2 => {
                        let (a, b) = (&encoded_args[0], &encoded_args[1]);
                        let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
                        return Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]));
                    }
                    _ => {}
                }
                // String methods with known semantics
                match f_name.as_str() {
                    // substring(str, start, end): fresh Int with length == end - start
                    "substring" | "substr" if encoded_args.len() == 3 => {
                        let str_val = &encoded_args[0];
                        let start = &encoded_args[1];
                        let end = &encoded_args[2];
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        let zero = tm.mk_integer(0);
                        // 0 <= start
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[start.clone(), zero.clone()]));
                        // start <= end
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Leq, &[start.clone(), end.clone()]));
                        // end <= len(str)
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let str_len =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Leq, &[end.clone(), str_len]));
                        // len(result) == end - start
                        let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                        let diff = tm.mk_term(cvc5::Kind::Sub, &[end.clone(), start.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Equal, &[res_len.clone(), diff]));
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
                        return Some(result);
                    }
                    // concat(a, b): fresh Int with len(result) == len(a) + len(b)
                    "concat" if encoded_args.len() == 2 => {
                        let l = &encoded_args[0];
                        let r = &encoded_args[1];
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), l.clone()]);
                        let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), r.clone()]);
                        let len_result =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[len_l.clone(), zero.clone()]));
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[len_r.clone(), zero.clone()]));
                        let sum = tm.mk_term(cvc5::Kind::Add, &[len_l, len_r]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]));
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                        return Some(result);
                    }
                    // indexOf/find/index_of(str, substr): fresh Int with -1 <= result < len(str)
                    "index_of" | "find" | "indexOf" if encoded_args.len() == 2 => {
                        let str_val = &encoded_args[0];
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        let neg_one = tm.mk_integer(-1);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), neg_one]));
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Lt, &[result.clone(), str_len]));
                        return Some(result);
                    }
                    // charAt/char_at(str, idx): fresh Int with 0 <= idx < len(str)
                    "char_at" | "charAt" if encoded_args.len() == 2 => {
                        let str_val = &encoded_args[0];
                        let idx = &encoded_args[1];
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[idx.clone(), zero]));
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Lt, &[idx.clone(), str_len]));
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        return Some(tm.mk_const(tm.integer_sort(), &fresh_name));
                    }
                    // replace(str, old, new): fresh Int with len(result) >= 0
                    "replace" if encoded_args.len() == 3 => {
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
                        return Some(result);
                    }
                    // split(str, delim): fresh Int with len(result) >= 1
                    "split" if encoded_args.len() == 2 => {
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                        let one = tm.mk_integer(1);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, one]));
                        return Some(result);
                    }
                    // trim/to_lowercase/to_uppercase: 0 <= len(result) <= len(str)
                    "trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper"
                        if encoded_args.len() == 1 =>
                    {
                        let str_val = &encoded_args[0];
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let str_len =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
                        let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[res_len.clone(), zero]));
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Leq, &[res_len, str_len]));
                        return Some(result);
                    }
                    // set(arr, i, v): fresh with get(result, i) == v, len(result) == len(arr)
                    "set" if encoded_args.len() == 3 => {
                        let arr = &encoded_args[0];
                        let i = &encoded_args[1];
                        let v = &encoded_args[2];
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        // get(result, i) == v
                        let get_sort = tm.mk_fun_sort(
                            &[tm.integer_sort(), tm.integer_sort()],
                            tm.integer_sort(),
                        );
                        let get_func = tm.mk_const(get_sort, "get");
                        let get_result_i =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), i.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_i, v.clone()]));
                        // len(result) == len(arr)
                        let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let len_func = tm.mk_const(len_sort, "__field_len");
                        let len_result =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), result.clone()]);
                        let len_arr = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, arr.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), len_arr]));
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                        return Some(result);
                    }
                    // put(map, k, v): fresh with get(result, k) == v, size(result) >= size(map)
                    "put" if encoded_args.len() == 3 => {
                        let map = &encoded_args[0];
                        let k = &encoded_args[1];
                        let v = &encoded_args[2];
                        let fresh_name = format!("__fresh_{}", state.fresh_counter);
                        state.fresh_counter += 1;
                        let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                        // get(result, k) == v
                        let get_sort = tm.mk_fun_sort(
                            &[tm.integer_sort(), tm.integer_sort()],
                            tm.integer_sort(),
                        );
                        let get_func = tm.mk_const(get_sort, "get");
                        let get_result_k =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), k.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_k, v.clone()]));
                        // size(result) >= size(map)
                        let size_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                        let size_func = tm.mk_const(size_sort, "size");
                        let size_result =
                            tm.mk_term(cvc5::Kind::ApplyUf, &[size_func.clone(), result.clone()]);
                        let size_map = tm.mk_term(cvc5::Kind::ApplyUf, &[size_func, map.clone()]);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[size_result.clone(), size_map]));
                        let zero = tm.mk_integer(0);
                        state
                            .axioms
                            .push(tm.mk_term(cvc5::Kind::Geq, &[size_result, zero]));
                        return Some(result);
                    }
                    _ => {}
                }
                // Boolean methods return Bool sort
                if matches!(
                    f_name.as_str(),
                    "contains"
                        | "is_empty"
                        | "is_some"
                        | "is_none"
                        | "is_ok"
                        | "is_err"
                        | "any"
                        | "all"
                        | "contains_key"
                        | "starts_with"
                        | "ends_with"
                        | "is_subset"
                        | "is_superset"
                ) {
                    let domain: Vec<cvc5::Sort> =
                        (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                    let func_sort = tm.mk_fun_sort(&domain, tm.boolean_sort());
                    let func_const = tm.mk_const(func_sort, &f_name);
                    let mut apply_args = vec![func_const];
                    apply_args.extend(encoded_args);
                    return Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args));
                }
                // Size methods get non-negativity axiom
                if matches!(
                    f_name.as_str(),
                    "len" | "length" | "size" | "count" | "capacity"
                ) {
                    let domain: Vec<cvc5::Sort> =
                        (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                    let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                    let func_const = tm.mk_const(func_sort, &f_name);
                    let mut apply_args = vec![func_const];
                    apply_args.extend(encoded_args);
                    let result = tm.mk_term(cvc5::Kind::ApplyUf, &apply_args);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                    return Some(result);
                }
                // Default: uninterpreted function (Int, ..., Int) -> Int
                let domain: Vec<cvc5::Sort> =
                    (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &f_name);
                let mut apply_args = vec![func_const];
                apply_args.extend(encoded_args);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args))
            } else {
                None
            }
        }
        // old(expr): add __old suffix for Ident, recurse for Field/MethodCall
        Expr::Old(inner) => match inner.as_ref() {
            Expr::Ident(name) => {
                let old_name = format!("{name}__old");
                let key = sanitize_smtlib_name(&old_name);
                Some(
                    vars.get(&key)
                        .cloned()
                        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
                )
            }
            Expr::Field(obj, field) => {
                // Deep chain flattening for old(a.b.c) -> a__b__c__old (#250)
                let full_expr = Expr::Field(obj.clone(), field.clone());
                if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(obj) {
                    let flat_name = flatten_field_chain_cvc5(&full_expr);
                    return Some(tm.mk_const(tm.integer_sort(), &format!("{flat_name}__old")));
                }
                let old_obj = encode_expr_cvc5(tm, &Expr::Old(obj.clone()), vars, state)?;
                let func_name = format!("__field_{field}");
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_obj]))
            }
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let old_recv = encode_expr_cvc5(tm, &Expr::Old(receiver.clone()), vars, state)?;
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, method);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_recv]))
            }
            _ => encode_expr_cvc5(tm, inner, vars, state),
        },
        Expr::Paren(inner) | Expr::Ghost(inner) => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Cast { expr: inner, .. } => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Let {
            name, value, body, ..
        } => {
            let v = encode_expr_cvc5(tm, value, vars, state)?;
            let mut local_vars = vars.clone();
            local_vars.insert(sanitize_smtlib_name(name), v);
            encode_expr_cvc5(tm, body, &local_vars, state)
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            if arms.is_empty() {
                return None;
            }
            let s = encode_expr_cvc5(tm, scrutinee, vars, state)?;
            let mut result: Option<cvc5::Term> = None;
            for arm in arms.iter().rev() {
                match &arm.pattern {
                    Pattern::Wildcard => {
                        let body = encode_expr_cvc5(tm, &arm.body, vars, state)?;
                        result = Some(body);
                    }
                    Pattern::Ident(name) => {
                        // Bind the name as a fresh variable
                        let mut local_vars = vars.clone();
                        bind_pattern_vars_cvc5(tm, &arm.pattern, &mut local_vars);
                        let body = encode_expr_cvc5(tm, &arm.body, &local_vars, state)?;
                        // Uppercase-initial ident = constructor name -> hash match
                        if name.starts_with(|c: char| c.is_uppercase()) {
                            let tag_hash = pattern_hash_cvc5(name);
                            let tag_val = tm.mk_integer(tag_hash);
                            let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val]);
                            if let Some(default) = result.as_ref() {
                                result = Some(
                                    tm.mk_term(cvc5::Kind::Ite, &[cond, body, default.clone()]),
                                );
                            } else {
                                result = Some(body);
                            }
                        } else {
                            // Lowercase ident = variable binding = catch-all
                            result = Some(body);
                        }
                    }
                    Pattern::Literal(lit) => {
                        let body = encode_expr_cvc5(tm, &arm.body, vars, state)?;
                        let lit_term = match lit {
                            Literal::Int(n) => {
                                let val: i64 = n.parse().ok()?;
                                tm.mk_integer(val)
                            }
                            Literal::Bool(b) => tm.mk_boolean(*b),
                            _ => return None,
                        };
                        let default = result.as_ref()?.clone();
                        let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), lit_term]);
                        result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
                    }
                    Pattern::Constructor { name, fields } => {
                        // Hash-based tag matching (same as Z3 backend)
                        let tag_hash = pattern_hash_cvc5(name);
                        let tag_val = tm.mk_integer(tag_hash);
                        let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), tag_val]);
                        // Bind field variables as fresh integer constants
                        let mut local_vars = vars.clone();
                        for field in fields {
                            bind_pattern_vars_cvc5(tm, field, &mut local_vars);
                        }
                        let body = encode_expr_cvc5(tm, &arm.body, &local_vars, state)?;
                        let default = result.as_ref()?.clone();
                        result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
                    }
                    Pattern::Tuple(pats) => {
                        // Bind each tuple element as a fresh variable
                        let mut local_vars = vars.clone();
                        for pat in pats {
                            bind_pattern_vars_cvc5(tm, pat, &mut local_vars);
                        }
                        let body = encode_expr_cvc5(tm, &arm.body, &local_vars, state)?;
                        // Tuple match is structural (always matches)
                        result = Some(body);
                    }
                }
            }
            result
        }
        // Field access: flatten deep chains or self-rooted, else UF
        Expr::Field(obj, field) => {
            // Deep field chain flattening (#250): state.head.extra.max -> state__head__extra__max
            let full_expr = Expr::Field(Box::new(obj.as_ref().clone()), field.clone());
            if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(obj) {
                let flat_name = flatten_field_chain_cvc5(&full_expr);
                // Boolean-valued fields at any depth
                if matches!(
                    field.as_str(),
                    "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
                ) {
                    return Some(tm.mk_const(tm.boolean_sort(), &flat_name));
                }
                // Size fields at any depth get non-negativity axiom
                if matches!(
                    field.as_str(),
                    "len" | "length" | "size" | "capacity" | "count"
                ) {
                    let v = tm.mk_const(tm.integer_sort(), &flat_name);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
                    return Some(v);
                }
                // General field: Integer variable
                return Some(tm.mk_const(tm.integer_sort(), &flat_name));
            }
            // Shallow field access: UF __field_name(receiver)
            let obj_val = encode_expr_cvc5(tm, obj, vars, state)?;
            let func_name = format!("__field_{field}");
            // Boolean fields return Bool sort
            if matches!(
                field.as_str(),
                "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
            ) {
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.boolean_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                return Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]));
            }
            // Size fields get non-negativity axiom
            if matches!(
                field.as_str(),
                "len" | "length" | "size" | "capacity" | "count"
            ) {
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                return Some(result);
            }
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &func_name);
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]))
        }
        // Index: UF __index(collection, index) with bounds axioms
        Expr::Index { expr: coll, index } => {
            let coll_val = encode_expr_cvc5(tm, coll, vars, state)?;
            let idx_val = encode_expr_cvc5(tm, index, vars, state)?;
            let zero = tm.mk_integer(0);
            // 0 <= index
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[idx_val.clone(), zero.clone()]));
            // len(collection) via UF
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, "__len");
            let len_val = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, coll_val.clone()]);
            // len >= 0
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
            // index < len
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[idx_val.clone(), len_val]));
            // UF __index(coll, idx)
            let idx_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let idx_func = tm.mk_const(idx_sort, "__index");
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[idx_func, coll_val, idx_val]))
        }
        // Block: encode all expressions, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            let mut result = None;
            for e in body {
                result = encode_expr_cvc5(tm, e, vars, state);
            }
            result
        }
        // Raw tokens: basic parsing (single token bools/ints/idents)
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            if tokens.len() == 1 {
                let t = &tokens[0];
                if t == "true" {
                    return Some(tm.mk_boolean(true));
                }
                if t == "false" {
                    return Some(tm.mk_boolean(false));
                }
                if let Ok(n) = t.parse::<i64>() {
                    return Some(tm.mk_integer(n));
                }
                let key = sanitize_smtlib_name(t);
                return vars
                    .get(&key)
                    .cloned()
                    .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)));
            }
            // Multi-token: try to parse as infix expression
            encode_raw_tokens_cvc5(tm, tokens, vars, state)
        }
        // Tuple: fresh Int with element-access axioms
        Expr::Tuple(elems) => {
            let tuple_name = format!("__tuple_{}", state.fresh_counter);
            state.fresh_counter += 1;
            let tuple_val = tm.mk_const(tm.integer_sort(), &tuple_name);
            let arity = elems.len();
            for (i, elem) in elems.iter().enumerate() {
                if let Some(elem_val) = encode_expr_cvc5(tm, elem, vars, state) {
                    let accessor_name = format!("__tuple_{arity}_{i}");
                    let acc_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let acc_func = tm.mk_const(acc_sort, &accessor_name);
                    let accessed = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_func, tuple_val.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val]));
                }
            }
            Some(tuple_val)
        }
        // MethodCall: prepend receiver, call UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_val = encode_expr_cvc5(tm, receiver, vars, state)?;
            let mut all_encoded = vec![recv_val];
            for arg in args {
                all_encoded.push(encode_expr_cvc5(tm, arg, vars, state)?);
            }
            let f_name = sanitize_smtlib_name(method);
            // String methods with known semantics (method call form)
            // all_encoded[0] is the receiver, remaining are args
            match f_name.as_str() {
                // receiver.substring(start, end)
                "substring" | "substr" if all_encoded.len() == 3 => {
                    let str_val = &all_encoded[0];
                    let start = &all_encoded[1];
                    let end = &all_encoded[2];
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[start.clone(), zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Leq, &[start.clone(), end.clone()]));
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let str_len =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Leq, &[end.clone(), str_len]));
                    let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let diff = tm.mk_term(cvc5::Kind::Sub, &[end.clone(), start.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[res_len.clone(), diff]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
                    return Some(result);
                }
                // receiver.concat(other)
                "concat" if all_encoded.len() == 2 => {
                    let l = &all_encoded[0];
                    let r = &all_encoded[1];
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), l.clone()]);
                    let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), r.clone()]);
                    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_l.clone(), zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_r.clone(), zero.clone()]));
                    let sum = tm.mk_term(cvc5::Kind::Add, &[len_l, len_r]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                    return Some(result);
                }
                // receiver.indexOf(sub) / receiver.find(sub) / receiver.index_of(sub)
                "index_of" | "find" | "indexOf" if all_encoded.len() == 2 => {
                    let str_val = &all_encoded[0];
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let neg_one = tm.mk_integer(-1);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), neg_one]));
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Lt, &[result.clone(), str_len]));
                    return Some(result);
                }
                // receiver.charAt(idx) / receiver.char_at(idx)
                "char_at" | "charAt" if all_encoded.len() == 2 => {
                    let str_val = &all_encoded[0];
                    let idx = &all_encoded[1];
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[idx.clone(), zero]));
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let str_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Lt, &[idx.clone(), str_len]));
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    return Some(tm.mk_const(tm.integer_sort(), &fresh_name));
                }
                // receiver.replace(old, new)
                "replace" if all_encoded.len() == 3 => {
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, zero]));
                    return Some(result);
                }
                // receiver.split(delim)
                "split" if all_encoded.len() == 2 => {
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let one = tm.mk_integer(1);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[res_len, one]));
                    return Some(result);
                }
                // receiver.trim/to_lowercase/to_uppercase: 0 <= len(result) <= len(receiver)
                "trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper"
                    if all_encoded.len() == 1 =>
                {
                    let str_val = &all_encoded[0];
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let str_len =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), str_val.clone()]);
                    let res_len = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[res_len.clone(), zero]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Leq, &[res_len, str_len]));
                    return Some(result);
                }
                // receiver.set(i, v): array set
                "set" if all_encoded.len() == 3 => {
                    let arr = &all_encoded[0];
                    let i = &all_encoded[1];
                    let v = &all_encoded[2];
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let get_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
                    let get_func = tm.mk_const(get_sort, "get");
                    let get_result_i =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), i.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_i, v.clone()]));
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let len_result =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), result.clone()]);
                    let len_arr = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, arr.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), len_arr]));
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                    return Some(result);
                }
                // receiver.put(k, v): map put
                "put" if all_encoded.len() == 3 => {
                    let map = &all_encoded[0];
                    let k = &all_encoded[1];
                    let v = &all_encoded[2];
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let get_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
                    let get_func = tm.mk_const(get_sort, "get");
                    let get_result_k =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[get_func, result.clone(), k.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[get_result_k, v.clone()]));
                    let size_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let size_func = tm.mk_const(size_sort, "size");
                    let size_result =
                        tm.mk_term(cvc5::Kind::ApplyUf, &[size_func.clone(), result.clone()]);
                    let size_map = tm.mk_term(cvc5::Kind::ApplyUf, &[size_func, map.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[size_result.clone(), size_map]));
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[size_result, zero]));
                    return Some(result);
                }
                _ => {}
            }
            // Boolean methods return Bool sort
            if matches!(
                f_name.as_str(),
                "contains"
                    | "is_empty"
                    | "is_some"
                    | "is_none"
                    | "is_ok"
                    | "is_err"
                    | "any"
                    | "all"
                    | "contains_key"
                    | "starts_with"
                    | "ends_with"
                    | "is_subset"
                    | "is_superset"
            ) {
                let domain: Vec<cvc5::Sort> =
                    (0..all_encoded.len()).map(|_| tm.integer_sort()).collect();
                let func_sort = tm.mk_fun_sort(&domain, tm.boolean_sort());
                let func_const = tm.mk_const(func_sort, &f_name);
                let mut apply_args = vec![func_const];
                apply_args.extend(all_encoded);
                return Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args));
            }
            // Size methods get non-negativity axiom
            if matches!(
                f_name.as_str(),
                "len" | "length" | "size" | "count" | "capacity"
            ) {
                let domain: Vec<cvc5::Sort> =
                    (0..all_encoded.len()).map(|_| tm.integer_sort()).collect();
                let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &f_name);
                let mut apply_args = vec![func_const];
                apply_args.extend(all_encoded);
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &apply_args);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                return Some(result);
            }
            // Default: uninterpreted function
            let domain: Vec<cvc5::Sort> =
                (0..all_encoded.len()).map(|_| tm.integer_sort()).collect();
            let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &f_name);
            let mut apply_args = vec![func_const];
            apply_args.extend(all_encoded);
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args))
        }
        // List: fresh Int with element-access and length axioms
        Expr::List(elems) => {
            let list_name = format!("__list_{}", state.fresh_counter);
            state.fresh_counter += 1;
            let list_val = tm.mk_const(tm.integer_sort(), &list_name);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "__list_get");
            for (i, elem) in elems.iter().enumerate() {
                if let Some(elem_val) = encode_expr_cvc5(tm, elem, vars, state) {
                    let idx = tm.mk_integer(i as i64);
                    let accessed = tm.mk_term(
                        cvc5::Kind::ApplyUf,
                        &[get_func.clone(), list_val.clone(), idx],
                    );
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val]));
                }
            }
            // Assert length
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, "__field_len");
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, list_val.clone()]);
            let expected_len = tm.mk_integer(elems.len() as i64);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result, expected_len]));
            Some(list_val)
        }
        // Apply: encode args for side effects, return named bool
        Expr::Apply { lemma_name, args } => {
            for arg in args {
                let _ = encode_expr_cvc5(tm, arg, vars, state);
            }
            let apply_name = format!("__apply_{lemma_name}");
            Some(tm.mk_const(tm.boolean_sort(), &apply_name))
        }
    }
}

/// Build a domain guard for quantifier bodies (CVC5 native API).
///
/// For range domains (`lo..hi`):
/// - `is_forall=true`:  `(lo <= x && x < hi) => body`
/// - `is_forall=false`: `(lo <= x && x < hi) && body`
///
/// For non-range domains (collections, identifiers), encode
/// membership as an uninterpreted `__domain_contains(domain, x)` predicate.
#[cfg(feature = "cvc5-verify")]
fn guard_quantifier_body_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    domain: &Expr,
    bound_var: &cvc5::Term<'a>,
    body: cvc5::Term<'a>,
    is_forall: bool,
    outer_vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    if let Expr::BinOp {
        op: BinOp::Range,
        lhs: lo,
        rhs: hi,
    } = domain
    {
        // Range domain: lo <= x && x < hi
        let lo_val =
            encode_expr_cvc5(tm, lo, outer_vars, state).unwrap_or_else(|| tm.mk_integer(0));
        let hi_val =
            encode_expr_cvc5(tm, hi, outer_vars, state).unwrap_or_else(|| tm.mk_integer(0));
        let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[bound_var.clone(), lo_val]);
        let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[bound_var.clone(), hi_val]);
        let in_range = tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
        if is_forall {
            tm.mk_term(cvc5::Kind::Implies, &[in_range, body])
        } else {
            tm.mk_term(cvc5::Kind::And, &[in_range, body])
        }
    } else {
        // Non-range domain: __domain_contains(domain, x) UF
        let domain_val = encode_expr_cvc5(tm, domain, outer_vars, state)
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), "__domain_unknown"));
        let contains_sort =
            tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
        let contains_fn = tm.mk_const(contains_sort, "__domain_contains");
        let membership = tm.mk_term(
            cvc5::Kind::ApplyUf,
            &[contains_fn, domain_val, bound_var.clone()],
        );
        if is_forall {
            tm.mk_term(cvc5::Kind::Implies, &[membership, body])
        } else {
            tm.mk_term(cvc5::Kind::And, &[membership, body])
        }
    }
}

/// Infer CVC5 trigger patterns from function calls in a quantifier body
/// that reference the bound variable. Returns `InstPattern` terms for
/// e-matching hints that help the solver instantiate quantifiers efficiently.
///
/// First checks the `TriggerManager` for user-provided triggers, then falls
/// back to scanning the body for `Expr::Call` expressions referencing the
/// bound variable.
#[cfg(feature = "cvc5-verify")]
fn infer_quantifier_patterns_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    body: &Expr,
    bound_var_name: &str,
    bound_cvc5: &cvc5::Term<'a>,
) -> Vec<cvc5::Term<'a>> {
    let mut patterns = Vec::new();

    // Check TriggerManager for user-provided or inferred triggers
    let trigger_mgr = crate::advanced::TriggerManager::new();
    let body_str = format!("{body:?}");
    if let Some(trigger) = trigger_mgr.infer_trigger(&body_str) {
        for term in &trigger.terms {
            if let Some(fname) = term.split('(').next() {
                let fname = fname.trim();
                let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func = tm.mk_const(fun_sort, fname);
                let app = tm.mk_term(cvc5::Kind::ApplyUf, &[func, bound_cvc5.clone()]);
                patterns.push(app);
            }
        }
    }

    // Direct scan: look for Call expressions that reference the bound variable
    if patterns.is_empty() {
        collect_trigger_calls_cvc5(tm, body, bound_var_name, bound_cvc5, &mut patterns);
    }

    patterns
}

/// Recursively scan an expression for function calls containing the
/// bound variable, and create CVC5 trigger terms from them.
#[cfg(feature = "cvc5-verify")]
fn collect_trigger_calls_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    bound_var: &str,
    bound_cvc5: &cvc5::Term<'a>,
    patterns: &mut Vec<cvc5::Term<'a>>,
) {
    match expr {
        Expr::Call { func, args } => {
            let refs_bound = args.iter().any(|a| expr_references_var(a, bound_var));
            if refs_bound {
                if let Expr::Ident(fname) = func.as_ref() {
                    let arity = args.len();
                    let param_sorts: Vec<cvc5::Sort> =
                        (0..arity).map(|_| tm.integer_sort()).collect();
                    let param_sort_refs: Vec<&cvc5::Sort> = param_sorts.iter().collect();
                    let fun_sort = tm.mk_fun_sort(&param_sort_refs, tm.integer_sort());
                    let func_decl = tm.mk_const(fun_sort, fname.as_str());
                    let mut uf_args = vec![func_decl];
                    for a in args {
                        if expr_references_var(a, bound_var) {
                            uf_args.push(bound_cvc5.clone());
                        } else {
                            uf_args.push(tm.mk_const(tm.integer_sort(), "__trigger_other"));
                        }
                    }
                    let app = tm.mk_term(cvc5::Kind::ApplyUf, &uf_args);
                    patterns.push(app);
                }
            }
            for a in args {
                collect_trigger_calls_cvc5(tm, a, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_trigger_calls_cvc5(tm, receiver, bound_var, bound_cvc5, patterns);
            for a in args {
                collect_trigger_calls_cvc5(tm, a, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_trigger_calls_cvc5(tm, lhs, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, rhs, bound_var, bound_cvc5, patterns);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
            collect_trigger_calls_cvc5(tm, e, bound_var, bound_cvc5, patterns);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_trigger_calls_cvc5(tm, cond, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, then_branch, bound_var, bound_cvc5, patterns);
            if let Some(eb) = else_branch {
                collect_trigger_calls_cvc5(tm, eb, bound_var, bound_cvc5, patterns);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_trigger_calls_cvc5(tm, e, bound_var, bound_cvc5, patterns);
            collect_trigger_calls_cvc5(tm, index, bound_var, bound_cvc5, patterns);
        }
        _ => {}
    }
}

/// Encode multi-token raw expressions for the native CVC5 backend.
///
/// Uses a full precedence-climbing (Pratt) parser supporting:
/// - 8 precedence levels (matching the AST expression parser)
/// - Parenthesized sub-expressions
/// - `old(expr)` syntax
/// - `forall`/`exists` quantifiers: `forall x in domain { body }`
/// - Comparison chaining: `a < b < c` desugars to `a < b && b < c`
/// - Prefix operators: `!`, `-`, `not`
/// - Dot-separated field access: `x.y.z` -> `x__y__z`
/// - Function calls: `f(a, b)` with built-in semantics for abs/min/max
#[cfg(feature = "cvc5-verify")]
fn encode_raw_tokens_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    if tokens.is_empty() {
        return Some(tm.mk_boolean(true));
    }
    let (val, _pos) = parse_raw_expr_cvc5(tm, tokens, 0, 0, vars, state)?;
    Some(val)
}

/// Return the precedence and CVC5 Kind for a binary operator token.
/// Returns `None` if the token is not a recognized infix operator.
#[cfg(feature = "cvc5-verify")]
fn raw_op_info_cvc5(tok: &str) -> Option<(u8, RawOpCvc5)> {
    match tok {
        "||" | "or" => Some((1, RawOpCvc5::Or)),
        "&&" | "and" => Some((3, RawOpCvc5::And)),
        "=>" | "==>" | "implies" => Some((3, RawOpCvc5::Implies)),
        "==" | "=" => Some((5, RawOpCvc5::Eq)),
        "!=" => Some((5, RawOpCvc5::Neq)),
        "<" => Some((7, RawOpCvc5::Lt)),
        ">" => Some((7, RawOpCvc5::Gt)),
        "<=" => Some((7, RawOpCvc5::Leq)),
        ">=" => Some((7, RawOpCvc5::Geq)),
        "+" => Some((9, RawOpCvc5::Add)),
        "-" => Some((9, RawOpCvc5::Sub)),
        "*" => Some((11, RawOpCvc5::Mul)),
        "/" | "div" => Some((11, RawOpCvc5::Div)),
        "%" | "mod" => Some((11, RawOpCvc5::Mod)),
        _ => None,
    }
}

/// CVC5 raw operator kinds (mirrors Z3 `RawOp`).
#[cfg(feature = "cvc5-verify")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawOpCvc5 {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Neq,
    Lt,
    Leq,
    Gt,
    Geq,
    And,
    Or,
    Implies,
}

#[cfg(feature = "cvc5-verify")]
fn is_comparison_cvc5(op: RawOpCvc5) -> bool {
    matches!(
        op,
        RawOpCvc5::Lt
            | RawOpCvc5::Leq
            | RawOpCvc5::Gt
            | RawOpCvc5::Geq
            | RawOpCvc5::Eq
            | RawOpCvc5::Neq
    )
}

/// Apply a binary operator to two CVC5 terms.
#[cfg(feature = "cvc5-verify")]
fn apply_raw_op_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    op: RawOpCvc5,
    lhs: cvc5::Term<'a>,
    rhs: cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    match op {
        RawOpCvc5::Add => tm.mk_term(cvc5::Kind::Add, &[lhs, rhs]),
        RawOpCvc5::Sub => tm.mk_term(cvc5::Kind::Sub, &[lhs, rhs]),
        RawOpCvc5::Mul => tm.mk_term(cvc5::Kind::Mult, &[lhs, rhs]),
        RawOpCvc5::Div => tm.mk_term(cvc5::Kind::IntsDivision, &[lhs, rhs]),
        RawOpCvc5::Mod => tm.mk_term(cvc5::Kind::IntsModulus, &[lhs, rhs]),
        RawOpCvc5::Eq => tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]),
        RawOpCvc5::Neq => {
            let eq = tm.mk_term(cvc5::Kind::Equal, &[lhs, rhs]);
            tm.mk_term(cvc5::Kind::Not, &[eq])
        }
        RawOpCvc5::Lt => tm.mk_term(cvc5::Kind::Lt, &[lhs, rhs]),
        RawOpCvc5::Leq => tm.mk_term(cvc5::Kind::Leq, &[lhs, rhs]),
        RawOpCvc5::Gt => tm.mk_term(cvc5::Kind::Gt, &[lhs, rhs]),
        RawOpCvc5::Geq => tm.mk_term(cvc5::Kind::Geq, &[lhs, rhs]),
        RawOpCvc5::And => tm.mk_term(cvc5::Kind::And, &[lhs, rhs]),
        RawOpCvc5::Or => tm.mk_term(cvc5::Kind::Or, &[lhs, rhs]),
        RawOpCvc5::Implies => tm.mk_term(cvc5::Kind::Implies, &[lhs, rhs]),
    }
}

/// Precedence-climbing expression parser for raw CVC5 tokens.
///
/// Returns `(term, next_position)`. Recurses with higher `min_prec` for
/// tighter-binding operators. Supports comparison chaining.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    pos: usize,
    min_prec: u8,
    vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_cvc5(tm, tokens, pos, vars, state)?;

    while pos < tokens.len() {
        let Some((op_prec, op_kind)) = raw_op_info_cvc5(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1; // consume operator

        let (rhs, next_pos) = parse_raw_expr_cvc5(tm, tokens, pos, op_prec + 1, vars, state)?;
        pos = next_pos;

        // Comparison chaining: if we just parsed `a < b` and the next
        // op is also a comparison, desugar `a < b < c` into `a < b && b < c`.
        if is_comparison_cvc5(op_kind)
            && pos < tokens.len()
            && let Some((next_prec, next_op)) = raw_op_info_cvc5(tokens[pos].as_str())
            && is_comparison_cvc5(next_op)
            && next_prec >= min_prec
        {
            let left_cmp = apply_raw_op_cvc5(tm, op_kind, lhs, rhs.clone());
            pos += 1; // consume next operator
            let (rhs2, next_pos2) =
                parse_raw_expr_cvc5(tm, tokens, pos, next_prec + 1, vars, state)?;
            pos = next_pos2;
            let right_cmp = apply_raw_op_cvc5(tm, next_op, rhs, rhs2);
            lhs = tm.mk_term(cvc5::Kind::And, &[left_cmp, right_cmp]);
            continue;
        }

        lhs = apply_raw_op_cvc5(tm, op_kind, lhs, rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw CVC5 tokens.
///
/// Handles: parenthesized expressions, `old(expr)`, `forall`/`exists`,
/// prefix operators (`!`, `-`, `not`), boolean/integer literals,
/// `result` keyword, specification keywords (skipped), dot-separated
/// field access, and function calls with built-in semantics.
#[cfg(feature = "cvc5-verify")]
fn parse_raw_atom_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    start: usize,
    vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<(cvc5::Term<'a>, usize)> {
    if start >= tokens.len() {
        // Past end: vacuously true
        return Some((tm.mk_boolean(true), start));
    }

    let tok = &tokens[start];

    // --- Unary not ---
    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Not, &[val]), next));
    }

    // --- Unary minus ---
    if tok == "-" {
        let (val, next) = parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state)?;
        return Some((tm.mk_term(cvc5::Kind::Neg, &[val]), next));
    }

    // --- Parenthesized expression ---
    if tok == "(" {
        let (val, end) = parse_raw_expr_cvc5(tm, tokens, start + 1, 0, vars, state)?;
        // skip closing ')'
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    // --- Boolean literals ---
    if tok == "true" {
        return Some((tm.mk_boolean(true), start + 1));
    }
    if tok == "false" {
        return Some((tm.mk_boolean(false), start + 1));
    }

    // --- `result` keyword ---
    if tok == "result" {
        let key = "__result";
        let v = vars
            .get(key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), key));
        return Some((v, start + 1));
    }

    // --- `old(expr)` ---
    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        // Find matching close paren
        let mut depth = 1usize;
        let mut p = start + 2;
        while p < tokens.len() && depth > 0 {
            match tokens[p].as_str() {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                p += 1;
            }
        }
        let end = p + 1; // skip closing ')'
        let inner_tokens = &tokens[start + 2..p];

        // old(x) -> x__old
        if inner_tokens.len() == 1 {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner_tokens[0]));
            let v = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            return Some((v, end));
        }
        // old(x.field) -> x__old with field access UF
        if inner_tokens.len() == 3 && inner_tokens[1] == "." {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner_tokens[0]));
            let old_var = vars
                .get(&old_name)
                .cloned()
                .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_name));
            let field = sanitize_smtlib_name(&inner_tokens[2]);
            let func_name = format!("__field_{field}");
            let fun_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func = tm.mk_const(fun_sort, &func_name);
            let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func, old_var]);
            return Some((result, end));
        }
        // General old(expr): parse inner expression, remap vars to __old
        // (simplified: parse as-is, create __old-suffixed vars for idents)
        let mut old_vars = vars.clone();
        for inner_tok in inner_tokens {
            if inner_tok
                .chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
                && !matches!(
                    inner_tok.as_str(),
                    "true"
                        | "false"
                        | "old"
                        | "forall"
                        | "exists"
                        | "result"
                        | "not"
                        | "and"
                        | "or"
                        | "implies"
                        | "mod"
                        | "div"
                        | "in"
                )
            {
                let sane = sanitize_smtlib_name(inner_tok);
                let old_key = format!("{sane}__old");
                if !old_vars.contains_key(&old_key) {
                    let term = tm.mk_const(tm.integer_sort(), &old_key);
                    old_vars.insert(old_key, term);
                }
            }
        }
        if let Some((val, _)) = parse_raw_expr_cvc5(tm, inner_tokens, 0, 0, &old_vars, state) {
            return Some((val, end));
        }
        // Fallback: fresh integer
        let fresh_name = format!("__old_fresh_{}", state.fresh_counter);
        state.fresh_counter += 1;
        return Some((tm.mk_const(tm.integer_sort(), &fresh_name), end));
    }

    // --- `forall`/`exists` quantifiers: `forall x in domain { body }` ---
    if (tok == "forall" || tok == "exists") && start + 4 < tokens.len() && tokens[start + 2] == "in"
    {
        let var_name = sanitize_smtlib_name(&tokens[start + 1]);
        let is_forall = tok == "forall";

        // Find body delimiter: either `:` or `{`
        let mut delim_pos = start + 3;
        let mut d = 0usize;
        while delim_pos < tokens.len() {
            match tokens[delim_pos].as_str() {
                "(" => d += 1,
                ")" => d = d.saturating_sub(1),
                ":" | "{" if d == 0 => break,
                _ => {}
            }
            delim_pos += 1;
        }

        if delim_pos < tokens.len() && (tokens[delim_pos] == ":" || tokens[delim_pos] == "{") {
            let body_start = delim_pos + 1;
            let body_end = if tokens[delim_pos] == "{" {
                // Find matching `}`
                let mut bd = 1usize;
                let mut ep = body_start;
                while ep < tokens.len() && bd > 0 {
                    match tokens[ep].as_str() {
                        "{" => bd += 1,
                        "}" => bd -= 1,
                        _ => {}
                    }
                    if bd > 0 {
                        ep += 1;
                    }
                }
                let body_slice_end = ep;
                let final_pos = ep + 1; // skip `}`
                (body_slice_end, final_pos)
            } else {
                // Colon: rest of tokens is body
                (tokens.len(), tokens.len())
            };

            // Bind quantifier variable
            let bound = tm.mk_var(tm.integer_sort(), &var_name);
            let mut local_vars = vars.clone();
            local_vars.insert(var_name.clone(), bound.clone());

            // Parse body
            let body_tokens = &tokens[body_start..body_end.0];
            if let Some((body_val, _)) =
                parse_raw_expr_cvc5(tm, body_tokens, 0, 0, &local_vars, state)
            {
                let var_list = tm.mk_term(cvc5::Kind::VariableList, &[bound]);
                let kind = if is_forall {
                    cvc5::Kind::Forall
                } else {
                    cvc5::Kind::Exists
                };
                let quantified = tm.mk_term(kind, &[var_list, body_val]);
                return Some((quantified, body_end.1));
            }

            return Some((tm.mk_boolean(true), body_end.1));
        }
    }

    // --- Integer literal ---
    if let Ok(n) = tok.parse::<i64>() {
        return Some((tm.mk_integer(n), start + 1));
    }

    // --- Skip specification keywords (taint/ghost/region/validate) ---
    if matches!(
        tok.as_str(),
        "taint" | "untrusted" | "validated" | "ghost" | "Region" | "validate"
    ) {
        return parse_raw_atom_cvc5(tm, tokens, start + 1, vars, state);
    }

    // --- Identifier (possibly with dot-separated field access) ---
    let mut name = sanitize_smtlib_name(tok);
    let mut next = start + 1;
    // Collapse `x.y.z` chains into `x__y__z`
    while next + 1 < tokens.len() && tokens[next] == "." {
        name.push_str("__");
        name.push_str(&sanitize_smtlib_name(&tokens[next + 1]));
        next += 2;
    }

    // Check for function call: `name(args)`
    if next < tokens.len() && tokens[next] == "(" {
        let mut depth = 1usize;
        let mut p = next + 1;
        while p < tokens.len() && depth > 0 {
            match tokens[p].as_str() {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                p += 1;
            }
        }

        // Parse arguments by splitting on commas at depth 0
        let arg_tokens = &tokens[next + 1..p];
        let mut arg_vals: Vec<cvc5::Term<'a>> = Vec::new();
        if !arg_tokens.is_empty() {
            let mut arg_start_idx = 0;
            let mut dd = 0usize;
            for (i, t) in arg_tokens.iter().enumerate() {
                match t.as_str() {
                    "(" => dd += 1,
                    ")" => dd = dd.saturating_sub(1),
                    "," if dd == 0 => {
                        let chunk = &arg_tokens[arg_start_idx..i];
                        if !chunk.is_empty()
                            && let Some((v, _)) = parse_raw_expr_cvc5(tm, chunk, 0, 0, vars, state)
                        {
                            arg_vals.push(v);
                        }
                        arg_start_idx = i + 1;
                    }
                    _ => {}
                }
            }
            // Last argument
            let chunk = &arg_tokens[arg_start_idx..];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_cvc5(tm, chunk, 0, 0, vars, state)
            {
                arg_vals.push(v);
            }
        }
        let end = p + 1; // skip closing ')'

        // Extract base function name (last segment after dots)
        let func_name = name.rsplit("__").next().unwrap_or(&name);

        // Built-in functions
        match func_name {
            "abs" if arg_vals.len() == 1 => {
                let x = arg_vals[0].clone();
                let zero = tm.mk_integer(0);
                let neg_x = tm.mk_term(cvc5::Kind::Neg, &[x.clone()]);
                let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, x, neg_x]), end));
            }
            "min" if arg_vals.len() == 2 => {
                let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
                let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]), end));
            }
            "max" if arg_vals.len() == 2 => {
                let (a, b) = (arg_vals[0].clone(), arg_vals[1].clone());
                let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
                return Some((tm.mk_term(cvc5::Kind::Ite, &[cond, a, b]), end));
            }
            "length" if arg_vals.is_empty() => {
                // x.length() -> UF with length >= 0 axiom
                let uf_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let uf = tm.mk_const(uf_sort, "__length");
                let base_var = vars
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &[uf, base_var]);
                let zero = tm.mk_integer(0);
                let axiom = tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]);
                state.axioms.push(axiom);
                return Some((result, end));
            }
            _ => {
                // Generic UF
                if arg_vals.is_empty() {
                    let v = vars
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
                    return Some((v, end));
                }
                let n_args = arg_vals.len();
                let domain: Vec<_> = (0..n_args).map(|_| tm.integer_sort()).collect();
                let domain_refs: Vec<&_> = domain.iter().collect();
                let fun_sort = tm.mk_fun_sort(&domain_refs, tm.integer_sort());
                let func = tm.mk_const(fun_sort, &name);
                let mut all_args = vec![func];
                all_args.extend(arg_vals);
                return Some((tm.mk_term(cvc5::Kind::ApplyUf, &all_args), end));
            }
        }
    }

    // Plain identifier
    let v = vars
        .get(&name)
        .cloned()
        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &name));
    Some((v, next))
}

// -------------------------------------------------------------------------
// Generic CVC5 validity checker (reusable for standalone functions)
// -------------------------------------------------------------------------

/// Check validity of `body` under `assumptions` using CVC5.
///
/// Encodes: assert all assumptions, negate body, check-sat.
/// UNSAT = body holds (Verified), SAT = counterexample.
///
/// This is the CVC5 equivalent of `z3_backend::solver::check_validity`.
/// Used by standalone entry-point functions (refinement, buffer bounds,
/// taint, measures, termination) and feature clause dispatch.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_validity_cvc5(
    desc: &str,
    assumptions: &[&Expr],
    body: &Expr,
) -> VerificationResult {
    // Pre-check for unmodelable features (matching Z3 backend behavior)
    if expr_has_unmodelable_features_cvc5(body) {
        let reasons = collect_unmodelable_reasons_cvc5(body);
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: format!(
                "clause uses features not yet encoded in SMT ({})",
                reasons.join(", ")
            ),
        };
    }

    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    let mut var_names = std::collections::HashSet::new();
    for a in assumptions {
        collect_vars(a, &mut var_names);
    }
    collect_vars(body, &mut var_names);

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    for name in &var_names {
        let term = tm.mk_const(tm.integer_sort(), name);
        var_map.insert(name.clone(), term);
    }

    let mut enc_state = Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
    };

    // Assert assumptions
    for a in assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, a, &var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    // Encode body
    let body_term = match encode_expr_cvc5(&tm, body, &var_map, &mut enc_state) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

    // Assert background axioms
    for axiom in &enc_state.axioms {
        solver.assert_formula(axiom.clone());
    }

    // Negate body, check-sat: UNSAT = valid
    let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
    solver.assert_formula(negated);

    let sat_result = solver.check_sat();
    if sat_result.is_unsat() {
        VerificationResult::Verified {
            clause_desc: desc.to_string(),
        }
    } else if sat_result.is_sat() {
        // Filter internal variables and sort alphabetically
        let mut variables: Vec<(String, String)> = var_map
            .iter()
            .filter(|(name, _)| !is_internal_cvc5_var(name))
            .map(|(name, term)| {
                let val = solver.get_value(term.clone());
                (name.clone(), val.to_string())
            })
            .collect();
        variables.sort_by(|(a, _), (b, _)| a.cmp(b));
        let model_str = variables
            .iter()
            .map(|(n, v)| format!("{n} = {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        let counter_model = if variables.is_empty() {
            None
        } else {
            Some(crate::result::CounterexampleModel { variables })
        };
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: model_str,
            counter_model,
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// Check satisfiability of `body` under `assumptions` using CVC5.
///
/// For invariants: assert all assumptions + body, check-sat.
/// SAT = invariant is satisfiable (Verified), UNSAT = unsatisfiable (Counterexample).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_satisfiability_cvc5(
    desc: &str,
    assumptions: &[&Expr],
    body: &Expr,
) -> VerificationResult {
    // Pre-check for unmodelable features (matching Z3 backend behavior)
    if expr_has_unmodelable_features_cvc5(body) {
        let reasons = collect_unmodelable_reasons_cvc5(body);
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: format!(
                "clause uses features not yet encoded in SMT ({})",
                reasons.join(", ")
            ),
        };
    }

    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    let mut var_names = std::collections::HashSet::new();
    for a in assumptions {
        collect_vars(a, &mut var_names);
    }
    collect_vars(body, &mut var_names);

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    for name in &var_names {
        let term = tm.mk_const(tm.integer_sort(), name);
        var_map.insert(name.clone(), term);
    }

    let mut enc_state = Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
    };

    for a in assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, a, &var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    let body_term = match encode_expr_cvc5(&tm, body, &var_map, &mut enc_state) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

    for axiom in &enc_state.axioms {
        solver.assert_formula(axiom.clone());
    }

    solver.assert_formula(body_term);

    let sat_result = solver.check_sat();
    if sat_result.is_sat() {
        VerificationResult::Verified {
            clause_desc: desc.to_string(),
        }
    } else if sat_result.is_unsat() {
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: "invariant is unsatisfiable".to_string(),
            counter_model: None,
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// CVC5 implementation of refinement subtype check.
///
/// `{v: T | antecedent} <: {v: T | consequent}`
/// Encodes: (assert antecedent) (assert (not consequent)) (check-sat)
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_refinement_subtype_cvc5(
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    check_validity_cvc5("refinement_subtype", &[antecedent], consequent)
}

/// CVC5 implementation of refinement subtype check with extra context.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_refinement_subtype_with_context_cvc5(
    context: &[Expr],
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    let mut assumptions: Vec<&Expr> = context.iter().collect();
    assumptions.push(antecedent);
    check_validity_cvc5("refinement_subtype_ctx", &assumptions, consequent)
}

/// CVC5 implementation of buffer bounds verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_buffer_bounds_cvc5(requires: &[Expr], ensures: &Expr) -> VerificationResult {
    let assumptions: Vec<&Expr> = requires.iter().collect();
    check_validity_cvc5("buffer_bounds", &assumptions, ensures)
}

/// CVC5 implementation of region containment verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_region_containment_cvc5(
    context: &[Expr],
    sub_lo: &Expr,
    sub_hi: &Expr,
    parent_lo: &Expr,
    parent_hi: &Expr,
) -> VerificationResult {
    // Build: forall i: sub_lo <= i < sub_hi => parent_lo <= i < parent_hi
    // Encode as two validity checks:
    // 1. context => sub_lo >= parent_lo
    // 2. context => sub_hi <= parent_hi
    let lo_check = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(sub_lo.clone()),
        rhs: Box::new(parent_lo.clone()),
    };
    let hi_check = Expr::BinOp {
        op: BinOp::Lte,
        lhs: Box::new(sub_hi.clone()),
        rhs: Box::new(parent_hi.clone()),
    };
    let combined = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(lo_check),
        rhs: Box::new(hi_check),
    };
    let assumptions: Vec<&Expr> = context.iter().collect();
    check_validity_cvc5("region_containment", &assumptions, &combined)
}

/// CVC5 implementation of measure-aware verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_with_measures_cvc5(
    requires: &[Expr],
    ensures: &Expr,
    _measures: &[crate::measures::MeasureDefinition],
) -> VerificationResult {
    // Measures are encoded as uninterpreted functions with axioms.
    // For CVC5, we encode as plain validity check (measure axioms
    // would need to be threaded through the encoder state).
    let assumptions: Vec<&Expr> = requires.iter().collect();
    check_validity_cvc5("verify_with_measures", &assumptions, ensures)
}

/// CVC5 implementation of decrease verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_decrease_cvc5(
    preconditions: &[Expr],
    measure_expr: &Expr,
    call_arg_expr: &Expr,
    clause_desc: String,
) -> VerificationResult {
    // Check: preconditions => measure(call_args) < measure(fn_args) && measure(call_args) >= 0
    let decrease_check = Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(measure_expr.clone()),
    };
    let non_neg = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(Expr::Literal(Literal::Int("0".to_string()))),
    };
    let combined = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(decrease_check),
        rhs: Box::new(non_neg),
    };
    let assumptions: Vec<&Expr> = preconditions.iter().collect();
    check_validity_cvc5(&clause_desc, &assumptions, &combined)
}

/// CVC5 implementation of taint safety verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_taint_safety_cvc5(
    taint_labels: &[(String, assura_types::TaintLabel)],
    _validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    use assura_types::TaintLabel;

    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    let zero = tm.mk_integer(0);
    let one = tm.mk_integer(1);
    let two = tm.mk_integer(2);

    // Create taint level variables
    for (name, label) in taint_labels {
        let level = match label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        var_map.insert(name.clone(), level);
    }

    // Check sensitive uses: each must have taint level >= required
    for (name, required_label) in sensitive_uses {
        let required_level = match required_label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        if let Some(actual) = var_map.get(name) {
            let check = tm.mk_term(cvc5::Kind::Geq, &[actual.clone(), required_level]);
            let neg = tm.mk_term(cvc5::Kind::Not, &[check]);
            // If the negation is satisfiable, the taint check fails
            solver.push(1);
            solver.assert_formula(neg);
            let result = solver.check_sat();
            solver.pop(1);
            if result.is_sat() {
                return VerificationResult::Counterexample {
                    clause_desc: "taint_safety".to_string(),
                    model: format!("{name} has insufficient taint level"),
                    counter_model: None,
                };
            }
        }
    }

    VerificationResult::Verified {
        clause_desc: "taint_safety".to_string(),
    }
}

/// CVC5 implementation of feature clause body verification.
///
/// Used by `smt_features::verify_feature_body` when the CVC5 solver is
/// selected. Collects sibling requires as assumptions, checks body validity.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_feature_body_cvc5(
    parent_name: &str,
    feature_label: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> VerificationResult {
    let desc = format!("{parent_name}: {feature_label}");

    // Skip declarative feature clauses (bare uppercase ident)
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        return VerificationResult::Unknown {
            clause_desc: desc,
            reason: format!("{feature_label} not yet encoded in SMT"),
        };
    }

    let requires: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    check_validity_cvc5(&desc, &requires, body)
}

/// CVC5 implementation of structural invariant inductive checking.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_structural_invariant_inductive_cvc5(
    parent_name: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Skip bare uppercase ident
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("{parent_name}: structural_invariant"),
            reason: "structural_invariant not yet encoded in SMT".into(),
        });
        return results;
    }

    // Step 1: Establishment (requires => invariant)
    let requires: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 = format!("{parent_name}: structural_invariant (establishment)");
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Preservation (requires + ensures => invariant)
    let mut assumptions: Vec<&Expr> = requires;
    let ensures: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    assumptions.extend(ensures);
    let desc2 = format!("{parent_name}: structural_invariant (preservation)");
    results.push(check_validity_cvc5(&desc2, &assumptions, body));

    results
}

// -------------------------------------------------------------------------
// Shell-out CVC5 fallback (no cvc5-verify feature)
// -------------------------------------------------------------------------

#[cfg(not(feature = "cvc5-verify"))]
fn verify_contract_cvc5_shellout(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Derive refinement narrowings from feature_max constants
    let narrowings = derive_narrowings_cvc5(constants);

    let mut requires_exprs: Vec<&Expr> = Vec::new();
    for clause in clauses {
        if clause.kind == ClauseKind::Requires {
            requires_exprs.push(&clause.body);
        }
    }

    // Build frame checker from modifies clauses
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        assura_types::FrameChecker::new(&modifies_bodies)
    };

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases => {
                let desc = format!("{contract_name}::{:?}", clause.kind);
                let result = check_clause_cvc5_shellout(
                    &desc,
                    &requires_exprs,
                    &clause.body,
                    clause.kind.clone(),
                    params,
                    return_ty,
                    constants,
                    &narrowings,
                    &frame_checker,
                    lemma_defs,
                    cache,
                );
                results.push(result);
            }
            ClauseKind::Other(kind) => {
                let feature_results = crate::smt_features::verify_feature_clause(
                    kind,
                    contract_name,
                    &clause.body,
                    clauses,
                );
                results.extend(feature_results);
            }
            _ => {}
        }
    }

    results
}

/// Result of running CVC5 binary on an SMT-LIB2 script.
#[cfg(not(feature = "cvc5-verify"))]
enum Cvc5Result {
    Unsat,
    Sat(String),
    Timeout,
    Error(String),
}

#[cfg(not(feature = "cvc5-verify"))]
#[expect(clippy::too_many_arguments)]
fn check_clause_cvc5_shellout(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    frame_checker: &assura_types::FrameChecker,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    cache: &mut SessionCache,
) -> VerificationResult {
    // Check cache first (#253)
    let cache_key = format!("{desc}::{kind:?}:{ensures_body:?}");
    if let Some(entry) = cache.lookup(&cache_key) {
        return match entry.result.as_str() {
            "verified" => VerificationResult::Verified {
                clause_desc: desc.to_string(),
            },
            other => VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: format!("cached: {other}"),
            },
        };
    }

    // Pre-check for unmodelable features (matching Z3 backend behavior)
    if expr_has_unmodelable_features_cvc5(ensures_body) {
        let reasons = collect_unmodelable_reasons_cvc5(ensures_body);
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: format!(
                "clause uses features not yet encoded in SMT ({})",
                reasons.join(", ")
            ),
        };
    }

    let mut vars = HashSet::new();
    for req in requires {
        collect_vars(req, &mut vars);
    }
    collect_vars(ensures_body, &mut vars);

    let mut script = String::new();
    script.push_str("(set-logic ALL)\n");

    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    // Assert type-level constraints (Nat params get >= 0)
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if vars.contains(&name) {
                script.push_str(&format!("(assert (>= {name} 0))\n"));
            }
        }
    }
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if vars.contains("__result") {
            script.push_str("(assert (>= __result 0))\n");
        }
        // Also constrain "result" (different encoding paths use different names)
        if vars.contains("result") {
            script.push_str("(assert (>= result 0))\n");
        }
    }

    // Bind feature_max constants to concrete values (#257)
    for (name, value) in constants {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            script.push_str(&format!("(assert (= {key} {value}))\n"));
        }
    }

    // Assert refinement narrowings: name <= max_value (#257)
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            script.push_str(&format!("(assert (<= {key} {value}))\n"));
        }
    }

    for req in requires {
        if let Some(smt) = expr_to_smtlib(req) {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }

    // Frame axioms: for ensures with modifies, assert var == old_var for unmodified vars
    if kind == ClauseKind::Ensures && frame_checker.has_modifies() {
        let frame_vars = frame_checker.frame_axiom_vars(ensures_body);
        for var_name in &frame_vars {
            let current = sanitize_smtlib_name(var_name);
            let old = sanitize_smtlib_name(&format!("{var_name}__old"));
            if !vars.contains(&old) {
                script.push_str(&format!("(declare-const {old} Int)\n"));
            }
            script.push_str(&format!("(assert (= {current} {old}))\n"));
        }
    }

    // Inject lemma postconditions for apply references (shell-out path)
    if let Some(defs) = lemma_defs {
        let apply_refs = collect_apply_refs_from_expr(ensures_body);
        for lemma_name in &apply_refs {
            if let Some(ensures_bodies) = defs.get(lemma_name) {
                for ens_body in ensures_bodies {
                    if let Some(smt) = expr_to_smtlib(ens_body) {
                        script.push_str(&format!("(assert {smt})\n"));
                    }
                }
            }
        }
    }

    if let Some(smt) = expr_to_smtlib(ensures_body) {
        match kind {
            ClauseKind::Invariant => {
                script.push_str(&format!("(assert {smt})\n"));
            }
            ClauseKind::MustNot => {
                script.push_str(&format!("(assert {smt})\n"));
            }
            _ => {
                script.push_str(&format!("(assert (not {smt}))\n"));
            }
        }
    } else {
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: "could not encode clause to SMT-LIB2".into(),
        };
    }

    script.push_str("(check-sat)\n");
    script.push_str("(get-model)\n");

    let result = match run_cvc5_binary(&script) {
        Cvc5Result::Unsat => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: "invariant is unsatisfiable".to_string(),
                    counter_model: None,
                }
            } else {
                VerificationResult::Verified {
                    clause_desc: desc.to_string(),
                }
            }
        }
        Cvc5Result::Sat(model_str) => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::Verified {
                    clause_desc: desc.to_string(),
                }
            } else {
                let counter_model = parse_smtlib_model(&model_str);
                // Build a filtered model string from the parsed model
                let filtered_model = counter_model
                    .as_ref()
                    .map(|cm| {
                        cm.variables
                            .iter()
                            .map(|(n, v)| format!("{n} = {v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or(model_str);
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: filtered_model,
                    counter_model,
                }
            }
        }
        Cvc5Result::Timeout => VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        },
        Cvc5Result::Error(reason) => VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason,
        },
    };

    // Insert result into session cache (#253)
    let result_str = match &result {
        VerificationResult::Verified { .. } => "verified",
        VerificationResult::Counterexample { .. } => "counterexample",
        VerificationResult::Timeout { .. } => "timeout",
        VerificationResult::Unknown { .. } => "unknown",
    };
    cache.insert(cache_key, result_str.to_string(), 0);

    result
}

#[cfg(not(feature = "cvc5-verify"))]
fn run_cvc5_binary(script: &str) -> Cvc5Result {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("cvc5");
    cmd.arg("--lang")
        .arg("smt2")
        .arg("--tlimit")
        .arg("1000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 not found on PATH: {e}"));
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(script.as_bytes())
    {
        return Cvc5Result::Error(format!("Failed to write SMT script to CVC5 stdin: {e}"));
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 execution failed: {e}"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();

    match first_line {
        "unsat" => Cvc5Result::Unsat,
        "sat" => {
            let model = stdout.lines().skip(1).collect::<Vec<_>>().join("\n");
            Cvc5Result::Sat(model)
        }
        "timeout" | "resourceout" => Cvc5Result::Timeout,
        "unknown" => Cvc5Result::Timeout,
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("timeout") || stderr.contains("resourceout") {
                Cvc5Result::Timeout
            } else {
                Cvc5Result::Error(format!("unexpected cvc5 output: {first_line}"))
            }
        }
    }
}

/// Convert an AST expression to an SMT-LIB2 string representation.
pub fn expr_to_smtlib(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            if let Some(stripped) = n.strip_prefix('-') {
                Some(format!("(- {stripped})"))
            } else {
                Some(n.clone())
            }
        }
        Expr::Literal(Literal::Bool(b)) => Some(b.to_string()),
        Expr::Literal(Literal::Float(f)) => {
            // Rational encoding matching CVC5 native Real sort
            let fv: f64 = f.parse().unwrap_or(0.0);
            let denom = 1_000_000i64;
            let numer = (fv * denom as f64) as i64;
            Some(format!("(/ {numer} {denom})"))
        }
        Expr::Literal(Literal::Str(s)) => {
            // Named integer constant matching Z3 pattern
            Some(format!("__str_{}", sanitize_smtlib_name(s)))
        }
        Expr::Ident(name) => {
            // "result" in ensures context maps to __result
            if name == "result" {
                Some("__result".to_string())
            } else {
                Some(sanitize_smtlib_name(name))
            }
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = expr_to_smtlib(lhs)?;
            let r = expr_to_smtlib(rhs)?;
            let smt_op = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "div",
                BinOp::Mod => "mod",
                BinOp::Eq => "=",
                BinOp::Neq => return Some(format!("(not (= {l} {r}))")),
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "=>",
                BinOp::Range => {
                    // Range (a..b): fresh Int constrained to [l, r)
                    return Some(format!(
                        "(let ((__range_fresh (+ {l} 0))) (and (>= __range_fresh {l}) (< __range_fresh {r})))"
                    ));
                }
                BinOp::In => {
                    // In (elem in collection): UF __contains(collection, elem)
                    return Some(format!("(__contains {r} {l})"));
                }
                BinOp::NotIn => {
                    // NotIn: negation of In
                    return Some(format!("(not (__contains {r} {l}))"));
                }
                BinOp::Concat => {
                    // Concat (a ++ b): fresh value with length axiom comment
                    // In shell-out mode we return a symbolic expression;
                    // the length axiom is implicit.
                    return Some(format!("(__concat {l} {r})"));
                }
            };
            Some(format!("({smt_op} {l} {r})"))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = expr_to_smtlib(inner)?;
            match op {
                UnaryOp::Not => Some(format!("(not {e})")),
                UnaryOp::Neg => Some(format!("(- {e})")),
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = expr_to_smtlib(cond)?;
            let t = expr_to_smtlib(then_branch)?;
            if let Some(e) = else_branch {
                let e = expr_to_smtlib(e)?;
                Some(format!("(ite {c} {t} {e})"))
            } else {
                // No else branch: treat as implication
                Some(format!("(=> {c} {t})"))
            }
        }
        Expr::Forall { var, domain, body } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            if let Expr::BinOp {
                op: BinOp::Range,
                lhs: lo,
                rhs: hi,
            } = domain.as_ref()
            {
                let lo_s = expr_to_smtlib(lo)?;
                let hi_s = expr_to_smtlib(hi)?;
                Some(format!(
                    "(forall (({v} Int)) (=> (and (>= {v} {lo_s}) (< {v} {hi_s})) {b}))"
                ))
            } else {
                let d = expr_to_smtlib(domain).unwrap_or_else(|| v.clone());
                Some(format!(
                    "(forall (({v} Int)) (=> (__domain_contains {d} {v}) {b}))"
                ))
            }
        }
        Expr::Exists { var, domain, body } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            if let Expr::BinOp {
                op: BinOp::Range,
                lhs: lo,
                rhs: hi,
            } = domain.as_ref()
            {
                let lo_s = expr_to_smtlib(lo)?;
                let hi_s = expr_to_smtlib(hi)?;
                Some(format!(
                    "(exists (({v} Int)) (and (and (>= {v} {lo_s}) (< {v} {hi_s})) {b}))"
                ))
            } else {
                let d = expr_to_smtlib(domain).unwrap_or_else(|| v.clone());
                Some(format!(
                    "(exists (({v} Int)) (and (__domain_contains {d} {v}) {b}))"
                ))
            }
        }
        Expr::Call { func, args } => {
            let f = match func.as_ref() {
                Expr::Ident(name) => sanitize_smtlib_name(name),
                _ => return None,
            };
            if args.is_empty() {
                return Some(f);
            }
            let arg_strs: Option<Vec<String>> = args.iter().map(expr_to_smtlib).collect();
            let arg_strs = arg_strs?;
            // Built-in functions with known semantics
            match f.as_str() {
                "abs" if arg_strs.len() == 1 => {
                    let x = &arg_strs[0];
                    Some(format!("(ite (>= {x} 0) {x} (- {x}))"))
                }
                "min" if arg_strs.len() == 2 => {
                    let (a, b) = (&arg_strs[0], &arg_strs[1]);
                    Some(format!("(ite (<= {a} {b}) {a} {b})"))
                }
                "max" if arg_strs.len() == 2 => {
                    let (a, b) = (&arg_strs[0], &arg_strs[1]);
                    Some(format!("(ite (>= {a} {b}) {a} {b})"))
                }
                // String methods: encode as UF with comment-style axiom hints
                "substring" | "substr" if arg_strs.len() == 3 => Some(format!(
                    "(substring {} {} {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                "concat" if arg_strs.len() == 2 => {
                    Some(format!("(__concat {} {})", arg_strs[0], arg_strs[1]))
                }
                "index_of" | "find" | "indexOf" if arg_strs.len() == 2 => {
                    Some(format!("(index_of {} {})", arg_strs[0], arg_strs[1]))
                }
                "char_at" | "charAt" if arg_strs.len() == 2 => {
                    Some(format!("(char_at {} {})", arg_strs[0], arg_strs[1]))
                }
                "replace" if arg_strs.len() == 3 => Some(format!(
                    "(replace {} {} {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                "split" if arg_strs.len() == 2 => {
                    Some(format!("(split {} {})", arg_strs[0], arg_strs[1]))
                }
                "trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper"
                    if arg_strs.len() == 1 =>
                {
                    Some(format!("({f} {})", arg_strs[0]))
                }
                "set" if arg_strs.len() == 3 => Some(format!(
                    "(set {} {} {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                "put" if arg_strs.len() == 3 => Some(format!(
                    "(put {} {} {})",
                    arg_strs[0], arg_strs[1], arg_strs[2]
                )),
                _ => Some(format!("({f} {})", arg_strs.join(" "))),
            }
        }
        Expr::Old(inner) => match inner.as_ref() {
            // old(x) -> x__old
            Expr::Ident(name) => {
                let old_name = if name == "result" {
                    "__result__old".to_string()
                } else {
                    format!("{}__old", sanitize_smtlib_name(name))
                };
                Some(old_name)
            }
            // old(obj.field) -> flatten deep chains, else UF
            Expr::Field(obj, field) => {
                let full_expr = Expr::Field(obj.clone(), field.clone());
                if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(obj) {
                    let flat_name = flatten_field_chain_cvc5(&full_expr);
                    return Some(format!("{flat_name}__old"));
                }
                let old_obj = expr_to_smtlib(&Expr::Old(obj.clone()))?;
                Some(format!("(__field_{field} {old_obj})"))
            }
            // old(obj.method(args)) -> (method (old obj))
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let old_recv = expr_to_smtlib(&Expr::Old(receiver.clone()))?;
                Some(format!("({method} {old_recv})"))
            }
            _ => expr_to_smtlib(inner),
        },
        Expr::Paren(inner) => expr_to_smtlib(inner),
        Expr::Cast { expr: inner, .. } => expr_to_smtlib(inner),
        Expr::Ghost(inner) => expr_to_smtlib(inner),
        Expr::Let {
            name, value, body, ..
        } => {
            let v = sanitize_smtlib_name(name);
            let val = expr_to_smtlib(value)?;
            let b = expr_to_smtlib(body)?;
            Some(format!("(let (({v} {val})) {b})"))
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            // Encode simple two-arm matches as nested ite chains
            if arms.is_empty() {
                return None;
            }
            let s = expr_to_smtlib(scrutinee)?;
            let mut result = None;
            for arm in arms.iter().rev() {
                match &arm.pattern {
                    Pattern::Wildcard => {
                        let body = expr_to_smtlib(&arm.body)?;
                        result = Some(body);
                    }
                    Pattern::Ident(name) => {
                        let body = expr_to_smtlib(&arm.body)?;
                        if name.starts_with(|c: char| c.is_uppercase()) {
                            let tag = pattern_hash_smtlib(name);
                            let default = result.as_ref()?;
                            result = Some(format!("(ite (= {s} {tag}) {body} {default})"));
                        } else {
                            result = Some(body);
                        }
                    }
                    Pattern::Literal(lit) => {
                        let body = expr_to_smtlib(&arm.body)?;
                        let lit_smt = match lit {
                            Literal::Int(n) => n.clone(),
                            Literal::Float(f) => {
                                let fv: f64 = f.parse().unwrap_or(0.0);
                                let denom = 1_000_000i64;
                                let numer = (fv * denom as f64) as i64;
                                format!("(/ {numer} {denom})")
                            }
                            Literal::Bool(b) => b.to_string(),
                            Literal::Str(_) => return None,
                        };
                        let default = result.as_ref()?;
                        result = Some(format!("(ite (= {s} {lit_smt}) {body} {default})"));
                    }
                    Pattern::Constructor { name, fields: _ } => {
                        let body = expr_to_smtlib(&arm.body)?;
                        let tag = pattern_hash_smtlib(name);
                        let default = result.as_ref()?;
                        result = Some(format!("(ite (= {s} {tag}) {body} {default})"));
                    }
                    Pattern::Tuple(_) => {
                        // Tuple match is structural (always matches)
                        let body = expr_to_smtlib(&arm.body)?;
                        result = Some(body);
                    }
                }
            }
            result
        }
        // Field access: flatten deep chains, else UF __field_name(obj)
        Expr::Field(obj, field) => {
            let full_expr = Expr::Field(Box::new(obj.as_ref().clone()), field.clone());
            if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(obj) {
                return Some(flatten_field_chain_cvc5(&full_expr));
            }
            let o = expr_to_smtlib(obj)?;
            Some(format!("(__field_{field} {o})"))
        }
        // Index: UF __index(coll, idx)
        Expr::Index { expr: coll, index } => {
            let c = expr_to_smtlib(coll)?;
            let i = expr_to_smtlib(index)?;
            Some(format!("(__index {c} {i})"))
        }
        // Block: encode all, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some("true".to_string());
            }
            // SMT-LIB has no block; encode the last expression
            expr_to_smtlib(body.last()?)
        }
        // Raw tokens: full precedence-climbing SMT-LIB2 encoding
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some("true".to_string());
            }
            let (val, _) = parse_raw_expr_smtlib(tokens, 0, 0)?;
            Some(val)
        }
        // Tuple: use a fresh variable name
        Expr::Tuple(_) => Some("__tuple_fresh".to_string()),
        // MethodCall: prepend receiver as first arg to UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let r = expr_to_smtlib(receiver)?;
            let arg_strs: Option<Vec<String>> = args.iter().map(expr_to_smtlib).collect();
            let arg_strs = arg_strs.unwrap_or_default();
            let total_arity = 1 + arg_strs.len(); // receiver + args
            // String/array method axioms for shell-out path
            match method.as_str() {
                "substring" | "substr" if total_arity == 3 => {
                    Some(format!("(substring {r} {} {})", arg_strs[0], arg_strs[1]))
                }
                "concat" if total_arity == 2 => Some(format!("(__concat {r} {})", arg_strs[0])),
                "index_of" | "find" | "indexOf" if total_arity == 2 => {
                    Some(format!("(index_of {r} {})", arg_strs[0]))
                }
                "char_at" | "charAt" if total_arity == 2 => {
                    Some(format!("(char_at {r} {})", arg_strs[0]))
                }
                "replace" if total_arity == 3 => {
                    Some(format!("(replace {r} {} {})", arg_strs[0], arg_strs[1]))
                }
                "split" if total_arity == 2 => Some(format!("(split {r} {})", arg_strs[0])),
                "trim" | "to_lowercase" | "to_uppercase" | "to_lower" | "to_upper"
                    if total_arity == 1 =>
                {
                    Some(format!("({method} {r})"))
                }
                "set" if total_arity == 3 => {
                    Some(format!("(set {r} {} {})", arg_strs[0], arg_strs[1]))
                }
                "put" if total_arity == 3 => {
                    Some(format!("(put {r} {} {})", arg_strs[0], arg_strs[1]))
                }
                _ => {
                    if arg_strs.is_empty() {
                        Some(format!("({method} {r})"))
                    } else {
                        Some(format!("({method} {r} {})", arg_strs.join(" ")))
                    }
                }
            }
        }
        // List: use a fresh variable name
        Expr::List(_) => Some("__list_fresh".to_string()),
        // Apply: return named bool
        Expr::Apply { lemma_name, .. } => Some(format!("__apply_{lemma_name}")),
    }
}

/// Sanitize a name for SMT-LIB2 (replace dots with underscores).
/// Hash a pattern name to a stable i64 for SMT-LIB match encoding
/// (shell-out path). Same FNV-1a algorithm as `pattern_hash_cvc5`.
fn pattern_hash_smtlib(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in name.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as i64
}

// -------------------------------------------------------------------------
// SMT-LIB2 precedence-climbing parser for Expr::Raw tokens (shell-out path)
// -------------------------------------------------------------------------

/// Return the precedence and SMT-LIB2 operator string for a binary operator.
fn raw_op_info_smtlib(tok: &str) -> Option<(u8, &'static str, bool)> {
    // Returns (precedence, smt_op, is_comparison)
    match tok {
        "||" | "or" => Some((1, "or", false)),
        "&&" | "and" => Some((3, "and", false)),
        "=>" | "==>" | "implies" => Some((3, "=>", false)),
        "==" | "=" => Some((5, "=", true)),
        "!=" => Some((5, "!=", true)),
        "<" => Some((7, "<", true)),
        ">" => Some((7, ">", true)),
        "<=" => Some((7, "<=", true)),
        ">=" => Some((7, ">=", true)),
        "+" => Some((9, "+", false)),
        "-" => Some((9, "-", false)),
        "*" => Some((11, "*", false)),
        "/" | "div" => Some((11, "div", false)),
        "%" | "mod" => Some((11, "mod", false)),
        _ => None,
    }
}

/// Format a binary operation as SMT-LIB2 prefix notation.
fn format_smtlib_binop(smt_op: &str, lhs: &str, rhs: &str) -> String {
    if smt_op == "!=" {
        format!("(not (= {lhs} {rhs}))")
    } else {
        format!("({smt_op} {lhs} {rhs})")
    }
}

/// Precedence-climbing expression parser for raw tokens producing SMT-LIB2 text.
///
/// Returns `(smtlib_string, next_position)`.
fn parse_raw_expr_smtlib(tokens: &[String], pos: usize, min_prec: u8) -> Option<(String, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_smtlib(tokens, pos)?;

    while pos < tokens.len() {
        let Some((op_prec, smt_op, is_cmp)) = raw_op_info_smtlib(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1; // consume operator

        let (rhs, next_pos) = parse_raw_expr_smtlib(tokens, pos, op_prec + 1)?;
        pos = next_pos;

        // Comparison chaining: `a < b < c` -> `(and (< a b) (< b c))`
        if is_cmp
            && pos < tokens.len()
            && let Some((next_prec, next_smt_op, next_is_cmp)) =
                raw_op_info_smtlib(tokens[pos].as_str())
            && next_is_cmp
            && next_prec >= min_prec
        {
            let left_cmp = format_smtlib_binop(smt_op, &lhs, &rhs);
            pos += 1; // consume next operator
            let (rhs2, next_pos2) = parse_raw_expr_smtlib(tokens, pos, next_prec + 1)?;
            pos = next_pos2;
            let right_cmp = format_smtlib_binop(next_smt_op, &rhs, &rhs2);
            lhs = format!("(and {left_cmp} {right_cmp})");
            continue;
        }

        lhs = format_smtlib_binop(smt_op, &lhs, &rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw tokens into SMT-LIB2 text.
fn parse_raw_atom_smtlib(tokens: &[String], start: usize) -> Option<(String, usize)> {
    if start >= tokens.len() {
        return Some(("true".to_string(), start));
    }

    let tok = &tokens[start];

    // --- Unary not ---
    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_smtlib(tokens, start + 1)?;
        return Some((format!("(not {val})"), next));
    }

    // --- Unary minus ---
    if tok == "-" {
        let (val, next) = parse_raw_atom_smtlib(tokens, start + 1)?;
        return Some((format!("(- {val})"), next));
    }

    // --- Parenthesized expression ---
    if tok == "(" {
        let (val, end) = parse_raw_expr_smtlib(tokens, start + 1, 0)?;
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    // --- Boolean literals ---
    if tok == "true" || tok == "false" {
        return Some((tok.clone(), start + 1));
    }

    // --- `result` keyword ---
    if tok == "result" {
        return Some(("__result".to_string(), start + 1));
    }

    // --- `old(expr)` ---
    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let mut depth = 1usize;
        let mut p = start + 2;
        while p < tokens.len() && depth > 0 {
            match tokens[p].as_str() {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                p += 1;
            }
        }
        let end = p + 1;
        let inner = &tokens[start + 2..p];

        if inner.len() == 1 {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner[0]));
            return Some((old_name, end));
        }
        // General old(expr): parse inner and suffix identifiers conceptually
        if let Some((val, _)) = parse_raw_expr_smtlib(inner, 0, 0) {
            return Some((val, end));
        }
        return Some(("__old_fresh".to_string(), end));
    }

    // --- `forall`/`exists` quantifiers ---
    if (tok == "forall" || tok == "exists") && start + 4 < tokens.len() && tokens[start + 2] == "in"
    {
        let var_name = sanitize_smtlib_name(&tokens[start + 1]);
        let quantifier = tok.as_str();

        // Find body delimiter: `:` or `{`
        let mut delim_pos = start + 3;
        let mut d = 0usize;
        while delim_pos < tokens.len() {
            match tokens[delim_pos].as_str() {
                "(" => d += 1,
                ")" => d = d.saturating_sub(1),
                ":" | "{" if d == 0 => break,
                _ => {}
            }
            delim_pos += 1;
        }

        if delim_pos < tokens.len() && (tokens[delim_pos] == ":" || tokens[delim_pos] == "{") {
            let body_start = delim_pos + 1;
            let (body_slice_end, final_pos) = if tokens[delim_pos] == "{" {
                let mut bd = 1usize;
                let mut ep = body_start;
                while ep < tokens.len() && bd > 0 {
                    match tokens[ep].as_str() {
                        "{" => bd += 1,
                        "}" => bd -= 1,
                        _ => {}
                    }
                    if bd > 0 {
                        ep += 1;
                    }
                }
                (ep, ep + 1)
            } else {
                (tokens.len(), tokens.len())
            };

            let body_tokens = &tokens[body_start..body_slice_end];
            if let Some((body_val, _)) = parse_raw_expr_smtlib(body_tokens, 0, 0) {
                return Some((
                    format!("({quantifier} (({var_name} Int)) {body_val})"),
                    final_pos,
                ));
            }
            return Some((format!("({quantifier} (({var_name} Int)) true)"), final_pos));
        }
    }

    // --- Integer literal ---
    if tok.parse::<i64>().is_ok() {
        return Some((tok.clone(), start + 1));
    }

    // --- Skip specification keywords ---
    if matches!(
        tok.as_str(),
        "taint" | "untrusted" | "validated" | "ghost" | "Region" | "validate"
    ) {
        return parse_raw_atom_smtlib(tokens, start + 1);
    }

    // --- Identifier with dot-separated field access ---
    let mut name = sanitize_smtlib_name(tok);
    let mut next = start + 1;
    while next + 1 < tokens.len() && tokens[next] == "." {
        name.push('_');
        name.push_str(&sanitize_smtlib_name(&tokens[next + 1]));
        next += 2;
    }

    // Function call: `name(args)` -> `(name arg1 arg2 ...)`
    if next < tokens.len() && tokens[next] == "(" {
        let mut depth = 1usize;
        let mut p = next + 1;
        while p < tokens.len() && depth > 0 {
            match tokens[p].as_str() {
                "(" => depth += 1,
                ")" => depth -= 1,
                _ => {}
            }
            if depth > 0 {
                p += 1;
            }
        }
        let arg_tokens = &tokens[next + 1..p];
        let mut arg_strs: Vec<String> = Vec::new();
        if !arg_tokens.is_empty() {
            let mut arg_start_idx = 0;
            let mut dd = 0usize;
            for (i, t) in arg_tokens.iter().enumerate() {
                match t.as_str() {
                    "(" => dd += 1,
                    ")" => dd = dd.saturating_sub(1),
                    "," if dd == 0 => {
                        let chunk = &arg_tokens[arg_start_idx..i];
                        if !chunk.is_empty()
                            && let Some((v, _)) = parse_raw_expr_smtlib(chunk, 0, 0)
                        {
                            arg_strs.push(v);
                        }
                        arg_start_idx = i + 1;
                    }
                    _ => {}
                }
            }
            let chunk = &arg_tokens[arg_start_idx..];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_smtlib(chunk, 0, 0)
            {
                arg_strs.push(v);
            }
        }
        let end = p + 1;

        if arg_strs.is_empty() {
            return Some((name, end));
        }
        return Some((format!("({name} {})", arg_strs.join(" ")), end));
    }

    Some((name, next))
}

fn sanitize_smtlib_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Collect all variable names referenced in an expression.
pub fn collect_vars(expr: &Expr, vars: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            if name == "result" {
                vars.insert("__result".to_string());
            } else {
                vars.insert(sanitize_smtlib_name(name));
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_vars(lhs, vars);
            collect_vars(rhs, vars);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_vars(inner, vars),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_vars(cond, vars);
            collect_vars(then_branch, vars);
            if let Some(e) = else_branch {
                collect_vars(e, vars);
            }
        }
        Expr::Forall {
            var, body, domain, ..
        }
        | Expr::Exists {
            var, body, domain, ..
        } => {
            // Do NOT insert the quantifier-bound variable as a global constant.
            // It is locally scoped by the (forall ((var Int)) ...) quantifier.
            // Declaring it as a global constant creates a name collision in CVC5.
            collect_vars(body, vars);
            collect_vars(domain, vars);
            // Remove the bound variable if it was collected from the body/domain.
            vars.remove(&sanitize_smtlib_name(var));
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Old(inner) | Expr::Paren(inner) | Expr::Ghost(inner) => {
            collect_vars(inner, vars);
        }
        Expr::Cast { expr: inner, .. } => collect_vars(inner, vars),
        Expr::Field(receiver, _) => collect_vars(receiver, vars),
        Expr::MethodCall { receiver, args, .. } => {
            collect_vars(receiver, vars);
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Index { expr, index } => {
            collect_vars(expr, vars);
            collect_vars(index, vars);
        }
        Expr::Let { value, body, .. } => {
            collect_vars(value, vars);
            collect_vars(body, vars);
        }
        Expr::Match { scrutinee, arms } => {
            collect_vars(scrutinee, vars);
            for arm in arms {
                collect_vars(&arm.body, vars);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_vars(item, vars);
            }
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Literal(_) => {}
        Expr::Raw(tokens) => {
            // Raw tokens may contain variable names; collect identifiers
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && tok != "true"
                    && tok != "false"
                {
                    vars.insert(sanitize_smtlib_name(tok));
                }
            }
        }
    }
}

/// Parse a CVC5 model output into a CounterexampleModel.
///
/// Filters out internal encoder variables and sorts the remaining
/// user variables alphabetically (matching Z3 backend behavior).
#[cfg_attr(feature = "cvc5-verify", expect(dead_code))]
pub(crate) fn parse_smtlib_model(model_str: &str) -> Option<CounterexampleModel> {
    // CVC5 model format: (define-fun name () Int value)
    let mut variables = Vec::new();
    for line in model_str.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(define-fun ") {
            // Extract name and value from: (define-fun name () Type value)
            let parts: Vec<&str> = trimmed
                .trim_start_matches("(define-fun ")
                .splitn(2, " () ")
                .collect();
            if parts.len() == 2 {
                let name = parts[0].to_string();
                // Value is after the type, before the closing paren
                let type_and_value = parts[1];
                if let Some(space_idx) = type_and_value.find(' ') {
                    let raw = &type_and_value[space_idx + 1..];
                    // Strip exactly one trailing ')' (the define-fun closer)
                    let value = raw.strip_suffix(')').unwrap_or(raw).trim().to_string();
                    if !is_internal_cvc5_var(&name) {
                        variables.push((name, value));
                    }
                }
            }
        }
    }
    if variables.is_empty() {
        None
    } else {
        // Sort alphabetically for deterministic output
        variables.sort_by(|(a, _), (b, _)| a.cmp(b));
        Some(CounterexampleModel { variables })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Literal, Pattern, UnaryOp};

    // -------------------------------------------------------------------
    // derive_narrowings_cvc5 tests (#257)
    // -------------------------------------------------------------------

    #[test]
    fn test_derive_narrowings_cvc5_basic() {
        let narrowings = derive_narrowings_cvc5(&[("max_size".into(), 100)]);
        assert_eq!(narrowings.len(), 1);
        assert_eq!(narrowings[0], ("size".into(), 100));
    }

    #[test]
    fn test_derive_narrowings_cvc5_empty() {
        let narrowings = derive_narrowings_cvc5(&[]);
        assert!(narrowings.is_empty());
    }

    #[test]
    fn test_derive_narrowings_cvc5_no_prefix() {
        let narrowings = derive_narrowings_cvc5(&[("size".into(), 50)]);
        assert!(narrowings.is_empty());
    }

    #[test]
    fn test_derive_narrowings_cvc5_uppercase_prefix() {
        let narrowings = derive_narrowings_cvc5(&[("MAX_BUFFER".into(), 1024)]);
        assert_eq!(narrowings.len(), 2);
        assert_eq!(narrowings[0], ("BUFFER".into(), 1024));
        assert_eq!(narrowings[1], ("buffer".into(), 1024));
    }

    #[test]
    fn test_derive_narrowings_cvc5_multiple() {
        let narrowings = derive_narrowings_cvc5(&[
            ("max_size".into(), 100),
            ("max_count".into(), 50),
            ("threshold".into(), 10),
        ]);
        assert_eq!(narrowings.len(), 2);
        assert_eq!(narrowings[0], ("size".into(), 100));
        assert_eq!(narrowings[1], ("count".into(), 50));
    }

    // -------------------------------------------------------------------
    // expr_to_smtlib tests
    // -------------------------------------------------------------------

    #[test]
    fn test_smtlib_int_positive() {
        let expr = Expr::Literal(Literal::Int("42".into()));
        assert_eq!(expr_to_smtlib(&expr), Some("42".into()));
    }

    #[test]
    fn test_smtlib_int_negative() {
        let expr = Expr::Literal(Literal::Int("-7".into()));
        assert_eq!(expr_to_smtlib(&expr), Some("(- 7)".into()));
    }

    #[test]
    fn test_smtlib_bool_true() {
        let expr = Expr::Literal(Literal::Bool(true));
        assert_eq!(expr_to_smtlib(&expr), Some("true".into()));
    }

    #[test]
    fn test_smtlib_bool_false() {
        let expr = Expr::Literal(Literal::Bool(false));
        assert_eq!(expr_to_smtlib(&expr), Some("false".into()));
    }

    #[test]
    fn test_smtlib_string_encodes_as_named_const() {
        let expr = Expr::Literal(Literal::Str("hello".into()));
        assert_eq!(expr_to_smtlib(&expr), Some("__str_hello".into()));
    }

    #[test]
    fn test_smtlib_ident() {
        let expr = Expr::Ident("x".into());
        assert_eq!(expr_to_smtlib(&expr), Some("x".into()));
    }

    #[test]
    fn test_smtlib_result_keyword() {
        let expr = Expr::Ident("result".into());
        assert_eq!(expr_to_smtlib(&expr), Some("__result".into()));
    }

    #[test]
    fn test_smtlib_dotted_ident_sanitized() {
        let expr = Expr::Ident("state.field".into());
        assert_eq!(expr_to_smtlib(&expr), Some("state_field".into()));
    }

    #[test]
    fn test_smtlib_binop_add() {
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(+ x 1)".into()));
    }

    #[test]
    fn test_smtlib_binop_neq() {
        let expr = Expr::BinOp {
            op: BinOp::Neq,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
    }

    #[test]
    fn test_smtlib_binop_div_is_integer() {
        let expr = Expr::BinOp {
            op: BinOp::Div,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("y".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(div x y)".into()));
    }

    #[test]
    fn test_smtlib_binop_implies() {
        let expr = Expr::BinOp {
            op: BinOp::Implies,
            lhs: Box::new(Expr::Ident("p".into())),
            rhs: Box::new(Expr::Ident("q".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
    }

    #[test]
    fn test_smtlib_binop_range_encodes() {
        let expr = Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
        };
        let s = expr_to_smtlib(&expr).expect("Range should encode");
        assert!(s.contains(">="), "missing >= in range encoding: {s}");
        assert!(s.contains("<"), "missing < in range encoding: {s}");
        assert!(
            s.contains("__range_fresh"),
            "missing fresh var in range: {s}"
        );
    }

    #[test]
    fn test_smtlib_binop_in() {
        let expr = Expr::BinOp {
            op: BinOp::In,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("collection".into())),
        };
        let s = expr_to_smtlib(&expr).expect("In should encode");
        assert!(s.contains("__contains"), "missing contains UF in: {s}");
        assert!(s.contains("collection"), "missing collection in: {s}");
        assert!(s.contains("x"), "missing element in: {s}");
    }

    #[test]
    fn test_smtlib_binop_notin() {
        let expr = Expr::BinOp {
            op: BinOp::NotIn,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("items".into())),
        };
        let s = expr_to_smtlib(&expr).expect("NotIn should encode");
        assert!(s.contains("not"), "missing negation in NotIn: {s}");
        assert!(
            s.contains("__contains"),
            "missing contains UF in NotIn: {s}"
        );
    }

    #[test]
    fn test_smtlib_binop_concat() {
        let expr = Expr::BinOp {
            op: BinOp::Concat,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        };
        let s = expr_to_smtlib(&expr).expect("Concat should encode");
        assert!(s.contains("__concat"), "missing concat UF in: {s}");
        assert!(s.contains("a"), "missing lhs in concat: {s}");
        assert!(s.contains("b"), "missing rhs in concat: {s}");
    }

    #[test]
    fn test_smtlib_unary_not() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Ident("flag".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(not flag)".into()));
    }

    #[test]
    fn test_smtlib_unary_neg() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Ident("x".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
    }

    #[test]
    fn test_smtlib_if_with_else() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Ident("t".into())),
            else_branch: Some(Box::new(Expr::Ident("e".into()))),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(ite c t e)".into()));
    }

    #[test]
    fn test_smtlib_if_without_else() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("p".into())),
            then_branch: Box::new(Expr::Ident("q".into())),
            else_branch: None,
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
    }

    #[test]
    fn test_smtlib_forall_non_range_domain() {
        // Non-range domain should produce __domain_contains guard
        let expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        assert_eq!(
            expr_to_smtlib(&expr),
            Some("(forall ((i Int)) (=> (__domain_contains xs i) (>= i 0)))".into())
        );
    }

    #[test]
    fn test_smtlib_exists_non_range_domain() {
        // Non-range domain should produce __domain_contains guard
        let expr = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::Ident("S".into())),
            body: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        assert_eq!(
            expr_to_smtlib(&expr),
            Some("(exists ((x Int)) (and (__domain_contains S x) (= x 0)))".into())
        );
    }

    #[test]
    fn test_smtlib_forall_range_domain() {
        // forall x in 0..10 { x >= 0 } should produce range guard
        let expr = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                op: BinOp::Range,
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(
            s,
            "(forall ((x Int)) (=> (and (>= x 0) (< x 10)) (>= x 0)))"
        );
    }

    #[test]
    fn test_smtlib_exists_range_domain() {
        // exists x in 0..10 { x == 5 } should produce range guard with conjunction
        let expr = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                op: BinOp::Range,
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
            }),
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(
            s,
            "(exists ((x Int)) (and (and (>= x 0) (< x 10)) (= x 5)))"
        );
    }

    #[test]
    fn test_smtlib_forall_range_variable_bounds() {
        // forall i in 0..n { i >= 0 } -- variable upper bound
        let expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::BinOp {
                op: BinOp::Range,
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                rhs: Box::new(Expr::Ident("n".into())),
            }),
            body: Box::new(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(forall ((i Int)) (=> (and (>= i 0) (< i n)) (>= i 0)))");
    }

    #[test]
    fn test_smtlib_call_no_args() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("foo".into())),
            args: vec![],
        };
        assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
    }

    #[test]
    fn test_smtlib_call_with_args() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("f".into())),
            args: vec![Expr::Ident("x".into()), Expr::Ident("y".into())],
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(f x y)".into()));
    }

    #[test]
    fn test_smtlib_old_adds_suffix() {
        let expr = Expr::Old(Box::new(Expr::Ident("x".into())));
        assert_eq!(expr_to_smtlib(&expr), Some("x__old".into()));
    }

    #[test]
    fn test_smtlib_paren_transparent() {
        let expr = Expr::Paren(Box::new(Expr::Literal(Literal::Int("5".into()))));
        assert_eq!(expr_to_smtlib(&expr), Some("5".into()));
    }

    #[test]
    fn test_smtlib_raw_single_token() {
        let expr = Expr::Raw(vec!["foo".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
        // Integer token
        let expr_int = Expr::Raw(vec!["42".into()]);
        assert_eq!(expr_to_smtlib(&expr_int), Some("42".into()));
        // Bool token
        let expr_bool = Expr::Raw(vec!["true".into()]);
        assert_eq!(expr_to_smtlib(&expr_bool), Some("true".into()));
    }

    #[test]
    fn test_smtlib_raw_precedence_climbing() {
        // "a + b * c" should parse as (+ a (* b c)) due to precedence
        let expr = Expr::Raw(vec![
            "a".into(),
            "+".into(),
            "b".into(),
            "*".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(+ a (* b c))".into()));
    }

    #[test]
    fn test_smtlib_raw_parentheses() {
        // "(a + b) * c" should parse as (* (+ a b) c)
        let expr = Expr::Raw(vec![
            "(".into(),
            "a".into(),
            "+".into(),
            "b".into(),
            ")".into(),
            "*".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(* (+ a b) c)".into()));
    }

    #[test]
    fn test_smtlib_raw_old_expression() {
        // "old ( x ) + 1" should parse old(x) + 1
        let expr = Expr::Raw(vec![
            "old".into(),
            "(".into(),
            "x".into(),
            ")".into(),
            "+".into(),
            "1".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(+ x__old 1)".into()));
    }

    #[test]
    fn test_smtlib_raw_nested_operators() {
        // "a + b - c + d" left-associative: (+ (- (+ a b) c) d)
        let expr = Expr::Raw(vec![
            "a".into(),
            "+".into(),
            "b".into(),
            "-".into(),
            "c".into(),
            "+".into(),
            "d".into(),
        ]);
        let result = expr_to_smtlib(&expr).unwrap();
        // Left-associative: ((a + b) - c) + d
        assert_eq!(result, "(+ (- (+ a b) c) d)");
    }

    #[test]
    fn test_smtlib_raw_comparison_chain() {
        // "a < b < c" desugars to (and (< a b) (< b c))
        let expr = Expr::Raw(vec![
            "a".into(),
            "<".into(),
            "b".into(),
            "<".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(and (< a b) (< b c))".into()));
    }

    #[test]
    fn test_smtlib_raw_unary_not() {
        // "! x" -> (not x)
        let expr = Expr::Raw(vec!["!".into(), "x".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(not x)".into()));
    }

    #[test]
    fn test_smtlib_raw_unary_neg() {
        // "- x" -> (- x)
        let expr = Expr::Raw(vec!["-".into(), "x".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
    }

    #[test]
    fn test_smtlib_raw_logical_ops() {
        // "a && b || c" should respect precedence: (or (and a b) c)
        let expr = Expr::Raw(vec![
            "a".into(),
            "&&".into(),
            "b".into(),
            "||".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(or (and a b) c)".into()));
    }

    #[test]
    fn test_smtlib_raw_neq() {
        // "a != b" -> (not (= a b))
        let expr = Expr::Raw(vec!["a".into(), "!=".into(), "b".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
    }

    #[test]
    fn test_smtlib_raw_mod_div() {
        // "a mod b" and "a div b"
        let expr_mod = Expr::Raw(vec!["a".into(), "mod".into(), "b".into()]);
        assert_eq!(expr_to_smtlib(&expr_mod), Some("(mod a b)".into()));

        let expr_div = Expr::Raw(vec!["a".into(), "div".into(), "b".into()]);
        assert_eq!(expr_to_smtlib(&expr_div), Some("(div a b)".into()));
    }

    #[test]
    fn test_smtlib_raw_complex_expression() {
        // "x >= 0 && x < max" -> (and (>= x 0) (< x max))
        let expr = Expr::Raw(vec![
            "x".into(),
            ">=".into(),
            "0".into(),
            "&&".into(),
            "x".into(),
            "<".into(),
            "max".into(),
        ]);
        assert_eq!(
            expr_to_smtlib(&expr),
            Some("(and (>= x 0) (< x max))".into())
        );
    }

    #[test]
    fn test_smtlib_raw_function_call() {
        // "abs ( x )" -> (abs x)
        let expr = Expr::Raw(vec!["abs".into(), "(".into(), "x".into(), ")".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(abs x)".into()));
    }

    #[test]
    fn test_smtlib_let_expr() {
        let expr = Expr::Let {
            name: "x".into(),
            value: Box::new(Expr::Literal(Literal::Int("5".into()))),
            body: Box::new(Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
            }),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(let ((x 5)) (+ x 1))".into()));
    }

    #[test]
    fn test_smtlib_match_with_literal_and_wildcard() {
        use assura_parser::ast::MatchArm;
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("n".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Literal(Literal::Int("0".into())),
                    body: Expr::Literal(Literal::Int("1".into())),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Ident("n".into()),
                },
            ],
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(ite (= n 0) 1 n)".into()));
    }

    #[test]
    fn test_smtlib_match_empty_arms() {
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("n".into())),
            arms: vec![],
        };
        assert_eq!(expr_to_smtlib(&expr), None);
    }

    #[test]
    fn test_smtlib_match_constructor_pattern() {
        use assura_parser::ast::MatchArm;
        // match x { Some(v) => v, None => 0 }
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Constructor {
                        name: "Some".into(),
                        fields: vec![Pattern::Ident("v".into())],
                    },
                    body: Expr::Ident("v".into()),
                },
                MatchArm {
                    pattern: Pattern::Constructor {
                        name: "None".into(),
                        fields: vec![],
                    },
                    body: Expr::Literal(Literal::Int("0".into())),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Literal(Literal::Int("0".into())),
                },
            ],
        };
        let smt = expr_to_smtlib(&expr).expect("should encode constructor match");
        // Should produce nested ite with hash tags for Some and None
        let some_hash = pattern_hash_smtlib("Some");
        let none_hash = pattern_hash_smtlib("None");
        assert!(smt.contains(&some_hash.to_string()));
        assert!(smt.contains(&none_hash.to_string()));
        assert!(smt.contains("ite"));
    }

    #[test]
    fn test_smtlib_match_tuple_pattern() {
        use assura_parser::ast::MatchArm;
        // match t { (a, b) => a }
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("t".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Tuple(vec![
                    Pattern::Ident("a".into()),
                    Pattern::Ident("b".into()),
                ]),
                body: Expr::Ident("a".into()),
            }],
        };
        let smt = expr_to_smtlib(&expr).expect("should encode tuple match");
        // Tuple is structural, body is just "a"
        assert_eq!(smt, "a");
    }

    #[test]
    fn test_smtlib_match_ident_constructor_like() {
        use assura_parser::ast::MatchArm;
        // match x { None => 1, _ => 0 }  (Ident "None" uppercase = constructor)
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Ident("None".into()),
                    body: Expr::Literal(Literal::Int("1".into())),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Literal(Literal::Int("0".into())),
                },
            ],
        };
        let smt = expr_to_smtlib(&expr).expect("should encode ident-as-constructor match");
        let none_hash = pattern_hash_smtlib("None");
        assert!(smt.contains(&none_hash.to_string()));
        assert!(smt.contains("ite"));
    }

    // -------------------------------------------------------------------
    // collect_vars tests
    // -------------------------------------------------------------------

    #[test]
    fn test_collect_vars_ident() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Ident("x".into()), &mut vars);
        assert!(vars.contains("x"));
    }

    #[test]
    fn test_collect_vars_result() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Ident("result".into()), &mut vars);
        assert!(vars.contains("__result"));
        assert!(!vars.contains("result"));
    }

    #[test]
    fn test_collect_vars_binop() {
        let mut vars = HashSet::new();
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        };
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("a"));
        assert!(vars.contains("b"));
    }

    #[test]
    fn test_collect_vars_if_all_branches() {
        let mut vars = HashSet::new();
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Ident("t".into())),
            else_branch: Some(Box::new(Expr::Ident("e".into()))),
        };
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("c"));
        assert!(vars.contains("t"));
        assert!(vars.contains("e"));
    }

    #[test]
    fn test_collect_vars_literal_no_vars() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Literal(Literal::Int("42".into())), &mut vars);
        assert!(vars.is_empty());
    }

    #[test]
    fn test_collect_vars_dotted_sanitized() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Ident("obj.field".into()), &mut vars);
        assert!(vars.contains("obj_field"));
    }

    // -------------------------------------------------------------------
    // parse_smtlib_model tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_model_define_fun() {
        let model = "(define-fun x () Int 42)\n(define-fun y () Int (- 1))";
        let parsed = parse_smtlib_model(model).unwrap();
        assert_eq!(parsed.variables.len(), 2);
        assert_eq!(parsed.variables[0].0, "x");
        assert_eq!(parsed.variables[0].1, "42");
        assert_eq!(parsed.variables[1].0, "y");
        assert_eq!(parsed.variables[1].1, "(- 1)");
    }

    #[test]
    fn test_parse_model_empty() {
        assert!(parse_smtlib_model("").is_none());
    }

    #[test]
    fn test_parse_model_no_define_fun() {
        assert!(parse_smtlib_model("sat\n(something else)").is_none());
    }

    #[test]
    fn test_parse_model_skips_coerce() {
        let model = "(define-fun __coerce_1 () Int 0)\n(define-fun x () Int 5)";
        let parsed = parse_smtlib_model(model).unwrap();
        assert_eq!(parsed.variables.len(), 1);
        assert_eq!(parsed.variables[0].0, "x");
    }

    // -------------------------------------------------------------------
    // is_internal_cvc5_var and counterexample model filtering (#260)
    // -------------------------------------------------------------------

    #[test]
    fn test_is_internal_cvc5_var_internal_prefixes() {
        assert!(is_internal_cvc5_var("__str_hello"));
        assert!(is_internal_cvc5_var("__tuple_0"));
        assert!(is_internal_cvc5_var("__list_vals"));
        assert!(is_internal_cvc5_var("__fresh_3"));
        assert!(is_internal_cvc5_var("__field_len"));
        assert!(is_internal_cvc5_var("__index_0"));
        assert!(is_internal_cvc5_var("__len_buf"));
        assert!(is_internal_cvc5_var("__arr_data"));
        assert!(is_internal_cvc5_var("__domain_contains_x"));
        assert!(is_internal_cvc5_var("__apply_func"));
        assert!(is_internal_cvc5_var("__coerce_1"));
        assert!(is_internal_cvc5_var("__trigger_pat"));
        assert!(is_internal_cvc5_var("__list_get_0"));
        assert!(is_internal_cvc5_var("__result"));
        assert!(is_internal_cvc5_var("__contains"));
        assert!(is_internal_cvc5_var("__obj_ptr"));
    }

    #[test]
    fn test_is_internal_cvc5_var_user_variables() {
        assert!(!is_internal_cvc5_var("x"));
        assert!(!is_internal_cvc5_var("buffer_size"));
        assert!(!is_internal_cvc5_var("payload_length"));
        assert!(!is_internal_cvc5_var("n"));
        assert!(!is_internal_cvc5_var("result_count"));
        assert!(!is_internal_cvc5_var("max_size"));
        assert!(!is_internal_cvc5_var("i"));
    }

    #[test]
    fn test_parse_model_filters_all_internal_vars() {
        let model = "\
(define-fun __str_hello () Int 1)\n\
(define-fun __field_len () Int 5)\n\
(define-fun __fresh_0 () Int 99)\n\
(define-fun __result () Int 42)\n\
(define-fun __coerce_1 () Int 0)\n\
(define-fun x () Int 10)\n\
(define-fun y () Int 20)";
        let parsed = parse_smtlib_model(model).unwrap();
        let names: Vec<&str> = parsed.variables.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["x", "y"]);
        assert!(!names.contains(&"__str_hello"));
        assert!(!names.contains(&"__field_len"));
        assert!(!names.contains(&"__fresh_0"));
        assert!(!names.contains(&"__result"));
        assert!(!names.contains(&"__coerce_1"));
    }

    #[test]
    fn test_parse_model_sorted_alphabetically() {
        let model = "\
(define-fun z_var () Int 3)\n\
(define-fun a_var () Int 1)\n\
(define-fun m_var () Int 2)";
        let parsed = parse_smtlib_model(model).unwrap();
        let names: Vec<&str> = parsed.variables.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a_var", "m_var", "z_var"]);
    }

    #[test]
    fn test_parse_model_all_internal_returns_none() {
        let model = "\
(define-fun __str_a () Int 1)\n\
(define-fun __fresh_0 () Int 2)\n\
(define-fun __coerce_1 () Int 3)";
        assert!(
            parse_smtlib_model(model).is_none(),
            "model with only internal vars should return None"
        );
    }

    // -------------------------------------------------------------------
    // collect_vars exhaustive coverage (issue #54)
    // -------------------------------------------------------------------

    #[test]
    fn collect_vars_field_access() {
        let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "field".into());
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("obj"));
    }

    #[test]
    fn collect_vars_method_call() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("list".into())),
            method: "len".into(),
            args: vec![Expr::Ident("idx".into())],
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("list"));
        assert!(vars.contains("idx"));
    }

    #[test]
    fn collect_vars_index() {
        let expr = Expr::Index {
            expr: Box::new(Expr::Ident("arr".into())),
            index: Box::new(Expr::Ident("i".into())),
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("arr"));
        assert!(vars.contains("i"));
    }

    #[test]
    fn collect_vars_let_expr() {
        let expr = Expr::Let {
            name: "tmp".into(),
            value: Box::new(Expr::Ident("a".into())),
            body: Box::new(Expr::Ident("b".into())),
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("a"));
        assert!(vars.contains("b"));
    }

    #[test]
    fn collect_vars_match_expr() {
        use assura_parser::ast::{MatchArm, Pattern};
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Ident("_".into()),
                body: Expr::Ident("y".into()),
            }],
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
    }

    #[test]
    fn collect_vars_list_tuple_block() {
        let list = Expr::List(vec![Expr::Ident("a".into()), Expr::Ident("b".into())]);
        let tuple = Expr::Tuple(vec![Expr::Ident("c".into())]);
        let block = Expr::Block(vec![Expr::Ident("d".into())]);
        let mut vars = HashSet::new();
        collect_vars(&list, &mut vars);
        collect_vars(&tuple, &mut vars);
        collect_vars(&block, &mut vars);
        assert!(vars.contains("a"));
        assert!(vars.contains("b"));
        assert!(vars.contains("c"));
        assert!(vars.contains("d"));
    }

    #[test]
    fn collect_vars_apply() {
        let expr = Expr::Apply {
            lemma_name: "lem".into(),
            args: vec![Expr::Ident("p".into())],
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("p"));
    }

    #[test]
    fn collect_vars_literal_is_empty() {
        let expr = Expr::Literal(Literal::Int("42".into()));
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.is_empty());
    }

    // -------------------------------------------------------------------
    // Regression: CVC5 must_not semantics (#166)
    // -------------------------------------------------------------------

    /// must_not(true) should NOT be verified: true is always possible.
    /// The CVC5 backend must assert the body directly (not negate it).
    #[test]
    fn test_cvc5_must_not_semantics() {
        // must_not { true } -- "true" is always satisfiable, so
        // asserting it directly gives SAT -> Counterexample.
        let clause = Clause {
            kind: ClauseKind::MustNot,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        };
        let results = verify_contract_cvc5("TestMustNot", &[clause]);
        // Should be Counterexample (the bad thing CAN happen)
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Counterexample { .. } | VerificationResult::Unknown { .. }
            ),
            "must_not(true) should be Counterexample or Unknown, got: {:?}",
            results[0]
        );
    }

    /// must_not(false) should verify: false is impossible.
    #[test]
    fn test_cvc5_must_not_impossible() {
        let clause = Clause {
            kind: ClauseKind::MustNot,
            body: Expr::Literal(Literal::Bool(false)),
            effect_variables: vec![],
        };
        let results = verify_contract_cvc5("TestMustNotFalse", &[clause]);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "must_not(false) should be Verified or Unknown (if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Regression: quantifier-bound vars not global (#167)
    // -------------------------------------------------------------------

    /// Quantifier-bound variables must NOT appear in the global
    /// `(declare-const ...)` section of the generated SMT-LIB2 script.
    #[test]
    fn test_cvc5_quantifier_var_not_global() {
        // forall i in xs: i >= 0
        let body = Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let forall_expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(body),
        };
        let mut vars = HashSet::new();
        collect_vars(&forall_expr, &mut vars);
        // "i" must NOT be in the global vars set
        assert!(
            !vars.contains("i"),
            "quantifier-bound variable 'i' must not be a global constant"
        );
        // "xs" (the domain) should still be collected
        assert!(
            vars.contains("xs"),
            "domain variable 'xs' should be collected"
        );
    }

    // -------------------------------------------------------------------
    // Unmodelable feature pre-check tests (cfg-independent)
    // -------------------------------------------------------------------

    #[test]
    fn test_unmodelable_typestate_detected() {
        // Raw tokens with @ should be detected as unmodelable
        let expr = Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]);
        assert!(
            expr_has_unmodelable_features_cvc5(&expr),
            "typestate @ annotation should be unmodelable"
        );
    }

    #[test]
    fn test_unmodelable_reason_typestate() {
        let expr = Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]);
        let reasons = collect_unmodelable_reasons_cvc5(&expr);
        assert_eq!(reasons, vec!["typestate annotation"]);
    }

    #[test]
    fn test_modelable_normal_expr() {
        // Normal binary expression should be modelable
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert!(
            !expr_has_unmodelable_features_cvc5(&expr),
            "normal binop should be modelable"
        );
    }

    #[test]
    fn test_unmodelable_nested_in_binop() {
        // Typestate nested in a binary expression
        let expr = Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Raw(vec![
                "conn".into(),
                "@".into(),
                "Connected".into(),
            ])),
        };
        assert!(
            expr_has_unmodelable_features_cvc5(&expr),
            "typestate nested in binop should be unmodelable"
        );
    }

    #[test]
    fn test_unmodelable_in_if_branch() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("flag".into())),
            then_branch: Box::new(Expr::Raw(vec!["s".into(), "@".into(), "Locked".into()])),
            else_branch: None,
        };
        assert!(
            expr_has_unmodelable_features_cvc5(&expr),
            "typestate in if-then should be unmodelable"
        );
    }

    #[test]
    fn test_unmodelable_in_forall_body() {
        let expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::Raw(vec!["item".into(), "@".into(), "Valid".into()])),
        };
        assert!(
            expr_has_unmodelable_features_cvc5(&expr),
            "typestate in forall body should be unmodelable"
        );
    }

    #[test]
    fn test_cvc5_shellout_unmodelable_typestate_skipped() {
        // Clause with @ annotation should produce Unknown via verify_contract_cvc5
        // (which dispatches to either native or shellout depending on feature flag)
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
            effect_variables: vec![],
        }];
        let results = verify_contract_cvc5("TestTypestate", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Unknown { reason, .. }
                    if reason.contains("not yet encoded")
            ),
            "expected Unknown with 'not yet encoded', got {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Lemma apply-ref collection tests (cfg-independent)
    // -------------------------------------------------------------------

    #[test]
    fn test_collect_apply_refs_simple() {
        let expr = Expr::Apply {
            lemma_name: "helper".into(),
            args: vec![Expr::Ident("x".into())],
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert_eq!(refs, vec!["helper"]);
    }

    #[test]
    fn test_collect_apply_refs_nested_in_binop() {
        let expr = Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Apply {
                lemma_name: "lem_a".into(),
                args: vec![Expr::Ident("x".into())],
            }),
            rhs: Box::new(Expr::Apply {
                lemma_name: "lem_b".into(),
                args: vec![Expr::Ident("y".into())],
            }),
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"lem_a".to_string()));
        assert!(refs.contains(&"lem_b".to_string()));
    }

    #[test]
    fn test_collect_apply_refs_no_apply() {
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_collect_apply_refs_nested_in_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("flag".into())),
            then_branch: Box::new(Expr::Apply {
                lemma_name: "branch_lem".into(),
                args: vec![],
            }),
            else_branch: Some(Box::new(Expr::Literal(Literal::Bool(true)))),
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert_eq!(refs, vec!["branch_lem"]);
    }

    // -------------------------------------------------------------------
    // SMT-LIB float encoding tests (#248)
    // -------------------------------------------------------------------

    #[test]
    fn test_smtlib_float_rational_encoding() {
        let expr = Expr::Literal(Literal::Float("3.14".into()));
        let result = expr_to_smtlib(&expr).unwrap();
        assert_eq!(result, "(/ 3140000 1000000)");
    }

    #[test]
    fn test_smtlib_float_zero() {
        let expr = Expr::Literal(Literal::Float("0.0".into()));
        let result = expr_to_smtlib(&expr).unwrap();
        assert_eq!(result, "(/ 0 1000000)");
    }

    #[test]
    fn test_smtlib_float_negative() {
        // Negative floats: the negation is applied by UnaryOp::Neg externally,
        // but the literal itself may parse as negative
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Literal(Literal::Float("2.5".into()))),
        };
        let result = expr_to_smtlib(&expr).unwrap();
        assert_eq!(result, "(- (/ 2500000 1000000))");
    }

    #[test]
    fn test_smtlib_match_float_pattern_rational() {
        // Match arm with float literal should use rational encoding
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                assura_parser::ast::MatchArm {
                    pattern: Pattern::Literal(Literal::Float("1.5".into())),
                    body: Expr::Literal(Literal::Bool(true)),
                },
                assura_parser::ast::MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Literal(Literal::Bool(false)),
                },
            ],
        };
        let result = expr_to_smtlib(&expr).unwrap();
        assert!(
            result.contains("(/ 1500000 1000000)"),
            "match float pattern should use rational: {result}"
        );
    }

    // Deep field chain flattening helpers (#250)
    // -------------------------------------------------------------------

    #[test]
    fn test_is_self_rooted_cvc5_ident_self() {
        let expr = Expr::Ident("self".into());
        assert!(is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_is_self_rooted_cvc5_ident_other() {
        let expr = Expr::Ident("x".into());
        assert!(!is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_is_self_rooted_cvc5_field_chain() {
        // self.value
        let expr = Expr::Field(Box::new(Expr::Ident("self".into())), "value".into());
        assert!(is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_is_self_rooted_cvc5_deep_chain() {
        // self.inner.value
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("self".into())),
                "inner".into(),
            )),
            "value".into(),
        );
        assert!(is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_field_chain_depth_cvc5_ident() {
        assert_eq!(field_chain_depth_cvc5(&Expr::Ident("x".into())), 0);
    }

    #[test]
    fn test_field_chain_depth_cvc5_single() {
        let expr = Expr::Field(Box::new(Expr::Ident("x".into())), "y".into());
        assert_eq!(field_chain_depth_cvc5(&expr), 1);
    }

    #[test]
    fn test_field_chain_depth_cvc5_deep() {
        // a.b.c -> depth 2
        let expr = Expr::Field(
            Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
            "c".into(),
        );
        assert_eq!(field_chain_depth_cvc5(&expr), 2);
    }

    #[test]
    fn test_has_deep_field_chain_cvc5() {
        // a.b -> depth 1, not deep
        let shallow = Expr::Field(Box::new(Expr::Ident("a".into())), "b".into());
        assert!(!has_deep_field_chain_cvc5(&shallow));

        // a.b.c -> depth 2, deep
        let deep = Expr::Field(
            Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
            "c".into(),
        );
        assert!(has_deep_field_chain_cvc5(&deep));
    }

    #[test]
    fn test_flatten_field_chain_cvc5_simple() {
        // a.b -> "a__b"
        let expr = Expr::Field(Box::new(Expr::Ident("a".into())), "b".into());
        assert_eq!(flatten_field_chain_cvc5(&expr), "a__b");
    }

    #[test]
    fn test_flatten_field_chain_cvc5_deep() {
        // state.head.extra.extra_max -> "state__head__extra__extra_max"
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Field(
                    Box::new(Expr::Ident("state".into())),
                    "head".into(),
                )),
                "extra".into(),
            )),
            "extra_max".into(),
        );
        assert_eq!(
            flatten_field_chain_cvc5(&expr),
            "state__head__extra__extra_max"
        );
    }

    #[test]
    fn test_flatten_field_chain_cvc5_paren() {
        // (a).b -> "a__b"
        let expr = Expr::Field(
            Box::new(Expr::Paren(Box::new(Expr::Ident("a".into())))),
            "b".into(),
        );
        assert_eq!(flatten_field_chain_cvc5(&expr), "a__b");
    }

    #[test]
    fn test_cvc5_deep_field_chain_smtlib_flattening() {
        // state.head.extra.extra_max should flatten in SMT-LIB output
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Field(
                    Box::new(Expr::Ident("state".into())),
                    "head".into(),
                )),
                "extra".into(),
            )),
            "extra_max".into(),
        );
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("state__head__extra__extra_max".into()));
    }

    #[test]
    fn test_cvc5_self_rooted_smtlib_flattening() {
        // self.value should flatten even at depth 1
        let expr = Expr::Field(Box::new(Expr::Ident("self".into())), "value".into());
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("self__value".into()));
    }

    #[test]
    fn test_cvc5_shallow_field_smtlib_no_flatten() {
        // obj.field at depth 1 (not self-rooted) should NOT flatten
        let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "field".into());
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("(__field_field obj)".into()));
    }

    #[test]
    fn test_cvc5_old_deep_field_smtlib_flattening() {
        // old(state.head.value) should flatten to state__head__value__old
        let inner = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("state".into())),
                "head".into(),
            )),
            "value".into(),
        );
        let expr = Expr::Old(Box::new(inner));
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("state__head__value__old".into()));
    }

    #[test]
    fn test_cvc5_old_self_rooted_smtlib_flattening() {
        // old(self.counter) should flatten to self__counter__old
        let inner = Expr::Field(Box::new(Expr::Ident("self".into())), "counter".into());
        let expr = Expr::Old(Box::new(inner));
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("self__counter__old".into()));
    }

    #[test]
    fn test_cvc5_deep_field_chain_contract_verifies() {
        // Contract: requires { x >= 0 && x < state.head.extra.max }
        //           ensures  { state.head.extra.max > x }
        // With flattening, both sides reference the same flat variable.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    }),
                    rhs: Box::new(Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Field(
                            Box::new(Expr::Field(
                                Box::new(Expr::Field(
                                    Box::new(Expr::Ident("state".into())),
                                    "head".into(),
                                )),
                                "extra".into(),
                            )),
                            "max".into(),
                        )),
                    }),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Field(
                        Box::new(Expr::Field(
                            Box::new(Expr::Field(
                                Box::new(Expr::Ident("state".into())),
                                "head".into(),
                            )),
                            "extra".into(),
                        )),
                        "max".into(),
                    )),
                    rhs: Box::new(Expr::Ident("x".into())),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("DeepFieldChain", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "deep field chain contract should verify (or Unknown if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_self_rooted_field_contract_verifies() {
        // Contract with self.value: requires { self.value > 0 } ensures { self.value >= 1 }
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Field(
                        Box::new(Expr::Ident("self".into())),
                        "value".into(),
                    )),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Field(
                        Box::new(Expr::Ident("self".into())),
                        "value".into(),
                    )),
                    rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("SelfRootedField", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "self-rooted field contract should verify (or Unknown if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_nested_field_boolean_smtlib() {
        // obj.inner.is_empty should flatten in SMT-LIB output
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("obj".into())),
                "inner".into(),
            )),
            "is_empty".into(),
        );
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("obj__inner__is_empty".into()));
    }

    #[test]
    fn test_cvc5_nested_field_size_smtlib() {
        // obj.inner.length should flatten in SMT-LIB output
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("obj".into())),
                "inner".into(),
            )),
            "length".into(),
        );
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("obj__inner__length".into()));
    }

    // -------------------------------------------------------------------
    // CVC5 native API tests (only when cvc5-verify feature enabled)
    // -------------------------------------------------------------------

    #[cfg(feature = "cvc5-verify")]
    mod native_tests {
        use super::*;
        use assura_parser::ast::Param;

        #[test]
        fn cvc5_with_types_fn_params_nat() {
            // FnDef-style: params passed explicitly (not via input() clause).
            // This is the path used for `fn check_table_bounds(root_bits: Nat, ...)`
            let params = vec![Param {
                name: "n".into(),
                ty: vec!["Nat".into()],
                parsed_type: None,
            }];
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("n".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            }];
            let mut cache = SessionCache::new();
            let results =
                verify_contract_cvc5_with_types("FnNatParam", &clauses, &params, &[], &mut cache);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "Nat param n >= 0 should verify via explicit params: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_trivial_ensures_verified() {
            // requires x > 0, ensures x > 0 (trivially true)
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        lhs: Box::new(Expr::Ident("x".into())),
                        op: BinOp::Gt,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        lhs: Box::new(Expr::Ident("x".into())),
                        op: BinOp::Gt,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NativeTest", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_counterexample() {
            // No requires, ensures x > 0 (counterexample: x = 0)
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeCounterexample", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Counterexample { .. }),
                "should have counterexample: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_invariant_satisfiable() {
            // invariant { x > 0 } -- satisfiable (x = 1)
            let clauses = vec![Clause {
                kind: ClauseKind::Invariant,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeInvariant", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "invariant should be satisfiable: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_must_not_true_counterexample() {
            // must_not { true } -- true is always possible, should be counterexample
            let clauses = vec![Clause {
                kind: ClauseKind::MustNot,
                body: Expr::Literal(Literal::Bool(true)),
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeMustNot", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Counterexample { .. }),
                "must_not(true) should be counterexample: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_must_not_false_verified() {
            // must_not { false } -- false is impossible, should verify
            let clauses = vec![Clause {
                kind: ClauseKind::MustNot,
                body: Expr::Literal(Literal::Bool(false)),
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeMustNotFalse", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "must_not(false) should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_nat_type_constraint() {
            // input(n: Nat), ensures n >= 0 -- should verify with Nat constraint
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Input,
                    body: Expr::Raw(vec!["n".into(), ":".into(), "Nat".into()]),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        lhs: Box::new(Expr::Ident("n".into())),
                        op: BinOp::Gte,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NatConstraint", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "Nat n >= 0 should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_unmodelable_typestate_skipped() {
            // A clause with @ annotations should produce Unknown, not a false counterexample
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("TestTypestate", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(
                    &results[0],
                    VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")
                ),
                "expected Unknown with 'not yet encoded', got {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_unmodelable_nested_typestate() {
            // Typestate annotation nested inside a binary expression
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    }),
                    rhs: Box::new(Expr::Raw(vec![
                        "conn".into(),
                        "@".into(),
                        "Connected".into(),
                    ])),
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NestedTypestate", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(
                    &results[0],
                    VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")
                ),
                "nested typestate should produce Unknown, got {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_check_validity_unmodelable() {
            // check_validity_cvc5 should also detect unmodelable features
            let body = Expr::Raw(vec!["state".into(), "@".into(), "Running".into()]);
            let result = check_validity_cvc5("validity_typestate", &[], &body);
            assert!(
                matches!(
                    &result,
                    VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")
                ),
                "check_validity_cvc5 should detect unmodelable: {:?}",
                result
            );
        }

        #[test]
        fn native_cvc5_check_satisfiability_unmodelable() {
            // check_satisfiability_cvc5 should also detect unmodelable features
            let body = Expr::Raw(vec!["lock".into(), "@".into(), "Acquired".into()]);
            let result = check_satisfiability_cvc5("sat_typestate", &[], &body);
            assert!(
                matches!(
                    &result,
                    VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")
                ),
                "check_satisfiability_cvc5 should detect unmodelable: {:?}",
                result
            );
        }

        // -------------------------------------------------------------------
        // String method axiom tests (CVC5 native, issue #251)
        // -------------------------------------------------------------------

        fn make_clause(kind: ClauseKind, body: Expr) -> Clause {
            Clause {
                kind,
                body,
                effect_variables: vec![],
            }
        }

        #[test]
        fn test_cvc5_string_substring_axiom() {
            // Contract: requires constraints on inputs,
            // ensures { substring(s, start, end).length() >= 0 }
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("len".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Lte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Ident("end_val".into())),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Call {
                                func: Box::new(Expr::Ident("substring".into())),
                                args: vec![
                                    Expr::Ident("s".into()),
                                    Expr::Ident("start".into()),
                                    Expr::Ident("end_val".into()),
                                ],
                            }),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("SubstringTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "Got unexpected counterexample: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_concat_axiom() {
            // ensures { concat(a, b).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("concat".into())),
                            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("ConcatTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "concat axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_indexof_axiom() {
            // requires { s.length() > 0 }
            // ensures { index_of(s, sub) >= -1 }
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("s".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("index_of".into())),
                            args: vec![Expr::Ident("s".into()), Expr::Ident("sub".into())],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("-1".into()))),
                    },
                ),
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("IndexOfTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "indexOf axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_charat_axiom() {
            // requires { idx >= 0 && s.length() > idx }
            // ensures { char_at(s, idx) >= 0 || char_at(s, idx) < 0 } (tautology -- tests axiom wiring)
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("idx".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("s".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Ident("idx".into())),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("idx".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("CharAtTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "charAt axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_replace_axiom() {
            // ensures { replace(s, old_s, new_s).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("replace".into())),
                            args: vec![
                                Expr::Ident("s".into()),
                                Expr::Ident("old_s".into()),
                                Expr::Ident("new_s".into()),
                            ],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("ReplaceTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "replace axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_split_axiom() {
            // ensures { split(s, delim).length() >= 1 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("split".into())),
                            args: vec![Expr::Ident("s".into()), Expr::Ident("delim".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("SplitTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "split axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_trim_axiom() {
            // ensures { trim(s).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("trim".into())),
                            args: vec![Expr::Ident("s".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("TrimTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "trim axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_array_set_axiom() {
            // ensures { set(arr, i, v).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("set".into())),
                            args: vec![
                                Expr::Ident("arr".into()),
                                Expr::Ident("i".into()),
                                Expr::Ident("v".into()),
                            ],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("ArraySetTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "array set axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_map_put_axiom() {
            // ensures { put(m, k, v).size() >= 0 } (via size axiom)
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("put".into())),
                            args: vec![
                                Expr::Ident("m".into()),
                                Expr::Ident("k".into()),
                                Expr::Ident("v".into()),
                            ],
                        }),
                        method: "size".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("MapPutTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "map put axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_method_call_substring_axiom() {
            // Test method call form: s.substring(start, end).length() >= 0
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Lte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Ident("end_val".into())),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::MethodCall {
                                receiver: Box::new(Expr::Ident("s".into())),
                                method: "substring".into(),
                                args: vec![
                                    Expr::Ident("start".into()),
                                    Expr::Ident("end_val".into()),
                                ],
                            }),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ];
            let results =
                crate::cvc5_backend::verify_contract_cvc5("MethodSubstringTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "method call substring axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_method_call_set_axiom() {
            // Test method call form: arr.set(i, v).length() >= 0
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("arr".into())),
                            method: "set".into(),
                            args: vec![Expr::Ident("i".into()), Expr::Ident("v".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("MethodArraySetTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "method call set axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_method_call_put_axiom() {
            // Test method call form: m.put(k, v).size() >= 0
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("m".into())),
                            method: "put".into(),
                            args: vec![Expr::Ident("k".into()), Expr::Ident("v".into())],
                        }),
                        method: "size".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("MethodMapPutTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "method call put axiom failed: {:?}",
                    r
                );
            }
        }
    }

    // -------------------------------------------------------------------
    // expr_to_smtlib string method tests (issue #251)
    // -------------------------------------------------------------------

    #[test]
    fn test_smtlib_call_substring() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("substring".into())),
            args: vec![
                Expr::Ident("s".into()),
                Expr::Literal(Literal::Int("0".into())),
                Expr::Literal(Literal::Int("5".into())),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(substring s 0 5)");
    }

    #[test]
    fn test_smtlib_call_concat() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("concat".into())),
            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(__concat a b)");
    }

    #[test]
    fn test_smtlib_call_index_of() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("index_of".into())),
            args: vec![Expr::Ident("s".into()), Expr::Ident("sub".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(index_of s sub)");
    }

    #[test]
    fn test_smtlib_call_char_at() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("char_at".into())),
            args: vec![
                Expr::Ident("s".into()),
                Expr::Literal(Literal::Int("3".into())),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(char_at s 3)");
    }

    #[test]
    fn test_smtlib_call_replace() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("replace".into())),
            args: vec![
                Expr::Ident("s".into()),
                Expr::Ident("old_s".into()),
                Expr::Ident("new_s".into()),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(replace s old_s new_s)");
    }

    #[test]
    fn test_smtlib_call_split() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("split".into())),
            args: vec![Expr::Ident("s".into()), Expr::Ident("delim".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(split s delim)");
    }

    #[test]
    fn test_smtlib_call_trim() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("trim".into())),
            args: vec![Expr::Ident("s".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(trim s)");
    }

    #[test]
    fn test_smtlib_call_set() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("set".into())),
            args: vec![
                Expr::Ident("arr".into()),
                Expr::Ident("i".into()),
                Expr::Ident("v".into()),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(set arr i v)");
    }

    #[test]
    fn test_smtlib_call_put() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("put".into())),
            args: vec![
                Expr::Ident("m".into()),
                Expr::Ident("k".into()),
                Expr::Ident("v".into()),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(put m k v)");
    }

    #[test]
    fn test_smtlib_method_substring() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("s".into())),
            method: "substring".into(),
            args: vec![
                Expr::Literal(Literal::Int("1".into())),
                Expr::Literal(Literal::Int("4".into())),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(substring s 1 4)");
    }

    #[test]
    fn test_smtlib_method_concat() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("a".into())),
            method: "concat".into(),
            args: vec![Expr::Ident("b".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(__concat a b)");
    }

    #[test]
    fn test_smtlib_method_set() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("arr".into())),
            method: "set".into(),
            args: vec![Expr::Ident("i".into()), Expr::Ident("v".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(set arr i v)");
    }

    #[test]
    fn test_smtlib_method_put() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("m".into())),
            method: "put".into(),
            args: vec![Expr::Ident("k".into()), Expr::Ident("v".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(put m k v)");
    }

    #[test]
    fn test_smtlib_method_trim() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("s".into())),
            method: "trim".into(),
            args: vec![],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(trim s)");
    }

    // -------------------------------------------------------------------
    // CVC5 match pattern tests (native API, issue #252)
    // -------------------------------------------------------------------

    #[cfg(feature = "cvc5-verify")]
    mod match_pattern_tests {
        use super::*;
        use assura_parser::ast::MatchArm;

        #[test]
        fn test_cvc5_match_constructor_pattern() {
            // ensures { match x { Some(v) => v > 0, None => true } }
            // with requires { x >= 0 } so scrut is constrained
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Match {
                        scrutinee: Box::new(Expr::Ident("x".into())),
                        arms: vec![
                            MatchArm {
                                pattern: Pattern::Constructor {
                                    name: "Positive".into(),
                                    fields: vec![Pattern::Ident("v".into())],
                                },
                                body: Expr::Literal(Literal::Bool(true)),
                            },
                            MatchArm {
                                pattern: Pattern::Wildcard,
                                body: Expr::Literal(Literal::Bool(true)),
                            },
                        ],
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("MatchConstructor", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // The match should encode without returning Unknown due to unhandled patterns
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")),
                    "Constructor pattern should be encoded, got: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_match_tuple_pattern() {
            // ensures { match t { (a, b) => true } }
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Match {
                    scrutinee: Box::new(Expr::Ident("t".into())),
                    arms: vec![MatchArm {
                        pattern: Pattern::Tuple(vec![
                            Pattern::Ident("a".into()),
                            Pattern::Ident("b".into()),
                        ]),
                        body: Expr::Literal(Literal::Bool(true)),
                    }],
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("MatchTuple", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // ensures { true } should verify
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "tuple match with body `true` should verify, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_match_nested_patterns() {
            // ensures { match x { Outer(Inner(v)) => true, _ => true } }
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Match {
                    scrutinee: Box::new(Expr::Ident("x".into())),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Constructor {
                                name: "Outer".into(),
                                fields: vec![Pattern::Constructor {
                                    name: "Inner".into(),
                                    fields: vec![Pattern::Ident("v".into())],
                                }],
                            },
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                        MatchArm {
                            pattern: Pattern::Wildcard,
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                    ],
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("MatchNested", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // All arms return true, so should verify
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "nested constructor match with all-true body should verify, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_match_enum_verifies() {
            // A simple enum-like match:
            //   requires { x >= 0 }
            //   ensures { match x { Zero => x == 0, _ => x >= 0 } }
            // We use Ident patterns with uppercase names as constructors.
            // Since both arms return expressions derivable from requires, it
            // should verify (or at worst produce a result, not Unknown).
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Match {
                        scrutinee: Box::new(Expr::Ident("x".into())),
                        arms: vec![
                            MatchArm {
                                pattern: Pattern::Ident("Zero".into()),
                                body: Expr::Literal(Literal::Bool(true)),
                            },
                            MatchArm {
                                pattern: Pattern::Wildcard,
                                body: Expr::BinOp {
                                    op: BinOp::Gte,
                                    lhs: Box::new(Expr::Ident("x".into())),
                                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                                },
                            },
                        ],
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("MatchEnum", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // Should not produce Unknown with "not yet encoded" reason
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")),
                    "Enum match should be encoded, got: {:?}",
                    r
                );
            }
        }
    }

    // -------------------------------------------------------------------
    // Frame axiom tests (CVC5 native, issue #256)
    // -------------------------------------------------------------------

    #[cfg(feature = "cvc5-verify")]
    mod frame_tests {
        use super::*;

        #[test]
        fn test_cvc5_frame_axiom_injection() {
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Modifies,
                    body: Expr::Ident("y".into()),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("FrameTest", &clauses);
            assert!(!results.is_empty());
        }

        #[test]
        fn test_cvc5_modifies_preserves_unmodified() {
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Modifies,
                    body: Expr::Ident("y".into()),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("FramePreserve", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "Frame axiom should prevent counterexample: {:?}",
                    r
                );
            }
        }

        // ---------------------------------------------------------------
        // Lemma injection tests (#254)
        // ---------------------------------------------------------------

        #[test]
        fn native_cvc5_lemma_injection_basic() {
            // Contract with apply(lemma): the ensures body contains an
            // apply expression, which should be encoded as a named bool.
            // Without lemma defs, this just produces a result (not a panic).
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Apply {
                        lemma_name: "helper_lemma".into(),
                        args: vec![Expr::Ident("x".into())],
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("LemmaTest", &clauses);
            assert!(!results.is_empty(), "should produce at least one result");
        }

        #[test]
        fn native_cvc5_lemma_postcondition_injected() {
            // Build a lemma_defs map where "pos_lemma" ensures x >= 0.
            // The ensures clause uses `apply pos_lemma(x)` inside a
            // conjunction with `true`. With the lemma postcondition
            // injected as an assumption, this should not produce false
            // counterexamples for the apply sub-expression.
            let mut lemma_defs = std::collections::HashMap::new();
            let lemma_ensures = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };
            lemma_defs.insert("pos_lemma".to_string(), vec![&lemma_ensures]);

            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::And,
                        lhs: Box::new(Expr::Apply {
                            lemma_name: "pos_lemma".into(),
                            args: vec![Expr::Ident("x".into())],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Bool(true))),
                    },
                    effect_variables: vec![],
                },
            ];
            let mut cache = SessionCache::new();
            let results = verify_contract_cvc5_with_lemmas(
                "ApplyPostcondTest",
                &clauses,
                &[],
                &[],
                Some(&lemma_defs),
                &[],
                &mut cache,
            );
            assert!(
                !results.is_empty(),
                "should produce at least one result with lemma injection"
            );
        }

        #[test]
        fn native_cvc5_lemma_injection_verifies_with_postcondition() {
            // The ensures clause says: x >= 0 (trivially follows from requires).
            // We also have an apply expression in the clause. With lemma defs
            // injecting x >= 0, the combined clause should still verify.
            let mut lemma_defs = std::collections::HashMap::new();
            let lemma_ensures = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };
            lemma_defs.insert("helper".to_string(), vec![&lemma_ensures]);

            // requires { x > 0 }
            // ensures { x >= 0 }  (trivially true from requires)
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let mut cache = SessionCache::new();
            let results = verify_contract_cvc5_with_lemmas(
                "LemmaVerifTest",
                &clauses,
                &[],
                &[],
                Some(&lemma_defs),
                &[],
                &mut cache,
            );
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "should verify with lemma injection: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_no_lemma_defs_still_works() {
            // When lemma_defs is None, the apply expression is just
            // encoded as a named boolean (no postcondition injected).
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Apply {
                    lemma_name: "unknown_lemma".into(),
                    args: vec![Expr::Ident("x".into())],
                },
                effect_variables: vec![],
            }];
            let mut cache = SessionCache::new();
            let results = verify_contract_cvc5_with_lemmas(
                "NoLemmaDefs",
                &clauses,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert!(
                !results.is_empty(),
                "should produce results even without lemma defs"
            );
        }

        // ---------------------------------------------------------------
        // CVC5 Real sort float encoding tests (#248)
        // ---------------------------------------------------------------

        #[test]
        fn test_cvc5_float_real_sort() {
            // Float literal in requires/ensures should encode as CVC5 Real sort.
            // requires { x > 0 }, requires { x < 1000000 },
            // ensures { x > 0 } -- trivially true from precondition
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("FloatRealSort", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "float Real sort should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_real_ite_promotion() {
            // ITE with mixed Int/Real branches should sort-promote.
            // requires { x > 0 }
            // ensures { if x > 0 then 1.5 else 0 > 0 }
            // The then branch is Real (1.5), else is Int (0).
            // Sort promotion converts the Int to Real so ITE succeeds.
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::If {
                            cond: Box::new(Expr::BinOp {
                                op: BinOp::Gt,
                                lhs: Box::new(Expr::Ident("x".into())),
                                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                            }),
                            then_branch: Box::new(Expr::Literal(Literal::Float("1.5".into()))),
                            else_branch: Some(Box::new(Expr::Literal(Literal::Int("0".into())))),
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("ItePromotion", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "ITE sort promotion should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_real_negation() {
            // Negated float should work with Real sort.
            // requires { x > 1.0 }, ensures { -x < 0.0 }
            // True because x > 1.0 implies -x < -1.0 < 0.0
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("1.0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::UnaryOp {
                            op: UnaryOp::Neg,
                            expr: Box::new(Expr::Ident("x".into())),
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("RealNeg", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "negated float Real should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_float_arithmetic_verifies() {
            // Float arithmetic: requires { x > 2.0 }, ensures { x + 1.0 > 3.0 }
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("2.0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::BinOp {
                            op: BinOp::Add,
                            lhs: Box::new(Expr::Ident("x".into())),
                            rhs: Box::new(Expr::Literal(Literal::Float("1.0".into()))),
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Float("3.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("FloatArith", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "float arithmetic should verify: {:?}",
                results[0]
            );
        }

        // ---------------------------------------------------------------
        // CVC5 quantifier trigger pattern inference tests (#247)
        // ---------------------------------------------------------------

        #[test]
        fn test_cvc5_quantifier_trigger_inference() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Call {
                    func: Box::new(Expr::Ident("f".into())),
                    args: vec![Expr::Ident("i".into())],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                !patterns.is_empty(),
                "should infer trigger from f(i) call in quantifier body"
            );
        }

        #[test]
        fn test_cvc5_trigger_no_call_no_pattern() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                patterns.is_empty(),
                "no function calls means no triggers: got {:?}",
                patterns.len()
            );
        }

        #[test]
        fn test_cvc5_trigger_nested_call() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("g".into())),
                        args: vec![Expr::Ident("i".into())],
                    }),
                    rhs: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("h".into())),
                        args: vec![Expr::Ident("i".into())],
                    }),
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                patterns.len() >= 2,
                "should infer triggers from both g(i) and h(i): got {}",
                patterns.len()
            );
        }

        #[test]
        fn test_cvc5_trigger_manager_integration() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::Call {
                func: Box::new(Expr::Ident("lookup".into())),
                args: vec![Expr::Ident("i".into())],
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                !patterns.is_empty(),
                "should infer trigger from lookup(i) via direct scan fallback"
            );
        }

        #[test]
        fn test_cvc5_quantified_with_trigger_verifies() {
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Forall {
                        var: "i".into(),
                        domain: Box::new(Expr::BinOp {
                            op: BinOp::Range,
                            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                            rhs: Box::new(Expr::Ident("x".into())),
                        }),
                        body: Box::new(Expr::BinOp {
                            op: BinOp::Gte,
                            lhs: Box::new(Expr::Ident("i".into())),
                            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                        }),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("QuantTriggerTest", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "quantified contract should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_multi_arg_trigger() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Call {
                    func: Box::new(Expr::Ident("lookup".into())),
                    args: vec![Expr::Ident("table".into()), Expr::Ident("i".into())],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                !patterns.is_empty(),
                "should infer trigger from multi-arg lookup(table, i)"
            );
        }

        // -------------------------------------------------------------------
        // CVC5 session cache tests (#253)
        // -------------------------------------------------------------------

        #[test]
        fn test_cvc5_session_cache_hit() {
            // Verify same contract twice; second call should return cached result
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];

            let mut cache = SessionCache::new();

            // First call: cache miss, runs CVC5
            let results1 = verify_contract_cvc5_with_lemmas(
                "CacheTest",
                &clauses,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results1.len(), 1);
            assert!(matches!(&results1[0], VerificationResult::Verified { .. }));
            assert_eq!(cache.entry_count(), 1);

            // Second call: cache hit, should not invoke CVC5
            let results2 = verify_contract_cvc5_with_lemmas(
                "CacheTest",
                &clauses,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results2.len(), 1);
            assert!(matches!(&results2[0], VerificationResult::Verified { .. }));
            // Cache should still have 1 entry (same key), with 1 hit
            assert_eq!(cache.entry_count(), 1);
            assert!(cache.hit_rate() > 0.0);
        }

        #[test]
        fn test_cvc5_session_cache_miss() {
            // Two different contracts should be cache misses
            let clauses_a = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let clauses_b = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("y".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("y".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];

            let mut cache = SessionCache::new();

            let results_a = verify_contract_cvc5_with_lemmas(
                "CacheA",
                &clauses_a,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results_a.len(), 1);
            assert_eq!(cache.entry_count(), 1);

            let results_b = verify_contract_cvc5_with_lemmas(
                "CacheB",
                &clauses_b,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results_b.len(), 1);
            // Both should be cache misses, so 2 entries
            assert_eq!(cache.entry_count(), 2);
        }
    }
}
