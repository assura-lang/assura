//! CVC5 field-access encoding (native + SMT-LIB helpers).
//!
//! Planning/naming policy lives in [`crate::encode_field_policy`]; this module
//! owns CVC5 term construction and keeps stable `cvc5_field_access::*` imports.

// Stable re-exports; shell `cvc5_expr_smtlib` may import encode_field_policy directly.
#[allow(
    unused_imports,
    reason = "re-export surface; callers may use encode_field_policy"
)]
pub(crate) use crate::encode_field_policy::{
    FieldAccessPlan, plan_field_access, shallow_field_smtlib,
};

#[cfg(feature = "cvc5-verify")]
use crate::encode_field_policy::field_uf_smtlib_name;
#[cfg(feature = "cvc5-verify")]
use assura_ast::{Expr, SpExpr};

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
    use crate::cvc5_encoder_state::canonical_length_cvc5;

    match plan_field_access(obj, field) {
        FieldAccessPlan::CanonicalLength { obj_name } => {
            Some(canonical_length_cvc5(tm, &obj_name, vars, state))
        }
        FieldAccessPlan::Flatten(flat_name) => {
            use crate::encode_field_policy::{FieldValueKind, classify_field_value_kind};
            match classify_field_value_kind(field) {
                FieldValueKind::Bool => Some(tm.mk_const(tm.boolean_sort(), &flat_name)),
                FieldValueKind::SizeNonNeg => {
                    let v = get_or_create_int_cvc5(tm, &flat_name, vars);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[v.clone(), zero]));
                    Some(v)
                }
                FieldValueKind::Int => Some(get_or_create_int_cvc5(tm, &flat_name, vars)),
            }
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
    use crate::encode_atom_policy::is_length_method_name;
    use crate::encode_field_policy::{FieldValueKind, classify_field_value_kind};

    if use_string_theory && is_length_method_name(field) && obj_val.sort().is_string() {
        let len = tm.mk_term(cvc5::Kind::StringLength, &[obj_val]);
        let zero = tm.mk_integer(0);
        axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
        return len;
    }

    let func_name = field_uf_smtlib_name(field);
    match classify_field_value_kind(field) {
        FieldValueKind::Bool => {
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.boolean_sort());
            let func_const = tm.mk_const(func_sort, &func_name);
            tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val])
        }
        FieldValueKind::SizeNonNeg => {
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &func_name);
            let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
            let zero = tm.mk_integer(0);
            axioms.push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
            result
        }
        FieldValueKind::Int => {
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &func_name);
            tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val])
        }
    }
}
