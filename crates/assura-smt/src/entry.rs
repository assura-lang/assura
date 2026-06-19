//! Public entry point functions for SMT verification.
//!
//! Contains `verify()`, `verify_with_options()`, `verify_parallel()`,
//! and all standalone verification functions (refinement, buffer bounds,
//! taint safety, measures, termination).

use assura_parser::ast::{BinOp, BlockKind, Clause, ClauseKind, Decl, Expr, Param, ServiceItem};
use assura_types::TypedFile;
use assura_types::checkers::expr_references_var;

use crate::SolverChoice;
use crate::advanced::{
    CodecDispatcher, LivenessChecker, LivenessKind, MemoryOrdering, ProphecyManager,
    WeakMemoryChecker,
};
use crate::cache::{SessionCache, VerificationCache};
use crate::measures::MeasureDefinition;
use crate::result::VerificationResult;

/// Extract the return type from `output(result: Nat)` clauses in a contract.
///
/// Contracts declare their output type via `output(result: Nat)` instead of
/// a function return type. The clause body is `Expr::Raw(["result", ":", "Nat"])`.
pub(crate) fn extract_output_return_type(clauses: &[Clause]) -> Vec<String> {
    for clause in clauses {
        if clause.kind == ClauseKind::Output
            && let Expr::Raw(tokens) = &clause.body
        {
            if tokens.len() >= 3 && tokens[1] == ":" {
                return tokens[2..].to_vec();
            }
            return tokens.clone();
        }
    }
    Vec::new()
}

/// Extract parameters from `input(raw_data: Bytes)` clauses in a contract.
pub(crate) fn extract_input_params(clauses: &[Clause]) -> Vec<Param> {
    for clause in clauses {
        if clause.kind == ClauseKind::Input
            && let Expr::Raw(tokens) = &clause.body
        {
            let mut params = Vec::new();
            let mut i = 0;
            while i < tokens.len() {
                if tokens[i] == "," {
                    i += 1;
                    continue;
                }
                let name = tokens[i].clone();
                i += 1;
                if i < tokens.len() && tokens[i] == ":" {
                    i += 1;
                    let mut ty = Vec::new();
                    while i < tokens.len() && tokens[i] != "," {
                        ty.push(tokens[i].clone());
                        i += 1;
                    }
                    params.push(Param {
                        name,
                        ty,
                        parsed_type: None,
                    });
                } else {
                    params.push(Param {
                        name,
                        ty: Vec::new(),
                        parsed_type: None,
                    });
                }
            }
            return params;
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Optional per-file inputs discovered outside the typed AST (e.g. IR sidecars).
#[derive(Debug, Default, Clone, Copy)]
pub struct VerifyFileExtras<'a> {
    pub ir_bodies: Option<&'a std::collections::HashMap<String, crate::ir::IrFunction>>,
    /// Block bodies (`fn #N`) from multi-function IR modules, keyed by contract name.
    pub ir_blocks: Option<
        &'a std::collections::HashMap<
            String,
            std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>,
        >,
    >,
}

/// Verify all contract clauses in a type-checked file.
///
/// Returns a `VerificationResult` for each verifiable clause (ensures,
/// invariant). Requires clauses are collected as assumptions but not
/// independently verified (they constrain the context for ensures).
pub fn verify(typed: &TypedFile) -> Vec<VerificationResult> {
    verify_from_source(typed, None)
}

/// Verify a type-checked file, auto-loading `{Name}.ir` sidecars when `source` is set.
pub fn verify_from_source(
    typed: &TypedFile,
    source: Option<&std::path::Path>,
) -> Vec<VerificationResult> {
    let loaded = source.map(|path| crate::ir_loader::LoadedVerifyExtras::load(path, typed));
    let extras = loaded.as_ref().and_then(|l| l.extras());
    verify_with_options(
        typed,
        &assura_config::VerifyOptions::default(),
        extras.as_ref(),
    )
}

/// Verify all contract clauses using the given verification options.
///
/// `options.solver` selects the SMT backend ("z3", "cvc5", "portfolio").
/// `options.timeout_ms` limits per-query solver time.
/// `options.layer` controls verification depth (0 = structural, 1+ = SMT).
pub fn verify_with_options(
    typed: &TypedFile,
    options: &assura_config::VerifyOptions,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    match options.solver {
        SolverChoice::Cvc5 => verify_file_with_cvc5(typed, extras),
        SolverChoice::Portfolio => {
            // Run Z3 and CVC5 concurrently, take the best result (#245)
            #[cfg(feature = "z3-verify")]
            {
                verify_portfolio_parallel(typed, options.timeout_ms, extras)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                verify_file_with_cvc5(typed, extras)
            }
        }
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_impl_with_timeout(typed, options.timeout_ms, extras)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = extras;
                crate::no_z3::verify_stub(typed)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Shared job collection (#213): eliminates duplicated Decl dispatch in
// verify_file_with_cvc5, verify_parallel_with_solver, and z3_backend.
// ---------------------------------------------------------------------------

/// A verification job: contract name, clauses, parameters, and return type.
pub(crate) type VerificationJob = (String, Vec<Clause>, Vec<Param>, Vec<String>);

/// Collect verification jobs from all declarations in a source file.
///
/// Each job is a (name, clauses, params, return_ty) tuple suitable for
/// passing to either the Z3 or CVC5 backend.
pub(crate) fn collect_verification_jobs(typed: &TypedFile) -> Vec<VerificationJob> {
    let mut jobs = Vec::new();
    for decl in &typed.resolved.source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                let output_ty = extract_output_return_type(&c.clauses);
                let mut input_params = extract_input_params(&c.clauses);
                input_params.extend_from_slice(&c.fn_params);
                jobs.push((c.name.clone(), c.clauses.clone(), input_params, output_ty));
            }
            Decl::FnDef(f) => {
                jobs.push((
                    f.name.clone(),
                    f.clauses.clone(),
                    f.params.clone(),
                    f.return_ty.clone(),
                ));
            }
            Decl::Extern(e) => {
                jobs.push((
                    e.name.clone(),
                    e.clauses.clone(),
                    e.params.clone(),
                    e.return_ty.clone(),
                ));
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::Operation { name, clauses } => {
                            jobs.push((
                                format!("{}.{}", s.name, name),
                                clauses.clone(),
                                vec![],
                                vec![],
                            ));
                        }
                        ServiceItem::Query { name, clauses } => {
                            jobs.push((
                                format!("{}.{}", s.name, name),
                                clauses.clone(),
                                vec![],
                                vec![],
                            ));
                        }
                        ServiceItem::Invariant(expr) => {
                            let inv_clause = Clause {
                                kind: ClauseKind::Invariant,
                                body: expr.clone(),
                                effect_variables: vec![],
                            };
                            jobs.push((
                                format!("{}::invariant", s.name),
                                vec![inv_clause],
                                vec![],
                                vec![],
                            ));
                        }
                        _ => {}
                    }
                }
            }
            Decl::Block { name, body, .. } => {
                jobs.push((name.clone(), body.clone(), vec![], vec![]));
            }
            Decl::Bind(b) => {
                jobs.push((
                    b.name.clone(),
                    b.clauses.clone(),
                    b.params.clone(),
                    b.return_ty.clone(),
                ));
            }
            Decl::Prophecy(_) | Decl::CodecRegistry(_) | Decl::TypeDef(_) | Decl::EnumDef(_) => {}
        }
    }
    jobs
}

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
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.as_str(), &c.clauses[..]),
            Decl::FnDef(f) => (f.name.as_str(), &f.clauses[..]),
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
        let (clauses, ctx_name) = match &decl.node {
            Decl::FnDef(f) => (&f.clauses[..], f.name.as_str()),
            Decl::Contract(c) => (&c.clauses[..], c.name.as_str()),
            _ => continue,
        };
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
                            format!("{:?}", clause.body),
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
        let (name, clauses): (&str, &[Clause]) = match &decl.node {
            Decl::Contract(c) => (&c.name, &c.clauses),
            Decl::FnDef(f) => (&f.name, &f.clauses),
            _ => continue,
        };
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
fn verify_file_with_cvc5(
    typed: &TypedFile,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Collect lemma definitions so `apply lemma_name(args)` can inject
    // postconditions as solver assumptions (matching Z3 backend behavior).
    let lemma_defs = crate::cvc5_backend::collect_lemma_defs_for_cvc5(typed);

    // #257: collect feature_max constants so the CVC5 encoder binds them
    // to concrete values instead of creating free solver variables.
    let constants = crate::cvc5_backend::collect_feature_max_constants_cvc5(typed);

    // #253: per-file session cache for CVC5 clause deduplication
    let mut session_cache = SessionCache::new();

    let ir_bodies = extras.and_then(|e| e.ir_bodies);

    // Clause-level verification via CVC5
    for (name, clauses, params, return_ty) in collect_verification_jobs(typed) {
        let ir_body = ir_bodies.and_then(|m| m.get(&name));
        results.extend(crate::cvc5_backend::verify_contract_cvc5_with_lemmas(
            &name,
            &clauses,
            &params,
            &return_ty,
            Some(&lemma_defs),
            &constants,
            ir_body,
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
fn verify_portfolio_parallel(
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

/// Merge Z3 and CVC5 results per-clause.
///
/// For each position, prefer the definitive result (Verified or Counterexample,
/// favoring Z3 for richer counter-models). Fall back to the less-bad
/// inconclusive result.
#[cfg(feature = "z3-verify")]
fn merge_portfolio_results(
    z3: Vec<VerificationResult>,
    cvc5: Vec<VerificationResult>,
) -> Vec<VerificationResult> {
    let mut merged = Vec::with_capacity(z3.len().max(cvc5.len()));
    let mut cvc5_iter = cvc5.into_iter();
    for z3r in z3 {
        if let Some(cvc5r) = cvc5_iter.next() {
            merged.push(pick_better_result(z3r, cvc5r));
        } else {
            merged.push(z3r);
        }
    }
    // Any extra CVC5 results (CVC5 found more clauses)
    merged.extend(cvc5_iter);
    merged
}

/// Pick the better of two results for the same clause.
///
/// Priority: Verified > Counterexample > Unknown > Timeout.
/// Between equal priorities, prefer Z3 (richer counter-models).
#[cfg(feature = "z3-verify")]
fn pick_better_result(z3r: VerificationResult, cvc5r: VerificationResult) -> VerificationResult {
    fn priority(r: &VerificationResult) -> u8 {
        match r {
            VerificationResult::Verified { .. } => 3,
            VerificationResult::Counterexample { .. } => 2,
            VerificationResult::Unknown { .. } => 1,
            VerificationResult::Timeout { .. } => 0,
        }
    }
    let z3_pri = priority(&z3r);
    let cvc5_pri = priority(&cvc5r);
    if z3_pri >= cvc5_pri { z3r } else { cvc5r }
}

/// Verify all declarations in parallel using rayon.
///
/// Each contract/function gets its own Z3 context (Z3 contexts are not
/// `Sync`). Independent declarations are verified concurrently using
/// rayon's work-stealing thread pool, achieving linear speedup on
/// multi-core machines for projects with many contracts.
///
/// Also uses the filesystem cache: cache hits are returned immediately,
/// only cache misses go to Z3 (potentially in parallel).
pub fn verify_parallel(typed: &TypedFile, cache: &VerificationCache) -> Vec<VerificationResult> {
    verify_parallel_from_source(typed, cache, None)
}

/// Parallel verification with automatic IR sidecar loading from `source`.
pub fn verify_parallel_from_source(
    typed: &TypedFile,
    cache: &VerificationCache,
    source: Option<&std::path::Path>,
) -> Vec<VerificationResult> {
    let loaded = source.map(|path| crate::ir_loader::LoadedVerifyExtras::load(path, typed));
    let extras = loaded.as_ref().and_then(|l| l.extras());
    verify_parallel_with_solver(typed, cache, SolverChoice::Z3, extras.as_ref())
}

/// Check whether any declaration in the source file has verifiable clauses
/// (requires, ensures, invariant).  Returns false if there is nothing to
/// send to the solver, allowing callers to skip thread-pool and cache init.
pub fn has_verifiable_clauses(source: &assura_parser::ast::SourceFile) -> bool {
    use assura_parser::ast::{ClauseKind, Decl};

    let verifiable = |clauses: &[assura_parser::ast::Clause]| {
        clauses.iter().any(|c| {
            matches!(
                c.kind,
                ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
            )
        })
    };

    source.decls.iter().any(|d| match &d.node {
        Decl::Contract(c) => verifiable(&c.clauses),
        Decl::FnDef(f) => verifiable(&f.clauses),
        Decl::Extern(e) => verifiable(&e.clauses),
        Decl::Service(s) => s.items.iter().any(|item| match item {
            assura_parser::ast::ServiceItem::Operation { clauses, .. }
            | assura_parser::ast::ServiceItem::Query { clauses, .. } => verifiable(clauses),
            assura_parser::ast::ServiceItem::Invariant(_) => true,
            _ => false,
        }),
        Decl::Block { body, .. } => verifiable(body),
        Decl::Bind(b) => verifiable(&b.clauses),
        _ => false,
    })
}

/// Verify all declarations in parallel using the specified solver.
pub fn verify_parallel_with_solver(
    typed: &TypedFile,
    cache: &VerificationCache,
    solver: SolverChoice,
    extras: Option<&VerifyFileExtras<'_>>,
) -> Vec<VerificationResult> {
    use rayon::prelude::*;

    // #180: collect feature_max constants so the encoder binds them
    // to concrete values instead of creating free Z3 variables.
    #[cfg(feature = "z3-verify")]
    let constants = crate::z3_backend::collect_feature_max_constants(typed);
    #[cfg(not(feature = "z3-verify"))]
    let constants: Vec<(String, i64)> = Vec::new();

    // Collect verification jobs (#213: shared with CVC5 and Z3 paths)
    let jobs = collect_verification_jobs(typed);

    let ir_bodies = extras.and_then(|e| e.ir_bodies);
    let ir_block_maps = extras.and_then(|e| e.ir_blocks);

    // Verify in parallel: each job gets its own solver context
    let per_job_results: Vec<Vec<VerificationResult>> = jobs
        .par_iter()
        .map(|(name, clauses, params, return_ty)| {
            // Check cache first
            if let Some(cached) = cache.get(name, clauses) {
                return cached;
            }
            let ir_body = ir_bodies.and_then(|m| m.get(name));
            let ir_blocks = ir_block_maps.and_then(|m| m.get(name));
            // Cache miss: run solver with type constraints
            let results = verify_contract_with_types_and_solver(
                name, clauses, params, return_ty, &constants, solver, ir_body, ir_blocks,
            );
            cache.put(name, clauses, &results);
            results
        })
        .collect();

    // Flatten into a single results vec
    per_job_results.into_iter().flatten().collect()
}

/// Verify a single contract's clauses using the default solver (Z3).
///
/// Unlike `verify()` which processes all declarations in a `TypedFile`,
/// this function verifies just the given contract's clauses. Each
/// ensures/invariant clause gets its own solver query with all requires
/// clauses asserted as assumptions.
///
/// Returns one `VerificationResult` per verifiable clause.
pub fn verify_contract(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
) -> Vec<VerificationResult> {
    verify_contract_with_solver(contract_name, clauses, SolverChoice::Z3)
}

/// Verify a single contract's clauses using the specified solver.
pub fn verify_contract_with_solver(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
    solver: SolverChoice,
) -> Vec<VerificationResult> {
    match solver {
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_contract_impl(contract_name, clauses)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = contract_name;
                clauses
                    .iter()
                    .filter(|c| {
                        matches!(
                            c.kind,
                            assura_parser::ast::ClauseKind::Ensures
                                | assura_parser::ast::ClauseKind::Invariant
                                | assura_parser::ast::ClauseKind::Rule
                                | assura_parser::ast::ClauseKind::MustNot
                                | assura_parser::ast::ClauseKind::Decreases
                        )
                    })
                    .map(|c| {
                        let desc = format!("{contract_name}::{:?}", c.kind);
                        VerificationResult::Unknown {
                            clause_desc: desc,
                            reason: "Z3 not available (compiled without z3-verify feature)".into(),
                        }
                    })
                    .collect()
            }
        }
        SolverChoice::Cvc5 => crate::cvc5_backend::verify_contract_cvc5(contract_name, clauses),
        SolverChoice::Portfolio => {
            // Run Z3 and CVC5 concurrently per-contract (#245)
            let z3_results = verify_contract_with_solver(contract_name, clauses, SolverChoice::Z3);
            let cvc5_results = crate::cvc5_backend::verify_contract_cvc5(contract_name, clauses);
            #[cfg(feature = "z3-verify")]
            {
                merge_portfolio_results(z3_results, cvc5_results)
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = z3_results;
                cvc5_results
            }
        }
    }
}

/// Verify a contract with type-level constraints from params and return type.
#[expect(clippy::too_many_arguments, reason = "per-job solver dispatch mirrors Z3 backend")]
fn verify_contract_with_types_and_solver(
    contract_name: &str,
    clauses: &[assura_parser::ast::Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    solver: SolverChoice,
    ir_body: Option<&crate::ir::IrFunction>,
    ir_blocks: Option<&std::collections::HashMap<usize, Vec<crate::ir::IrInstr>>>,
) -> Vec<VerificationResult> {
    let _ = ir_blocks;
    match solver {
        SolverChoice::Z3 => {
            #[cfg(feature = "z3-verify")]
            {
                crate::z3_backend::verify_contract_impl_with_types_and_ir(
                    contract_name,
                    clauses,
                    params,
                    return_ty,
                    constants,
                    ir_body,
                    ir_blocks,
                )
            }
            #[cfg(not(feature = "z3-verify"))]
            {
                let _ = (constants, ir_body);
                verify_contract_with_solver(contract_name, clauses, solver)
            }
        }
        SolverChoice::Cvc5 | SolverChoice::Portfolio => {
            let mut cache = SessionCache::new();
            crate::cvc5_backend::verify_contract_cvc5_with_lemmas(
                contract_name,
                clauses,
                params,
                return_ty,
                None,
                constants,
                ir_body,
                &mut cache,
            )
        }
    }
}

/// Check whether a refinement subtype relation holds:
///
/// `{v: T | antecedent} <: {v: T | consequent}`
///
/// Encodes: `(assert antecedent) (assert (not consequent)) (check-sat)`
///
/// UNSAT => subtyping holds (Verified).
/// SAT  => counterexample exists.
pub fn check_refinement_subtype(antecedent: &Expr, consequent: &Expr) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::check_refinement_subtype_impl(antecedent, consequent)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::check_refinement_subtype_cvc5(antecedent, consequent)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        crate::no_z3::refinement_stub(antecedent, consequent)
    }
}

/// Verify buffer bounds safety for a contract.
///
/// Given a set of requires (assumptions) and an ensures clause that
/// references buffer access, checks whether the requires clauses are
/// sufficient to prove bounds safety. Specifically:
///
/// - Buffer capacity is modeled as an uninterpreted non-negative integer
/// - Offset and length constraints from requires are asserted
/// - The ensures clause is checked for validity under those assumptions
///
/// This is the SMT encoding for MEM.1 memory region contracts.
pub fn verify_buffer_bounds(requires: &[Expr], ensures: &Expr) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_buffer_bounds_impl(requires, ensures)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_buffer_bounds_cvc5(requires, ensures)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        let _ = (requires, ensures);
        VerificationResult::Unknown {
            clause_desc: "buffer_bounds".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Verify region containment: that all indices in sub_region are within parent_region.
///
/// SMT encoding: `forall i: sub_lo <= i < sub_hi => parent_lo <= i < parent_hi`
///
/// The `context` expressions provide additional assumptions (e.g., bounds on
/// the buffer capacity). Returns Verified if the containment holds for all
/// possible values satisfying the context, or Counterexample otherwise.
pub fn verify_region_containment(
    context: &[Expr],
    sub_lo: &Expr,
    sub_hi: &Expr,
    parent_lo: &Expr,
    parent_hi: &Expr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_region_containment_impl(
            context, sub_lo, sub_hi, parent_lo, parent_hi,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_region_containment_cvc5(
            context, sub_lo, sub_hi, parent_lo, parent_hi,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        let _ = (context, sub_lo, sub_hi, parent_lo, parent_hi);
        VerificationResult::Unknown {
            clause_desc: "region_containment".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Check refinement subtyping with extra context assumptions.
///
/// The `context` expressions are asserted alongside the antecedent before
/// negating the consequent. Useful when the subtyping depends on
/// constraints from enclosing scopes (e.g., function parameters).
pub fn check_refinement_subtype_with_context(
    context: &[Expr],
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::check_refinement_subtype_with_context_impl(
            context, antecedent, consequent,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::check_refinement_subtype_with_context_cvc5(
            context, antecedent, consequent,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        crate::no_z3::refinement_ctx_stub(context, antecedent, consequent)
    }
}

/// Verify taint safety for a contract: prove that tainted data cannot flow
/// to sensitive positions without validation.
///
/// The SMT encoding models taint labels as integers in the lattice:
/// `Untrusted(0) < Validated(1) < Trusted(2)`.
///
/// For each variable with a taint label, a Z3 integer represents its taint
/// level. Flow constraints assert that taint propagates through operations
/// (union semantics: result taint = min of operand taints), and sensitive
/// positions require a minimum taint level (Validated or Trusted).
///
/// Returns `Verified` if the taint constraints are satisfiable with no
/// violations, or `Counterexample` with the violating variable assignment.
pub fn verify_taint_safety(
    taint_labels: &[(String, assura_types::TaintLabel)],
    validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_taint_safety_impl(taint_labels, validation_fns, sensitive_uses)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_taint_safety_cvc5(taint_labels, validation_fns, sensitive_uses)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        let _ = (taint_labels, validation_fns, sensitive_uses);
        VerificationResult::Unknown {
            clause_desc: "taint_safety".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Verify a contract using measure-enriched SMT context.
///
/// Each measure in `measures` is encoded as an uninterpreted function in Z3,
/// with its standard axioms asserted. The `requires` expressions are asserted
/// as assumptions, and the `ensures` expression is checked for validity under
/// those assumptions plus the measure axioms.
///
/// This is the primary entry point for measure-aware verification.
pub fn verify_with_measures(
    requires: &[Expr],
    ensures: &Expr,
    measures: &[MeasureDefinition],
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_with_measures_impl(requires, ensures, measures)
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_with_measures_cvc5(requires, ensures, measures)
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        let _ = (requires, ensures, measures);
        VerificationResult::Unknown {
            clause_desc: "verify_with_measures".into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Termination (decreases) verification
// ---------------------------------------------------------------------------

/// Verify that a decreases measure strictly decreases at a recursive call site.
///
/// Given:
/// - `preconditions`: the function's requires clauses (assumed true)
/// - `measure_expr`: the decreases expression in terms of function params
/// - `call_arg_expr`: the argument at the call site corresponding to the measure
/// - `clause_desc`: description for the verification result
///
/// Checks: `preconditions => measure(call_args) < measure(fn_args) && measure(call_args) >= 0`
///
/// UNSAT on the negation => verified (measure decreases).
/// SAT => counterexample (measure does not decrease).
pub fn verify_decrease(
    preconditions: &[Expr],
    measure_expr: &Expr,
    call_arg_expr: &Expr,
    clause_desc: String,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        crate::z3_backend::verify_decrease_impl(
            preconditions,
            measure_expr,
            call_arg_expr,
            clause_desc,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), feature = "cvc5-verify"))]
    {
        crate::cvc5_backend::verify_decrease_cvc5(
            preconditions,
            measure_expr,
            call_arg_expr,
            clause_desc,
        )
    }
    #[cfg(all(not(feature = "z3-verify"), not(feature = "cvc5-verify")))]
    {
        let _ = (preconditions, measure_expr, call_arg_expr);
        VerificationResult::Unknown {
            clause_desc,
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Incremental contract evolution (#199)
// ---------------------------------------------------------------------------

/// Result of a contract evolution check.
#[derive(Debug, Clone)]
pub struct EvolutionResult {
    /// Name of the contract being checked.
    pub contract_name: String,
    /// Precondition weakening check: every input valid under the old contract
    /// must be valid under the new contract.
    pub precondition_weakening: VerificationResult,
    /// Postcondition strengthening check: every guarantee of the new contract
    /// must imply the old guarantee.
    pub postcondition_strengthening: VerificationResult,
}

/// Verify that a contract evolution is backward-compatible.
///
/// Given an old and new version of a contract's clauses, checks:
/// 1. **Precondition weakening**: `old_requires => new_requires`
///    (the new contract accepts at least everything the old one did)
/// 2. **Postcondition strengthening**: `new_ensures => old_ensures`
///    (the new contract's guarantees are at least as strong)
///
/// Both are standard Z3 validity checks.
pub fn verify_evolution(
    contract_name: &str,
    old_clauses: &[Clause],
    new_clauses: &[Clause],
) -> EvolutionResult {
    // Collect requires and ensures from both versions
    let old_requires: Vec<&Expr> = old_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let new_requires: Vec<&Expr> = new_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let old_ensures: Vec<&Expr> = old_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    let new_ensures: Vec<&Expr> = new_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();

    // ---- Precondition weakening: old_requires => new_requires ----
    // All old preconditions must imply all new preconditions.
    // If old has no requires, it accepts everything, so new must also accept
    // everything (new_requires must be trivially true).
    // If new has no requires, it accepts everything, so weakening holds trivially.
    let precondition_weakening = if new_requires.is_empty() {
        // New accepts everything; weakening holds trivially
        VerificationResult::verified(format!("{contract_name}: precondition weakening"))
    } else {
        check_implication(
            &old_requires,
            &new_requires,
            &format!("{contract_name}: precondition weakening"),
        )
    };

    // ---- Postcondition strengthening: new_ensures => old_ensures ----
    // All new postconditions must imply all old postconditions.
    // If old has no ensures, there are no guarantees to maintain, so
    // strengthening holds trivially.
    // If new has no ensures but old does, strengthening fails (lost guarantees).
    let postcondition_strengthening = if old_ensures.is_empty() {
        // Old had no guarantees; any new guarantees are fine
        VerificationResult::verified(format!("{contract_name}: postcondition strengthening"))
    } else if new_ensures.is_empty() {
        // Old had guarantees, new dropped them
        VerificationResult::Counterexample {
            clause_desc: format!("{contract_name}: postcondition strengthening"),
            model: "new contract drops all ensures clauses from old contract".into(),
            counter_model: None,
        }
    } else {
        check_implication(
            &new_ensures,
            &old_ensures,
            &format!("{contract_name}: postcondition strengthening"),
        )
    };

    EvolutionResult {
        contract_name: contract_name.to_string(),
        precondition_weakening,
        postcondition_strengthening,
    }
}

/// Check that all antecedents together imply all consequents together.
///
/// Encodes: `(and antecedents) => (and consequents)` via
/// `(assert antecedents) (assert (not (and consequents))) (check-sat)`
/// UNSAT = implication holds.
fn check_implication(
    antecedents: &[&Expr],
    consequents: &[&Expr],
    desc: &str,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        use crate::z3_backend::encoder::{Encoder, expr_has_unmodelable_features};
        use crate::z3_backend::solver::check_validity;
        use z3::Solver;

        // Check if any expressions have unmodelable features
        let all_exprs: Vec<&&Expr> = antecedents.iter().chain(consequents.iter()).collect();
        for expr in &all_exprs {
            if expr_has_unmodelable_features(expr) {
                return VerificationResult::Unknown {
                    clause_desc: desc.to_string(),
                    reason: "clause uses features not yet encoded in SMT".into(),
                };
            }
        }

        let solver = Solver::new();
        let mut params = z3::Params::new();
        params.set_u32("timeout", 2000);
        solver.set_params(&params);
        let mut encoder = Encoder::new();

        // Assert all antecedents
        for expr in antecedents {
            let val = encoder.encode_expr(expr);
            solver.assert(val.as_bool());
        }
        for axiom in &encoder.background_axioms {
            solver.assert(axiom);
        }
        encoder.background_axioms.clear();

        // Negate conjunction of consequents
        // If there is only one consequent, negate it directly.
        // If multiple, negate their conjunction (not(c1 && c2 && ...)).
        if consequents.len() == 1 {
            let val = encoder.encode_expr(consequents[0]);
            let bool_val = val.as_bool();
            for axiom in &encoder.background_axioms {
                solver.assert(axiom);
            }
            solver.assert(bool_val.not());
        } else {
            // Build conjunction of all consequents, then negate
            let mut conjunction_parts = Vec::new();
            for expr in consequents {
                let val = encoder.encode_expr(expr);
                conjunction_parts.push(val.as_bool());
            }
            for axiom in &encoder.background_axioms {
                solver.assert(axiom);
            }
            let refs: Vec<&z3::ast::Bool> = conjunction_parts.iter().collect();
            let conjunction = z3::ast::Bool::and(&refs);
            solver.assert(conjunction.not());
        }

        let mut results = Vec::new();
        check_validity(&solver, desc.to_string(), &mut results);
        results
            .into_iter()
            .next()
            .unwrap_or(VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "no result from solver".into(),
            })
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (antecedents, consequents);
        VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}

/// Verify evolution of all matching contracts between two parsed files.
///
/// Matches contracts by name between old and new files. For each pair,
/// runs the precondition weakening and postcondition strengthening checks.
/// Returns results for all matched contracts plus warnings for removed contracts.
pub fn verify_file_evolution(
    old_source: &assura_parser::ast::SourceFile,
    new_source: &assura_parser::ast::SourceFile,
) -> Vec<EvolutionResult> {
    use assura_parser::ast::Decl;

    fn collect_contracts(source: &assura_parser::ast::SourceFile) -> Vec<(String, Vec<Clause>)> {
        source
            .decls
            .iter()
            .filter_map(|d| match &d.node {
                Decl::Contract(c) => Some((c.name.clone(), c.clauses.clone())),
                Decl::FnDef(f) => Some((f.name.clone(), f.clauses.clone())),
                Decl::Extern(e) => Some((e.name.clone(), e.clauses.clone())),
                Decl::Bind(b) => Some((b.name.clone(), b.clauses.clone())),
                _ => None,
            })
            .collect()
    }

    let old_contracts = collect_contracts(old_source);
    let new_contracts = collect_contracts(new_source);

    let new_map: std::collections::HashMap<&str, &[Clause]> = new_contracts
        .iter()
        .map(|(name, clauses)| (name.as_str(), clauses.as_slice()))
        .collect();

    let mut results = Vec::new();

    for (name, old_clauses) in &old_contracts {
        if let Some(new_clauses) = new_map.get(name.as_str()) {
            results.push(verify_evolution(name, old_clauses, new_clauses));
        }
        // Contracts removed in new version: no evolution check needed
        // (handled by the structural diff in the CLI)
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::*;

    fn make_clause(kind: ClauseKind) -> Clause {
        Clause {
            kind,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }
    }

    fn make_source(decls: Vec<Decl>) -> SourceFile {
        SourceFile {
            project: None,
            module: None,
            imports: vec![],
            decls: decls
                .into_iter()
                .map(|d| Spanned {
                    node: d,
                    span: 0..1,
                })
                .collect(),
        }
    }

    // ---- has_verifiable_clauses tests ----

    #[test]
    fn has_verifiable_empty_source() {
        let source = make_source(vec![]);
        assert!(!has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_contract_with_ensures() {
        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: "C".into(),
            type_params: vec![],
            clauses: vec![make_clause(ClauseKind::Ensures)],
            fn_params: vec![],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_contract_with_only_input() {
        let source = make_source(vec![Decl::Contract(ContractDecl {
            name: "C".into(),
            type_params: vec![],
            clauses: vec![make_clause(ClauseKind::Input)],
            fn_params: vec![],
        })]);
        assert!(!has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_fndef_with_requires() {
        let source = make_source(vec![Decl::FnDef(FnDef {
            name: "f".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![],
            return_ty: vec![],
            return_type_expr: None,
            clauses: vec![make_clause(ClauseKind::Requires)],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_extern_with_invariant() {
        let source = make_source(vec![Decl::Extern(ExternDecl {
            name: "e".into(),
            params: vec![],
            return_ty: vec![],
            return_type_expr: None,
            clauses: vec![make_clause(ClauseKind::Invariant)],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_service_operation() {
        let source = make_source(vec![Decl::Service(ServiceDecl {
            name: "S".into(),
            items: vec![ServiceItem::Operation {
                name: "op".into(),
                clauses: vec![make_clause(ClauseKind::Ensures)],
            }],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_service_invariant() {
        let source = make_source(vec![Decl::Service(ServiceDecl {
            name: "S".into(),
            items: vec![ServiceItem::Invariant(Expr::Literal(Literal::Bool(true)))],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_service_query_no_clauses() {
        let source = make_source(vec![Decl::Service(ServiceDecl {
            name: "S".into(),
            items: vec![ServiceItem::Query {
                name: "q".into(),
                clauses: vec![],
            }],
        })]);
        assert!(!has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_block_with_ensures() {
        let source = make_source(vec![Decl::Block {
            kind: BlockKind::Axiomatic,
            name: "b".into(),
            value: None,
            body: vec![make_clause(ClauseKind::Ensures)],
        }]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_bind_with_requires() {
        let source = make_source(vec![Decl::Bind(BindDecl {
            name: "bd".into(),
            target_path: "path".into(),
            params: vec![],
            return_ty: vec![],
            return_type_expr: None,
            clauses: vec![make_clause(ClauseKind::Requires)],
        })]);
        assert!(has_verifiable_clauses(&source));
    }

    #[test]
    fn has_verifiable_typedef_enum_prophecy() {
        let source = make_source(vec![
            Decl::TypeDef(TypeDef {
                name: "T".into(),
                type_params: vec![],
                body: TypeBody::Alias(vec!["Int".into()]),
            }),
            Decl::EnumDef(EnumDef {
                name: "E".into(),
                type_params: vec![],
                variants: vec![],
            }),
            Decl::Prophecy(ProphecyDecl {
                name: "p".into(),
                ty_tokens: vec![],
            }),
        ]);
        assert!(!has_verifiable_clauses(&source));
    }

    // ---- verify_contract tests ----

    #[test]
    fn verify_contract_no_clauses() {
        let results = verify_contract("Test", &[]);
        assert!(results.is_empty());
    }

    #[test]
    fn verify_contract_input_only() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::Input)]);
        assert!(results.is_empty());
    }

    #[test]
    fn verify_contract_ensures_returns_result() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::Ensures)]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_with_requires_and_ensures() {
        let results = verify_contract(
            "Test",
            &[
                make_clause(ClauseKind::Requires),
                make_clause(ClauseKind::Ensures),
            ],
        );
        // Only the ensures clause produces a verification result
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_multiple_ensures() {
        let results = verify_contract(
            "Test",
            &[
                make_clause(ClauseKind::Ensures),
                make_clause(ClauseKind::Invariant),
                make_clause(ClauseKind::Rule),
            ],
        );
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn verify_contract_cvc5_solver() {
        let results = verify_contract_with_solver(
            "Test",
            &[make_clause(ClauseKind::Ensures)],
            SolverChoice::Cvc5,
        );
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_portfolio_solver() {
        let results = verify_contract_with_solver(
            "Test",
            &[make_clause(ClauseKind::Ensures)],
            SolverChoice::Portfolio,
        );
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_decreases() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::Decreases)]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_must_not() {
        let results = verify_contract("Test", &[make_clause(ClauseKind::MustNot)]);
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn verify_contract_clause_desc_format() {
        let results = verify_contract("MyContract", &[make_clause(ClauseKind::Ensures)]);
        assert_eq!(results.len(), 1);
        // The description should contain the contract name
        match &results[0] {
            VerificationResult::Verified { clause_desc, .. }
            | VerificationResult::Counterexample { clause_desc, .. }
            | VerificationResult::Timeout { clause_desc }
            | VerificationResult::Unknown { clause_desc, .. } => {
                assert!(
                    clause_desc.contains("MyContract"),
                    "clause_desc should contain contract name: {clause_desc}"
                );
            }
        }
    }

    // ---- extract_output_return_type tests ----

    #[test]
    fn extract_output_return_type_nat() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec!["result".into(), ":".into(), "Nat".into()]),
            effect_variables: vec![],
        }];
        assert_eq!(extract_output_return_type(&clauses), vec!["Nat"]);
    }

    #[test]
    fn extract_output_return_type_complex() {
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec![
                "result".into(),
                ":".into(),
                "List".into(),
                "<".into(),
                "Int".into(),
                ">".into(),
            ]),
            effect_variables: vec![],
        }];
        assert_eq!(
            extract_output_return_type(&clauses),
            vec!["List", "<", "Int", ">"]
        );
    }

    #[test]
    fn extract_output_return_type_no_colon_fallback() {
        // Fallback path: tokens without ":" at position 1 are returned as-is
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Raw(vec!["Nat".into()]),
            effect_variables: vec![],
        }];
        assert_eq!(extract_output_return_type(&clauses), vec!["Nat"]);
    }

    #[test]
    fn extract_output_return_type_missing() {
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_output_return_type(&clauses).is_empty());
    }

    #[test]
    fn extract_output_return_type_non_raw_body() {
        // Output clause with non-Raw body (should be skipped)
        let clauses = vec![Clause {
            kind: ClauseKind::Output,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_output_return_type(&clauses).is_empty());
    }

    // ---- extract_input_params tests ----

    #[test]
    fn extract_input_params_single() {
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Raw(vec!["raw_data".into(), ":".into(), "Bytes".into()]),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "raw_data");
        assert_eq!(params[0].ty, vec!["Bytes"]);
    }

    #[test]
    fn extract_input_params_multiple() {
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Raw(vec![
                "x".into(),
                ":".into(),
                "Int".into(),
                ",".into(),
                "y".into(),
                ":".into(),
                "Nat".into(),
            ]),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, "x");
        assert_eq!(params[0].ty, vec!["Int"]);
        assert_eq!(params[1].name, "y");
        assert_eq!(params[1].ty, vec!["Nat"]);
    }

    #[test]
    fn extract_input_params_no_type() {
        // Parameter without a type annotation
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Raw(vec!["x".into()]),
            effect_variables: vec![],
        }];
        let params = extract_input_params(&clauses);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "x");
        assert!(params[0].ty.is_empty());
    }

    #[test]
    fn extract_input_params_empty() {
        let clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_input_params(&clauses).is_empty());
    }

    #[test]
    fn extract_input_params_non_raw_body() {
        // Input clause with non-Raw body (should be skipped)
        let clauses = vec![Clause {
            kind: ClauseKind::Input,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        }];
        assert!(extract_input_params(&clauses).is_empty());
    }

    // ---- #199: Contract evolution verification tests ----

    #[test]
    fn evolution_identical_contracts_pass() {
        // Same requires and ensures; evolution should be compatible
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
        let result = verify_evolution("TestContract", &clauses, &clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Verified { .. }
            ),
            "identical preconditions should pass weakening: {:?}",
            result.precondition_weakening
        );
        assert!(
            matches!(
                result.postcondition_strengthening,
                VerificationResult::Verified { .. }
            ),
            "identical postconditions should pass strengthening: {:?}",
            result.postcondition_strengthening
        );
    }

    #[test]
    fn evolution_weakened_precondition_passes() {
        // Old: requires x > 10
        // New: requires x > 0 (weaker, accepts more inputs)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Verified { .. }
            ),
            "weakened precondition should pass: {:?}",
            result.precondition_weakening
        );
    }

    #[test]
    fn evolution_strengthened_precondition_fails() {
        // Old: requires x > 0
        // New: requires x > 10 (stronger, rejects inputs old accepted)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            },
            effect_variables: vec![],
        }];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Counterexample { .. }
            ),
            "strengthened precondition should fail weakening: {:?}",
            result.precondition_weakening
        );
    }

    #[test]
    fn evolution_dropped_ensures_fails() {
        // Old: ensures x > 0
        // New: no ensures (lost guarantees)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses: Vec<Clause> = vec![];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.postcondition_strengthening,
                VerificationResult::Counterexample { .. }
            ),
            "dropping ensures should fail strengthening: {:?}",
            result.postcondition_strengthening
        );
    }

    #[test]
    fn evolution_no_requires_accepts_anything() {
        // Old: no requires (accepts everything)
        // New: requires x > 0 (restricts inputs)
        // This should FAIL weakening because old accepted x = -1 but new rejects it
        let old_clauses: Vec<Clause> = vec![];
        let new_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        // old has no requires, so old_requires is trivially true.
        // new_requires is x > 0. Is true => x > 0 valid? No (x could be -1).
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Counterexample { .. }
            ),
            "adding requires to previously unconstrained should fail: {:?}",
            result.precondition_weakening
        );
    }

    #[test]
    fn evolution_new_removes_requires_passes() {
        // Old: requires x > 0
        // New: no requires (accepts everything; strictly weaker)
        let old_clauses = vec![Clause {
            kind: ClauseKind::Requires,
            body: Expr::BinOp {
                lhs: Box::new(Expr::Ident("x".into())),
                op: BinOp::Gt,
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let new_clauses: Vec<Clause> = vec![];
        let result = verify_evolution("TestContract", &old_clauses, &new_clauses);
        assert!(
            matches!(
                result.precondition_weakening,
                VerificationResult::Verified { .. }
            ),
            "removing requires (accepting everything) should pass: {:?}",
            result.precondition_weakening
        );
    }
}
