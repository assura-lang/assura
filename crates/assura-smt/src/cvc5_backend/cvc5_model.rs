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
            let rest = trimmed.trim_start_matches("(define-fun ");
            let Some((name, after)) = rest.split_once(" () ") else {
                continue;
            };
            let after = after.strip_suffix(')').unwrap_or(after);
            let value = if let Some(rest) = after.trim().strip_prefix("(_ BitVec") {
                let Some(close) = rest.find(')') else {
                    continue;
                };
                let raw = rest[close + 1..].trim();
                display_bitvector(raw).unwrap_or_else(|| raw.to_string())
            } else if let Some(space_idx) = after.find(' ') {
                after[space_idx + 1..].trim().to_string()
            } else {
                continue;
            };
            if crate::encode_atom_policy::is_requires_track_noise(name, &value) {
                continue;
            }
            if crate::encode_atom_policy::is_counterexample_user_var(name) {
                let clean =
                    crate::encode_atom_policy::counterexample_display_name(name).to_string();
                variables.push((clean, value));
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

fn display_bitvector(raw: &str) -> Option<String> {
    let (bits, width) = bitvector_magnitude(raw)?;
    if width == 0 || width > 64 {
        return Some(bits.to_string());
    }
    let mask = (1u128 << width) - 1;
    let bits = bits & mask;
    let signed = if bits & (1u128 << (width - 1)) != 0 {
        (bits as i128) - (1i128 << width)
    } else {
        bits as i128
    };
    Some(signed.to_string())
}

fn bitvector_magnitude(raw: &str) -> Option<(u128, u32)> {
    if let Some(rest) = raw.strip_prefix("#b") {
        let digits: String = rest
            .chars()
            .take_while(|ch| *ch == '0' || *ch == '1')
            .collect();
        return Some((u128::from_str_radix(&digits, 2).ok()?, digits.len() as u32));
    }
    if let Some(rest) = raw.strip_prefix("#x") {
        let digits: String = rest
            .chars()
            .take_while(|ch| ch.is_ascii_hexdigit())
            .collect();
        return Some((
            u128::from_str_radix(&digits, 16).ok()?,
            (digits.len() as u32).saturating_mul(4),
        ));
    }
    let rest = raw.strip_prefix("(_ bv")?;
    let number: String = rest
        .chars()
        .skip_while(|ch| ch.is_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    Some((number.parse().ok()?, 64))
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
