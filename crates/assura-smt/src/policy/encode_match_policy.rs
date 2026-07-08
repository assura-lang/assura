//! Shared **match / pattern** encode policy (encode convergence step).
//!
//! Owns solver-neutral decisions for `match` arms: constructor-tag ident patterns
//! (uppercase-initial names hashed via [`crate::encode_method_policy::pattern_hash_name`]),
//! arm kind classification for ite-chain planning, and SMT-LIB tag equality shapes.
//!
//! Term construction (`encode_match_smtlib` / `encode_match_cvc5` / Z3 `encode_match`)
//! stays backend-local; this module is the single source for "is this ident a ctor tag?"
//! and related arm-shape helpers.
//!
//! Complements [`crate::encode_method_policy`] (`pattern_hash_name`) and
//! [`crate::encode_atom_policy`] (`match_adt_fresh_name`, ADT UF names).

use assura_ast::{Literal, MatchArm, Pattern};

use crate::encode_method_policy::pattern_hash_name;

/// Uppercase-initial identifier patterns are enum constructor tags (hash-matched).
///
/// Lowercase / `_`-leading idents bind variables instead of testing a tag.
pub(crate) fn is_constructor_tag_pattern(name: &str) -> bool {
    name.starts_with(|c: char| c.is_uppercase())
}

/// How a single match arm participates in the right-to-left ite chain.
///
/// Backends still encode bodies/conditions; this classifies the **arm shape** only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchArmKind {
    /// `_` → body becomes the default (no condition).
    Wildcard,
    /// Lowercase / non-ctor ident: bind only, body replaces result (no ite).
    BindIdent,
    /// Uppercase ident: `ite (= scrut tag_hash) body default`.
    CtorTagIdent,
    /// Literal arm: `ite (= scrut lit) body default`.
    Literal,
    /// `Ctor(fields)`: `ite (constructor_test) body default` (+ field binds in native).
    Constructor,
    /// Tuple pattern: bind fields, body replaces result (no scrutinee test in current backends).
    Tuple,
}

/// Classify one arm's pattern for shared encode planning (Z3/CVC5 parity).
pub(crate) fn classify_match_arm(arm: &MatchArm) -> MatchArmKind {
    match &arm.pattern {
        Pattern::Wildcard => MatchArmKind::Wildcard,
        Pattern::Ident(name) if is_constructor_tag_pattern(name) => MatchArmKind::CtorTagIdent,
        Pattern::Ident(_) => MatchArmKind::BindIdent,
        Pattern::Literal(_) => MatchArmKind::Literal,
        Pattern::Constructor { .. } => MatchArmKind::Constructor,
        Pattern::Tuple(_) => MatchArmKind::Tuple,
    }
}

/// SMT-LIB2 integer atom for a constructor/tag name (FNV-1a via policy).
pub(crate) fn constructor_tag_smtlib(name: &str) -> String {
    pattern_hash_name(name).to_string()
}

/// SMT-LIB2 condition `(= scrutinee tag_hash)` for constructor-tag ident arms.
pub(crate) fn ctor_tag_eq_smtlib(scrutinee_smt: &str, ctor_name: &str) -> String {
    format!("(= {scrutinee_smt} {})", constructor_tag_smtlib(ctor_name))
}

/// SMT-LIB2 literal atom for match arm conditions (Str unsupported → `None`).
pub(crate) fn match_literal_smtlib(lit: &Literal) -> Option<String> {
    match lit {
        Literal::Int(n) => Some(n.clone()),
        Literal::Float(f) => Some(crate::encode_atom_policy::float_literal_to_smtlib(f)),
        // Bool is 0/1 Int in SMT vars (parity with IR / Z3 encode_match).
        Literal::Bool(b) => Some(if *b { "1".into() } else { "0".into() }),
        Literal::Str(_) => None,
    }
}

/// SMT-LIB2 condition `(= scrutinee lit)` for literal arms.
pub(crate) fn literal_eq_smtlib(scrutinee_smt: &str, lit: &Literal) -> Option<String> {
    let lit_smt = match_literal_smtlib(lit)?;
    Some(format!("(= {scrutinee_smt} {lit_smt})"))
}

/// Default CVC5 shell constructor test: tag equality via [`ctor_tag_eq_smtlib`].
///
/// Callers may pass a richer `constructor_test` for ADT accessors; this is the
/// baseline used when only the ctor name is known. Kept public for backends that
/// do not have an ADT registry yet (and unit tests).
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "baseline constructor_test for future/non-ADT callers"
    )
)]
pub(crate) fn default_constructor_test_smtlib(ctor_name: &str, scrutinee_smt: &str) -> String {
    ctor_tag_eq_smtlib(scrutinee_smt, ctor_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::{Expr, Spanned};

    fn arm(pat: Pattern) -> MatchArm {
        MatchArm {
            pattern: pat,
            body: Spanned::no_span(Expr::Ident("y".into())),
        }
    }

    #[test]
    fn constructor_tag_detects_uppercase() {
        assert!(is_constructor_tag_pattern("Some"));
        assert!(is_constructor_tag_pattern("None"));
        assert!(!is_constructor_tag_pattern("x"));
        assert!(!is_constructor_tag_pattern("_tmp"));
    }

    #[test]
    fn classify_arm_kinds() {
        assert_eq!(
            classify_match_arm(&arm(Pattern::Wildcard)),
            MatchArmKind::Wildcard
        );
        assert_eq!(
            classify_match_arm(&arm(Pattern::Ident("x".into()))),
            MatchArmKind::BindIdent
        );
        assert_eq!(
            classify_match_arm(&arm(Pattern::Ident("Ok".into()))),
            MatchArmKind::CtorTagIdent
        );
        assert_eq!(
            classify_match_arm(&arm(Pattern::Literal(Literal::Int("1".into())))),
            MatchArmKind::Literal
        );
        assert_eq!(
            classify_match_arm(&arm(Pattern::Constructor {
                name: "Some".into(),
                fields: vec![],
            })),
            MatchArmKind::Constructor
        );
        assert_eq!(
            classify_match_arm(&arm(Pattern::Tuple(vec![Pattern::Wildcard]))),
            MatchArmKind::Tuple
        );
    }

    #[test]
    fn ctor_tag_eq_uses_pattern_hash() {
        let hash = pattern_hash_name("Some");
        assert_eq!(ctor_tag_eq_smtlib("s", "Some"), format!("(= s {hash})"));
        assert_eq!(
            default_constructor_test_smtlib("Some", "s"),
            ctor_tag_eq_smtlib("s", "Some")
        );
    }

    #[test]
    fn match_literal_smtlib_shapes() {
        assert_eq!(
            match_literal_smtlib(&Literal::Int("42".into())).as_deref(),
            Some("42")
        );
        assert_eq!(
            match_literal_smtlib(&Literal::Bool(true)).as_deref(),
            Some("1")
        );
        assert_eq!(
            match_literal_smtlib(&Literal::Bool(false)).as_deref(),
            Some("0")
        );
        assert!(match_literal_smtlib(&Literal::Str("hi".into())).is_none());
        assert_eq!(
            literal_eq_smtlib("x", &Literal::Int("0".into())).as_deref(),
            Some("(= x 0)")
        );
    }
}
