//! Shared if-then-else encoding for CVC5 shell-out and native backends.

/// Encode `if cond then t [else e]` as SMT-LIB2 (`ite` or implication).
pub(crate) fn encode_if_smtlib(cond: &str, then_branch: &str, else_branch: Option<&str>) -> String {
    if let Some(e) = else_branch {
        format!("(ite {cond} {then_branch} {e})")
    } else {
        format!("(=> {cond} {then_branch})")
    }
}

/// Encode `if cond then t [else e]` as a native CVC5 term (with Real/Int promotion on branches).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_if_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    cond: cvc5::Term<'a>,
    then_branch: cvc5::Term<'a>,
    else_branch: Option<cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    if let Some(e) = else_branch {
        let (t_final, e_final) = if then_branch.sort().is_real() && e.sort().is_integer() {
            (then_branch, tm.mk_term(cvc5::Kind::ToReal, &[e]))
        } else if then_branch.sort().is_integer() && e.sort().is_real() {
            (tm.mk_term(cvc5::Kind::ToReal, &[then_branch]), e)
        } else {
            (then_branch, e)
        };
        tm.mk_term(cvc5::Kind::Ite, &[cond, t_final, e_final])
    } else {
        tm.mk_term(cvc5::Kind::Implies, &[cond, then_branch])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_with_else_uses_ite() {
        assert_eq!(encode_if_smtlib("c", "t", Some("e")), "(ite c t e)");
    }

    #[test]
    fn if_without_else_uses_implies() {
        assert_eq!(encode_if_smtlib("c", "t", None), "(=> c t)");
    }
}
