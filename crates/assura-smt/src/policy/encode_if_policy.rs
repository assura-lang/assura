//! Shared **if / then / else** encode policy (encode convergence step).
//!
//! Owns solver-neutral SMT-LIB2 shapes (`ite` vs implication) and a small
//! [`IfEncodePlan`] so Z3, CVC5 shell, and CVC5 native agree on *which* form
//! applies before term construction. Branch term/sort promotion stays
//! backend-local (Z3 `ite` on values; CVC5 `ToReal` on mixed Int/Real).

/// Which SMT form to emit for `if cond then t [else e]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IfEncodePlan {
    /// Full if-then-else: SMT-LIB `(ite cond then else)`.
    Ite,
    /// Missing else branch: treat as implication `(=> cond then)` (shell/native
    /// CVC5) or encode only then-branch under cond in Z3 (backend-specific).
    ImpliesThenOnly,
}

/// Classify whether the if-expression has an else branch.
pub(crate) fn plan_if_encode(has_else: bool) -> IfEncodePlan {
    if has_else {
        IfEncodePlan::Ite
    } else {
        IfEncodePlan::ImpliesThenOnly
    }
}

/// Encode `if cond then t [else e]` as SMT-LIB2 (`ite` or implication).
pub(crate) fn encode_if_smtlib(cond: &str, then_branch: &str, else_branch: Option<&str>) -> String {
    match plan_if_encode(else_branch.is_some()) {
        IfEncodePlan::Ite => {
            let e = else_branch.expect("Ite plan requires else_branch");
            format!("(ite {cond} {then_branch} {e})")
        }
        IfEncodePlan::ImpliesThenOnly => format!("(=> {cond} {then_branch})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_plans_and_smtlib_shapes() {
        assert_eq!(plan_if_encode(true), IfEncodePlan::Ite);
        assert_eq!(plan_if_encode(false), IfEncodePlan::ImpliesThenOnly);
        assert_eq!(encode_if_smtlib("c", "t", Some("e")), "(ite c t e)");
        assert_eq!(encode_if_smtlib("c", "t", None), "(=> c t)");
    }
}
