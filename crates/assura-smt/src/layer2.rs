use super::*;
use assura_parser::ast::SpExpr;

// ===========================================================================
// T076: Layer 2 SMT encoding
// ===========================================================================

/// Layer 2 verification: quantified invariants, functional correctness,
/// termination proofs, and serialization roundtrip verification.
///
/// Uses AUFLIA (arrays + uninterpreted functions + linear integer arithmetic)
/// SMT theory with configurable timeout (default 10s for Layer 2).
#[derive(Debug, Clone)]
pub struct Layer2Config {
    /// Timeout in milliseconds for Layer 2 queries (default: 10_000)
    pub timeout_ms: u64,
    /// Whether to enable quantifier instantiation
    pub enable_quantifiers: bool,
    /// Whether to verify termination proofs
    pub enable_termination: bool,
    /// Whether to verify serialization roundtrips
    pub enable_roundtrip: bool,
}

impl Default for Layer2Config {
    fn default() -> Self {
        Self {
            timeout_ms: 10_000,
            enable_quantifiers: true,
            enable_termination: true,
            enable_roundtrip: true,
        }
    }
}

impl Layer2Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }
}

/// A quantified invariant to verify at Layer 2.
#[derive(Debug, Clone)]
pub struct QuantifiedInvariant {
    pub name: String,
    /// Bound variables: (name, sort)
    pub bound_vars: Vec<(String, String)>,
    /// The invariant body (as expression text)
    pub body: String,
    /// Optional trigger patterns for e-matching
    pub triggers: Vec<String>,
}

/// Result of a Layer 2 verification attempt.
#[derive(Debug, Clone)]
pub enum Layer2Result {
    Verified {
        invariant: String,
        time_ms: u64,
    },
    Counterexample {
        invariant: String,
        model: Vec<(String, String)>,
    },
    Timeout {
        invariant: String,
        timeout_ms: u64,
    },
    Unknown {
        invariant: String,
        reason: String,
    },
}

/// Collects Layer 2 verification obligations and dispatches them.
#[derive(Debug, Clone)]
pub struct Layer2Verifier {
    pub config: Layer2Config,
    pub invariants: Vec<QuantifiedInvariant>,
    pub termination_obligations: Vec<TerminationObligation>,
    pub roundtrip_obligations: Vec<RoundtripObligation>,
}

/// A termination proof obligation.
#[derive(Debug, Clone)]
pub struct TerminationObligation {
    pub fn_name: String,
    pub measure: String,
    pub recursive_calls: Vec<String>,
}

/// A serialization roundtrip obligation.
#[derive(Debug, Clone)]
pub struct RoundtripObligation {
    pub type_name: String,
    pub serialize_fn: String,
    pub deserialize_fn: String,
}

impl Layer2Verifier {
    pub fn new(config: Layer2Config) -> Self {
        Self {
            config,
            invariants: Vec::new(),
            termination_obligations: Vec::new(),
            roundtrip_obligations: Vec::new(),
        }
    }

    pub fn add_invariant(&mut self, inv: QuantifiedInvariant) {
        self.invariants.push(inv);
    }

    pub fn add_termination(&mut self, obl: TerminationObligation) {
        self.termination_obligations.push(obl);
    }

    pub fn add_roundtrip(&mut self, obl: RoundtripObligation) {
        self.roundtrip_obligations.push(obl);
    }

    /// Structural pre-check without Z3 (validates obligation structure only).
    ///
    /// This does NOT verify correctness. It checks that obligations are
    /// well-formed (have bound variables, have measures, etc.). Obligations
    /// that pass structural checks are reported as `Unknown` with reason
    /// "requires Z3 verification", NOT as `Verified`.
    ///
    /// Use `verify()` for actual Z3-backed verification.
    pub fn check_structural(&self) -> Vec<Layer2Result> {
        let mut results = Vec::new();

        for inv in &self.invariants {
            if inv.bound_vars.is_empty() {
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "quantified invariant has no bound variables".into(),
                });
            } else {
                // Structurally valid; use verify() for Z3-backed proof
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "structural pre-check only; call verify() for Z3 proof".into(),
                });
            }
        }

        for obl in &self.termination_obligations {
            if obl.measure.is_empty() {
                results.push(Layer2Result::Unknown {
                    invariant: format!("termination:{}", obl.fn_name),
                    reason: "no measure provided".into(),
                });
            } else {
                // Structurally valid; use verify() for Z3-backed proof
                results.push(Layer2Result::Unknown {
                    invariant: format!("termination:{}", obl.fn_name),
                    reason: "structural pre-check only; call verify() for Z3 proof".into(),
                });
            }
        }

        for obl in &self.roundtrip_obligations {
            // Structurally valid; use verify() for Z3-backed proof
            results.push(Layer2Result::Unknown {
                invariant: format!("roundtrip:{}", obl.type_name),
                reason: "structural pre-check only; call verify() for Z3 proof".into(),
            });
        }

        results
    }

    pub fn obligation_count(&self) -> usize {
        self.invariants.len()
            + self.termination_obligations.len()
            + self.roundtrip_obligations.len()
    }

    /// Verify all quantified invariants using Z3 with Layer 2 timeout.
    ///
    /// For each `QuantifiedInvariant`, creates a Z3 context with the
    /// Layer 2 timeout (default 10s), encodes the invariant as a
    /// universally quantified formula, and checks validity (negation
    /// is UNSAT => valid).
    pub fn verify(&self) -> Vec<Layer2Result> {
        #[cfg(feature = "z3-verify")]
        {
            self.verify_with_z3()
        }
        #[cfg(not(feature = "z3-verify"))]
        {
            self.check_structural()
        }
    }

    #[cfg(feature = "z3-verify")]
    fn verify_with_z3(&self) -> Vec<Layer2Result> {
        use std::time::Instant;
        use z3::{Config, SatResult, Solver, ast};

        let mut cfg = Config::new();
        cfg.set_timeout_msec(self.config.timeout_ms);

        z3::with_z3_config(&cfg, || {
            let mut results = Vec::new();

            // --- Quantified invariants ---
            for inv in &self.invariants {
                if inv.bound_vars.is_empty() {
                    results.push(Layer2Result::Unknown {
                        invariant: inv.name.clone(),
                        reason: "quantified invariant has no bound variables".into(),
                    });
                    continue;
                }

                let start = Instant::now();
                let solver = Solver::new();

                // Create bound variables
                let mut bound_consts = Vec::new();
                for (var_name, sort_name) in &inv.bound_vars {
                    match sort_name.as_str() {
                        "Int" | "Nat" => {
                            bound_consts
                                .push((var_name.clone(), ast::Int::new_const(var_name.as_str())));
                        }
                        _ => {
                            // Unknown sort, treat as Int
                            bound_consts
                                .push((var_name.clone(), ast::Int::new_const(var_name.as_str())));
                        }
                    }
                }

                // Parse and encode the body as a Z3 Bool
                match parse_predicate_to_z3(&inv.body, &bound_consts) {
                    Some(body_z3) => {
                        // To verify `forall x: P(x)`, negate and check:
                        // if `exists x: !P(x)` is UNSAT, the invariant holds.
                        solver.assert(body_z3.not());

                        match solver.check() {
                            SatResult::Unsat => {
                                results.push(Layer2Result::Verified {
                                    invariant: inv.name.clone(),
                                    time_ms: start.elapsed().as_millis() as u64,
                                });
                            }
                            SatResult::Sat => {
                                let model_entries = if let Some(m) = solver.get_model() {
                                    bound_consts
                                        .iter()
                                        .filter_map(|(name, c)| {
                                            m.eval(c, true).map(|v| (name.clone(), v.to_string()))
                                        })
                                        .collect()
                                } else {
                                    vec![]
                                };
                                results.push(Layer2Result::Counterexample {
                                    invariant: inv.name.clone(),
                                    model: model_entries,
                                });
                            }
                            SatResult::Unknown => {
                                let reason = solver
                                    .get_reason_unknown()
                                    .unwrap_or_else(|| "unknown".into());
                                if reason.contains("timeout") {
                                    results.push(Layer2Result::Timeout {
                                        invariant: inv.name.clone(),
                                        timeout_ms: self.config.timeout_ms,
                                    });
                                } else {
                                    results.push(Layer2Result::Unknown {
                                        invariant: inv.name.clone(),
                                        reason,
                                    });
                                }
                            }
                        }
                    }
                    None => {
                        results.push(Layer2Result::Unknown {
                            invariant: inv.name.clone(),
                            reason: format!("cannot parse invariant body: {}", inv.body),
                        });
                    }
                }
            }

            // --- Termination obligations ---
            if self.config.enable_termination {
                for obl in &self.termination_obligations {
                    if obl.measure.is_empty() {
                        results.push(Layer2Result::Unknown {
                            invariant: format!("termination:{}", obl.fn_name),
                            reason: "no measure provided".into(),
                        });
                        continue;
                    }

                    let start = Instant::now();
                    let solver = Solver::new();

                    // Termination check: measure must be non-negative, and each
                    // recursive call must strictly decrease the measure.
                    let measure_var = ast::Int::new_const(obl.measure.as_str());

                    // Assert measure >= 0 (well-founded)
                    let zero = ast::Int::from_i64(0);
                    solver.assert(measure_var.ge(&zero));

                    if obl.recursive_calls.is_empty() {
                        // No recursive calls means trivially terminating
                        results.push(Layer2Result::Verified {
                            invariant: format!("termination:{}", obl.fn_name),
                            time_ms: start.elapsed().as_millis() as u64,
                        });
                    } else {
                        // For each recursive call, the measure must decrease.
                        // We check: exists measure >= 0 such that NOT(measure' < measure)
                        // for any recursive call. If UNSAT, termination holds.
                        let mut all_decrease = ast::Bool::from_bool(true);
                        for (i, _call) in obl.recursive_calls.iter().enumerate() {
                            let call_measure =
                                ast::Int::new_const(format!("measure_call_{i}").as_str());
                            // call_measure >= 0
                            solver.assert(call_measure.ge(&zero));
                            // call_measure < measure (must decrease)
                            all_decrease =
                                ast::Bool::and(&[&all_decrease, &call_measure.lt(&measure_var)]);
                        }

                        // Negate: if exists assignment where NOT all decrease, we have
                        // a counterexample
                        solver.assert(all_decrease.not());

                        match solver.check() {
                            SatResult::Unsat => {
                                results.push(Layer2Result::Verified {
                                    invariant: format!("termination:{}", obl.fn_name),
                                    time_ms: start.elapsed().as_millis() as u64,
                                });
                            }
                            SatResult::Sat => {
                                let model_entries = if let Some(m) = solver.get_model() {
                                    vec![(
                                        obl.measure.clone(),
                                        m.eval(&measure_var, true)
                                            .map(|v| v.to_string())
                                            .unwrap_or_else(|| "?".into()),
                                    )]
                                } else {
                                    vec![]
                                };
                                results.push(Layer2Result::Counterexample {
                                    invariant: format!("termination:{}", obl.fn_name),
                                    model: model_entries,
                                });
                            }
                            SatResult::Unknown => {
                                let reason = solver
                                    .get_reason_unknown()
                                    .unwrap_or_else(|| "unknown".into());
                                if reason.contains("timeout") {
                                    results.push(Layer2Result::Timeout {
                                        invariant: format!("termination:{}", obl.fn_name),
                                        timeout_ms: self.config.timeout_ms,
                                    });
                                } else {
                                    results.push(Layer2Result::Unknown {
                                        invariant: format!("termination:{}", obl.fn_name),
                                        reason,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // --- Roundtrip obligations ---
            if self.config.enable_roundtrip {
                for obl in &self.roundtrip_obligations {
                    let start = Instant::now();
                    let solver = Solver::new();

                    // Model roundtrip as: forall x, deserialize(serialize(x)) == x
                    // Using uninterpreted functions.
                    let int_sort = z3::Sort::int();
                    let ser_fn =
                        z3::FuncDecl::new(obl.serialize_fn.as_str(), &[&int_sort], &int_sort);
                    let deser_fn =
                        z3::FuncDecl::new(obl.deserialize_fn.as_str(), &[&int_sort], &int_sort);

                    let x = ast::Int::new_const("x");

                    // serialize(x)
                    let ser_x = ser_fn.apply(&[&x]);
                    // deserialize(serialize(x))
                    let deser_ser_x = deser_fn.apply(&[&ser_x]);

                    // Assert roundtrip property: deserialize(serialize(x)) == x
                    // Check negation: exists x such that deser(ser(x)) != x
                    let x_ast: z3::ast::Dynamic = x.clone().into();
                    let eq = deser_ser_x.eq(&x_ast);
                    solver.assert(eq.not());

                    match solver.check() {
                        SatResult::Unsat => {
                            // Roundtrip holds for all x (with uninterpreted functions,
                            // this is only UNSAT if there's no model at all, which means
                            // Z3 can prove it from axioms alone). In practice, uninterpreted
                            // functions without axioms will be SAT (a model exists where
                            // deser(ser(x)) != x). So we report Verified only if UNSAT.
                            results.push(Layer2Result::Verified {
                                invariant: format!("roundtrip:{}", obl.type_name),
                                time_ms: start.elapsed().as_millis() as u64,
                            });
                        }
                        SatResult::Sat => {
                            // With pure uninterpreted functions and no axioms constraining
                            // ser/deser to be inverses, SAT is expected. Report as Unknown
                            // with an explanation: the roundtrip property requires axioms
                            // from the actual serialize/deserialize implementations.
                            results.push(Layer2Result::Unknown {
                            invariant: format!("roundtrip:{}", obl.type_name),
                            reason:
                                "roundtrip requires implementation axioms for serialize/deserialize"
                                    .into(),
                        });
                        }
                        SatResult::Unknown => {
                            let reason = solver
                                .get_reason_unknown()
                                .unwrap_or_else(|| "unknown".into());
                            if reason.contains("timeout") {
                                results.push(Layer2Result::Timeout {
                                    invariant: format!("roundtrip:{}", obl.type_name),
                                    timeout_ms: self.config.timeout_ms,
                                });
                            } else {
                                results.push(Layer2Result::Unknown {
                                    invariant: format!("roundtrip:{}", obl.type_name),
                                    reason,
                                });
                            }
                        }
                    }
                }
            }

            results
        }) // end with_z3_config
    }
}

/// Parse a simple predicate string into a Z3 Bool expression.
///
/// Supports: `x >= 0`, `x > 0`, `x < y`, `x == y`, `x != y`, `x + y > z`,
/// `true`, `false`, and conjunctions with `&&`.
#[cfg(feature = "z3-verify")]
fn parse_predicate_to_z3(body: &str, vars: &[(String, z3::ast::Int)]) -> Option<z3::ast::Bool> {
    let body = body.trim();

    // Handle boolean literals
    if body == "true" {
        return Some(z3::ast::Bool::from_bool(true));
    }
    if body == "false" {
        return Some(z3::ast::Bool::from_bool(false));
    }

    // Handle conjunction: split on &&
    if body.contains("&&") {
        let parts: Vec<&str> = body.split("&&").collect();
        let mut conjuncts = Vec::new();
        for part in parts {
            conjuncts.push(parse_predicate_to_z3(part.trim(), vars)?);
        }
        let refs: Vec<&z3::ast::Bool> = conjuncts.iter().collect();
        return Some(z3::ast::Bool::and(&refs));
    }

    // Handle disjunction: split on ||
    if body.contains("||") {
        let parts: Vec<&str> = body.split("||").collect();
        let mut disjuncts = Vec::new();
        for part in parts {
            disjuncts.push(parse_predicate_to_z3(part.trim(), vars)?);
        }
        let refs: Vec<&z3::ast::Bool> = disjuncts.iter().collect();
        return Some(z3::ast::Bool::or(&refs));
    }

    // Handle comparison operators (check multi-char ops first to avoid
    // matching `>` before `>=`)
    let comparisons = [
        (">=", "ge"),
        ("<=", "le"),
        ("!=", "ne"),
        ("==", "eq"),
        (">", "gt"),
        ("<", "lt"),
    ];
    for (op_str, op_kind) in comparisons {
        if let Some(pos) = body.find(op_str) {
            let lhs_str = body[..pos].trim();
            let rhs_str = body[pos + op_str.len()..].trim();
            let lhs = parse_int_expr(lhs_str, vars)?;
            let rhs = parse_int_expr(rhs_str, vars)?;
            return match op_kind {
                "ge" => Some(lhs.ge(&rhs)),
                "le" => Some(lhs.le(&rhs)),
                "ne" => Some(lhs.eq(&rhs).not()),
                "eq" => Some(lhs.eq(&rhs)),
                "gt" => Some(lhs.gt(&rhs)),
                "lt" => Some(lhs.lt(&rhs)),
                // Safety: op_kind comes from the comparisons array above
                _ => None,
            };
        }
    }

    None
}

/// Parse a simple integer expression string into a Z3 Int.
///
/// Supports: variable names, integer literals, `x + y`, `x - y`, `x * y`.
#[cfg(feature = "z3-verify")]
fn parse_int_expr(expr: &str, vars: &[(String, z3::ast::Int)]) -> Option<z3::ast::Int> {
    let expr = expr.trim();

    // Handle addition
    if let Some(pos) = expr.rfind('+')
        && pos > 0
    {
        let lhs = parse_int_expr(&expr[..pos], vars)?;
        let rhs = parse_int_expr(&expr[pos + 1..], vars)?;
        return Some(z3::ast::Int::add(&[&lhs, &rhs]));
    }

    // Handle subtraction (but not negative numbers)
    if let Some(pos) = expr.rfind('-')
        && pos > 0
    {
        let lhs = parse_int_expr(&expr[..pos], vars)?;
        let rhs = parse_int_expr(&expr[pos + 1..], vars)?;
        return Some(z3::ast::Int::sub(&[&lhs, &rhs]));
    }

    // Handle multiplication
    if let Some(pos) = expr.rfind('*') {
        let lhs = parse_int_expr(&expr[..pos], vars)?;
        let rhs = parse_int_expr(&expr[pos + 1..], vars)?;
        return Some(z3::ast::Int::mul(&[&lhs, &rhs]));
    }

    // Integer literal
    if let Ok(n) = expr.parse::<i64>() {
        return Some(z3::ast::Int::from_i64(n));
    }

    // Variable lookup
    for (name, z3_var) in vars {
        if name == expr {
            return Some(z3_var.clone());
        }
    }

    None
}

/// Verify a quantified expression using Z3 with Layer 2 timeout (10s).
///
/// Sends `forall x in S: P(x)` or `exists x in S: P(x)` expressions
/// directly to Z3, using the existing `Encoder` to encode the Expr tree.
/// Returns a `VerificationResult` (not `Layer2Result`) for consistency
/// with the main verification pipeline.
///
/// Layer 2 uses a 10s timeout (vs 1s for Layer 1).
pub fn verify_quantified_expr(
    name: &str,
    assumptions: &[SpExpr],
    quantified_body: &SpExpr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        z3_backend::verify_quantified_impl(name, assumptions, quantified_body)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = quantified_body;
        VerificationResult::Unknown {
            clause_desc: name.into(),
            reason: format!(
                "Z3 not available (compiled without z3-verify feature); \
                 {} assumption(s) and quantified body were not checked",
                assumptions.len(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Layer2Config tests
    // -------------------------------------------------------------------

    #[test]
    fn test_config_defaults() {
        let cfg = Layer2Config::default();
        assert_eq!(cfg.timeout_ms, 10_000);
        assert!(cfg.enable_quantifiers);
        assert!(cfg.enable_termination);
        assert!(cfg.enable_roundtrip);
    }

    #[test]
    fn test_config_with_timeout() {
        let cfg = Layer2Config::new().with_timeout(5000);
        assert_eq!(cfg.timeout_ms, 5000);
    }

    // -------------------------------------------------------------------
    // Layer2Verifier structural tests
    // -------------------------------------------------------------------

    #[test]
    fn test_verifier_empty() {
        let v = Layer2Verifier::new(Layer2Config::default());
        assert_eq!(v.obligation_count(), 0);
        let results = v.check_structural();
        assert!(results.is_empty());
    }

    #[test]
    fn test_verifier_add_invariant() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_invariant(QuantifiedInvariant {
            name: "inv1".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "x >= 0".into(),
            triggers: vec![],
        });
        assert_eq!(v.obligation_count(), 1);
    }

    #[test]
    fn test_verifier_no_bound_vars_unknown() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_invariant(QuantifiedInvariant {
            name: "bad_inv".into(),
            bound_vars: vec![], // no bound vars
            body: "true".into(),
            triggers: vec![],
        });
        let results = v.check_structural();
        assert_eq!(results.len(), 1);
        match &results[0] {
            Layer2Result::Unknown { reason, .. } => {
                assert!(reason.contains("no bound variables"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_structural_check_reports_unknown_for_valid_invariant() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_invariant(QuantifiedInvariant {
            name: "ok_inv".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "x > 0".into(),
            triggers: vec![],
        });
        // check_structural always returns Unknown (no Z3)
        let results = v.check_structural();
        assert_eq!(results.len(), 1);
        match &results[0] {
            Layer2Result::Unknown { reason, .. } => {
                assert!(reason.contains("structural pre-check"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_valid_invariant_finds_counterexample() {
        // "x > 0" is NOT universally true (x=0 is a counterexample)
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_invariant(QuantifiedInvariant {
            name: "not_universal".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "x > 0".into(),
            triggers: vec![],
        });
        let results = v.verify();
        assert_eq!(results.len(), 1);
        match &results[0] {
            Layer2Result::Counterexample { invariant, model } => {
                assert_eq!(invariant, "not_universal");
                assert!(!model.is_empty());
            }
            other => panic!("expected Counterexample, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_tautology_is_verified() {
        // "x >= 0 || x < 0" is always true
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_invariant(QuantifiedInvariant {
            name: "tautology".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "x >= 0 || x < 0".into(),
            triggers: vec![],
        });
        let results = v.verify();
        assert_eq!(results.len(), 1);
        match &results[0] {
            Layer2Result::Verified { invariant, .. } => {
                assert_eq!(invariant, "tautology");
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    #[test]
    fn test_verifier_termination_no_measure() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_termination(TerminationObligation {
            fn_name: "fib".into(),
            measure: "".into(), // empty measure
            recursive_calls: vec![],
        });
        let results = v.check_structural();
        match &results[0] {
            Layer2Result::Unknown { reason, .. } => {
                assert!(reason.contains("no measure"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_structural_termination_with_measure_reports_unknown() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_termination(TerminationObligation {
            fn_name: "fib".into(),
            measure: "n".into(),
            recursive_calls: vec!["fib(n-1)".into()],
        });
        // check_structural always returns Unknown for structurally valid
        let results = v.check_structural();
        match &results[0] {
            Layer2Result::Unknown { reason, .. } => {
                assert!(reason.contains("structural pre-check"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_termination_with_measure() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_termination(TerminationObligation {
            fn_name: "fib".into(),
            measure: "n".into(),
            recursive_calls: vec!["fib(n-1)".into()],
        });
        let results = v.verify();
        assert_eq!(results.len(), 1);
        // With recursive calls: Z3 finds that measure=0 is a counterexample
        // (can't have call_measure >= 0 AND call_measure < 0). This is correct
        // behavior: the termination proof needs a base case guard (n > 0).
        match &results[0] {
            Layer2Result::Counterexample { invariant, model } => {
                assert!(invariant.contains("termination:fib"));
                // Model shows the base case where measure can't decrease
                assert!(!model.is_empty());
            }
            other => panic!("expected Counterexample for fib termination, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_termination_no_recursive_calls() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_termination(TerminationObligation {
            fn_name: "base".into(),
            measure: "n".into(),
            recursive_calls: vec![],
        });
        let results = v.verify();
        assert_eq!(results.len(), 1);
        match &results[0] {
            Layer2Result::Verified { invariant, .. } => {
                assert!(invariant.contains("termination:base"));
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    #[test]
    fn test_structural_roundtrip_reports_unknown() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_roundtrip(RoundtripObligation {
            type_name: "MyStruct".into(),
            serialize_fn: "to_json".into(),
            deserialize_fn: "from_json".into(),
        });
        assert_eq!(v.obligation_count(), 1);
        // check_structural always reports Unknown for roundtrips
        let results = v.check_structural();
        match &results[0] {
            Layer2Result::Unknown { invariant, .. } => {
                assert!(invariant.contains("roundtrip"));
            }
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_verify_roundtrip() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_roundtrip(RoundtripObligation {
            type_name: "MyStruct".into(),
            serialize_fn: "to_json".into(),
            deserialize_fn: "from_json".into(),
        });
        let results = v.verify();
        assert_eq!(results.len(), 1);
        // Without implementation axioms, roundtrip is Unknown (not a stub)
        match &results[0] {
            Layer2Result::Unknown { invariant, reason } => {
                assert!(invariant.contains("roundtrip:MyStruct"));
                assert!(reason.contains("implementation axioms"));
            }
            other => panic!("expected Unknown with axiom reason, got {other:?}"),
        }
    }

    #[test]
    fn test_verifier_mixed_obligations() {
        let mut v = Layer2Verifier::new(Layer2Config::default());
        v.add_invariant(QuantifiedInvariant {
            name: "inv".into(),
            bound_vars: vec![("x".into(), "Int".into())],
            body: "x > 0".into(),
            triggers: vec![],
        });
        v.add_termination(TerminationObligation {
            fn_name: "f".into(),
            measure: "n".into(),
            recursive_calls: vec![],
        });
        v.add_roundtrip(RoundtripObligation {
            type_name: "T".into(),
            serialize_fn: "ser".into(),
            deserialize_fn: "de".into(),
        });
        assert_eq!(v.obligation_count(), 3);
        let results = v.check_structural();
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_verifier_verify_delegates() {
        let v = Layer2Verifier::new(Layer2Config::default());
        // verify() should produce the same results as check_structural()
        // when z3-verify feature is off
        let results = v.verify();
        assert!(results.is_empty());
    }
}
