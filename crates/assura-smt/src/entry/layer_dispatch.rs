//! Layer 2 and Layer 3 verification entry points.
//!
//! Contains the obligation collection, BMC dispatch, liveness reduction,
//! and helper functions (domain_to_sort, expr_to_predicate_string) that
//! were previously in `assura-pipeline`. The pipeline now calls
//! `verify_layer2` and `verify_layer3` instead of reimplementing this logic.

use assura_ast::{self, ClauseKind, Decl, Expr, SpExpr};
use assura_config::CompilerConfig;

use crate::advanced::{LivenessChecker, LivenessKind};
use crate::bmc::{BmcConfig, BmcEngine, BmcProperty, BmcResult, BmcSort, BmcTraceStep};
use crate::layer2::{
    Layer2Config, Layer2Result, Layer2Verifier, QuantifiedInvariant, RoundtripObligation,
    TerminationObligation,
};
use crate::result::VerificationResult;

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/// Run Layer 2 verification (quantified invariants, termination, roundtrips).
///
/// Collects obligations from the typed AST and dispatches to the Layer 2 verifier.
pub fn verify_layer2(
    typed: &assura_types::TypedFile,
    config: &CompilerConfig,
) -> Vec<VerificationResult> {
    let layer2_config = Layer2Config {
        timeout_ms: config.verify.timeout_ms.max(10_000),
        enable_quantifiers: true,
        enable_termination: true,
        enable_roundtrip: true,
    };
    let mut verifier = Layer2Verifier::new(layer2_config);

    // Walk all declarations to collect obligations
    for decl in &typed.resolved.source.decls {
        collect_invariant_obligations(&decl.node, &mut verifier);
        collect_termination_obligations(&decl.node, &mut verifier);
        collect_roundtrip_obligations(&decl.node, &mut verifier);
    }

    // Collect precomputed table verification obligations
    let table_obligations = assura_types::collect_table_smt_obligations(&typed.resolved.source);
    for obligation in &table_obligations {
        verifier.add_roundtrip(RoundtripObligation {
            type_name: obligation.table_name.clone(),
            serialize_fn: obligation.generator_fn.clone(),
            deserialize_fn: format!("table_lookup_{}", obligation.table_name),
        });
    }

    if verifier.obligation_count() == 0 {
        return Vec::new();
    }

    verifier
        .verify()
        .into_iter()
        .map(layer2_result_to_verification_result)
        .collect()
}

/// Run Layer 3 verification (BMC + k-induction) on the typed AST.
///
/// Collects state-machine-like declarations and runs BMC safety/liveness checks.
pub fn verify_layer3(
    typed: &assura_types::TypedFile,
    config: &CompilerConfig,
) -> Vec<VerificationResult> {
    let timeout = config.verify.timeout_ms.max(30_000);
    let mut results = Vec::new();

    for decl in &typed.resolved.source.decls {
        let decl_name = decl.node.name().unwrap_or("anonymous");
        let clauses = decl.node.clauses();

        // Liveness blocks: reduce to safety via monitor automata
        if let Decl::Block {
            kind: assura_ast::BlockKind::Liveness,
            body,
            ..
        } = &decl.node
        {
            results.extend(run_liveness_reduction(decl_name, body, timeout));
            continue;
        }

        // Standard contracts: invariant-based safety checking
        let params = decl.node.params();
        if params.is_empty() && clauses.is_empty() {
            continue;
        }

        let mut safety_properties = Vec::new();
        let mut initial_constraints = Vec::new();

        for clause in clauses {
            match &clause.kind {
                ClauseKind::Invariant => {
                    let pred = expr_to_predicate_string(&clause.body);
                    if !pred.is_empty() && pred != "true" {
                        let negated = assura_ast::negate_expr(&clause.body);
                        let neg_pred = expr_to_predicate_string(&negated);
                        safety_properties.push((format!("{decl_name}::invariant"), neg_pred));
                    }
                }
                ClauseKind::Requires => {
                    let pred = expr_to_predicate_string(&clause.body);
                    if !pred.is_empty() && pred != "true" {
                        initial_constraints.push(pred);
                    }
                }
                _ => {}
            }
        }

        if safety_properties.is_empty() {
            continue;
        }

        let bmc_config = BmcConfig::new().with_bound(10).with_timeout(timeout);

        for (prop_name, bad_pred) in &safety_properties {
            let mut engine = BmcEngine::new(bmc_config.clone());

            for param in params {
                engine.add_state_variable(param.name.clone(), BmcSort::Int);
            }

            for ic in &initial_constraints {
                engine.add_initial_constraint(ic.clone());
            }

            engine.add_property(BmcProperty::Safety {
                name: prop_name.clone(),
                bad_predicate: bad_pred.clone(),
            });

            for br in engine.check() {
                results.push(bmc_result_to_verification_result(br));
            }
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Obligation collectors
// ---------------------------------------------------------------------------

/// Extract quantified invariant obligations from a declaration's clauses.
fn collect_invariant_obligations(decl: &Decl, verifier: &mut Layer2Verifier) {
    let decl_name = decl.name().unwrap_or("anonymous");
    let clauses = decl.clauses();

    for clause in clauses {
        if clause.kind != ClauseKind::Invariant {
            continue;
        }
        match &clause.body.node {
            Expr::Forall { var, domain, body } => {
                let sort = domain_to_sort(domain);
                verifier.add_invariant(QuantifiedInvariant {
                    name: format!("{decl_name}::invariant(forall {var})"),
                    bound_vars: vec![(var.clone(), sort)],
                    body: expr_to_predicate_string(body),
                    triggers: vec![],
                });
            }
            Expr::Exists { var, domain, body } => {
                let sort = domain_to_sort(domain);
                verifier.add_invariant(QuantifiedInvariant {
                    name: format!("{decl_name}::invariant(exists {var})"),
                    bound_vars: vec![(var.clone(), sort)],
                    body: expr_to_predicate_string(body),
                    triggers: vec![],
                });
            }
            _ => {}
        }
    }
}

/// Extract termination obligations from `decreases` clauses.
fn collect_termination_obligations(decl: &Decl, verifier: &mut Layer2Verifier) {
    let decl_name = decl.name().unwrap_or("anonymous");
    let clauses = decl.clauses();

    for clause in clauses {
        if clause.kind != ClauseKind::Decreases {
            continue;
        }
        verifier.add_termination(TerminationObligation {
            fn_name: decl_name.to_string(),
            measure: expr_to_predicate_string(&clause.body),
            recursive_calls: vec![],
        });
    }
}

/// Extract roundtrip obligations from paired serialize/deserialize declarations.
fn collect_roundtrip_obligations(decl: &Decl, verifier: &mut Layer2Verifier) {
    if let Decl::CodecRegistry(registry) = decl {
        for codec in &registry.codecs {
            verifier.add_roundtrip(RoundtripObligation {
                type_name: format!("{}::{}", registry.name, codec.name),
                serialize_fn: format!("{}_encode", codec.name),
                deserialize_fn: format!("{}_decode", codec.name),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Liveness reduction
// ---------------------------------------------------------------------------

/// Reduce liveness obligations to safety properties via monitor automata,
/// then dispatch to BMC.
fn run_liveness_reduction(
    decl_name: &str,
    clauses: &[assura_ast::Clause],
    timeout: u64,
) -> Vec<VerificationResult> {
    let mut checker = LivenessChecker::new();

    for clause in clauses {
        let body_str = expr_to_predicate_string(&clause.body);

        match &clause.body.node {
            Expr::Call { func, args } => {
                let func_name = match &func.node {
                    Expr::Ident(n) => n.as_str(),
                    _ => "",
                };
                match func_name {
                    "eventually" => {
                        let goal = args
                            .first()
                            .map(expr_to_predicate_string)
                            .unwrap_or_else(|| body_str.clone());
                        checker.add_obligation(
                            format!("{decl_name}::eventually"),
                            LivenessKind::Eventually,
                            "true".into(),
                            goal,
                        );
                    }
                    "eventually_within" => {
                        let goal = args
                            .first()
                            .map(expr_to_predicate_string)
                            .unwrap_or_else(|| body_str.clone());
                        checker.add_obligation(
                            format!("{decl_name}::eventually_within"),
                            LivenessKind::EventuallyWithin(100),
                            "true".into(),
                            goal,
                        );
                    }
                    "leads_to" => {
                        let premise = args
                            .first()
                            .map(expr_to_predicate_string)
                            .unwrap_or_default();
                        let conclusion = args
                            .get(1)
                            .map(expr_to_predicate_string)
                            .unwrap_or_default();
                        checker.add_obligation(
                            format!("{decl_name}::leads_to"),
                            LivenessKind::LeadsTo,
                            premise,
                            conclusion,
                        );
                    }
                    _ => {}
                }
            }
            _ => {
                if clause.kind == ClauseKind::Other("assume".into()) {
                    checker.add_fairness(body_str);
                }
            }
        }
    }

    if checker.obligation_count() == 0 {
        return Vec::new();
    }

    let reductions = checker.reduce_to_safety();
    let bmc_config = BmcConfig::new().with_bound(10).with_timeout(timeout);

    let mut results = Vec::new();
    for reduction in &reductions {
        let components = reduction.to_bmc_components();
        let mut engine = BmcEngine::new(bmc_config.clone());

        for sv in &components.state_vars {
            engine.add_state_variable(sv.name.clone(), sv.sort.clone());
        }

        for ic in &components.initial_constraints {
            engine.add_initial_constraint(ic.clone());
        }

        engine.add_property(components.property);

        for br in engine.check() {
            results.push(bmc_result_to_verification_result(br));
        }
    }

    results
}

// ---------------------------------------------------------------------------
// Result conversion
// ---------------------------------------------------------------------------

fn bmc_result_to_verification_result(result: BmcResult) -> VerificationResult {
    match result {
        BmcResult::Safe { property, bound } => VerificationResult::Verified {
            clause_desc: format!("{property} (BMC safe up to {bound})"),
            unsat_core: None,
        },
        BmcResult::Counterexample {
            property,
            step,
            trace,
        } => {
            let model = format_bmc_trace(&trace, step);
            VerificationResult::Counterexample {
                clause_desc: format!("{property} (BMC counterexample at step {step})"),
                model,
                counter_model: None,
            }
        }
        BmcResult::Lasso {
            property,
            stem_length,
            loop_length,
            trace,
        } => {
            let model = format_lasso_trace(&trace, stem_length, loop_length);
            VerificationResult::Counterexample {
                clause_desc: format!("{property} (lasso: stem={stem_length}, loop={loop_length})"),
                model,
                counter_model: None,
            }
        }
        BmcResult::Unknown { property, reason } => VerificationResult::Unknown {
            clause_desc: format!("{property} (BMC)"),
            reason,
        },
    }
}

fn layer2_result_to_verification_result(r: Layer2Result) -> VerificationResult {
    match r {
        Layer2Result::Verified { invariant, .. } => VerificationResult::Verified {
            clause_desc: format!("layer2:{invariant}"),
            unsat_core: None,
        },
        Layer2Result::Counterexample { invariant, model } => {
            let model_str = model
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            VerificationResult::Counterexample {
                clause_desc: format!("layer2:{invariant}"),
                model: model_str,
                counter_model: None,
            }
        }
        Layer2Result::Timeout {
            invariant,
            timeout_ms,
        } => VerificationResult::Timeout {
            clause_desc: format!("layer2:{invariant} (timeout {timeout_ms}ms)"),
        },
        Layer2Result::Unknown { invariant, reason } => VerificationResult::Unknown {
            clause_desc: format!("layer2:{invariant}"),
            reason,
        },
    }
}

// ---------------------------------------------------------------------------
// AST -> text helpers
// ---------------------------------------------------------------------------

/// Convert a domain expression to a Z3 sort name.
fn domain_to_sort(domain: &SpExpr) -> String {
    match &domain.node {
        Expr::Ident(name) => match name.as_str() {
            "Int" | "Nat" | "Float" => name.clone(),
            "Bool" => "Bool".into(),
            "String" | "Bytes" => "String".into(),
            _ => "Int".into(),
        },
        Expr::Raw(tokens) if !tokens.is_empty() => tokens[0].clone(),
        _ => "Int".into(),
    }
}

/// Convert an AST expression to a simple predicate string for Layer 2/3.
///
/// Best-effort textual representation. The Layer 2 verifier has its own
/// expression parser (`parse_predicate_to_z3`).
fn expr_to_predicate_string(expr: &SpExpr) -> String {
    match &expr.node {
        Expr::BinOp { op, lhs, rhs } => {
            let l = expr_to_predicate_string(lhs);
            let r = expr_to_predicate_string(rhs);
            let op_str = match op {
                assura_ast::BinOp::And => "&&",
                assura_ast::BinOp::Or => "||",
                _ => op.as_str(),
            };
            format!("{l} {op_str} {r}")
        }
        Expr::UnaryOp { op, expr } => {
            let o = expr_to_predicate_string(expr);
            format!("{}{o}", op.as_str())
        }
        Expr::Literal(lit) => match lit {
            assura_ast::Literal::Int(n) => n.clone(),
            assura_ast::Literal::Float(f) => f.clone(),
            assura_ast::Literal::Bool(b) => b.to_string(),
            assura_ast::Literal::Str(s) => format!("\"{s}\""),
        },
        Expr::Ident(name) => name.clone(),
        Expr::Forall { var, body, .. } => {
            format!("forall {var}: {}", expr_to_predicate_string(body))
        }
        Expr::Exists { var, body, .. } => {
            format!("exists {var}: {}", expr_to_predicate_string(body))
        }
        Expr::Raw(tokens) => tokens.join(" "),
        _ => format!("{expr:?}"),
    }
}

/// Format a BMC counterexample trace as a human-readable string.
fn format_bmc_trace(trace: &[BmcTraceStep], bad_step: usize) -> String {
    let mut lines = Vec::new();
    lines.push("BMC counterexample trace:".to_string());
    for step in trace {
        let marker = if step.step == bad_step {
            " <-- BAD STATE"
        } else {
            ""
        };
        let vals: Vec<String> = step
            .assignments
            .iter()
            .map(|(name, val)| format!("{name}={val}"))
            .collect();
        lines.push(format!(
            "  step {}: {{{}}}{marker}",
            step.step,
            vals.join(", ")
        ));
    }
    lines.join("\n")
}

/// Format a lasso counterexample trace with loop visualization.
fn format_lasso_trace(trace: &[BmcTraceStep], stem_length: usize, loop_length: usize) -> String {
    let mut lines = Vec::new();
    lines.push(format!(
        "Lasso counterexample (stem={stem_length}, loop={loop_length}):"
    ));

    if stem_length > 0 {
        lines.push("  Stem:".to_string());
        for step in trace.iter().take(stem_length) {
            let vals: Vec<String> = step
                .assignments
                .iter()
                .map(|(name, val)| format!("{name}={val}"))
                .collect();
            lines.push(format!("    step {}: {{{}}}", step.step, vals.join(", ")));
        }
    }

    lines.push("  Loop:".to_string());
    let loop_end = stem_length + loop_length;
    for step in trace.iter().skip(stem_length).take(loop_length + 1) {
        let marker = if step.step == loop_end {
            " --> back to loop start"
        } else {
            ""
        };
        let vals: Vec<String> = step
            .assignments
            .iter()
            .map(|(name, val)| format!("{name}={val}"))
            .collect();
        lines.push(format!(
            "    step {}: {{{}}}{}",
            step.step,
            vals.join(", "),
            marker
        ));
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{BinOp, Expr, Literal, Spanned};

    #[test]
    fn layer2_result_to_verification_result_conversion() {
        let verified = Layer2Result::Verified {
            invariant: "inv1".into(),
            time_ms: 42,
        };
        let result = layer2_result_to_verification_result(verified);
        assert!(matches!(result, VerificationResult::Verified { .. }));

        let ce = Layer2Result::Counterexample {
            invariant: "inv2".into(),
            model: vec![("x".into(), "0".into())],
        };
        let result = layer2_result_to_verification_result(ce);
        assert!(matches!(result, VerificationResult::Counterexample { .. }));

        let timeout = Layer2Result::Timeout {
            invariant: "inv3".into(),
            timeout_ms: 10000,
        };
        let result = layer2_result_to_verification_result(timeout);
        assert!(matches!(result, VerificationResult::Timeout { .. }));

        let unknown = Layer2Result::Unknown {
            invariant: "inv4".into(),
            reason: "solver inconclusive".into(),
        };
        let result = layer2_result_to_verification_result(unknown);
        assert!(matches!(result, VerificationResult::Unknown { .. }));
    }

    #[test]
    fn expr_to_predicate_string_basic() {
        // Simple ident
        let expr = Spanned::no_span(Expr::Ident("x".into()));
        assert_eq!(expr_to_predicate_string(&expr), "x");

        // Integer literal
        let expr = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
        assert_eq!(expr_to_predicate_string(&expr), "42");

        // BinOp
        let expr = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
            op: BinOp::Gte,
            rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
        });
        assert_eq!(expr_to_predicate_string(&expr), "x >= 0");

        // Raw tokens
        let expr = Spanned::no_span(Expr::Raw(vec!["a".into(), "+".into(), "b".into()]));
        assert_eq!(expr_to_predicate_string(&expr), "a + b");
    }

    #[test]
    fn format_bmc_trace_output() {
        let trace = vec![
            BmcTraceStep {
                step: 0,
                assignments: vec![("n".into(), "5".into())],
            },
            BmcTraceStep {
                step: 1,
                assignments: vec![("n".into(), "-1".into())],
            },
        ];
        let output = format_bmc_trace(&trace, 1);
        assert!(output.contains("BMC counterexample trace:"));
        assert!(output.contains("step 0"));
        assert!(output.contains("step 1"));
        assert!(output.contains("BAD STATE"));
    }

    #[test]
    fn format_lasso_trace_output() {
        let trace = vec![
            BmcTraceStep {
                step: 0,
                assignments: vec![("s".into(), "0".into())],
            },
            BmcTraceStep {
                step: 1,
                assignments: vec![("s".into(), "1".into())],
            },
            BmcTraceStep {
                step: 2,
                assignments: vec![("s".into(), "0".into())],
            },
        ];
        let output = format_lasso_trace(&trace, 1, 1);
        assert!(output.contains("Lasso counterexample"));
        assert!(output.contains("Stem:"));
        assert!(output.contains("Loop:"));
    }

    #[test]
    fn bmc_result_safe() {
        let safe = BmcResult::Safe {
            property: "inv".into(),
            bound: 10,
        };
        let result = bmc_result_to_verification_result(safe);
        assert!(matches!(result, VerificationResult::Verified { .. }));
    }

    #[test]
    fn bmc_result_counterexample() {
        let ce = BmcResult::Counterexample {
            property: "inv".into(),
            step: 3,
            trace: vec![BmcTraceStep {
                step: 3,
                assignments: vec![("n".into(), "-1".into())],
            }],
        };
        let result = bmc_result_to_verification_result(ce);
        assert!(matches!(result, VerificationResult::Counterexample { .. }));
    }

    #[test]
    fn bmc_result_unknown() {
        let unk = BmcResult::Unknown {
            property: "inv".into(),
            reason: "timeout".into(),
        };
        let result = bmc_result_to_verification_result(unk);
        assert!(matches!(result, VerificationResult::Unknown { .. }));
    }

    #[test]
    fn domain_to_sort_mapping() {
        let int_domain = Spanned::no_span(Expr::Ident("Int".into()));
        assert_eq!(domain_to_sort(&int_domain), "Int");

        let nat_domain = Spanned::no_span(Expr::Ident("Nat".into()));
        assert_eq!(domain_to_sort(&nat_domain), "Nat");

        let bool_domain = Spanned::no_span(Expr::Ident("Bool".into()));
        assert_eq!(domain_to_sort(&bool_domain), "Bool");

        let custom_domain = Spanned::no_span(Expr::Ident("MyType".into()));
        assert_eq!(domain_to_sort(&custom_domain), "Int"); // default

        let raw_domain = Spanned::no_span(Expr::Raw(vec!["Float".into()]));
        assert_eq!(domain_to_sort(&raw_domain), "Float");
    }
}
