//! Shared **call / method encode order** policy (encode convergence step 5).
//!
//! Documents and classifies the dispatch order used by:
//! - Z3 `Encoder::encode_call` (`call_kind = classify_encode_call`, then `matches!` arms)
//! - CVC5 `encode_known_builtin_cvc5` (`KnownBuiltin` match + [`debug_assert_known_builtin_encode_kind`])
//! - CVC5 `encode_uf_call_cvc5` (post-builtin UF / size / bool path via `call_kind`)
//! - CVC5 shell `encode_call_smtlib` / `encode_method_call_smtlib` (known builtin text +
//!   fallthrough `classify_encode_call` for size/bool/UF)
//!
//! Backends still build solver terms locally; this module only returns a neutral
//! [`EncodeCallKind`] so Z3/CVC5/shell agree on *which* path applies before term
//! construction.
//!
//! Complements [`crate::encode_method_policy`] (builtin tables / `is_*_builtin`)
//! and [`crate::encode_atom_policy`] (UF/name atoms).

use crate::encode_atom_policy::{is_length_method_name, is_size_field_name};
use crate::encode_method_policy::{
    KnownBuiltin, classify_known_builtin, is_abs_builtin, is_bool_returning_uf,
    is_case_fold_method, is_char_at_builtin, is_clear_builtin, is_clone_builtin,
    is_concat_append_builtin, is_drop_builtin, is_first_builtin, is_get_builtin,
    is_index_of_builtin, is_insert_builtin, is_min_max_builtin, is_pop_builtin, is_push_builtin,
    is_put_builtin, is_remove_builtin, is_replace_builtin, is_reverse_builtin, is_set_builtin,
    is_slice_builtin, is_split_builtin, is_substring_builtin, is_tail_builtin, is_take_builtin,
    is_trim_builtin,
};

/// Fast paths handled **before** integer-arg encoding / [`classify_encode_call`].
///
/// Z3 `encode_call` and CVC5 call/method entry points share these categories;
/// term construction (ADT ctor, `str.len`, canonical length var) stays backend-local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EncodeCallPreamble {
    /// Uppercase-leading name may be an ADT constructor (`Some`, `Ok`, …).
    PossibleAdtConstructor,
    /// `length` / `len` / … at arity 1 (string theory or canonical-ident fast path).
    LengthMethodArity1,
    /// No preamble fast path; continue with normal encode/classify.
    None,
}

/// Classify pre-`arg_vals` / pre-`encode_known_builtin` fast paths.
///
/// `is_uppercase_ident` is true when the first character of the call target is
/// uppercase (Z3/CVC5 constructor heuristic).
pub(crate) fn classify_encode_call_preamble(
    func_name: &str,
    arity: usize,
    is_uppercase_ident: bool,
) -> EncodeCallPreamble {
    if is_uppercase_ident {
        return EncodeCallPreamble::PossibleAdtConstructor;
    }
    if is_length_method_name(func_name) && arity == 1 {
        return EncodeCallPreamble::LengthMethodArity1;
    }
    EncodeCallPreamble::None
}

/// Method-call length fast path (`receiver.length()` / `.len()` with no extra args).
pub(crate) fn is_receiver_length_method(method: &str, extra_arg_count: usize) -> bool {
    is_length_method_name(method) && extra_arg_count == 0
}

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
    /// `clamp(x, lo, hi)` nested min/max ite.
    Clamp,
    /// `signum(x)` clamp to [-1, 1].
    Signum,
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
    if matches!(
        classify_known_builtin(func_name, arity),
        Some(KnownBuiltin::Clamp)
    ) {
        return EncodeCallKind::Clamp;
    }
    if matches!(
        classify_known_builtin(func_name, arity),
        Some(KnownBuiltin::Signum)
    ) {
        return EncodeCallKind::Signum;
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
/// Used by CVC5 known-builtin encode and shell `encode_call_smtlib` parity checks.
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
        KnownBuiltin::Clamp => EncodeCallKind::Clamp,
        KnownBuiltin::Signum => EncodeCallKind::Signum,
        KnownBuiltin::Get => EncodeCallKind::Get,
        KnownBuiltin::Set => EncodeCallKind::Set,
        KnownBuiltin::Put => EncodeCallKind::Put,
    }
}

/// Which specialized axiom pattern a [`BoolReturningUf`](EncodeCallKind::BoolReturningUf) call gets.
///
/// All `BoolReturningUf` calls share Bool sort, but some get additional length/size
/// axioms. This sub-classification avoids hardcoded `func_name == "is_empty"` checks
/// in every backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoolCallAxiom {
    /// `is_empty(x) <=> len(x) == 0` (bidirectional).
    IsEmpty,
    /// `contains(s, sub) => len(s) >= len(sub)`.
    Contains,
    /// `starts_with` / `ends_with`: `len(s) >= len(affix)`, plus empty affix always true.
    AffixPredicate,
    /// `contains_key(m, k) => size(m) >= 1`.
    ContainsKey,
    /// No specialized axioms (generic bool UF).
    Generic,
}

/// Classify a `BoolReturningUf` call into its specialized axiom pattern.
///
/// Only meaningful when [`classify_encode_call`] returns [`EncodeCallKind::BoolReturningUf`].
/// Backends use this to select the right axiom set without hardcoding function name strings.
pub(crate) fn classify_bool_call_axiom(func_name: &str, arity: usize) -> BoolCallAxiom {
    if func_name == "is_empty" && arity == 1 {
        return BoolCallAxiom::IsEmpty;
    }
    if func_name == "contains" && arity == 2 {
        return BoolCallAxiom::Contains;
    }
    if matches!(func_name, "starts_with" | "ends_with") && arity == 2 {
        return BoolCallAxiom::AffixPredicate;
    }
    if func_name == "contains_key" && arity == 2 {
        return BoolCallAxiom::ContainsKey;
    }
    BoolCallAxiom::Generic
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
    fn classify_encode_call_preamble_arms() {
        assert_eq!(
            classify_encode_call_preamble("Some", 1, true),
            EncodeCallPreamble::PossibleAdtConstructor
        );
        assert_eq!(
            classify_encode_call_preamble("length", 1, false),
            EncodeCallPreamble::LengthMethodArity1
        );
        assert_eq!(
            classify_encode_call_preamble("len", 1, false),
            EncodeCallPreamble::LengthMethodArity1
        );
        assert_eq!(
            classify_encode_call_preamble("min", 2, false),
            EncodeCallPreamble::None
        );
        assert!(is_receiver_length_method("length", 0));
        assert!(is_receiver_length_method("len", 0));
        assert!(!is_receiver_length_method("length", 1));
        assert!(!is_receiver_length_method("push", 0));
    }

    #[test]
    fn size_field_call_kind_matches_field_value_kind() {
        // Call-path SizeFieldUf and field-path SizeNonNeg share the same name tables.
        use crate::encode_field_policy::{FieldValueKind, classify_field_value_kind};
        for name in ["len", "length", "size", "capacity", "count"] {
            assert_eq!(
                classify_encode_call(name, 1),
                EncodeCallKind::SizeFieldUf,
                "{name}"
            );
            assert_eq!(
                classify_field_value_kind(name),
                FieldValueKind::SizeNonNeg,
                "{name}"
            );
        }
    }

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

    #[test]
    fn classify_bool_call_axiom_specialized() {
        assert_eq!(
            classify_bool_call_axiom("is_empty", 1),
            BoolCallAxiom::IsEmpty
        );
        assert_eq!(
            classify_bool_call_axiom("contains", 2),
            BoolCallAxiom::Contains
        );
        assert_eq!(
            classify_bool_call_axiom("starts_with", 2),
            BoolCallAxiom::AffixPredicate
        );
        assert_eq!(
            classify_bool_call_axiom("ends_with", 2),
            BoolCallAxiom::AffixPredicate
        );
        assert_eq!(
            classify_bool_call_axiom("contains_key", 2),
            BoolCallAxiom::ContainsKey
        );
        // Wrong arity falls to Generic.
        assert_eq!(
            classify_bool_call_axiom("is_empty", 2),
            BoolCallAxiom::Generic
        );
        assert_eq!(
            classify_bool_call_axiom("contains", 1),
            BoolCallAxiom::Generic
        );
        // Non-specialized bool UF.
        assert_eq!(
            classify_bool_call_axiom("is_valid", 1),
            BoolCallAxiom::Generic
        );
    }

    #[test]
    fn bool_call_axiom_implies_bool_returning_uf_kind() {
        // Every specialized BoolCallAxiom name must classify as BoolReturningUf.
        for (name, arity) in [
            ("is_empty", 1),
            ("contains", 2),
            ("starts_with", 2),
            ("ends_with", 2),
            ("contains_key", 2),
        ] {
            assert_eq!(
                classify_encode_call(name, arity),
                EncodeCallKind::BoolReturningUf,
                "{name}/{arity} must be BoolReturningUf"
            );
            assert_ne!(
                classify_bool_call_axiom(name, arity),
                BoolCallAxiom::Generic,
                "{name}/{arity} must have specialized axiom"
            );
        }
    }
}
