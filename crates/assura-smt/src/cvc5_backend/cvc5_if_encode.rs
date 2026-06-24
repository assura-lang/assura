//! CVC5 if-then-else term builders (shell SMT-LIB re-exports policy; native ite).
//!
//! SMT-LIB shape / plan lives in [`crate::encode_if_policy`]; this module owns
//! CVC5-native `Ite` / `Implies` with Int/Real promotion on branches.

#[allow(
    unused_imports,
    reason = "re-export surface; cvc5_expr_smtlib prefers encode_if_policy directly"
)]
pub(crate) use crate::encode_if_policy::{IfEncodePlan, encode_if_smtlib, plan_if_encode};

/// Encode `if cond then t [else e]` as a native CVC5 term (with Real/Int promotion on branches).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_if_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    cond: cvc5::Term<'a>,
    then_branch: cvc5::Term<'a>,
    else_branch: Option<cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    use crate::encode_if_policy::{IfEncodePlan, plan_if_encode};

    match plan_if_encode(else_branch.is_some()) {
        IfEncodePlan::Ite => {
            let e = else_branch.expect("Ite plan requires else_branch");
            let (t_final, e_final) = if then_branch.sort().is_real() && e.sort().is_integer() {
                (then_branch, tm.mk_term(cvc5::Kind::ToReal, &[e]))
            } else if then_branch.sort().is_integer() && e.sort().is_real() {
                (tm.mk_term(cvc5::Kind::ToReal, &[then_branch]), e)
            } else {
                (then_branch, e)
            };
            tm.mk_term(cvc5::Kind::Ite, &[cond, t_final, e_final])
        }
        IfEncodePlan::ImpliesThenOnly => tm.mk_term(cvc5::Kind::Implies, &[cond, then_branch]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_with_else_uses_ite() {
        assert_eq!(encode_if_smtlib("c", "t", Some("e")), "(ite c t e)");
        assert_eq!(plan_if_encode(true), IfEncodePlan::Ite);
    }

    #[test]
    fn if_without_else_uses_implies() {
        assert_eq!(encode_if_smtlib("c", "t", None), "(=> c t)");
        assert_eq!(plan_if_encode(false), IfEncodePlan::ImpliesThenOnly);
    }
}
