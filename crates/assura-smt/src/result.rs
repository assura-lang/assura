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
        let r = VerificationResult::Verified {
            clause_desc: "SafeDiv: ensures".into(),
        };
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
        let r = VerificationResult::Verified {
            clause_desc: "test".into(),
        };
        let r2 = r.clone();
        assert!(format!("{r:?}") == format!("{r2:?}"));
    }
}
