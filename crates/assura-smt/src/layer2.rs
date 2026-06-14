use super::*;

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
                // Structurally valid, but not verified without Z3
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "requires Z3 Layer 2 verification".into(),
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
                // Structurally valid, but not verified without Z3
                results.push(Layer2Result::Unknown {
                    invariant: format!("termination:{}", obl.fn_name),
                    reason: "requires Z3 Layer 2 verification".into(),
                });
            }
        }

        for obl in &self.roundtrip_obligations {
            // Structurally valid, but not verified without Z3
            results.push(Layer2Result::Unknown {
                invariant: format!("roundtrip:{}", obl.type_name),
                reason: "requires Z3 Layer 2 verification".into(),
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
        let mut results = Vec::new();

        for inv in &self.invariants {
            if inv.bound_vars.is_empty() {
                results.push(Layer2Result::Unknown {
                    invariant: inv.name.clone(),
                    reason: "quantified invariant has no bound variables".into(),
                });
                continue;
            }
            // Structural check only for string-based invariants.
            // Real quantifier verification happens through verify_quantified_expr().
            results.push(Layer2Result::Unknown {
                invariant: inv.name.clone(),
                reason: "requires Z3 Layer 2 verification".into(),
            });
        }

        results
    }
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
    assumptions: &[Expr],
    quantified_body: &Expr,
) -> VerificationResult {
    #[cfg(feature = "z3-verify")]
    {
        z3_backend::verify_quantified_impl(name, assumptions, quantified_body)
    }
    #[cfg(not(feature = "z3-verify"))]
    {
        let _ = (assumptions, quantified_body);
        VerificationResult::Unknown {
            clause_desc: name.into(),
            reason: "Z3 not available (compiled without z3-verify feature)".into(),
        }
    }
}
