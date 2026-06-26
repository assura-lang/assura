//! Shared **index access** encode policy (encode convergence step).
//!
//! Owns solver-neutral plans for `coll[idx]`. Canonical UF names / SMT-LIB2
//! (`INDEX_UF_NAME`, `index_access_smtlib`) live in [`crate::encode_atom_policy`];
//! this module does not re-export them (avoids unused-re-export noise in default
//! builds). Term construction stays backend-local (Z3 `encode_index`, CVC5
//! `encode_index_access_cvc5`).
//!
//! Complements [`crate::encode_atom_policy`] and [`crate::encode_field_policy`].

/// Solver-neutral plan for encoding `coll[idx]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(dead_code, reason = "used by cvc5_index_access (#602)")
)]
pub(crate) enum IndexAccessPlan {
    /// Apply `__index(coll, idx)` UF; optionally assert `0 <= idx < __len(coll)`.
    UfWithOptionalBounds { emit_bounds_axioms: bool },
}

/// Classify index access for backends.
///
/// - CVC5 shell SMT-LIB: UF application only (`emit_bounds_axioms = false` in text path).
/// - CVC5 native + Z3 `encode_index`: bounds axioms on `__len` (`emit_bounds_axioms = true`).
///
/// Z3 additionally links array `select` to `__index` (backend-local term shape).
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(dead_code, reason = "used by cvc5_index_access (#602)")
)]
pub(crate) fn plan_index_access(emit_bounds_axioms: bool) -> IndexAccessPlan {
    IndexAccessPlan::UfWithOptionalBounds { emit_bounds_axioms }
}

/// Canonical index UF name (delegates to atom policy).
pub(crate) fn index_uf_name() -> &'static str {
    crate::encode_atom_policy::INDEX_UF_NAME
}

/// Length UF used only for index bounds axioms (delegates to atom policy).
#[cfg_attr(
    not(any(test, feature = "cvc5-verify")),
    allow(dead_code, reason = "used by cvc5_index_access (#602)")
)]
pub(crate) fn index_bounds_len_uf_name() -> &'static str {
    crate::encode_atom_policy::INDEX_BOUNDS_LEN_UF_NAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode_atom_policy::{INDEX_BOUNDS_LEN_UF_NAME, INDEX_UF_NAME, index_access_smtlib};

    #[test]
    fn index_uf_and_plan() {
        assert_eq!(INDEX_UF_NAME, "__index");
        assert_eq!(INDEX_BOUNDS_LEN_UF_NAME, "__len");
        assert_eq!(index_uf_name(), "__index");
        assert_eq!(index_bounds_len_uf_name(), "__len");
        assert_eq!(index_access_smtlib("buf", "i"), "(__index buf i)");
        assert_eq!(
            plan_index_access(true),
            IndexAccessPlan::UfWithOptionalBounds {
                emit_bounds_axioms: true
            }
        );
        assert_eq!(
            plan_index_access(false),
            IndexAccessPlan::UfWithOptionalBounds {
                emit_bounds_axioms: false
            }
        );
    }
}
