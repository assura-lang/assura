//! CVC5 model parsing and counterexample filtering.

use crate::CounterexampleModel;

/// Parse a CVC5 model output into a CounterexampleModel.
///
/// Filters out internal encoder variables and sorts the remaining
/// user variables alphabetically (matching Z3 backend behavior).
pub(crate) fn parse_smtlib_model(model_str: &str) -> Option<CounterexampleModel> {
    let mut variables = Vec::new();
    for line in model_str.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(define-fun ") {
            let parts: Vec<&str> = trimmed
                .trim_start_matches("(define-fun ")
                .splitn(2, " () ")
                .collect();
            if parts.len() == 2 {
                let name = parts[0].to_string();
                let type_and_value = parts[1];
                if let Some(space_idx) = type_and_value.find(' ') {
                    let raw = &type_and_value[space_idx + 1..];
                    let value = raw.strip_suffix(')').unwrap_or(raw).trim().to_string();
                    if crate::encode_atom_policy::is_counterexample_user_var(&name) {
                        variables.push((name, value));
                    }
                }
            }
        }
    }
    if variables.is_empty() {
        None
    } else {
        variables.sort_by(|(a, _), (b, _)| a.cmp(b));
        Some(CounterexampleModel { variables })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_extracts_user_vars() {
        let model = "(define-fun x () Int 42)\n(define-fun __fresh_1 () Int 0)\n";
        let parsed = parse_smtlib_model(model).unwrap();
        assert_eq!(parsed.variables.len(), 1);
        assert_eq!(parsed.variables[0], ("x".into(), "42".into()));
    }

    #[test]
    fn parse_model_empty_returns_none() {
        assert!(parse_smtlib_model("").is_none());
    }
}
