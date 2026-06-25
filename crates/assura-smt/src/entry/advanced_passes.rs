//! Advanced SMT passes: prophecy, weak memory, liveness, layer2, codec, portfolio helpers.

use assura_ast::{expr_to_string, BinOp, BlockKind, ClauseKind, Decl, Expr, SpExpr};
use assura_types::TypedFile;
use assura_types::checkers::expr_references_var;

use crate::advanced::{
    CodecDispatcher, LivenessChecker, LivenessKind, MemoryOrdering, ProphecyManager,
    WeakMemoryChecker,
};
use crate::cache::SessionCache;
use crate::result::VerificationResult;
use crate::verify_context::ContractVerifyContext;

use super::helpers::VerifyFileExtras;
use super::jobs::collect_verification_jobs;

// ---------------------------------------------------------------------------
// Shared advanced-pass helpers (solver-agnostic, used by both Z3 and CVC5)
// ---------------------------------------------------------------------------

/// Parse a string into the SMT-local MemoryOrdering enum.
fn parse_memory_ordering(s: &str) -> Option<MemoryOrdering> {
    match s {
        "relaxed" => Some(MemoryOrdering::Relaxed),
        "acquire" => Some(MemoryOrdering::Acquire),
        "release" => Some(MemoryOrdering::Release),
        "acqrel" | "acq_rel" => Some(MemoryOrdering::AcqRel),
        "seq_cst" => Some(MemoryOrdering::SeqCst),
        _ => None,
    }
}

/// Extract a numeric argument from an expression tree (for eventually_within bounds).
fn extract_numeric_arg(expr: &SpExpr) -> Option<u64> {
    match &expr.node {
        Expr::Literal(assura_ast::Literal::Int(s)) => s.parse().ok(),
        Expr::Call { args, .. } => args.iter().find_map(extract_numeric_arg),
        Expr::Raw(tokens) => tokens.iter().find_map(|t| t.parse::<u64>().ok()),
        Expr::Block(exprs) => exprs.iter().find_map(extract_numeric_arg),
        _ => None,
    }
}

/// Scan an expression for prophecy resolution calls: resolve(var, value).
fn resolve_prophecy_vars(expr: &SpExpr, fn_name: &str, pm: &mut ProphecyManager) {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node
                && (name == "resolve" || name == "resolve_prophecy")
                && let Some(first) = args.first()
                && let Expr::Ident(var_name) = &first.node
            {
                let value = args
                    .get(1)
                    .map(|a| expr_to_string(a))
                    .unwrap_or_default();
                if let Err(e) = pm.resolve(&format!("{fn_name}:{var_name}"), value) {
                    eprintln!("warning: prophecy resolution failed: {e}");
                }
            }
            for arg in args {
                resolve_prophecy_vars(arg, fn_name, pm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            resolve_prophecy_vars(lhs, fn_name, pm);
            resolve_prophecy_vars(rhs, fn_name, pm);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            resolve_prophecy_vars(inner, fn_name, pm)
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                resolve_prophecy_vars(e, fn_name, pm);
            }
        }
        _ => {}
    }
}

/// Scan an expression for prophecy constraint patterns (equality with prophecy vars).
fn constrain_prophecy_vars(expr: &SpExpr, fn_name: &str, pm: &mut ProphecyManager) {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node
                && (name == "constrain" || name == "constrain_prophecy")
                && let Some(first) = args.first()
                && let Expr::Ident(var_name) = &first.node
            {
                let constraint = args
                    .get(1)
                    .map(|a| expr_to_string(a))
                    .unwrap_or_default();
                pm.add_constraint(&format!("{fn_name}:{var_name}"), constraint);
            }
            for arg in args {
                constrain_prophecy_vars(arg, fn_name, pm);
            }
        }
        Expr::BinOp { lhs, rhs, op } => {
            // An equality like `prophecy(x) == expr` constrains x
            if *op == BinOp::Eq
                && let Expr::Call { func, args } = &lhs.as_ref().node
                && let Expr::Ident(name) = &func.as_ref().node
                && (name == "prophecy" || name == "prophesy")
                && let Some(first) = args.first()
                && let Expr::Ident(var_name) = &first.node
            {
                pm.add_constraint(&format!("{fn_name}:{var_name}"), expr_to_string(rhs));
            }
            constrain_prophecy_vars(lhs, fn_name, pm);
            constrain_prophecy_vars(rhs, fn_name, pm);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            constrain_prophecy_vars(inner, fn_name, pm)
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                constrain_prophecy_vars(e, fn_name, pm);
            }
        }
        _ => {}
    }
}

/// Collect prophecy variable references from ensures clauses.
fn collect_prophecy_refs(expr: &SpExpr, fn_name: &str, pm: &mut ProphecyManager) {
    match &expr.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node
                && (name == "prophecy" || name == "prophesy")
                && let Some(first) = args.first()
                && let Expr::Ident(var_name) = &first.node
            {
                pm.declare(format!("{fn_name}:{var_name}"));
            }
            for arg in args {
                collect_prophecy_refs(arg, fn_name, pm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_prophecy_refs(lhs, fn_name, pm);
            collect_prophecy_refs(rhs, fn_name, pm);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            collect_prophecy_refs(inner, fn_name, pm)
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Five advanced verification passes (solver-agnostic)
// ---------------------------------------------------------------------------

/// Run weak memory ordering checks on all declarations (#230).
///
/// Scans contracts/functions for ordering annotations and keyword patterns,
/// then checks for data races.
pub(crate) fn run_weak_memory_checks(typed: &TypedFile) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut wm_checker = WeakMemoryChecker::new();
    for decl in &typed.resolved.source.decls {
        let name = match decl.node.name() {
            Some(n) => n,
            None => continue,
        };
        let clauses = decl.node.clauses();
        if clauses.is_empty() {
            continue;
        }
        // Prefer structured ClauseKind::Ordering over keyword scanning
        let mut found_ordering = false;
        for clause in clauses {
            if clause.kind == ClauseKind::Ordering {
                let ordering_str = match &clause.body.node {
                    Expr::Ident(s) => Some(s.as_str()),
                    Expr::Raw(tokens) => tokens
                        .iter()
                        .find(|t| parse_memory_ordering(t).is_some())
                        .map(|t| t.as_str()),
                    _ => None,
                };
                if let Some(ord) = ordering_str.and_then(parse_memory_ordering) {
                    wm_checker.record_access(1, name.to_string(), true, ord);
                    found_ordering = true;
                }
            }
        }
        // Fall back to keyword scanning in effects clauses
        if !found_ordering {
            for clause in clauses {
                if clause.kind == ClauseKind::Effects
                    && (expr_references_var(&clause.body, "relaxed")
                        || expr_references_var(&clause.body, "acquire")
                        || expr_references_var(&clause.body, "release")
                        || expr_references_var(&clause.body, "seq_cst"))
                {
                    let ordering = if expr_references_var(&clause.body, "seq_cst") {
                        MemoryOrdering::SeqCst
                    } else if expr_references_var(&clause.body, "acquire") {
                        MemoryOrdering::Acquire
                    } else if expr_references_var(&clause.body, "release") {
                        MemoryOrdering::Release
                    } else {
                        MemoryOrdering::Relaxed
                    };
                    wm_checker.record_access(1, name.to_string(), true, ordering);
                }
            }
        }
    }
    for race in wm_checker.check_data_races() {
        results.push(VerificationResult::Unknown {
            clause_desc: "weak_memory".into(),
            reason: race,
        });
    }
    results
}

/// Run prophecy variable checks on all declarations (#233).
///
/// Registers top-level prophecy declarations, scans ensures/requires for
/// prophecy refs, resolutions, and constraints, then checks for unresolved
/// and unconstrained prophecy variables.
pub(crate) fn run_prophecy_checks(typed: &TypedFile) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut pm = ProphecyManager::new();
    // Register top-level prophecy declarations
    for decl in &typed.resolved.source.decls {
        if let Decl::Prophecy(p) = &decl.node {
            pm.declare(p.name.clone());
        }
    }
    for decl in &typed.resolved.source.decls {
        let ctx_name = match decl.node.name() {
            Some(n) => n,
            None => continue,
        };
        let clauses = decl.node.clauses();
        if clauses.is_empty() {
            continue;
        }
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                collect_prophecy_refs(&clause.body, ctx_name, &mut pm);
            }
            if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
                resolve_prophecy_vars(&clause.body, ctx_name, &mut pm);
                constrain_prophecy_vars(&clause.body, ctx_name, &mut pm);
            }
        }
    }
    for err in pm.check_all_resolved() {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("prophecy [{}]", err.code),
            reason: err.message,
        });
    }
    for err in pm.check_unconstrained() {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("prophecy [{}]", err.code),
            reason: err.message,
        });
    }
    results
}

/// Run liveness obligation checks on all declarations (#231).
///
/// Extracts obligations from liveness blocks and contract ensures clauses,
/// then checks fairness, bounds, and unverified obligations.
pub(crate) fn run_liveness_checks(typed: &TypedFile) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut lc = LivenessChecker::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::Liveness => {
                for clause in body {
                    match &clause.kind {
                        ClauseKind::Other(k) if k == "assume" => {
                            let text = expr_to_string(&clause.body);
                            if text.contains("fair") {
                                lc.add_fairness(format!("{name}:fair"));
                            }
                        }
                        ClauseKind::Other(k) if k == "prove" => {
                            let text = expr_to_string(&clause.body);
                            let liveness_kind = if expr_references_var(&clause.body, "leads_to") {
                                LivenessKind::LeadsTo
                            } else if expr_references_var(&clause.body, "eventually_within") {
                                let bound = extract_numeric_arg(&clause.body).unwrap_or(100);
                                LivenessKind::EventuallyWithin(bound)
                            } else {
                                LivenessKind::Eventually
                            };
                            lc.add_obligation(
                                format!("{name}:prove"),
                                liveness_kind,
                                text.clone(),
                                text,
                            );
                        }
                        _ => {}
                    }
                }
            }
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Ensures
                        && (expr_references_var(&clause.body, "eventually")
                            || expr_references_var(&clause.body, "leads_to"))
                    {
                        lc.add_obligation(
                            format!("{}:liveness", c.name),
                            LivenessKind::Eventually,
                            expr_to_string(&clause.body),
                            String::new(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    for err in lc.check_fairness() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness:fairness".into(),
            reason: err,
        });
    }
    for err in lc.check_bounded() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness:bounds".into(),
            reason: err,
        });
    }
    for err in lc.check_unverified() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness".into(),
            reason: err,
        });
    }
    results
}

/// Run Layer 2 verification: quantified invariants, termination, roundtrip (#232).
pub(crate) fn run_layer2_checks(typed: &TypedFile, timeout_ms: u64) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let l2_config = crate::layer2::Layer2Config::new().with_timeout(timeout_ms);
    let mut l2 = crate::layer2::Layer2Verifier::new(l2_config);

    for decl in &typed.resolved.source.decls {
        let name = match decl.node.name() {
            Some(n) => n,
            None => continue,
        };
        let clauses = decl.node.clauses();
        if clauses.is_empty() {
            continue;
        }
        for clause in clauses {
            if clause.kind == ClauseKind::Invariant {
                match &clause.body.node {
                    Expr::Forall { var, domain, body } => {
                        let sort = expr_to_string(domain);
                        l2.add_invariant(crate::layer2::QuantifiedInvariant {
                            name: format!("{name}:invariant"),
                            bound_vars: vec![(var.clone(), sort)],
                            body: expr_to_string(body),
                            triggers: Vec::new(),
                        });
                    }
                    Expr::Exists { var, domain, body } => {
                        let sort = expr_to_string(domain);
                        l2.add_invariant(crate::layer2::QuantifiedInvariant {
                            name: format!("{name}:invariant"),
                            bound_vars: vec![(var.clone(), sort)],
                            body: expr_to_string(body),
                            triggers: Vec::new(),
                        });
                    }
                    _ => {}
                }
            }
            if clause.kind == ClauseKind::Decreases {
                l2.add_termination(crate::layer2::TerminationObligation {
                    fn_name: name.to_string(),
                    measure: expr_to_string(&clause.body),
                    recursive_calls: Vec::new(),
                });
            }
        }
    }

    if l2.obligation_count() > 0 {
        for l2r in l2.verify() {
            match l2r {
                crate::layer2::Layer2Result::Verified { invariant, .. } => {
                    results.push(VerificationResult::verified(format!("layer2:{invariant}")));
                }
                crate::layer2::Layer2Result::Counterexample {
                    invariant, model, ..
                } => {
                    let model_str = model
                        .iter()
                        .map(|(k, v)| format!("{k} = {v}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    results.push(VerificationResult::Counterexample {
                        clause_desc: format!("layer2:{invariant}"),
                        model: model_str,
                        counter_model: None,
                    });
                }
                crate::layer2::Layer2Result::Timeout {
                    invariant,
                    timeout_ms: t,
                } => {
                    results.push(VerificationResult::Timeout {
                        clause_desc: format!("layer2:{invariant} (timeout {t}ms)"),
                    });
                }
                crate::layer2::Layer2Result::Unknown { invariant, reason } => {
                    results.push(VerificationResult::Unknown {
                        clause_desc: format!("layer2:{invariant}"),
                        reason,
                    });
                }
            }
        }
    }
    results
}

/// Run codec ambiguity checks on all declarations (#234).
pub(crate) fn run_codec_checks(typed: &TypedFile) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut codec_disp = CodecDispatcher::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::CodecRegistry(cr) = &decl.node {
            for entry in &cr.codecs {
                if let assura_ast::MagicPattern::Bytes { bytes, .. } = &entry.magic {
                    codec_disp.register(entry.name.clone(), bytes.clone(), 0);
                }
            }
        }
    }
    for (a, b) in codec_disp.check_ambiguity() {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("codec:ambiguity:{a}/{b}"),
            reason: format!(
                "codecs `{a}` and `{b}` share identical magic bytes at the same offset"
            ),
        });
    }
    results
}

/// Run all five advanced verification passes (solver-agnostic).
///
/// Called by both the Z3 and CVC5 file-level verification paths.
pub(crate) fn run_advanced_passes(typed: &TypedFile, timeout_ms: u64) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    results.extend(run_weak_memory_checks(typed));
    results.extend(run_prophecy_checks(typed));
    results.extend(run_liveness_checks(typed));
    results.extend(run_layer2_checks(typed, timeout_ms));
    results.extend(run_codec_checks(typed));
    results
}

/// Verify all contracts in a file using the CVC5 backend.
pub(crate) fn verify_file_with_cvc5(
    typed: &TypedFile,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Collect lemma definitions so `apply lemma_name(args)` can inject
    // postconditions as solver assumptions (matching Z3 backend behavior).
    let lemma_defs = crate::cvc5_backend::collect_lemma_defs_for_cvc5(typed);

    // #257: collect feature_max constants so the CVC5 encoder binds them
    // to concrete values instead of creating free solver variables.
    let constants = crate::feature_max::collect_feature_max_constants(typed);

    // #253: per-file session cache for CVC5 clause deduplication
    let mut session_cache = SessionCache::new();

    // Clause-level verification via CVC5
    for (name, clauses, params, return_ty) in collect_verification_jobs(typed) {
        let ctx = ContractVerifyContext {
            contract_name: &name,
            clauses: &clauses,
            params: &params,
            return_ty: &return_ty,
            constants: &constants,
            ir: crate::verify_context::LoadedIrContext::for_contract(
                &name,
                extras,
                Some(&typed.type_env),
            ),
        };
        results.extend(crate::cvc5_backend::verify_contract_cvc5_with_lemmas(
            &ctx,
            Some(&lemma_defs),
            &mut session_cache,
        ));
    }

    // Run the same 5 advanced passes that the Z3 backend runs
    results.extend(run_advanced_passes(typed, 2000));

    results
}

/// Run Z3 and CVC5 concurrently, merge results per-clause (#245).
///
/// Uses `std::thread::scope` for exactly 2 threads (not rayon). Each solver
/// gets its own context. The merge prefers definitive results (Verified or
/// Counterexample) over inconclusive ones (Timeout or Unknown).
#[cfg(feature = "z3-verify")]
pub(crate) fn verify_portfolio_parallel(
    typed: &TypedFile,
    timeout_ms: u64,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    use std::sync::mpsc;

    std::thread::scope(|s| {
        let (tx_z3, rx) = mpsc::channel::<(&str, Vec<VerificationResult>)>();
        let tx_cvc5 = tx_z3.clone();

        s.spawn(move || {
            let results = crate::z3_backend::verify_impl_with_timeout(typed, timeout_ms, extras);
            let _ = tx_z3.send(("z3", results));
        });
        s.spawn(move || {
            let results = verify_file_with_cvc5(typed, extras);
            let _ = tx_cvc5.send(("cvc5", results));
        });

        // Collect both results
        let mut z3_results = None;
        let mut cvc5_results = None;
        for (solver, results) in rx {
            match solver {
                "z3" => z3_results = Some(results),
                _ => cvc5_results = Some(results),
            }
        }

        let z3 = z3_results.unwrap_or_default();
        let cvc5 = cvc5_results.unwrap_or_default();
        merge_portfolio_results(z3, cvc5)
    })
}

/// Merge Z3 and CVC5 results per-clause (delegates to [`crate::portfolio_policy`]).
#[cfg(feature = "z3-verify")]
pub(crate) fn merge_portfolio_results(
    z3: Vec<VerificationResult>,
    cvc5: Vec<VerificationResult>,
) -> Vec<VerificationResult> {
    crate::portfolio_policy::merge_portfolio_results(z3, cvc5)
}

/// Pick the better of two results for the same clause (delegates to [`crate::portfolio_policy`]).
#[cfg(feature = "z3-verify")]
#[allow(dead_code)] // re-export for tests / call sites that still use this name
pub(crate) fn pick_better_result(
    z3r: VerificationResult,
    cvc5r: VerificationResult,
) -> VerificationResult {
    crate::portfolio_policy::pick_better_portfolio_result(z3r, cvc5r)
}
