//! Verification result types for the SMT verification pipeline.
//!
//! Contains `VerificationResult` (the main result enum) and
//! `CounterexampleModel` (structured counterexample extraction).

/// Structured counterexample model extracted from Z3.
#[derive(Debug, Clone)]
pub struct CounterexampleModel {
    /// Variable name/value pairs from the Z3 model.
    pub variables: Vec<(String, String)>,
}

impl CounterexampleModel {
    /// Produce a JSON string: `{"variables": {"x": "0", "b": "-1"}}`.
    pub fn to_json(&self) -> String {
        let mut buf = String::from("{\"variables\": {");
        for (i, (name, value)) in self.variables.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            // Escape any quotes in name/value for valid JSON
            buf.push('"');
            buf.push_str(&name.replace('\\', "\\\\").replace('"', "\\\""));
            buf.push_str("\": \"");
            buf.push_str(&value.replace('\\', "\\\\").replace('"', "\\\""));
            buf.push('"');
        }
        buf.push_str("}}");
        buf
    }
}

/// The result of verifying a single contract clause.
#[derive(Debug, Clone)]
pub enum VerificationResult {
    /// The clause was proven valid.
    Verified {
        /// Human-readable description of what was verified.
        clause_desc: String,
        /// Labels of tracked assumptions in the unsat core (requires clauses
        /// that were necessary to prove validity), when available.
        unsat_core: Option<Vec<String>>,
    },
    /// A counterexample was found (the clause does not hold).
    Counterexample {
        /// Human-readable description of the clause.
        clause_desc: String,
        /// Z3 model showing the counterexample (raw string).
        model: String,
        /// Structured counterexample with parsed variable values.
        counter_model: Option<CounterexampleModel>,
    },
    /// The solver timed out before reaching a conclusion.
    Timeout {
        /// Human-readable description of the clause.
        clause_desc: String,
    },
    /// The solver returned Unknown (e.g., non-linear arithmetic).
    Unknown {
        /// Human-readable description of the clause.
        clause_desc: String,
        /// Reason the solver could not decide.
        reason: String,
    },
}

/// Substring in [`VerificationResult::Unknown`] reasons that means a known
/// compiler limitation (CLI treats as warning / exit 0), not a solver failure.
pub const KNOWN_SMT_LIMITATION_MARKER: &str = "not yet encoded in SMT";

/// True when an `Unknown` reason is a known unimplemented encoding path.
///
/// Use this in CLI, MCP, and tests instead of open-coding the substring so
/// agents do not invent slightly different markers (e.g. "not encoded yet").
pub fn is_known_smt_limitation(reason: &str) -> bool {
    reason.contains(KNOWN_SMT_LIMITATION_MARKER)
}

/// Build a canonical "not yet encoded" reason string (CLI warning severity).
pub fn not_encoded_reason(detail: impl AsRef<str>) -> String {
    let d = detail.as_ref();
    if d.is_empty() {
        KNOWN_SMT_LIMITATION_MARKER.to_string()
    } else if d.contains(KNOWN_SMT_LIMITATION_MARKER) {
        d.to_string()
    } else {
        format!("{d} {KNOWN_SMT_LIMITATION_MARKER}")
    }
}

impl VerificationResult {
    /// Build a verified result without an unsat core.
    pub fn verified(clause_desc: impl Into<String>) -> Self {
        Self::Verified {
            clause_desc: clause_desc.into(),
            unsat_core: None,
        }
    }

    /// Known compiler limitation (not a solver failure). Reason always includes
    /// [`KNOWN_SMT_LIMITATION_MARKER`] so CLI/MCP treat it as a warning.
    pub fn unknown_not_encoded(
        clause_desc: impl Into<String>,
        feature_or_detail: impl AsRef<str>,
    ) -> Self {
        Self::Unknown {
            clause_desc: clause_desc.into(),
            reason: not_encoded_reason(feature_or_detail),
        }
    }

    /// Whether this result is a known encoding gap (warning) rather than solver Unknown (error).
    pub fn is_known_limitation(&self) -> bool {
        match self {
            Self::Unknown { reason, .. } => is_known_smt_limitation(reason),
            _ => false,
        }
    }

    /// Build a verified result with an extracted unsat core.
    pub fn verified_with_core(clause_desc: impl Into<String>, unsat_core: Vec<String>) -> Self {
        Self::Verified {
            clause_desc: clause_desc.into(),
            unsat_core: if unsat_core.is_empty() {
                None
            } else {
                Some(unsat_core)
            },
        }
    }

    /// Human-readable clause description from any result variant.
    pub fn clause_desc(&self) -> &str {
        match self {
            Self::Verified { clause_desc, .. }
            | Self::Counterexample { clause_desc, .. }
            | Self::Timeout { clause_desc }
            | Self::Unknown { clause_desc, .. } => clause_desc,
        }
    }

    /// Contract/declaration name prefix (`"Foo::ensures"` → `"Foo"`).
    pub fn contract_name(&self) -> &str {
        self.clause_desc().split("::").next().unwrap_or("")
    }

    /// Rich JSON for CLI `--json` output (includes unsat cores and structured CEX).
    pub fn to_json_value(&self) -> serde_json::Value {
        match self {
            Self::Verified {
                clause_desc,
                unsat_core,
            } => {
                let mut val = serde_json::json!({
                    "status": "verified",
                    "clause": clause_desc,
                });
                if let Some(core) = unsat_core {
                    val["unsat_core"] = serde_json::json!(core);
                }
                val
            }
            Self::Counterexample {
                clause_desc,
                model,
                counter_model,
            } => {
                let mut val = serde_json::json!({
                    "status": "counterexample",
                    "clause": clause_desc,
                    "model": model,
                });
                if let Some(cm) = counter_model {
                    let vars: serde_json::Map<String, serde_json::Value> = cm
                        .variables
                        .iter()
                        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                        .collect();
                    val["variables"] = serde_json::Value::Object(vars);
                }
                val
            }
            Self::Timeout { clause_desc } => serde_json::json!({
                "status": "timeout",
                "clause": clause_desc,
            }),
            Self::Unknown {
                clause_desc,
                reason,
            } => serde_json::json!({
                "status": "unknown",
                "clause": clause_desc,
                "reason": reason,
            }),
        }
    }

    /// Status string for gRPC responses.
    pub fn grpc_status(&self) -> String {
        match self {
            Self::Verified { .. } => "verified".into(),
            Self::Counterexample { .. } => "counterexample".into(),
            Self::Timeout { .. } => "timeout".into(),
            Self::Unknown { reason, .. } => format!("unknown: {reason}"),
        }
    }

    /// Counterexample model text for gRPC responses.
    pub fn grpc_counterexample(&self) -> String {
        match self {
            Self::Counterexample { model, .. } => model.clone(),
            _ => String::new(),
        }
    }
}

/// JSON-serializable summary of a verification result (MCP, server, pipeline).
#[derive(Debug, Clone, serde::Serialize)]
pub struct VerificationSummary {
    pub status: String,
    pub clause: String,
    pub model: Option<String>,
    pub reason: Option<String>,
}

impl From<&VerificationResult> for VerificationSummary {
    fn from(r: &VerificationResult) -> Self {
        match r {
            VerificationResult::Verified { clause_desc, .. } => Self {
                status: "verified".into(),
                clause: clause_desc.clone(),
                model: None,
                reason: None,
            },
            VerificationResult::Counterexample {
                clause_desc, model, ..
            } => Self {
                status: "counterexample".into(),
                clause: clause_desc.clone(),
                model: Some(model.clone()),
                reason: None,
            },
            VerificationResult::Timeout { clause_desc } => Self {
                status: "timeout".into(),
                clause: clause_desc.clone(),
                model: None,
                reason: None,
            },
            VerificationResult::Unknown {
                clause_desc,
                reason,
            } => Self {
                status: "unknown".into(),
                clause: clause_desc.clone(),
                model: None,
                reason: Some(reason.clone()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counterexample_model_to_json_empty() {
        let m = CounterexampleModel { variables: vec![] };
        assert_eq!(m.to_json(), "{\"variables\": {}}");
    }

    #[test]
    fn counterexample_model_to_json_single() {
        let m = CounterexampleModel {
            variables: vec![("x".into(), "0".into())],
        };
        assert_eq!(m.to_json(), "{\"variables\": {\"x\": \"0\"}}");
    }

    #[test]
    fn counterexample_model_to_json_multiple() {
        let m = CounterexampleModel {
            variables: vec![("x".into(), "42".into()), ("y".into(), "-1".into())],
        };
        let json = m.to_json();
        assert!(json.contains("\"x\": \"42\""));
        assert!(json.contains("\"y\": \"-1\""));
        assert!(json.starts_with("{\"variables\": {"));
        assert!(json.ends_with("}}"));
    }

    #[test]
    fn counterexample_model_escapes_quotes() {
        let m = CounterexampleModel {
            variables: vec![("name".into(), "he said \"hello\"".into())],
        };
        let json = m.to_json();
        assert!(json.contains("\\\"hello\\\""));
    }

    #[test]
    fn counterexample_model_escapes_backslash() {
        let m = CounterexampleModel {
            variables: vec![("path".into(), "a\\b".into())],
        };
        let json = m.to_json();
        assert!(json.contains("a\\\\b"));
    }

    #[test]
    fn verification_result_debug_verified() {
        let r = VerificationResult::verified("SafeDiv: ensures");
        let debug = format!("{r:?}");
        assert!(debug.contains("Verified"));
        assert!(debug.contains("SafeDiv"));
    }

    #[test]
    fn verification_result_debug_counterexample() {
        let r = VerificationResult::Counterexample {
            clause_desc: "SafeDiv: ensures".into(),
            model: "x = 0, b = 0".into(),
            counter_model: Some(CounterexampleModel {
                variables: vec![("b".into(), "0".into())],
            }),
        };
        let debug = format!("{r:?}");
        assert!(debug.contains("Counterexample"));
    }

    #[test]
    fn verification_result_debug_timeout() {
        let r = VerificationResult::Timeout {
            clause_desc: "complex: ensures".into(),
        };
        let debug = format!("{r:?}");
        assert!(debug.contains("Timeout"));
    }

    #[test]
    fn verification_result_debug_unknown() {
        let r = VerificationResult::Unknown {
            clause_desc: "nonlinear: ensures".into(),
            reason: "non-linear arithmetic".into(),
        };
        let debug = format!("{r:?}");
        assert!(debug.contains("Unknown"));
        assert!(debug.contains("non-linear"));
    }

    #[test]
    fn verification_result_clone() {
        let r = VerificationResult::verified("test");
        let r2 = r.clone();
        assert!(format!("{r:?}") == format!("{r2:?}"));
    }
}
