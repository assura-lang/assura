//! Shared **atom / naming** conventions for expression encode (encode convergence groundwork).
//!
//! Owns solver-neutral identifiers and SMT-LIB atom shapes that Z3, CVC5 native, and
//! CVC5 shell must agree on (`result` → `__result`, dotted idents, int/float/str/apply
//! atoms, standard `BinOp` SMT-LIB operators). This is **not** full expr-encode
//! unification: term construction (`Encoder` / `encode_expr_cvc5`) remains backend-local.
//!
//! Complements [`crate::ir_lower::IrTermBuilder`] (IR terms) and
//! [`crate::unmodelable`] (field-chain flatten names).

use assura_ast::{BinOp, Literal};

/// SMT variable for contract `result` / return value (Z3 and CVC5 agree on this spelling).
pub(crate) const RESULT_VAR_NAME: &str = "__result";

/// Rational denominator for Float → rational encoding (matches historical Z3/CVC5).
pub(crate) const FLOAT_RATIONAL_DENOM: i64 = 1_000_000;

/// Sanitize an Assura identifier for SMT-LIB2 / solver constant names (`.` → `_`).
pub(crate) fn sanitize_smt_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Append a dotted raw-token segment (`tok . segment`) using one `_` separator.
pub(crate) fn append_raw_dotted_segment(base: &mut String, segment: &str) {
    base.push('_');
    base.push_str(&sanitize_smt_name(segment));
}

/// Map source `result` ident to [`RESULT_VAR_NAME`]; other idents are sanitized only.
pub(crate) fn encode_ident_name(name: &str) -> String {
    if name == "result" {
        RESULT_VAR_NAME.to_string()
    } else {
        sanitize_smt_name(name)
    }
}

/// SMT-LIB / solver name for `old(x)` snapshots.
///
/// Result-aware: `result` → `__result__old` (matches [`encode_ident_name`]).
/// Prefer this for CVC5 shell/native paths that already bind `result` as
/// [`RESULT_VAR_NAME`].
pub(crate) fn old_ident_name(name: &str) -> String {
    if name == "result" {
        format!("{RESULT_VAR_NAME}__old")
    } else {
        format!("{}__old", sanitize_smt_name(name))
    }
}

/// `old(x)` snapshot when the live variable is stored under the **source** name
/// (e.g. Z3 still binds `result` as `result`, not [`RESULT_VAR_NAME`]).
///
/// Does **not** special-case `result`; use [`old_ident_name`] when live names
/// go through [`encode_ident_name`].
///
/// Safe for already-sanitized flat names (`a_b` stays `a_b__old`).
pub(crate) fn old_snapshot_name(name: &str) -> String {
    format!("{}__old", sanitize_smt_name(name))
}

/// Fresh temporary for complex `old(expr)` that cannot rename in place.
///
/// Referenced from CVC5 raw-native (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) fn old_fresh_temp_name(counter: impl std::fmt::Display) -> String {
    format!("__old_fresh_{counter}")
}

/// Canonical length binding name (`__canonical_len_{sanitized}`).
pub(crate) fn canonical_length_name(name: &str) -> String {
    format!("__canonical_len_{}", sanitize_smt_name(name))
}

/// String literal constant name (`__str_{sanitized content}`).
pub(crate) fn string_literal_const_name(s: &str) -> String {
    format!("__str_{}", sanitize_smt_name(s))
}

/// Lemma `apply` boolean constant name (`__apply_{lemma}`).
pub(crate) fn apply_lemma_const_name(lemma_name: &str) -> String {
    format!("__apply_{lemma_name}")
}

/// Uninterpreted field/property accessor UIF name (`__field_{field}`).
pub(crate) fn field_uif_name(field: &str) -> String {
    format!("__field_{field}")
}

/// Length accessor UIF shared by `.len` / `.length` / string field access (`__field_len`).
///
/// Must stay aligned with [`field_uif_name`]`("len")`.
pub(crate) const FIELD_LEN_UF_NAME: &str = "__field_len";

/// Alternate length UIF spelling used in some collection axioms (`len` without `__field_`).
pub(crate) const LEN_UF_NAME: &str = "len";

/// Collection/map element accessor UIF (`get(coll, key)`), distinct from [`LIST_GET_UF_NAME`].
pub(crate) const GET_UF_NAME: &str = "get";

/// Collection/map size UIF (`size(coll)`), often linked to [`LEN_UF_NAME`] via axioms.
pub(crate) const SIZE_UF_NAME: &str = "size";

/// Source method name `length` (aliases to [`LEN_UF_NAME`] / [`FIELD_LEN_UF_NAME`] at encode time).
pub(crate) const LENGTH_METHOD_NAME: &str = "length";

/// Whether `uf` is either [`LEN_UF_NAME`] or [`FIELD_LEN_UF_NAME`].
pub(crate) fn is_length_uf_name(uf: &str) -> bool {
    uf == LEN_UF_NAME || uf == FIELD_LEN_UF_NAME
}

/// Both length UIF spellings (order: `len`, then `__field_len`).
pub(crate) fn length_uf_names() -> [&'static str; 2] {
    [LEN_UF_NAME, FIELD_LEN_UF_NAME]
}

/// Source field/method names that encode as a non-negative size/length value.
pub(crate) const SIZE_FIELD_NAMES: &[&str] = &[
    LEN_UF_NAME,
    LENGTH_METHOD_NAME,
    SIZE_UF_NAME,
    "capacity",
    "count",
];

/// Whether `name` is a size-like field/method (`len` / `length` / `size` / `count` / `capacity`).
pub(crate) fn is_size_field_name(name: &str) -> bool {
    SIZE_FIELD_NAMES.contains(&name)
}

/// Whether `name` is specifically `len` or `length` (canonical length method/field).
pub(crate) fn is_length_method_name(name: &str) -> bool {
    name == LEN_UF_NAME || name == LENGTH_METHOD_NAME
}

/// Typestate snapshot variable (`__typestate_{name}`).
pub(crate) fn typestate_var_name(name: &str) -> String {
    format!("__typestate_{}", sanitize_smt_name(name))
}

/// Raw-token length UIF used in CVC5 dotted `.length` encode (`__length`).
///
/// Distinct from method name [`LENGTH_METHOD_NAME`] and field UIF [`FIELD_LEN_UF_NAME`].
/// Referenced from CVC5 raw-native (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) const RAW_LENGTH_UF_NAME: &str = "__length";

/// Fresh temporary constant (`__fresh_{n}`).
pub(crate) fn fresh_temp_name(counter: impl std::fmt::Display) -> String {
    format!("__fresh_{counter}")
}

/// Fresh list object constant (`__list_{n}`).
///
/// Referenced from CVC5 list encode (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) fn list_fresh_name(counter: impl std::fmt::Display) -> String {
    format!("__list_{counter}")
}

/// Placeholder list object for SMT-LIB list encode when no element is present.
pub(crate) const LIST_FRESH_PLACEHOLDER: &str = "__list_fresh";

/// Fresh tuple object constant (`__tuple_{n}`).
///
/// Referenced from CVC5 tuple encode (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) fn tuple_fresh_name(counter: impl std::fmt::Display) -> String {
    format!("__tuple_{counter}")
}

/// Fresh array/object constant (`__arr_{n}`) used by Z3 index/array encode.
pub(crate) fn arr_fresh_name(counter: impl std::fmt::Display) -> String {
    format!("__arr_{counter}")
}

/// Fresh match ADT scrutinee constant (`__match_adt_{n}`).
pub(crate) fn match_adt_fresh_name(counter: impl std::fmt::Display) -> String {
    format!("__match_adt_{counter}")
}

/// Fresh call-result constant when the callee name is unknown (`__call_{n}`).
pub(crate) fn call_fresh_name(counter: impl std::fmt::Display) -> String {
    format!("__call_{counter}")
}

/// ADT discriminant/tag UIF (`__adt_tag_{adt}`).
pub(crate) fn adt_tag_uf_name(adt_name: &str) -> String {
    format!("__adt_tag_{adt_name}")
}

/// ADT constructor field accessor UIF (`__adt_{adt}_{accessor}`).
pub(crate) fn adt_accessor_uf_name(adt_name: &str, accessor: &str) -> String {
    format!("__adt_{adt_name}_{accessor}")
}

/// Fresh variable for ADT exhaustiveness axiom (`__adt_exh_{adt}`).
pub(crate) fn adt_exhaust_var_name(adt_name: &str) -> String {
    format!("__adt_exh_{adt_name}")
}

/// Fresh variable pair leg for ADT injectivity axiom (`__adt_inj_{adt}_{ctor}_a` / `_b`).
pub(crate) fn adt_inject_var_name(adt_name: &str, ctor_name: &str, leg: char) -> String {
    format!("__adt_inj_{adt_name}_{ctor_name}_{leg}")
}

/// IR field projection UIF (`__ir_field_{ty_suffix}_{index}`).
pub(crate) fn ir_field_uf_name(ty_suffix: &str, index: usize) -> String {
    format!("__ir_field_{ty_suffix}_{index}")
}

/// IR constructor UIF (`__ir_construct_{type_id}`).
pub(crate) fn ir_construct_uf_name(type_id: &str) -> String {
    format!("__ir_construct_{type_id}")
}

/// Bit-vector-as-real temporary (`__bv_as_real_{bits}`).
pub(crate) fn bv_as_real_name(bits: impl std::fmt::Display) -> String {
    format!("__bv_as_real_{bits}")
}

/// Tuple element accessor UIF (`__tuple_{arity}_{index}`).
pub(crate) fn tuple_accessor_name(arity: usize, index: usize) -> String {
    format!("__tuple_{arity}_{index}")
}

/// List element accessor UIF name (`__list_get`).
///
/// Used by Z3 list encode and CVC5 `encode_list_cvc5`.
pub(crate) const LIST_GET_UF_NAME: &str = "__list_get";

/// Membership / `in` UIF (`__contains`).
///
/// Shared by Z3 encoder, CVC5 native binops, and SMT-LIB `in_binop_smtlib`.
pub(crate) const CONTAINS_UF_NAME: &str = "__contains";

/// String/bytes concatenation UIF (`__concat`).
///
/// Shared by SMT-LIB `concat_binop_smtlib` and CVC5/Z3 concat paths.
pub(crate) const CONCAT_UF_NAME: &str = "__concat";

/// Collection index accessor UIF (`__index(coll, idx)`).
///
/// Used by Z3 `encode_index`, CVC5 `cvc5_index_access`, and `cvc5_native_builtins`.
pub(crate) const INDEX_UF_NAME: &str = "__index";

/// Length UIF used for index bounds axioms only (`__len(coll)`).
///
/// Distinct from [`LEN_UF_NAME`] (`"len"`, collection/string method alias) and
/// [`FIELD_LEN_UF_NAME`] (`__field_len`, field/method length accessor).
pub(crate) const INDEX_BOUNDS_LEN_UF_NAME: &str = "__len";

/// Placeholder for multi-arg trigger patterns when an arg is not the quantified var.
///
/// Shared by Z3 and CVC5 quantifier trigger encoding.
pub(crate) const TRIGGER_OTHER_NAME: &str = "__trigger_other";

/// Render `(__index coll idx)` in SMT-LIB2.
pub(crate) fn index_access_smtlib(coll: &str, idx: &str) -> String {
    format!("({INDEX_UF_NAME} {coll} {idx})")
}

/// Integer literal as SMT-LIB2 text (negatives use `(- n)`).
pub(crate) fn encode_int_literal_smtlib(n: &str) -> String {
    if let Some(stripped) = n.strip_prefix('-') {
        format!("(- {stripped})")
    } else {
        n.to_string()
    }
}

/// Float string → `(numerator, denominator)` rational parts.
pub(crate) fn float_to_rational_parts(f: &str) -> (i64, i64) {
    let fv: f64 = f.parse().unwrap_or(0.0);
    let numer = (fv * FLOAT_RATIONAL_DENOM as f64) as i64;
    (numer, FLOAT_RATIONAL_DENOM)
}

/// Float literal as SMT-LIB rational `(/ numer denom)`.
pub(crate) fn float_literal_to_smtlib(f: &str) -> String {
    let (numer, denom) = float_to_rational_parts(f);
    format!("(/ {numer} {denom})")
}

/// Literal → SMT-LIB2 atom (solver-neutral text; not a solver term).
pub(crate) fn encode_literal_smtlib(lit: &Literal) -> Option<String> {
    match lit {
        Literal::Int(n) => Some(encode_int_literal_smtlib(n)),
        Literal::Bool(b) => Some(b.to_string()),
        Literal::Float(f) => Some(float_literal_to_smtlib(f)),
        Literal::Str(s) => Some(string_literal_const_name(s)),
    }
}

/// Vacuous / empty `Expr::Raw` in SMT-LIB2.
pub(crate) fn encode_raw_empty_smtlib() -> &'static str {
    "true"
}

/// Fast path for a single raw token in SMT-LIB2.
pub(crate) fn encode_raw_single_token_smtlib(token: &str) -> Option<String> {
    if token == "true" || token == "false" {
        return Some(token.to_string());
    }
    if token.parse::<i64>().is_ok() {
        return Some(encode_int_literal_smtlib(token));
    }
    Some(encode_ident_name(token))
}

/// SMT-LIB2 operator symbol for standard (non-special) AST [`BinOp`]s.
///
/// Returns `None` for `Neq`, `Range`, `In`, `NotIn`, `Concat` (backends format specially).
pub(crate) fn standard_ast_binop_smtlib_op(op: &BinOp) -> Option<&'static str> {
    match op {
        BinOp::Add => Some("+"),
        BinOp::Sub => Some("-"),
        BinOp::Mul => Some("*"),
        BinOp::Div => Some("div"),
        BinOp::Mod => Some("mod"),
        BinOp::Eq => Some("="),
        BinOp::Lt => Some("<"),
        BinOp::Lte => Some("<="),
        BinOp::Gt => Some(">"),
        BinOp::Gte => Some(">="),
        BinOp::And => Some("and"),
        BinOp::Or => Some("or"),
        BinOp::Implies => Some("=>"),
        BinOp::Neq | BinOp::Range | BinOp::In | BinOp::NotIn | BinOp::Concat => None,
    }
}

/// Format a standard AST binop as SMT-LIB2 prefix form.
pub(crate) fn format_standard_ast_binop_smtlib(op: &BinOp, l: &str, r: &str) -> Option<String> {
    let smt_op = standard_ast_binop_smtlib_op(op)?;
    Some(format!("({smt_op} {l} {r})"))
}

pub(crate) fn format_neq_ast_binop_smtlib(l: &str, r: &str) -> String {
    format!("(not (= {l} {r}))")
}

pub(crate) fn range_binop_smtlib(l: &str, r: &str) -> String {
    format!("(let ((__range_fresh (+ {l} 0))) (and (>= __range_fresh {l}) (< __range_fresh {r})))")
}

pub(crate) fn in_binop_smtlib(elem: &str, coll: &str) -> String {
    format!("({CONTAINS_UF_NAME} {coll} {elem})")
}

pub(crate) fn not_in_binop_smtlib(elem: &str, coll: &str) -> String {
    format!("(not ({CONTAINS_UF_NAME} {coll} {elem}))")
}

pub(crate) fn concat_binop_smtlib(l: &str, r: &str) -> String {
    format!("({CONCAT_UF_NAME} {l} {r})")
}

/// Termination measure axiom temporary (`__ax_{measure}_xs`).
pub(crate) fn measure_ax_xs_name(measure: &str) -> String {
    format!("__ax_{measure}_xs")
}

/// Termination measure axiom temporary (`__ax_{measure}_xs2`).
pub(crate) fn measure_ax_xs2_name(measure: &str) -> String {
    format!("__ax_{measure}_xs2")
}

/// Termination measure axiom temporary (`__ax_{measure}_x`).
pub(crate) fn measure_ax_x_name(measure: &str) -> String {
    format!("__ax_{measure}_x")
}

/// Termination measure axiom temporary (`__ax_{measure}_eq_xs`).
pub(crate) fn measure_ax_eq_xs_name(measure: &str) -> String {
    format!("__ax_{measure}_eq_xs")
}

/// Append axiom function name (`__append_{measure}`).
pub(crate) fn measure_append_uf_name(measure: &str) -> String {
    format!("__append_{measure}")
}

/// IR block result temporary (`__ir_block{block_id}_result`).
pub(crate) fn ir_block_result_name(block_id: impl std::fmt::Display) -> String {
    format!("__ir_block{block_id}_result")
}

/// IR block slot temporary (`__ir_block{block_id}_slot_{slot}`).
pub(crate) fn ir_block_slot_name(
    block_id: impl std::fmt::Display,
    slot: impl std::fmt::Display,
) -> String {
    format!("__ir_block{block_id}_slot_{slot}")
}

/// IR block label (`__ir_block_{block_id}`).
pub(crate) fn ir_block_label_name(block_id: impl std::fmt::Display) -> String {
    format!("__ir_block_{block_id}")
}

/// IR call temporary prefix (`__ir_call_{func}_`).
pub(crate) fn ir_call_temp_prefix(func: &str) -> String {
    format!("__ir_call_{func}_")
}

/// IR call UIF (`__ir_call_{func}`).
pub(crate) fn ir_call_uf_name(func: &str) -> String {
    format!("__ir_call_{func}")
}

/// IR typestate UIF (`__ir_state_{state}`).
pub(crate) fn ir_state_uf_name(state: &str) -> String {
    format!("__ir_state_{state}")
}

/// IR type tag temporary (`__ir_tag_{type_id}`).
pub(crate) fn ir_tag_name(type_id: &str) -> String {
    format!("__ir_tag_{type_id}")
}

/// IR instruction slot temporary (`__ir_slot_{target}`).
pub(crate) fn ir_slot_name(target: impl std::fmt::Display) -> String {
    format!("__ir_slot_{target}")
}

/// Named length temporary for IR exec (`__len_{name}`).
///
/// Used from `ir_exec` tests and available for backend `canonical_length_for_name` impls.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn ir_exec_len_name(name: &str) -> String {
    format!("__len_{name}")
}

/// Opaque object pointer fallback (`__obj_{ptr}`).
pub(crate) fn obj_ptr_name(ptr: impl std::fmt::Display) -> String {
    format!("__obj_{ptr}")
}

/// ADT constructor value temporary (`__adt_val_{counter}_{ctor}`).
/// Referenced from CVC5 ADT encode (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) fn adt_val_fresh_name(counter: impl std::fmt::Display, ctor_name: &str) -> String {
    format!("__adt_val_{counter}_{ctor_name}")
}

/// Exact internal names (not prefix-matched).
///
/// Includes [`crate::encode_method_policy::MEASURE_EMPTY_CONST_NAME`].
pub(crate) const INTERNAL_ENCODER_EXACT_NAMES: &[&str] = &["__empty"];

/// Prefixes for internal encoder / model-filter variable names.
///
/// Single source for [`is_internal_encoder_var`]; extend here when adding SMT temporaries.
pub(crate) const INTERNAL_ENCODER_PREFIXES: &[&str] = &[
    "__str_",
    "__tuple_",
    "__list_",
    "__fresh_",
    "__field_",
    "__index",
    "__len",
    "__arr_",
    "__domain_contains",
    "__apply_",
    "__coerce",
    "__trigger_",
    "__list_get",
    "__result",
    "__contains",
    "__obj_",
    "__ir_",
    "__canonical_len_",
    "__match_adt_",
    "__call_",
    "__old_fresh_",
    "__typestate_",
    "__adt_",
    "__bv_as_real_",
    "__ax_",
    "__append_",
    "__ir_slot_",
    "__ir_state_",
    "__ir_tag_",
    "__ir_call_",
    "__ir_block",
    "__adt_val_",
];

/// Whether a model/counterexample variable name is internal encoder noise (shared filter heuristic).
pub(crate) fn is_internal_encoder_var(name: &str) -> bool {
    INTERNAL_ENCODER_EXACT_NAMES.contains(&name)
        || INTERNAL_ENCODER_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// Whether a model variable should appear in user-facing counterexample output.
///
/// Internal encoder temporaries are suppressed, except [`RESULT_VAR_NAME`] which
/// represents contract `result` and must stay visible.
pub(crate) fn is_counterexample_user_var(name: &str) -> bool {
    !is_internal_encoder_var(name) || name == RESULT_VAR_NAME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_and_sanitize() {
        assert_eq!(encode_ident_name("result"), RESULT_VAR_NAME);
        assert_eq!(encode_ident_name("a.b"), "a_b");
        assert_eq!(old_ident_name("result"), "__result__old");
        assert_eq!(old_ident_name("x.y"), "x_y__old");
        assert_eq!(old_snapshot_name("result"), "result__old");
        assert_eq!(old_snapshot_name("x.y"), "x_y__old");
        assert_eq!(old_snapshot_name("a_b"), "a_b__old");
        assert_eq!(old_fresh_temp_name(4), "__old_fresh_4");
        assert_eq!(canonical_length_name("buf"), "__canonical_len_buf");
        assert_eq!(field_uif_name("len"), FIELD_LEN_UF_NAME);
        assert_eq!(field_uif_name("len"), "__field_len");
        assert!(is_length_uf_name(FIELD_LEN_UF_NAME));
        assert!(is_length_uf_name(LEN_UF_NAME));
        assert!(!is_length_uf_name("size"));
        assert_eq!(length_uf_names(), [LEN_UF_NAME, FIELD_LEN_UF_NAME]);
        assert_eq!(GET_UF_NAME, "get");
        assert_eq!(SIZE_UF_NAME, "size");
        assert_eq!(LENGTH_METHOD_NAME, "length");
        assert_eq!(RAW_LENGTH_UF_NAME, "__length");
        assert!(is_size_field_name("len"));
        assert!(is_size_field_name("capacity"));
        assert!(!is_size_field_name("push"));
        assert_eq!(typestate_var_name("conn"), "__typestate_conn");
        assert_eq!(typestate_var_name("a.b"), "__typestate_a_b");
        assert_eq!(fresh_temp_name(3), "__fresh_3");
        assert_eq!(list_fresh_name(1), "__list_1");
        assert_eq!(LIST_FRESH_PLACEHOLDER, "__list_fresh");
        assert_eq!(tuple_fresh_name(2), "__tuple_2");
        assert_eq!(tuple_accessor_name(3, 0), "__tuple_3_0");
        assert_eq!(arr_fresh_name(7), "__arr_7");
        assert_eq!(match_adt_fresh_name(2), "__match_adt_2");
        assert_eq!(call_fresh_name(9), "__call_9");
        assert_eq!(adt_tag_uf_name("Opt"), "__adt_tag_Opt");
        assert_eq!(adt_accessor_uf_name("Opt", "val"), "__adt_Opt_val");
        assert_eq!(adt_exhaust_var_name("Opt"), "__adt_exh_Opt");
        assert_eq!(
            adt_inject_var_name("Opt", "Some", 'a'),
            "__adt_inj_Opt_Some_a"
        );
        assert_eq!(ir_field_uf_name("pair", 0), "__ir_field_pair_0");
        assert_eq!(ir_construct_uf_name("T1"), "__ir_construct_T1");
        assert_eq!(bv_as_real_name(32), "__bv_as_real_32");
        assert_eq!(measure_ax_xs_name("m"), "__ax_m_xs");
        assert_eq!(measure_append_uf_name("m"), "__append_m");
        assert_eq!(LIST_GET_UF_NAME, "__list_get");
        assert_eq!(CONTAINS_UF_NAME, "__contains");
        assert_eq!(CONCAT_UF_NAME, "__concat");
        assert_eq!(INDEX_UF_NAME, "__index");
        assert_eq!(INDEX_BOUNDS_LEN_UF_NAME, "__len");
        assert_eq!(TRIGGER_OTHER_NAME, "__trigger_other");
        assert_eq!(index_access_smtlib("buf", "i"), "(__index buf i)");
        assert_eq!(in_binop_smtlib("x", "s"), "(__contains s x)");
        assert_eq!(concat_binop_smtlib("a", "b"), "(__concat a b)");
        assert!(is_internal_encoder_var("__field_len"));
        assert!(is_internal_encoder_var(RESULT_VAR_NAME));
        assert!(is_internal_encoder_var("__match_adt_0"));
        assert!(is_internal_encoder_var("__call_1"));
        assert!(is_internal_encoder_var("__old_fresh_4"));
        assert!(is_internal_encoder_var("__typestate_conn"));
        assert!(is_internal_encoder_var("__adt_tag_Opt"));
        assert!(is_internal_encoder_var("__bv_as_real_8"));
        assert!(is_internal_encoder_var("__empty"));
        assert!(is_internal_encoder_var("__ir_block0_result"));
        assert!(INTERNAL_ENCODER_PREFIXES.contains(&"__field_"));
        assert!(INTERNAL_ENCODER_EXACT_NAMES.contains(&"__empty"));
        assert!(!is_internal_encoder_var("payload_length"));
        assert!(is_counterexample_user_var("payload_length"));
        assert!(is_counterexample_user_var(RESULT_VAR_NAME));
        assert!(!is_counterexample_user_var("__fresh_0"));
        assert!(!is_counterexample_user_var("__field_len"));
    }

    #[test]
    fn int_and_apply_atoms() {
        assert_eq!(encode_int_literal_smtlib("-3"), "(- 3)");
        assert_eq!(encode_int_literal_smtlib("7"), "7");
        assert_eq!(apply_lemma_const_name("pos"), "__apply_pos");
        assert_eq!(string_literal_const_name("hi.there"), "__str_hi_there");
    }

    #[test]
    fn standard_binop_ops() {
        assert_eq!(standard_ast_binop_smtlib_op(&BinOp::Add), Some("+"));
        assert_eq!(standard_ast_binop_smtlib_op(&BinOp::Implies), Some("=>"));
        assert_eq!(standard_ast_binop_smtlib_op(&BinOp::Neq), None);
        assert_eq!(
            format_standard_ast_binop_smtlib(&BinOp::And, "a", "b").as_deref(),
            Some("(and a b)")
        );
    }

    #[test]
    fn literal_smtlib_round_shapes() {
        assert_eq!(
            encode_literal_smtlib(&Literal::Bool(true)).as_deref(),
            Some("true")
        );
        assert_eq!(
            encode_literal_smtlib(&Literal::Int("-1".into())).as_deref(),
            Some("(- 1)")
        );
    }
}
