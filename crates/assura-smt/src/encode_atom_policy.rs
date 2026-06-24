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
pub(crate) fn old_ident_name(name: &str) -> String {
    if name == "result" {
        format!("{RESULT_VAR_NAME}__old")
    } else {
        format!("{}__old", sanitize_smt_name(name))
    }
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

/// Whether `uf` is either [`LEN_UF_NAME`] or [`FIELD_LEN_UF_NAME`].
pub(crate) fn is_length_uf_name(uf: &str) -> bool {
    uf == LEN_UF_NAME || uf == FIELD_LEN_UF_NAME
}

/// Both length UIF spellings (order: `len`, then `__field_len`).
pub(crate) fn length_uf_names() -> [&'static str; 2] {
    [LEN_UF_NAME, FIELD_LEN_UF_NAME]
}

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

/// Fresh tuple object constant (`__tuple_{n}`).
///
/// Referenced from CVC5 tuple encode (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) fn tuple_fresh_name(counter: impl std::fmt::Display) -> String {
    format!("__tuple_{counter}")
}

/// Tuple element accessor UIF (`__tuple_{arity}_{index}`).
pub(crate) fn tuple_accessor_name(arity: usize, index: usize) -> String {
    format!("__tuple_{arity}_{index}")
}

/// List element accessor UIF name (`__list_get`).
///
/// Referenced from CVC5 list encode (`cvc5-verify` only in default builds).
#[cfg_attr(not(feature = "cvc5-verify"), allow(dead_code))]
pub(crate) const LIST_GET_UF_NAME: &str = "__list_get";

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
    format!("(__contains {coll} {elem})")
}

pub(crate) fn not_in_binop_smtlib(elem: &str, coll: &str) -> String {
    format!("(not (__contains {coll} {elem}))")
}

pub(crate) fn concat_binop_smtlib(l: &str, r: &str) -> String {
    format!("(__concat {l} {r})")
}

/// Whether a model/counterexample variable name is internal encoder noise (shared filter heuristic).
pub(crate) fn is_internal_encoder_var(name: &str) -> bool {
    name.starts_with("__str_")
        || name.starts_with("__tuple_")
        || name.starts_with("__list_")
        || name.starts_with("__fresh_")
        || name.starts_with("__field_")
        || name.starts_with("__index")
        || name.starts_with("__len")
        || name.starts_with("__arr_")
        || name.starts_with("__domain_contains")
        || name.starts_with("__apply_")
        || name.starts_with("__coerce")
        || name.starts_with("__trigger_")
        || name.starts_with("__list_get")
        || name.starts_with("__result")
        || name.starts_with("__contains")
        || name.starts_with("__obj_")
        || name.starts_with("__ir_")
        || name.starts_with("__canonical_len_")
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
        assert_eq!(canonical_length_name("buf"), "__canonical_len_buf");
        assert_eq!(field_uif_name("len"), FIELD_LEN_UF_NAME);
        assert_eq!(field_uif_name("len"), "__field_len");
        assert!(is_length_uf_name(FIELD_LEN_UF_NAME));
        assert!(is_length_uf_name(LEN_UF_NAME));
        assert!(!is_length_uf_name("size"));
        assert_eq!(length_uf_names(), [LEN_UF_NAME, FIELD_LEN_UF_NAME]);
        assert_eq!(fresh_temp_name(3), "__fresh_3");
        assert_eq!(list_fresh_name(1), "__list_1");
        assert_eq!(tuple_fresh_name(2), "__tuple_2");
        assert_eq!(tuple_accessor_name(3, 0), "__tuple_3_0");
        assert_eq!(LIST_GET_UF_NAME, "__list_get");
        assert!(is_internal_encoder_var("__field_len"));
        assert!(is_internal_encoder_var(RESULT_VAR_NAME));
        assert!(!is_internal_encoder_var("payload_length"));
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
