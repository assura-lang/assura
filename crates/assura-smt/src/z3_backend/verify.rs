//! High-level Z3 verification dispatch: clause verification, contract
//! verification, quantified verification, lemma collection, and
//! prophecy/trigger helpers.

use super::encoder::{Encoder, collect_unmodelable_reasons, expr_has_unmodelable_features};
use super::solver::extract_counter_model;
use super::solver::{check_satisfiability, check_validity, clause_desc};
use crate::advanced::ProphecyManager;
use crate::cache::SessionCache;
use crate::*;
use assura_parser::ast::{BinOp, BlockKind, Clause};
use assura_types::checkers::expr_references_var;
use z3::{SatResult, Solver, ast};

// -----------------------------------------------------------------------
// Contract clause verification
// -----------------------------------------------------------------------

/// Verify a set of clauses from a contract, fn, or extern declaration.
fn verify_clauses(
    parent_name: &str,
    clauses: &[Clause],
    lemma_defs: &std::collections::HashMap<String, Vec<&Expr>>,
    cache: &mut SessionCache,
    results: &mut Vec<VerificationResult>,
) {
    let requires: Vec<&Clause> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .collect();

    let verifiable: Vec<&Clause> = clauses
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                ClauseKind::Ensures
                    | ClauseKind::Invariant
                    | ClauseKind::Rule
                    | ClauseKind::MustNot
                    | ClauseKind::Decreases
            )
        })
        .collect();

    // Process feature-specific Other clauses via SMT feature dispatch
    for clause in clauses {
        if let ClauseKind::Other(kind) = &clause.kind
            && let Some(result) = crate::smt_features::verify_feature_clause(kind, parent_name)
        {
            results.push(result);
        }
    }

    if verifiable.is_empty() {
        return;
    }

    // T045: Build frame checker from modifies clauses
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        let body_refs: Vec<&Expr> = modifies_bodies.to_vec();
        assura_types::FrameChecker::new(&body_refs)
    };

    for clause in &verifiable {
        let desc = clause_desc(parent_name, &clause.kind);

        // Skip clauses that reference features not yet encoded in SMT.
        // Sending an incomplete encoding to Z3 produces false counterexamples
        // (Z3 finds trivial models for unconstrained uninterpreted functions).
        if expr_has_unmodelable_features(&clause.body) {
            let reasons = collect_unmodelable_reasons(&clause.body);
            results.push(VerificationResult::Unknown {
                clause_desc: desc,
                reason: format!(
                    "clause uses features not yet encoded in SMT ({})",
                    reasons.join(", ")
                ),
            });
            continue;
        }

        // T113: Check verification cache before invoking Z3
        let clause_hash = format!("{desc}:{:?}", clause.body);
        if let Some(cached) = cache.lookup(&clause_hash) {
            // Replay cached result
            match cached.result.as_str() {
                "verified" => results.push(VerificationResult::Verified { clause_desc: desc }),
                "timeout" => results.push(VerificationResult::Timeout { clause_desc: desc }),
                other => results.push(VerificationResult::Unknown {
                    clause_desc: desc,
                    reason: other.to_string(),
                }),
            }
            continue;
        }

        let solver = Solver::new();

        let mut encoder = Encoder::new();

        // Register known function names for trigger inference
        for other_clause in clauses {
            collect_function_names_for_triggers(&other_clause.body, &mut encoder.trigger_manager);
        }

        // Assert all requires as assumptions
        for req in &requires {
            let req_val = encoder.encode_expr(&req.body);
            let req_bool = req_val.as_bool();
            solver.assert(&req_bool);
        }
        // Assert background axioms from requires encoding (e.g., map
        // read-over-write, string length axioms)
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // T044: Inject lemma ensures as assumptions for any `apply` refs
        let apply_refs = collect_apply_refs(clauses);
        for lemma_name in &apply_refs {
            if let Some(ensures_bodies) = lemma_defs.get(lemma_name) {
                for ensures_body in ensures_bodies {
                    let ens_val = encoder.encode_expr(ensures_body);
                    let ens_bool = ens_val.as_bool();
                    solver.assert(&ens_bool);
                }
            }
        }

        // T045: For ensures clauses with a modifies set, inject frame
        // axioms: for every variable referenced in the ensures that is
        // NOT in the modifies set, assert `var == old(var)`.
        if clause.kind == ClauseKind::Ensures && frame_checker.has_modifies() {
            let frame_vars = frame_checker.frame_axiom_vars(&clause.body);
            for var_name in &frame_vars {
                // Create the current-state variable
                let current = encoder.get_or_create_int(var_name);
                // Create the old-state variable (uses __old suffix)
                let old_name = format!("{var_name}__old");
                let old_var = encoder.get_or_create_int(&old_name);
                // Assert frame axiom: current == old
                let axiom = current.eq(&old_var);
                solver.assert(&axiom);
            }
        }

        // Encode the clause body
        let clause_val = encoder.encode_expr(&clause.body);
        let clause_bool = clause_val.as_bool();

        // Assert background axioms (e.g., len >= 0) collected during encoding
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }

        let result_before = results.len();
        match clause.kind {
            ClauseKind::Ensures | ClauseKind::Rule => {
                // Validity check: assert NOT clause, check-sat
                solver.assert(clause_bool.not());
                check_validity(&solver, desc, results);
            }
            ClauseKind::Invariant => {
                // Satisfiability check: assert clause directly
                solver.assert(&clause_bool);
                check_satisfiability(&solver, desc, results);
            }
            ClauseKind::MustNot => {
                // Must-not: the bad thing should be impossible under requires
                solver.assert(&clause_bool);
                check_validity(&solver, desc, results);
            }
            ClauseKind::Decreases => {
                // Decreases: verify the expression is non-negative (well-founded).
                // Encode as: the clause expression (decreasing measure) >= 0 must hold.
                let zero = ast::Int::from_i64(0);
                let measure = clause_val.as_int(&mut encoder.fresh_counter);
                let non_neg = measure.ge(&zero);
                solver.assert(non_neg.not());
                check_validity(&solver, desc, results);
            }
            _ => {}
        }

        // T113: Cache the verification result
        if let Some(result) = results.get(result_before) {
            let result_str = match result {
                VerificationResult::Verified { .. } => "verified",
                VerificationResult::Timeout { .. } => "timeout",
                VerificationResult::Unknown { reason, .. } => reason.as_str(),
                VerificationResult::Counterexample { .. } => "counterexample",
            };
            cache.insert(clause_hash, result_str.to_string(), 0);
        }
    }
}

/// Verify a standalone invariant expression (e.g., service invariant).
fn verify_invariant_expr(parent_name: &str, expr: &Expr, results: &mut Vec<VerificationResult>) {
    let desc = format!("{parent_name}::invariant");
    let solver = Solver::new();
    let mut encoder = Encoder::new();
    let val = encoder.encode_expr(expr);
    let bool_val = val.as_bool();
    solver.assert(&bool_val);
    check_satisfiability(&solver, desc, results);
}

// -----------------------------------------------------------------------
// Entry point
// -----------------------------------------------------------------------

/// Collect all lemma definitions from the source AST.
///
/// Returns a map from lemma name to its ensures clause bodies.
fn collect_lemma_defs(typed: &TypedFile) -> std::collections::HashMap<String, Vec<&Expr>> {
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

/// Scan clause bodies for `apply lemma_name(args)` expressions and
/// collect the referenced lemma names.
fn collect_apply_refs(clauses: &[Clause]) -> Vec<String> {
    let mut refs = Vec::new();
    for clause in clauses {
        collect_apply_refs_expr(&clause.body, &mut refs);
    }
    refs
}

fn collect_apply_refs_expr(expr: &Expr, refs: &mut Vec<String>) {
    match expr {
        Expr::Apply { lemma_name, args } => {
            refs.push(lemma_name.clone());
            for arg in args {
                collect_apply_refs_expr(arg, refs);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_apply_refs_expr(lhs, refs);
            collect_apply_refs_expr(rhs, refs);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner)
        | Expr::Field(inner, _)
        | Expr::Cast { expr: inner, .. } => {
            collect_apply_refs_expr(inner, refs);
        }
        Expr::Call { func, args } => {
            collect_apply_refs_expr(func, refs);
            for a in args {
                collect_apply_refs_expr(a, refs);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_apply_refs_expr(receiver, refs);
            for a in args {
                collect_apply_refs_expr(a, refs);
            }
        }
        Expr::Index { expr: e, index } => {
            collect_apply_refs_expr(e, refs);
            collect_apply_refs_expr(index, refs);
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_apply_refs_expr(domain, refs);
            collect_apply_refs_expr(body, refs);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_apply_refs_expr(cond, refs);
            collect_apply_refs_expr(then_branch, refs);
            if let Some(eb) = else_branch {
                collect_apply_refs_expr(eb, refs);
            }
        }
        Expr::List(items) | Expr::Block(items) => {
            for item in items {
                collect_apply_refs_expr(item, refs);
            }
        }
        _ => {}
    }
}

/// Verify a quantified formula using Z3.
///
/// Encodes assumptions and the negated quantified body, then checks
/// satisfiability. UNSAT means the formula holds universally.
pub(crate) fn verify_quantified_impl(
    name: &str,
    assumptions: &[Expr],
    quantified_body: &Expr,
) -> VerificationResult {
    let solver = Solver::new();
    // Layer 2 timeout: 10 seconds
    let mut params = z3::Params::new();
    params.set_u32("timeout", 10000);
    solver.set_params(&params);

    let mut encoder = Encoder::new();

    // Assert assumptions
    for assumption in assumptions {
        let val = encoder.encode_expr(assumption);
        let bool_val = val.as_bool();
        solver.assert(&bool_val);
    }

    // Encode the quantified body
    let body_val = encoder.encode_expr(quantified_body);
    let body_bool = body_val.as_bool();

    // Negate and check: UNSAT means the formula holds
    solver.assert(body_bool.not());

    match solver.check() {
        SatResult::Unsat => VerificationResult::Verified {
            clause_desc: name.into(),
        },
        SatResult::Sat => {
            let (model_str, counter_model) = if let Some(m) = solver.get_model() {
                let cm = extract_counter_model(&m);
                (format!("{m}"), Some(cm))
            } else {
                ("(no model)".into(), None)
            };
            VerificationResult::Counterexample {
                clause_desc: name.into(),
                model: model_str,
                counter_model,
            }
        }
        SatResult::Unknown => {
            let reason = solver
                .get_reason_unknown()
                .unwrap_or_else(|| "unknown".into());
            if reason.contains("timeout") {
                VerificationResult::Timeout {
                    clause_desc: name.into(),
                }
            } else {
                VerificationResult::Unknown {
                    clause_desc: name.into(),
                    reason,
                }
            }
        }
    }
}

pub(crate) fn verify_contract_impl(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();
    let mut cache = SessionCache::new();
    let lemma_defs = std::collections::HashMap::new();
    verify_clauses(
        contract_name,
        clauses,
        &lemma_defs,
        &mut cache,
        &mut results,
    );
    results
}

pub(crate) fn verify_impl_with_timeout(
    typed: &TypedFile,
    timeout_ms: u64,
) -> Vec<VerificationResult> {
    let _ = timeout_ms; // timeout is set per-solver in verify_clauses
    let mut results = Vec::new();
    let mut cache = SessionCache::new();

    // T044: collect all lemma definitions for apply injection
    let lemma_defs = collect_lemma_defs(typed);

    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                verify_clauses(&c.name, &c.clauses, &lemma_defs, &mut cache, &mut results);
            }
            Decl::FnDef(f) => {
                verify_clauses(&f.name, &f.clauses, &lemma_defs, &mut cache, &mut results);
            }
            Decl::Extern(e) => {
                verify_clauses(&e.name, &e.clauses, &lemma_defs, &mut cache, &mut results);
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::Operation { name, clauses } => {
                            let qname = format!("{}.{}", s.name, name);
                            verify_clauses(&qname, clauses, &lemma_defs, &mut cache, &mut results);
                        }
                        ServiceItem::Query { name, clauses } => {
                            let qname = format!("{}.{}", s.name, name);
                            verify_clauses(&qname, clauses, &lemma_defs, &mut cache, &mut results);
                        }
                        ServiceItem::Invariant(expr) => {
                            verify_invariant_expr(&s.name, expr, &mut results);
                        }
                        _ => {}
                    }
                }
            }
            Decl::Block { name, body, .. } => {
                verify_clauses(name, body, &lemma_defs, &mut cache, &mut results);
            }
            Decl::Bind(b) => {
                verify_clauses(&b.name, &b.clauses, &lemma_defs, &mut cache, &mut results);
            }
            // Prophecy variables don't have verifiable clauses directly;
            // they are used as existential witnesses in contract proofs.
            Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }

    // Helper: parse a string into the SMT-local MemoryOrdering enum.
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

    // T092: weak memory ordering checks on concurrent contracts
    // Detects ordering from structured ClauseKind::Ordering clauses first,
    // then falls back to keyword scanning in ClauseKind::Effects bodies.
    let mut wm_checker = WeakMemoryChecker::new();
    for decl in &typed.resolved.source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.as_str(), &c.clauses),
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses),
            _ => continue,
        };
        // Prefer structured ClauseKind::Ordering over keyword scanning
        let mut found_ordering = false;
        for clause in clauses {
            if clause.kind == ClauseKind::Ordering {
                let ordering_str = match &clause.body {
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

    // T093: prophecy variable checks (unresolved prophecies)
    let mut pm = ProphecyManager::new();
    // Register top-level prophecy declarations
    for decl in &typed.resolved.source.decls {
        if let Decl::Prophecy(p) = &decl.node {
            pm.declare(p.name.clone());
        }
    }
    for decl in &typed.resolved.source.decls {
        let (clauses, ctx_name) = match &decl.node {
            Decl::FnDef(f) => (&f.clauses, f.name.as_str()),
            Decl::Contract(c) => (&c.clauses, c.name.as_str()),
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Ensures {
                collect_prophecy_refs(&clause.body, ctx_name, &mut pm);
            }
            // Resolve prophecy variables from resolve() calls
            if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
                resolve_prophecy_vars(&clause.body, ctx_name, &mut pm);
            }
            // Constrain prophecy variables from constraint expressions
            if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
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

    // T094: liveness obligation checks (G006)
    // Extract obligations from structured `liveness` blocks and from
    // contracts that use eventually/leads_to in ensures clauses.
    let mut lc = LivenessChecker::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::Liveness => {
                // Extract obligations from liveness block clauses
                for clause in body {
                    match &clause.kind {
                        ClauseKind::Other(k) if k == "assume" => {
                            // Check for fairness assumptions
                            let text = format!("{:?}", clause.body);
                            if text.contains("fair") {
                                lc.add_fairness(format!("{name}:fair"));
                            }
                        }
                        ClauseKind::Other(k) if k == "prove" => {
                            let text = format!("{:?}", clause.body);
                            let liveness_kind = if expr_references_var(&clause.body, "leads_to") {
                                LivenessKind::LeadsTo
                            } else if expr_references_var(&clause.body, "eventually_within") {
                                // Extract bound from the expression if present
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
                // Also scan contract ensures for legacy liveness patterns
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Ensures
                        && (expr_references_var(&clause.body, "eventually")
                            || expr_references_var(&clause.body, "leads_to"))
                    {
                        lc.add_obligation(
                            format!("{}:liveness", c.name),
                            LivenessKind::Eventually,
                            format!("{:?}", clause.body),
                            String::new(),
                        );
                    }
                }
            }
            _ => {}
        }
    }
    // Check fairness constraints for leads_to obligations
    for err in lc.check_fairness() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness:fairness".into(),
            reason: err,
        });
    }
    // Check bounded obligations have valid bounds
    for err in lc.check_bounded() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness:bounds".into(),
            reason: err,
        });
    }
    // BMC verification: attempt bounded model checking for each obligation
    for err in lc.check_unverified() {
        results.push(VerificationResult::Unknown {
            clause_desc: "liveness".into(),
            reason: err,
        });
    }

    // T076: Layer 2 verification (quantified invariants, termination, roundtrip)
    let l2_config = crate::layer2::Layer2Config::new().with_timeout(timeout_ms);
    let mut l2 = crate::layer2::Layer2Verifier::new(l2_config);

    for decl in &typed.resolved.source.decls {
        let (name, clauses): (&str, &[Clause]) = match &decl.node {
            Decl::Contract(c) => (&c.name, &c.clauses),
            Decl::FnDef(f) => (&f.name, &f.clauses),
            _ => continue,
        };
        // Extract invariant clauses as quantified invariants
        for clause in clauses {
            if clause.kind == ClauseKind::Invariant {
                match &clause.body {
                    Expr::Forall { var, domain, body } => {
                        let sort = format!("{domain:?}");
                        l2.add_invariant(crate::layer2::QuantifiedInvariant {
                            name: format!("{name}:invariant"),
                            bound_vars: vec![(var.clone(), sort)],
                            body: format!("{body:?}"),
                            triggers: Vec::new(),
                        });
                    }
                    Expr::Exists { var, domain, body } => {
                        let sort = format!("{domain:?}");
                        l2.add_invariant(crate::layer2::QuantifiedInvariant {
                            name: format!("{name}:invariant"),
                            bound_vars: vec![(var.clone(), sort)],
                            body: format!("{body:?}"),
                            triggers: Vec::new(),
                        });
                    }
                    _ => {}
                }
            }

            // Extract decreases clauses as termination obligations
            if clause.kind == ClauseKind::Decreases {
                l2.add_termination(crate::layer2::TerminationObligation {
                    fn_name: name.to_string(),
                    measure: format!("{:?}", clause.body),
                    recursive_calls: Vec::new(),
                });
            }
        }
    }

    if l2.obligation_count() > 0 {
        for l2r in l2.verify() {
            match l2r {
                crate::layer2::Layer2Result::Verified { invariant, .. } => {
                    results.push(VerificationResult::Verified {
                        clause_desc: format!("layer2:{invariant}"),
                    });
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

    // T073: CodecDispatcher ambiguity checking
    let mut codec_disp = crate::advanced::CodecDispatcher::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::CodecRegistry(cr) = &decl.node {
            for entry in &cr.codecs {
                if let assura_parser::ast::MagicPattern::Bytes { bytes, .. } = &entry.magic {
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

/// Collect function names from an expression tree and register them
/// with the trigger manager for quantifier e-matching.
fn collect_function_names_for_triggers(expr: &Expr, tm: &mut crate::advanced::TriggerManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                tm.register_function(name.clone());
            }
            for a in args {
                collect_function_names_for_triggers(a, tm);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            tm.register_function(method.clone());
            collect_function_names_for_triggers(receiver, tm);
            for a in args {
                collect_function_names_for_triggers(a, tm);
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_function_names_for_triggers(lhs, tm);
            collect_function_names_for_triggers(rhs, tm);
        }
        Expr::UnaryOp { expr: e, .. } | Expr::Paren(e) | Expr::Old(e) | Expr::Ghost(e) => {
            collect_function_names_for_triggers(e, tm);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_function_names_for_triggers(cond, tm);
            collect_function_names_for_triggers(then_branch, tm);
            if let Some(eb) = else_branch {
                collect_function_names_for_triggers(eb, tm);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            collect_function_names_for_triggers(domain, tm);
            collect_function_names_for_triggers(body, tm);
        }
        Expr::Index { expr: e, index } => {
            collect_function_names_for_triggers(e, tm);
            collect_function_names_for_triggers(index, tm);
        }
        _ => {}
    }
}

/// Extract a numeric argument from an expression tree (for eventually_within bounds).
fn extract_numeric_arg(expr: &Expr) -> Option<u64> {
    match expr {
        Expr::Literal(assura_parser::ast::Literal::Int(s)) => s.parse().ok(),
        Expr::Call { args, .. } => args.iter().find_map(extract_numeric_arg),
        Expr::Raw(tokens) => tokens.iter().find_map(|t| t.parse::<u64>().ok()),
        Expr::Block(exprs) => exprs.iter().find_map(extract_numeric_arg),
        _ => None,
    }
}

/// Scan an expression for prophecy resolution calls: resolve(var, value).
fn resolve_prophecy_vars(expr: &Expr, fn_name: &str, pm: &mut ProphecyManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref()
                && (name == "resolve" || name == "resolve_prophecy")
                && let Some(Expr::Ident(var_name)) = args.first()
            {
                let value = args.get(1).map(|a| format!("{a:?}")).unwrap_or_default();
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
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) | Expr::Ghost(expr) => {
            resolve_prophecy_vars(expr, fn_name, pm)
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
fn constrain_prophecy_vars(expr: &Expr, fn_name: &str, pm: &mut ProphecyManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref()
                && (name == "constrain" || name == "constrain_prophecy")
                && let Some(Expr::Ident(var_name)) = args.first()
            {
                let constraint = args.get(1).map(|a| format!("{a:?}")).unwrap_or_default();
                pm.add_constraint(&format!("{fn_name}:{var_name}"), constraint);
            }
            for arg in args {
                constrain_prophecy_vars(arg, fn_name, pm);
            }
        }
        Expr::BinOp { lhs, rhs, op } => {
            // An equality like `prophecy(x) == expr` constrains x
            if *op == BinOp::Eq
                && let Expr::Call { func, args } = lhs.as_ref()
                && let Expr::Ident(name) = func.as_ref()
                && (name == "prophecy" || name == "prophesy")
                && let Some(Expr::Ident(var_name)) = args.first()
            {
                pm.add_constraint(&format!("{fn_name}:{var_name}"), format!("{rhs:?}"));
            }
            constrain_prophecy_vars(lhs, fn_name, pm);
            constrain_prophecy_vars(rhs, fn_name, pm);
        }
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) | Expr::Ghost(expr) => {
            constrain_prophecy_vars(expr, fn_name, pm)
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
fn collect_prophecy_refs(expr: &Expr, fn_name: &str, pm: &mut ProphecyManager) {
    match expr {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref()
                && (name == "prophecy" || name == "prophesy")
                && let Some(Expr::Ident(var_name)) = args.first()
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
        Expr::UnaryOp { expr, .. } | Expr::Paren(expr) | Expr::Old(expr) | Expr::Ghost(expr) => {
            collect_prophecy_refs(expr, fn_name, pm)
        }
        _ => {}
    }
}
