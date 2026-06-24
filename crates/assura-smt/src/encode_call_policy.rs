//! Shared **call / method encode order** policy (encode convergence step 5).
//!
//! Documents and classifies the dispatch order used by Z3 `encode_call` and
//! CVC5 `encode_known_builtin_cvc5` / method UF fallbacks. Backends still build
//! solver terms locally; this module only returns a neutral [`EncodeCallKind`]
//! so Z3/CVC5/shell agree on *which* path applies before term construction.
//!
//! Complements [`crate::encode_method_policy`] (builtin tables / `is_*_builtin`)
//! and [`crate::encode_atom_policy`] (UF/name atoms).

use crate::encode_atom_policy::is_size_field_name;
use crate::encode_method_policy::{
    KnownBuiltin, is_abs_builtin, is_bool_returning_uf, is_case_fold_method, is_char_at_builtin,
    is_clear_builtin, is_clone_builtin, is_concat_append_builtin, is_drop_builtin,
    is_first_builtin, is_get_builtin, is_index_of_builtin, is_insert_builtin, is_min_max_builtin,
    is_pop_builtin, is_push_builtin, is_put_builtin, is_remove_builtin, is_replace_builtin,
    is_reverse_builtin, is_set_builtin, is_slice_builtin, is_split_builtin, is_substring_builtin,
    is_tail_builtin, is_take_builtin, is_trim_builtin,
};

/// Which encode-call / method path should apply for `func_name` at `arity`.
///
/// Order matches historical Z3 `encode_call` (min/max → bool UF → string/seq
/// builtins → abs → get/set/put → size UF → uninterpreted UF).
///
/// Used by Z3/CVC5 for `debug_assert` parity and (incrementally) `match` dispatch.
/// Term construction stays backend-local; guards still use `is_*_builtin` tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeCallKind {
    /// `min` / `max` at arity 2 (ite encoding, not free UF).
    MinMax,
    /// Bool-returning method UF (`contains`, `is_empty`, …).
    BoolReturningUf,
    /// `substring` / `substr` length axioms.
    Substring,
    /// `concat` / `append` length-sum axioms.
    ConcatAppend,
    /// `index_of` / `find` / `indexOf`.
    IndexOf,
    /// `char_at` / `charAt` / `code_unit_at`.
    CharAt,
    /// `replace` weak length axiom.
    Replace,
    /// `split` collection len >= 1.
    Split,
    /// `trim` or case-fold (`to_lower`, …) length <= input.
    TrimOrCaseFold,
    /// `clone` / `to_string` / `reverse` length-preserving.
    CloneOrReverse,
    /// `clear` length 0.
    Clear,
    /// `push` / `push_back` / `push_front` length +1.
    Push,
    /// `pop` / `tail` / `rest` length max(0, old-1).
    PopOrTail,
    /// `insert` length +1 + store axiom.
    Insert,
    /// `remove` / `remove_at` length max(0, old-1).
    Remove,
    /// `slice` length end-start.
    Slice,
    /// `take` length min(n, old).
    Take,
    /// `drop` length max(0, old-n).
    Drop,
    /// `first` / `last` / `head` / … weak fresh int.
    First,
    /// `abs` ite encoding.
    Abs,
    /// `get` unify with `__index`.
    Get,
    /// `set` store + length preserve.
    Set,
    /// `put` map store + size axioms.
    Put,
    /// Size/length method name with non-negativity axiom (arity 1+).
    SizeFieldUf,
    /// Fallback uninterpreted function (arity-dependent).
    UninterpretedUf,
}

/// Classify `func_name` at `arity` into the first matching [`EncodeCallKind`].
///
/// Callers pass the **last segment** of a dotted method name (same as Z3
/// `encode_call` / CVC5 method base name).
pub(crate) fn classify_encode_call(func_name: &str, arity: usize) -> EncodeCallKind {
    if is_min_max_builtin(func_name, arity) {
        return EncodeCallKind::MinMax;
    }
    if is_bool_returning_uf(func_name) {
        return EncodeCallKind::BoolReturningUf;
    }
    if is_substring_builtin(func_name, arity) {
        return EncodeCallKind::Substring;
    }
    if is_concat_append_builtin(func_name, arity) {
        return EncodeCallKind::ConcatAppend;
    }
    if is_index_of_builtin(func_name, arity) {
        return EncodeCallKind::IndexOf;
    }
    if is_char_at_builtin(func_name, arity) {
        return EncodeCallKind::CharAt;
    }
    if is_replace_builtin(func_name, arity) {
        return EncodeCallKind::Replace;
    }
    if is_split_builtin(func_name, arity) {
        return EncodeCallKind::Split;
    }
    if is_trim_builtin(func_name, arity) || is_case_fold_method(func_name, arity) {
        return EncodeCallKind::TrimOrCaseFold;
    }
    if is_clone_builtin(func_name, arity) || is_reverse_builtin(func_name, arity) {
        return EncodeCallKind::CloneOrReverse;
    }
    if is_clear_builtin(func_name, arity) {
        return EncodeCallKind::Clear;
    }
    if is_push_builtin(func_name, arity) {
        return EncodeCallKind::Push;
    }
    if is_pop_builtin(func_name, arity) || is_tail_builtin(func_name, arity) {
        return EncodeCallKind::PopOrTail;
    }
    if is_insert_builtin(func_name, arity) {
        return EncodeCallKind::Insert;
    }
    if is_remove_builtin(func_name, arity) {
        return EncodeCallKind::Remove;
    }
    if is_slice_builtin(func_name, arity) {
        return EncodeCallKind::Slice;
    }
    if is_take_builtin(func_name, arity) {
        return EncodeCallKind::Take;
    }
    if is_drop_builtin(func_name, arity) {
        return EncodeCallKind::Drop;
    }
    if is_first_builtin(func_name, arity) {
        return EncodeCallKind::First;
    }
    if is_abs_builtin(func_name, arity) {
        return EncodeCallKind::Abs;
    }
    if is_get_builtin(func_name, arity) {
        return EncodeCallKind::Get;
    }
    if is_set_builtin(func_name, arity) {
        return EncodeCallKind::Set;
    }
    if is_put_builtin(func_name, arity) {
        return EncodeCallKind::Put;
    }
    if is_size_field_name(func_name) && arity >= 1 {
        return EncodeCallKind::SizeFieldUf;
    }
    EncodeCallKind::UninterpretedUf
}

/// Map a classified [`KnownBuiltin`] to the matching [`EncodeCallKind`].
///
/// CVC5 `encode_known_builtin_cvc5` matches on `KnownBuiltin`; this keeps that
/// path aligned with the Z3 `encode_call` order table without duplicating guards.
/// Default (no `cvc5-verify`) builds only exercise this via unit tests.
#[cfg_attr(not(any(test, feature = "cvc5-verify")), allow(dead_code))]
#[inline]
pub(crate) fn encode_call_kind_from_known_builtin(kind: KnownBuiltin) -> EncodeCallKind {
    match kind {
        KnownBuiltin::Min | KnownBuiltin::Max => EncodeCallKind::MinMax,
        KnownBuiltin::Substring => EncodeCallKind::Substring,
        KnownBuiltin::Concat | KnownBuiltin::Append => EncodeCallKind::ConcatAppend,
        KnownBuiltin::IndexOf => EncodeCallKind::IndexOf,
        KnownBuiltin::CharAt => EncodeCallKind::CharAt,
        KnownBuiltin::Replace => EncodeCallKind::Replace,
        KnownBuiltin::Split => EncodeCallKind::Split,
        KnownBuiltin::Trim => EncodeCallKind::TrimOrCaseFold,
        KnownBuiltin::Clone | KnownBuiltin::Reverse => EncodeCallKind::CloneOrReverse,
        KnownBuiltin::Clear => EncodeCallKind::Clear,
        KnownBuiltin::Push => EncodeCallKind::Push,
        KnownBuiltin::Pop | KnownBuiltin::Tail => EncodeCallKind::PopOrTail,
        KnownBuiltin::Insert => EncodeCallKind::Insert,
        KnownBuiltin::Remove => EncodeCallKind::Remove,
        KnownBuiltin::Slice => EncodeCallKind::Slice,
        KnownBuiltin::Take => EncodeCallKind::Take,
        KnownBuiltin::Drop => EncodeCallKind::Drop,
        KnownBuiltin::First => EncodeCallKind::First,
        KnownBuiltin::Abs => EncodeCallKind::Abs,
        KnownBuiltin::Get => EncodeCallKind::Get,
        KnownBuiltin::Set => EncodeCallKind::Set,
        KnownBuiltin::Put => EncodeCallKind::Put,
    }
}

/// Debug-only check: branch guard (`expected`) agrees with [`classify_encode_call`].
///
/// Call at the entry of each encode_call arm so Z3/CVC5 cannot diverge from the
/// shared order table without a failing debug build.
#[inline]
pub(crate) fn debug_assert_encode_call_kind(
    func_name: &str,
    arity: usize,
    expected: EncodeCallKind,
) {
    debug_assert_eq!(
        classify_encode_call(func_name, arity),
        expected,
        "encode_call_policy mismatch for {func_name}/{arity}"
    );
}

/// Debug-only: `KnownBuiltin` arm agrees with [`classify_encode_call`] for `op`/`arity`.
///
/// Called from CVC5 `encode_known_builtin_cvc5` (`cvc5-verify` feature).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
#[inline]
pub(crate) fn debug_assert_known_builtin_encode_kind(op: &str, arity: usize, kind: KnownBuiltin) {
    debug_assert_encode_call_kind(op, arity, encode_call_kind_from_known_builtin(kind));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_order_min_max_before_uf() {
        assert_eq!(classify_encode_call("min", 2), EncodeCallKind::MinMax);
        assert_eq!(classify_encode_call("max", 2), EncodeCallKind::MinMax);
        assert_eq!(
            classify_encode_call("contains", 2),
            EncodeCallKind::BoolReturningUf
        );
        assert_eq!(
            classify_encode_call("is_empty", 1),
            EncodeCallKind::BoolReturningUf
        );
    }

    #[test]
    fn classify_string_and_collection_paths() {
        assert_eq!(
            classify_encode_call("substring", 3),
            EncodeCallKind::Substring
        );
        assert_eq!(
            classify_encode_call("concat", 2),
            EncodeCallKind::ConcatAppend
        );
        assert_eq!(classify_encode_call("abs", 1), EncodeCallKind::Abs);
        assert_eq!(classify_encode_call("get", 2), EncodeCallKind::Get);
        assert_eq!(classify_encode_call("set", 3), EncodeCallKind::Set);
        assert_eq!(classify_encode_call("put", 3), EncodeCallKind::Put);
        assert_eq!(classify_encode_call("push", 2), EncodeCallKind::Push);
        assert_eq!(classify_encode_call("len", 1), EncodeCallKind::SizeFieldUf);
        assert_eq!(
            classify_encode_call("unknown_f", 1),
            EncodeCallKind::UninterpretedUf
        );
    }

    #[test]
    fn classify_abs_not_shadowed_by_size_or_uf() {
        // abs is not a size field; arity 1 must hit Abs not UninterpretedUf.
        assert_eq!(classify_encode_call("abs", 1), EncodeCallKind::Abs);
        assert_ne!(
            classify_encode_call("abs", 2),
            EncodeCallKind::Abs,
            "wrong arity falls through"
        );
    }

    #[test]
    fn known_builtin_maps_to_encode_call_kind() {
        assert_eq!(
            encode_call_kind_from_known_builtin(KnownBuiltin::Min),
            EncodeCallKind::MinMax
        );
        assert_eq!(
            encode_call_kind_from_known_builtin(KnownBuiltin::Substring),
            EncodeCallKind::Substring
        );
        assert_eq!(
            encode_call_kind_from_known_builtin(KnownBuiltin::Push),
            EncodeCallKind::Push
        );
        assert_eq!(
            encode_call_kind_from_known_builtin(KnownBuiltin::Pop),
            EncodeCallKind::PopOrTail
        );
        assert_eq!(
            encode_call_kind_from_known_builtin(KnownBuiltin::Tail),
            EncodeCallKind::PopOrTail
        );
        assert_eq!(
            encode_call_kind_from_known_builtin(KnownBuiltin::Abs),
            EncodeCallKind::Abs
        );
        // Cross-check: classified op agrees with direct classify_encode_call.
        for (op, arity, kb) in [
            ("min", 2, KnownBuiltin::Min),
            ("substring", 3, KnownBuiltin::Substring),
            ("concat", 2, KnownBuiltin::Concat),
            ("push", 2, KnownBuiltin::Push),
            ("get", 2, KnownBuiltin::Get),
        ] {
            assert_eq!(
                classify_encode_call(op, arity),
                encode_call_kind_from_known_builtin(kb)
            );
        }
    }
}
