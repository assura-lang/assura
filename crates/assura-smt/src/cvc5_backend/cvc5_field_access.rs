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
    use crate::encode_atom_policy::is_size_field_name;
    use crate::encode_method_policy::is_bool_field_name;

    if (field == crate::encode_atom_policy::LEN_UF_NAME
        || field == crate::encode_atom_policy::LENGTH_METHOD_NAME)
        && let Expr::Ident(name) = &obj.node
    {
        return Some(canonical_length_cvc5(tm, name, vars, state));
    }

    match plan_field_access(obj, field) {
        FieldAccessPlan::Flatten(flat_name) => {
            if is_bool_field_name(field) {
                return Some(tm.mk_const(tm.boolean_sort(), &flat_name));
            }
            if is_size_field_name(field) {
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
    use crate::encode_atom_policy::is_size_field_name;
    use crate::encode_method_policy::is_bool_field_name;

    if use_string_theory
        && (field == crate::encode_atom_policy::LEN_UF_NAME
            || field == crate::encode_atom_policy::LENGTH_METHOD_NAME)
        && obj_val.sort().is_string()
    {
        let len = tm.mk_term(cvc5::Kind::StringLength, &[obj_val]);
        let zero = tm.mk_integer(0);
        axioms.push(tm.mk_term(cvc5::Kind::Geq, &[len.clone(), zero]));
        return len;
    }

    let func_name = field_uf_smtlib_name(field);
    if is_bool_field_name(field) {
        let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.boolean_sort());
        let func_const = tm.mk_const(func_sort, &func_name);
        return tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
    }
    if is_size_field_name(field) {
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
