//! Shared field-access strategy for CVC5 shell-out and native backends.

use assura_ast::{Expr, SpExpr, Spanned};

use crate::cvc5_common::{
    flatten_field_chain_cvc5, has_deep_field_chain_cvc5, is_self_rooted_cvc5,
};

/// How a field access `obj.field` should be encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldAccessPlan {
    /// Deep/self-rooted chain flattened to a single name (`a__b__c`).
    Flatten(String),
    /// Shallow access via UF `__field_{field}(obj)`.
    ShallowUf { field: String },
}

/// Decide flatten-vs-UF encoding for `obj.field`.
pub(crate) fn plan_field_access(obj: &SpExpr, field: &str) -> FieldAccessPlan {
    let full_expr = Spanned::no_span(Expr::Field(Box::new(obj.clone()), field.to_string()));
    if has_deep_field_chain_cvc5(&full_expr) || is_self_rooted_cvc5(&full_expr) {
        FieldAccessPlan::Flatten(flatten_field_chain_cvc5(&full_expr))
    } else {
        FieldAccessPlan::ShallowUf {
            field: field.to_string(),
        }
    }
}

pub(crate) fn field_uf_smtlib_name(field: &str) -> String {
    format!("__field_{field}")
}

/// Render a shallow field UF in SMT-LIB2: `(__field_f obj)`.
pub(crate) fn shallow_field_smtlib(field: &str, obj_smt: &str) -> String {
    format!("({} {obj_smt})", field_uf_smtlib_name(field))
}

/// Render `old(flattened)` as `{flat}__old`.
pub(crate) fn old_flat_field_smtlib(flat_name: &str) -> String {
    format!("{flat_name}__old")
}

#[cfg(feature = "cvc5-verify")]
fn get_or_create_int_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    name: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
) -> cvc5::Term<'a> {
    vars.entry(name.to_string())
        .or_insert_with(|| tm.mk_const(tm.integer_sort(), name))
        .clone()
}

/// Encode `obj.field` for the native CVC5 backend (flatten, shallow UF, or length).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_field_cvc5<'a, E>(
    tm: &'a cvc5::TermManager,
    obj: &SpExpr,
    field: &str,
    vars: &mut std::collections::HashMap<String, cvc5::Term<'a>>,
    state: &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    mut encode: E,
) -> Option<cvc5::Term<'a>>
where
    E: FnMut(
        &SpExpr,
        &mut std::collections::HashMap<String, cvc5::Term<'a>>,
        &mut crate::cvc5_encoder_state::Cvc5EncoderState<'a>,
    ) -> Option<cvc5::Term<'a>>,
{
    use crate::cvc5_builtins::{is_bool_field, is_size_field};
    use crate::cvc5_encoder_state::canonical_length_cvc5;

    if matches!(field, "len" | "length")
        && let Expr::Ident(name) = &obj.node
    {
        return Some(canonical_length_cvc5(tm, name, vars, state));
    }

    match plan_field_access(obj, field) {
        FieldAccessPlan::Flatten(flat_name) => {
            if is_bool_field(field) {
                return Some(tm.mk_const(tm.boolean_sort(), &flat_name));
            }
            if is_size_field(field) {
                let v = get_or_create_int_cvc5(tm, &flat_name, vars);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
                return Some(v);
            }
            Some(get_or_create_int_cvc5(tm, &flat_name, vars))
        }
        FieldAccessPlan::ShallowUf { field: f } => {
            let obj_val = encode(obj, vars, state)?;
            Some(encode_shallow_field_cvc5(
                tm,
                &f,
                obj_val,
                &mut state.axioms,
                state.use_string_theory,
            ))
        }
    }
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn encode_shallow_field_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    field: &str,
    obj_val: cvc5::Term<'a>,
    axioms: &mut Vec<cvc5::Term<'a>>,
    use_string_theory: bool,
) -> cvc5::Term<'a> {
    use crate::cvc5_builtins::{is_bool_field, is_size_field};

    if use_string_theory && matches!(field, "len" | "length") && obj_val.sort().is_string() {
        let len = tm.mk_term(cvc5::Kind::StringLength, &[obj_val]);
        let zero = tm.mk_integer(0);
        axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
        return len;
    }

    let func_name = field_uf_smtlib_name(field);
    if is_bool_field(field) {
        let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.boolean_sort());
        let func_const = tm.mk_const(func_sort, &func_name);
        return tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
    }
    if is_size_field(field) {
        let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
        let func_const = tm.mk_const(func_sort, &func_name);
        let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
        let zero = tm.mk_integer(0);
        axioms.push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
        return result;
    }
    let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
    let func_const = tm.mk_const(func_sort, &func_name);
    tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shallow_field_for_simple_access() {
        let obj = Spanned::no_span(Expr::Ident("x".into()));
        assert_eq!(
            plan_field_access(&obj, "y"),
            FieldAccessPlan::ShallowUf { field: "y".into() }
        );
    }

    #[test]
    fn flatten_deep_chain() {
        let _obj = Spanned::no_span(Expr::Field(
            Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("a".into()))),
                "b".into(),
            ))),
            "c".into(),
        ));
        // plan on obj.field would be wrong - use parent
        let parent = Spanned::no_span(Expr::Ident("state".into()));
        assert!(matches!(
            plan_field_access(&parent, "head"),
            FieldAccessPlan::ShallowUf { .. }
        ));
    }
}
