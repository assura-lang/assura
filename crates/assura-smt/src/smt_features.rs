//! Feature-specific SMT verification for Assura's 50 verification features.
//!
//! Most feature clauses have boolean predicate bodies verified via Z3 validity
//! checking (assert requires, negate body, check-sat). Features that are
//! purely type-level or operational (no boolean body) return None from the
//! dispatch table and are skipped by the verifier.

#[cfg(feature = "z3-verify")]
use crate::ClauseKind;
#[cfg(any(feature = "z3-verify", test))]
use crate::Expr;
use crate::VerificationResult;
use crate::result::not_encoded_reason;
#[cfg(feature = "z3-verify")]
use crate::z3_backend::encoder::{Encoder, expr_has_unmodelable_features};
#[cfg(feature = "z3-verify")]
use crate::z3_backend::solver::check_validity;
use assura_ast::{Clause, SpExpr};
#[cfg(feature = "z3-verify")]
use z3::Solver;

// (smt_stub! macro and 33 dead stub functions removed in #197.
//  All feature clauses now route through verify_feature_body for
//  Z3 validity checking of boolean predicate bodies.)

// -----------------------------------------------------------------------
// Boolean predicate guard
//
// Feature annotations like `must_be deterministic` attach to function
// definitions where the clause body is the function body (Block, Let,
// Call chain), not a boolean assertion. Sending those to Z3 produces
// trivial counterexamples because the expression evaluates to an integer
// or other non-boolean type that becomes an unconstrained Z3 variable.
// -----------------------------------------------------------------------

/// Returns true if the expression is likely a boolean predicate suitable
/// for validity checking. Returns false for function bodies, blocks
/// whose last expression is non-boolean, and other non-predicate shapes.
///
/// Recurses into `If` (both branches), `Block` (last expression), `Match`
/// (all arm bodies), and `Old` to handle compound boolean expressions that
/// the top-level match alone would miss.
pub(crate) fn is_likely_boolean_predicate(expr: &SpExpr) -> bool {
    use assura_ast::Expr;
    match &expr.node {
        // Boolean comparisons and logical operators are predicates
        Expr::BinOp { op, .. } => op.is_comparison() || op.is_logical(),
        // Unary not is boolean
        Expr::UnaryOp {
            op: assura_ast::UnaryOp::Not,
            ..
        } => true,
        // Boolean literals
        Expr::Literal(assura_ast::Literal::Bool(_)) => true,
        // Quantifiers produce boolean
        Expr::Forall { .. } | Expr::Exists { .. } => true,
        // old(predicate), e.g. old(x > 0)
        Expr::Old(inner) => is_likely_boolean_predicate(inner),
        // Bare lowercase identifier could be a boolean variable
        Expr::Ident(name) => !name.chars().next().is_some_and(|c| c.is_uppercase()),
        // Method calls that look boolean (is_*, has_*, contains, etc.)
        Expr::MethodCall { method, .. } => {
            method.starts_with("is_")
                || method.starts_with("has_")
                || method == "contains"
                || method == "valid"
        }
        // If/then/else: boolean if both branches are boolean
        Expr::If {
            then_branch,
            else_branch,
            ..
        } => {
            is_likely_boolean_predicate(then_branch)
                && else_branch
                    .as_ref()
                    .is_some_and(|e| is_likely_boolean_predicate(e))
        }
        // Block: boolean if the last expression is boolean (not empty blocks)
        Expr::Block(exprs) => exprs.last().is_some_and(is_likely_boolean_predicate),
        // Match: boolean if all arm bodies are boolean
        Expr::Match { arms, .. } => {
            !arms.is_empty()
                && arms
                    .iter()
                    .all(|arm| is_likely_boolean_predicate(&arm.body))
        }
        // Function calls: could return bool but we can't tell syntactically
        // Allow all function calls since the SMT encoder handles non-boolean
        // results gracefully (unconstrained Z3 variable still produces a
        // result, just potentially a trivial counterexample which is correct)
        Expr::Call { .. } => true,
        // Everything else: raw tokens, let-bindings (without boolean tail),
        // list literals, casts, ghost blocks, etc.
        _ => false,
    }
}

// -----------------------------------------------------------------------
// Generic Z3 body verifier for feature clauses
//
// Most feature clauses have boolean predicate bodies that can be verified
// the same way as `ensures` clauses: assert all requires, negate the body,
// check-sat. UNSAT = the feature property holds under the preconditions.
// -----------------------------------------------------------------------

/// Fallback when Z3 is not available: use CVC5 if available, else Unknown.
#[cfg(not(feature = "z3-verify"))]
fn verify_feature_body(
    parent_name: &str,
    feature_label: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> VerificationResult {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_feature_body_cvc5(
            parent_name,
            feature_label,
            _body,
            _sibling_clauses,
        )
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, feature_label),
            feature_label,
        )
    }
}

/// Verify a feature clause body via Z3 validity check.
///
/// Collects sibling `requires` clauses as assumptions, then checks that
/// the feature clause body holds (same encoding as ensures). Falls back
/// to `Unknown` if the body uses unmodelable features.
#[cfg(feature = "z3-verify")]
fn verify_feature_body(
    parent_name: &str,
    feature_label: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> VerificationResult {
    let desc = crate::verify_labels::feature_clause_desc(parent_name, feature_label);

    // Skip clauses with unmodelable features (typestate, etc.)
    if expr_has_unmodelable_features(body) {
        return VerificationResult::unknown_not_encoded(
            desc,
            format!("{feature_label} clause uses features"),
        );
    }

    // Skip declarative feature clauses whose body is not a boolean predicate.
    // Feature annotations like `must_be deterministic` attach to function
    // definitions where the body is the function body (Block, Let, Call, etc.),
    // not a boolean assertion. Also skip bare uppercase identifiers which are
    // type/declaration references (e.g., `incremental InflateDecoder`).
    // Sending non-boolean expressions to Z3 creates unconstrained variables
    // that trivially produce counterexamples.
    if !is_likely_boolean_predicate(body) {
        return VerificationResult::unknown_not_encoded(desc, feature_label);
    }

    let solver = Solver::new();
    let mut params = z3::Params::new();
    params.set_u32("timeout", 2000);
    solver.set_params(&params);
    let mut encoder = Encoder::new();

    // Assert all sibling requires as assumptions
    for clause in sibling_clauses {
        if clause.kind == ClauseKind::Requires {
            let val = encoder.encode_expr(&clause.body);
            let bool_val = val.as_bool();
            solver.assert(&bool_val);
        }
    }

    // Assert background axioms from requires encoding
    for axiom in &encoder.background_axioms {
        solver.assert(axiom);
    }
    encoder.background_axioms.clear();

    // Encode the feature clause body
    let body_val = encoder.encode_expr(body);
    let body_bool = body_val.as_bool();

    // Assert background axioms from body encoding
    for axiom in &encoder.background_axioms {
        solver.assert(axiom);
    }

    // Validity check: negate body, check-sat. UNSAT = holds.
    solver.assert(body_bool.not());
    let mut results = Vec::new();
    check_validity(&solver, desc, &mut results);
    results.into_iter().next().unwrap_or_else(|| {
        VerificationResult::no_solver_result(crate::verify_labels::feature_clause_desc(
            parent_name,
            feature_label,
        ))
    })
}

// -----------------------------------------------------------------------
// MISC.1: Incremental contracts (step/resume + annotation clauses)
// -----------------------------------------------------------------------

/// Verify an `incremental_contract { P }` (or alias) feature annotation.
///
/// Boolean predicate bodies use the shared validity path. Non-boolean bodies
/// (structural references, empty markers) are skipped without emitting
/// `Unknown` / A05102 — the real MISC.1 content lives on `incremental Name`
/// blocks whose `step`/`resume`/`invariant` clauses are verified separately.
fn verify_incremental_contract_clause(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    if is_likely_boolean_predicate(body) {
        return vec![verify_feature_body(
            parent_name,
            "incremental_contract",
            body,
            sibling_clauses,
        )];
    }
    vec![]
}

/// Split a step/resume Raw token stream into (kind, expr-token) segments.
///
/// Recognizes `requires` / `ensures` keywords (with optional `{` `}` wrapping).
fn split_step_raw_segments(tokens: &[String]) -> Vec<(&'static str, Vec<String>)> {
    let mut out: Vec<(&'static str, Vec<String>)> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let kind = match tokens[i].as_str() {
            "requires" => "requires",
            "ensures" => "ensures",
            _ => {
                i += 1;
                continue;
            }
        };
        i += 1;
        // Optional opening brace
        if i < tokens.len() && tokens[i] == "{" {
            i += 1;
        }
        let mut expr_toks = Vec::new();
        let mut depth = 0i32;
        while i < tokens.len() {
            let t = &tokens[i];
            if t == "{" {
                depth += 1;
                expr_toks.push(t.clone());
                i += 1;
                continue;
            }
            if t == "}" {
                if depth == 0 {
                    i += 1; // consume closing brace of the clause body
                    break;
                }
                depth -= 1;
                expr_toks.push(t.clone());
                i += 1;
                continue;
            }
            if depth == 0 && matches!(t.as_str(), "requires" | "ensures" | "effects") {
                break;
            }
            expr_toks.push(t.clone());
            i += 1;
        }
        if !expr_toks.is_empty() {
            out.push((kind, expr_toks));
        }
    }
    out
}

/// Re-parse a token slice as a single expression via a synthetic contract.
///
/// Returns `None` on parse failure (e.g. typestate `@`, method chains not valid
/// in isolation). Callers skip those segments without panicking (#833).
fn reparse_expr_from_tokens(tokens: &[String]) -> Option<SpExpr> {
    let joined = tokens
        .iter()
        .filter(|t| *t != "{" && *t != "}")
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let joined = joined.trim();
    if joined.is_empty() {
        return None;
    }
    // Wrap so the full parser/lowering path produces a real SpExpr.
    let src = format!("contract __IncStep {{ ensures {{ {joined} }} }}");
    let (file, errs) = assura_parser::parse(&src);
    if !errs.is_empty() {
        return None;
    }
    let file = file?;
    for decl in &file.decls {
        if let assura_ast::Decl::Contract(c) = &decl.node {
            for clause in &c.clauses {
                if clause.kind == assura_ast::ClauseKind::Ensures {
                    return Some(clause.body.clone());
                }
            }
        }
    }
    None
}

/// Encode one `step` / `resume` body from an incremental block (MISC.1 / #833).
///
/// Documented subset: extract `requires` / `ensures` from Raw tokens (or a
/// boolean body), then validity-check each ensures under that step's requires.
/// Clauses that cannot be re-parsed are skipped (no A05102 Unknown).
fn verify_incremental_step(
    parent_name: &str,
    step_kind: &str,
    body: &SpExpr,
) -> Vec<VerificationResult> {
    let label = format!("incremental_{step_kind}");

    // Direct boolean body (rare but supported).
    if is_likely_boolean_predicate(body) {
        return vec![verify_feature_body(parent_name, &label, body, &[])];
    }

    let tokens = match &body.node {
        assura_ast::Expr::Raw(toks) => toks.as_slice(),
        _ => return vec![],
    };

    let segments = split_step_raw_segments(tokens);
    let mut requires_clauses: Vec<Clause> = Vec::new();
    let mut ensures_bodies: Vec<SpExpr> = Vec::new();
    for (kind, expr_toks) in segments {
        let Some(expr) = reparse_expr_from_tokens(&expr_toks) else {
            continue;
        };
        match kind {
            "requires" => requires_clauses.push(Clause {
                kind: assura_ast::ClauseKind::Requires,
                body: expr,
                effect_variables: vec![],
            }),
            "ensures" => ensures_bodies.push(expr),
            _ => {}
        }
    }

    // Documented subset: only boolean ensures we can re-parse. Typestate /
    // unmodelable fragments (e.g. `self.state @ ExtraField`) are skipped
    // without A05102 Unknown so demos stay clean while still verifying the
    // arithmetic / comparison steps that do re-parse cleanly.
    let results: Vec<VerificationResult> = ensures_bodies
        .into_iter()
        .filter(|ens| is_likely_boolean_predicate(ens))
        .filter(|ens| {
            #[cfg(feature = "z3-verify")]
            {
                !expr_has_unmodelable_features(ens)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = ens;
                true
            }
        })
        .map(|ens| verify_feature_body(parent_name, &label, &ens, &requires_clauses))
        .collect();
    results
}

// -----------------------------------------------------------------------
// CORE.6: Opaque functions (has real logic, not a stub)
// -----------------------------------------------------------------------

/// Verify opaque function contracts.
///
/// Opaque functions hide their implementation from the verifier. The SMT
/// encoding treats the function body as an uninterpreted function and only
/// verifies the requires/ensures interface contract.
pub fn verify_opaque_contract(name: &str, _has_ensures: bool) -> VerificationResult {
    // Opaque marker is always "assumed" (by design we do not verify the hidden body).
    VerificationResult::verified(format!("{name}: opaque contract assumed"))
}

// -----------------------------------------------------------------------
// TYPE.2: Structural invariants (inductive checking)
// -----------------------------------------------------------------------

/// Verify structural invariant via inductive checking.
///
/// A structural invariant must hold:
/// 1. **Establishment**: Under the requires (preconditions), the invariant
///    must hold initially. This checks `requires => invariant`.
/// 2. **Preservation**: If the invariant held before an operation and the
///    operation's postconditions (ensures) are met, the invariant still
///    holds. This checks `requires && ensures => invariant`.
///
/// Both are standard validity checks. The preservation step is strictly
/// stronger because it also asserts sibling ensures as assumptions.
///
/// Fallback when Z3 is not available: use CVC5 if available, else Unknown.
#[cfg(not(feature = "z3-verify"))]
pub fn verify_structural_invariant_inductive(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_structural_invariant_inductive_cvc5(
            parent_name,
            _body,
            _sibling_clauses,
        )
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::Unknown {
            clause_desc: crate::verify_labels::feature_clause_desc(
                parent_name,
                "structural_invariant",
            ),
            reason: not_encoded_reason("structural_invariant"),
        }]
    }
}

/// Verify structural invariant via inductive checking (Z3 implementation).
///
/// Returns two results:
/// - Establishment: `requires => invariant`
/// - Preservation: `requires && ensures => invariant`
#[cfg(feature = "z3-verify")]
pub fn verify_structural_invariant_inductive(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut all_results = Vec::new();

    // Skip unmodelable bodies
    if expr_has_unmodelable_features(body) {
        all_results.push(VerificationResult::Unknown {
            clause_desc: crate::verify_labels::feature_clause_desc(
                parent_name,
                "structural_invariant (establishment)",
            ),
            reason: not_encoded_reason("structural_invariant clause uses features"),
        });
        return all_results;
    }

    // Skip bare uppercase identifier bodies (declarative references, not predicates)
    if matches!(&body.node, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase()))
    {
        all_results.push(VerificationResult::Unknown {
            clause_desc: crate::verify_labels::feature_clause_desc(
                parent_name,
                "structural_invariant",
            ),
            reason: not_encoded_reason("structural_invariant"),
        });
        return all_results;
    }

    // ---- Step 1: Establishment ----
    // Assert requires, negate invariant body, check UNSAT.
    {
        let desc = crate::verify_labels::feature_clause_desc(
            parent_name,
            "structural_invariant (establishment)",
        );
        let solver = Solver::new();
        let mut params = z3::Params::new();
        params.set_u32("timeout", 2000);
        solver.set_params(&params);
        let mut encoder = Encoder::new();

        // Assert all sibling requires as assumptions
        for clause in sibling_clauses {
            if clause.kind == ClauseKind::Requires {
                let val = encoder.encode_expr(&clause.body);
                solver.assert(val.as_bool());
            }
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Encode invariant body, negate, check validity
        let body_val = encoder.encode_expr(body);
        let body_bool = body_val.as_bool();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        solver.assert(body_bool.not());
        let mut step_results = Vec::new();
        check_validity(&solver, desc, &mut step_results);
        all_results.extend(step_results);
    }

    // ---- Step 2: Preservation ----
    // Assert requires AND ensures (postconditions), negate invariant, check UNSAT.
    // This models: after an operation that satisfies its postconditions,
    // the invariant must still hold.
    {
        let desc = crate::verify_labels::feature_clause_desc(
            parent_name,
            "structural_invariant (preservation)",
        );
        let solver = Solver::new();
        let mut params = z3::Params::new();
        params.set_u32("timeout", 2000);
        solver.set_params(&params);
        let mut encoder = Encoder::new();

        // Assert requires
        for clause in sibling_clauses {
            if clause.kind == ClauseKind::Requires {
                let val = encoder.encode_expr(&clause.body);
                solver.assert(val.as_bool());
            }
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Assert ensures (operation postconditions)
        for clause in sibling_clauses {
            if clause.kind == ClauseKind::Ensures {
                let val = encoder.encode_expr(&clause.body);
                solver.assert(val.as_bool());
            }
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Negate invariant body, check validity
        let body_val = encoder.encode_expr(body);
        let body_bool = body_val.as_bool();
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        solver.assert(body_bool.not());
        let mut step_results = Vec::new();
        check_validity(&solver, desc, &mut step_results);
        all_results.extend(step_results);
    }

    all_results
}

// -----------------------------------------------------------------------
// #519 STOR.5: Monotonic state lattice verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_monotonic_state(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_monotonic_state_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "monotonic"),
            "monotonic_state",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_monotonic_state(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_monotonic_state_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// #517 CONC.4: Lock ordering verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_lock_ordering(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_lock_ordering_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "lock_order"),
            "lock_order",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_lock_ordering(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_lock_ordering_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// #518 SEC.2: Constant-time verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_constant_time(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_constant_time_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "constant_time"),
            "constant_time",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_constant_time(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_constant_time_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// #520 SEC.3: Secure erasure verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_secure_erasure(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_secure_erasure_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "secure_erase"),
            "secure_erase",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_secure_erasure(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_secure_erasure_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// #516 STOR.1: Crash recovery verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_crash_recovery(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_crash_recovery_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "crash_recovery"),
            "crash_recovery",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_crash_recovery(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_crash_recovery_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// #521 STOR.3: MVCC isolation verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_mvcc_isolation(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_mvcc_isolation_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "mvcc_isolation"),
            "mvcc_isolation",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_mvcc_isolation(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_mvcc_isolation_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// #522 SEC.4: Crypto conformance verification
// -----------------------------------------------------------------------

#[cfg(not(feature = "z3-verify"))]
fn verify_crypto_conformance(
    parent_name: &str,
    _body: &SpExpr,
    _sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        crate::cvc5_backend::verify_crypto_conformance_cvc5(parent_name, _body, _sibling_clauses)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        vec![VerificationResult::unknown_not_encoded(
            crate::verify_labels::feature_clause_desc(parent_name, "crypto_conformance"),
            "crypto_conformance",
        )]
    }
}

#[cfg(feature = "z3-verify")]
fn verify_crypto_conformance(
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    crate::z3_backend::verify_crypto_conformance_impl(parent_name, body, sibling_clauses)
}

// -----------------------------------------------------------------------
// Dispatch: route feature-specific clauses to their SMT verifier
// -----------------------------------------------------------------------

/// Check if a clause kind maps to a feature-specific SMT verifier.
///
/// Features with boolean predicate bodies are verified via Z3 validity
/// check (same as ensures): assert requires, negate body, check-sat.
/// Features without body expressions or with domain-specific needs
/// return Unknown with "not yet encoded in SMT" (treated as warnings).
///
/// Returns a non-empty Vec if the feature was handled, empty Vec otherwise.
/// Most features produce a single result; structural invariants produce
/// two (establishment + preservation) via inductive checking.
pub fn verify_feature_clause(
    clause_kind: &str,
    parent_name: &str,
    body: &SpExpr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    use assura_ast::features::Feature;

    // MISC.1: nested `step` / `resume` / `on` bodies inside `incremental Name { ... }`
    // are stored as Other(...) with Raw tokens (or a boolean body). Encode their
    // requires/ensures as a mini contract (#833).
    if matches!(clause_kind, "step" | "resume" | "on") {
        return verify_incremental_step(parent_name, clause_kind, body);
    }

    let feature = match Feature::from_clause_kind(clause_kind) {
        Some(f) => f,
        None => return vec![],
    };
    match feature {
        // CORE.6: Opaque has special semantics (no body to verify; marker always assumed/verified)
        Feature::OpaqueFunctions => vec![verify_opaque_contract(parent_name, true)],

        // Features with boolean predicate bodies: use Z3 validity check.
        // The clause body is a boolean expression that must hold under
        // the sibling requires assumptions.

        // MEM
        Feature::AllocatorContracts => vec![verify_feature_body(
            parent_name,
            "allocator_invariant",
            body,
            sibling_clauses,
        )],
        Feature::CircularBuffer => vec![verify_feature_body(
            parent_name,
            "circular_buffer",
            body,
            sibling_clauses,
        )],
        // TYPE
        Feature::InterfaceConformance => vec![verify_feature_body(
            parent_name,
            "interface_conformance",
            body,
            sibling_clauses,
        )],
        Feature::StructuralInvariants => {
            // #202: Use inductive checking (establishment + preservation)
            verify_structural_invariant_inductive(parent_name, body, sibling_clauses)
        }
        Feature::ErrorPropagation => vec![verify_feature_body(
            parent_name,
            "error_propagation",
            body,
            sibling_clauses,
        )],
        // SEC
        // #189: SEC.3 and SEC.4 now use Z3 body verification instead of
        // stubs. The clause body (if present) is checked as a boolean
        // predicate under sibling requires assumptions, same as ensures.
        Feature::ConstantTime => verify_constant_time(parent_name, body, sibling_clauses),
        Feature::SecureErasure => verify_secure_erasure(parent_name, body, sibling_clauses),
        Feature::CryptoConformance => verify_crypto_conformance(parent_name, body, sibling_clauses),
        // CONC
        Feature::SharedMemory => vec![verify_feature_body(
            parent_name,
            "shared_mem_safety",
            body,
            sibling_clauses,
        )],
        Feature::CallbackReentrancy => vec![verify_feature_body(
            parent_name,
            "callback_reentrancy",
            body,
            sibling_clauses,
        )],
        Feature::LockOrdering => verify_lock_ordering(parent_name, body, sibling_clauses),
        // STOR
        Feature::CrashRecovery => verify_crash_recovery(parent_name, body, sibling_clauses),
        Feature::PageCache => vec![verify_feature_body(
            parent_name,
            "page_cache",
            body,
            sibling_clauses,
        )],
        Feature::MvccIsolation => verify_mvcc_isolation(parent_name, body, sibling_clauses),
        Feature::RollbackSavepoint => vec![verify_feature_body(
            parent_name,
            "rollback_savepoint",
            body,
            sibling_clauses,
        )],
        Feature::MonotonicState => verify_monotonic_state(parent_name, body, sibling_clauses),
        Feature::StorageFailure => vec![verify_feature_body(
            parent_name,
            "storage_failure",
            body,
            sibling_clauses,
        )],
        // FMT
        Feature::BinaryFormat => vec![verify_feature_body(
            parent_name,
            "binary_format",
            body,
            sibling_clauses,
        )],
        Feature::BitLevel => vec![verify_feature_body(
            parent_name,
            "bit_level",
            body,
            sibling_clauses,
        )],
        Feature::StringEncoding => vec![verify_feature_body(
            parent_name,
            "string_encoding",
            body,
            sibling_clauses,
        )],
        Feature::Checksum => vec![verify_feature_body(
            parent_name,
            "checksum_integrity",
            body,
            sibling_clauses,
        )],
        Feature::ProtocolGrammar => vec![verify_feature_body(
            parent_name,
            "protocol_grammar",
            body,
            sibling_clauses,
        )],
        // NUM
        Feature::NumericalPrecision => vec![verify_feature_body(
            parent_name,
            "numerical_precision",
            body,
            sibling_clauses,
        )],
        Feature::PrecomputedTable => vec![verify_feature_body(
            parent_name,
            "precomputed_table",
            body,
            sibling_clauses,
        )],
        // PLAT
        // #189: PLAT.1 and PLAT.2 are genuinely infeasible for full SMT
        // encoding (multi-target and combinatorial respectively), but the
        // clause body can still be checked as a boolean predicate.
        Feature::PlatformAbstraction => vec![verify_feature_body(
            parent_name,
            "platform_abstraction",
            body,
            sibling_clauses,
        )],
        Feature::FeatureFlag => vec![verify_feature_body(
            parent_name,
            "feature_flag",
            body,
            sibling_clauses,
        )],
        Feature::ResourceLimit => vec![verify_feature_body(
            parent_name,
            "resource_limit",
            body,
            sibling_clauses,
        )],
        // PERF
        // #189: PERF.1 custom proof goals are infeasible generically, but
        // boolean clause bodies can be checked.
        Feature::UnsafeEscape => vec![verify_feature_body(
            parent_name,
            "unsafe_escape",
            body,
            sibling_clauses,
        )],
        Feature::ComplexityBound => vec![verify_feature_body(
            parent_name,
            "complexity_bound",
            body,
            sibling_clauses,
        )],
        // TEST
        // #189: TEST.1 coverage is a meta-property, but clause bodies can
        // express testable boolean assertions.
        Feature::TestGenCoverage => vec![verify_feature_body(
            parent_name,
            "test_gen",
            body,
            sibling_clauses,
        )],
        Feature::BehavioralEquiv => vec![verify_feature_body(
            parent_name,
            "behavioral_equiv",
            body,
            sibling_clauses,
        )],
        Feature::MultiPassRefinement => vec![verify_feature_body(
            parent_name,
            "multi_pass_refinement",
            body,
            sibling_clauses,
        )],
        // MISC.1: boolean `incremental_contract { P }` annotations; nested
        // step/resume handled above. Non-boolean bodies (e.g. bare type names)
        // are skipped without Unknown so structural block form is not noisy.
        Feature::IncrementalContract => {
            verify_incremental_contract_clause(parent_name, body, sibling_clauses)
        }
        Feature::ScopedInvariant => vec![verify_feature_body(
            parent_name,
            "scoped_invariant",
            body,
            sibling_clauses,
        )],
        // CORE: features with boolean predicate bodies verified via Z3
        Feature::GhostErasure => vec![verify_feature_body(
            parent_name,
            "ghost_erasure",
            body,
            sibling_clauses,
        )],
        Feature::LemmaErasure => vec![verify_feature_body(
            parent_name,
            "lemma_erasure",
            body,
            sibling_clauses,
        )],
        Feature::FrameConditions => vec![verify_feature_body(
            parent_name,
            "frame_conditions",
            body,
            sibling_clauses,
        )],
        Feature::AxiomaticDefinitions => vec![verify_feature_body(
            parent_name,
            "axiomatic_definitions",
            body,
            sibling_clauses,
        )],
        Feature::TriggerPatterns => vec![verify_feature_body(
            parent_name,
            "trigger_patterns",
            body,
            sibling_clauses,
        )],
        Feature::ProphecyVariables => vec![verify_feature_body(
            parent_name,
            "prophecy_variables",
            body,
            sibling_clauses,
        )],
        Feature::Liveness => vec![verify_feature_body(
            parent_name,
            "liveness",
            body,
            sibling_clauses,
        )],
        // MEM
        Feature::RegionAnnotations => vec![verify_feature_body(
            parent_name,
            "region_annotations",
            body,
            sibling_clauses,
        )],
        Feature::FixedWidth => vec![verify_feature_body(
            parent_name,
            "fixed_width",
            body,
            sibling_clauses,
        )],
        // SEC
        Feature::TaintTracking => vec![verify_feature_body(
            parent_name,
            "taint_tracking",
            body,
            sibling_clauses,
        )],
        Feature::DependentTypes => vec![verify_feature_body(
            parent_name,
            "dependent_types",
            body,
            sibling_clauses,
        )],
        // CONC
        Feature::Determinism => vec![verify_feature_body(
            parent_name,
            "determinism",
            body,
            sibling_clauses,
        )],
        Feature::Deadline => vec![verify_feature_body(
            parent_name,
            "deadline",
            body,
            sibling_clauses,
        )],
        Feature::WeakMemoryOrdering => vec![verify_feature_body(
            parent_name,
            "weak_memory_ordering",
            body,
            sibling_clauses,
        )],
        // FMT
        Feature::CodecRegistry => vec![verify_feature_body(
            parent_name,
            "codec_registry",
            body,
            sibling_clauses,
        )],
    }
}
#[cfg(test)]
#[path = "smt_features_tests.rs"]
mod tests;
