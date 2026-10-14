//! ADT (algebraic data type) emulation for CVC5 (#263).
//!
//! Mirrors the Z3 backend's ADT encoding using integer tags and
//! uninterpreted functions.

/// A single ADT constructor for CVC5 emulation.
#[derive(Debug, Clone)]
pub(crate) struct Cvc5AdtConstructor {
    pub(crate) name: String,
    pub(crate) tag: i64,
    pub(crate) accessors: Vec<String>,
}

/// An ADT definition for CVC5 emulation.
#[derive(Debug, Clone)]
pub(crate) struct Cvc5AdtDef {
    pub(crate) name: String,
    pub(crate) constructors: Vec<Cvc5AdtConstructor>,
}

/// Native CVC5 UF symbols declared by `define_adt_cvc5_native`.
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5AdtNativeSymbols {
    pub(crate) adt_name: String,
    pub(crate) tag_fn: cvc5::Term,
    pub(crate) acc_fns: std::collections::HashMap<String, cvc5::Term>,
}

/// Define an ADT for CVC5 and generate SMT-LIB2 assertions.
pub(crate) fn define_adt_cvc5(
    adt_name: &str,
    constructors: &[(&str, &[&str])],
) -> (Cvc5AdtDef, Vec<String>) {
    let mut adt_ctors = Vec::new();
    let mut assertions = Vec::new();

    for (tag, (ctor_name, accessors)) in constructors.iter().enumerate() {
        adt_ctors.push(Cvc5AdtConstructor {
            name: ctor_name.to_string(),
            tag: tag as i64,
            accessors: accessors.iter().map(|a| a.to_string()).collect(),
        });
    }

    let adt_def = Cvc5AdtDef {
        name: adt_name.to_string(),
        constructors: adt_ctors,
    };

    let tag_fn = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
    assertions.push(format!("(declare-fun {tag_fn} (Int) Int)"));

    for ctor in &adt_def.constructors {
        for accessor in &ctor.accessors {
            let acc_fn = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
            assertions.push(format!("(declare-fun {acc_fn} (Int) Int)"));
        }
    }

    let tag_eqs: Vec<String> = adt_def
        .constructors
        .iter()
        .map(|c| format!("(= ({tag_fn} x) {})", c.tag))
        .collect();
    let exhaustive = if tag_eqs.len() == 1 {
        tag_eqs[0].clone()
    } else {
        format!("(or {})", tag_eqs.join(" "))
    };
    assertions.push(format!("(assert (forall ((x Int)) {exhaustive}))"));

    for ctor in &adt_def.constructors {
        if ctor.accessors.is_empty() {
            assertions.push(format!(
                "(assert (forall ((a Int) (b Int)) \
                 (=> (and (= ({tag_fn} a) {}) (= ({tag_fn} b) {})) (= a b))))",
                ctor.tag, ctor.tag
            ));
        } else {
            let mut conjuncts = vec![
                format!("(= ({tag_fn} a) {})", ctor.tag),
                format!("(= ({tag_fn} b) {})", ctor.tag),
            ];
            for accessor in &ctor.accessors {
                let acc_fn = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
                conjuncts.push(format!("(= ({acc_fn} a) ({acc_fn} b))"));
            }
            assertions.push(format!(
                "(assert (forall ((a Int) (b Int)) \
                 (=> (and {}) (= a b))))",
                conjuncts.join(" ")
            ));
        }
    }

    (adt_def, assertions)
}

/// SMT-LIB2 declarations and axioms for baseline ADT infrastructure (shell-out path).
#[cfg(not(feature = "cvc5-verify"))]
pub(crate) fn cvc5_adt_prelude_lines() -> Vec<String> {
    let (def, mut lines) = define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);
    lines.push(format!("; adt: {}", def.name));
    let tester = adt_is_constructor_smt("Option", "Some", "x", &def);
    let accessor = adt_accessor_smt("Option", "value", "x");
    lines.push(format!("; adt tester: {tester}"));
    lines.push(format!("; adt accessor: {accessor}"));
    lines
}

/// Returns `(= (__adt_tag_<adt> <value>) <tag>)`.
///
/// Delegates SMT-LIB shape to [`crate::encode_adt_policy`]; tag lookup uses
/// ordered constructor names from `adt_def` (sequential tags 0..n-1).
pub(crate) fn adt_is_constructor_smt(
    adt_name: &str,
    ctor_name: &str,
    value: &str,
    adt_def: &Cvc5AdtDef,
) -> String {
    let ctor_names: Vec<&str> = adt_def
        .constructors
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    // Prefer stored tag when present (matches definition order for normal defs).
    let tag = adt_def
        .constructors
        .iter()
        .find(|c| c.name == ctor_name)
        .map_or_else(
            || crate::encode_adt_policy::adt_ctor_tag_or_zero(&ctor_names, ctor_name),
            |c| c.tag,
        );
    crate::encode_adt_policy::adt_is_constructor_smtlib(adt_name, value, tag)
}

/// Returns `(__adt_<adt>_<accessor> <value>)`.
#[cfg_attr(
    all(not(test), feature = "cvc5-verify"),
    expect(dead_code, reason = "shell-out prelude only; native uses UF accessors")
)]
pub(crate) fn adt_accessor_smt(adt_name: &str, accessor: &str, value: &str) -> String {
    crate::encode_adt_policy::adt_accessor_smtlib(adt_name, accessor, value)
}

#[cfg(feature = "cvc5-verify")]
fn build_adt_cvc5_native(
    tm: &mut cvc5::TermManager,
    adt_name: &str,
    constructors: &[(&str, &[&str])],
) -> (Cvc5AdtDef, Cvc5AdtNativeSymbols) {
    let mut adt_ctors = Vec::new();

    for (tag, (ctor_name, accessors)) in constructors.iter().enumerate() {
        adt_ctors.push(Cvc5AdtConstructor {
            name: ctor_name.to_string(),
            tag: tag as i64,
            accessors: accessors.iter().map(|a| a.to_string()).collect(),
        });
    }

    let adt_def = Cvc5AdtDef {
        name: adt_name.to_string(),
        constructors: adt_ctors,
    };

    let tag_fn_name = crate::encode_atom_policy::adt_tag_uf_name(adt_name);
    let tag_sort = {
        let __cvc10 = tm.integer_sort();
        let __cvc9 = tm.integer_sort();
        tm.mk_fun_sort(&[__cvc10], __cvc9)
    }
    .expect("cvc5");
    let tag_fn = tm.mk_const(tag_sort, &tag_fn_name).expect("cvc5");

    let mut acc_fns = std::collections::HashMap::new();
    for ctor in &adt_def.constructors {
        for accessor in &ctor.accessors {
            let acc_fn_name = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
            let acc_sort = {
                let __cvc8 = tm.integer_sort();
                let __cvc7 = tm.integer_sort();
                tm.mk_fun_sort(&[__cvc8], __cvc7)
            }
            .expect("cvc5");
            let acc_fn_term = tm.mk_const(acc_sort, &acc_fn_name).expect("cvc5");
            acc_fns.insert(acc_fn_name, acc_fn_term);
        }
    }

    (
        adt_def,
        Cvc5AdtNativeSymbols {
            adt_name: adt_name.to_string(),
            tag_fn,
            acc_fns,
        },
    )
}

/// Declare struct ADT UF symbols for IR field/construct encoding (no solver axioms).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn declare_struct_adt_ufs_cvc5_native(
    tm: &mut cvc5::TermManager,
    type_name: &str,
    field_names: &[&str],
) -> (Cvc5AdtDef, Cvc5AdtNativeSymbols) {
    build_adt_cvc5_native(tm, type_name, &[(type_name, field_names)])
}

#[cfg(feature = "cvc5-verify")]
#[cfg_attr(not(test), allow(dead_code, reason = "cvc5 native ADT tests"))]
pub(crate) fn define_adt_cvc5_native(
    tm: &mut cvc5::TermManager,
    solver: &mut cvc5::Solver,
    adt_name: &str,
    constructors: &[(&str, &[&str])],
) -> (Cvc5AdtDef, Cvc5AdtNativeSymbols) {
    let (adt_def, symbols) = build_adt_cvc5_native(tm, adt_name, constructors);
    let tag_fn = symbols.tag_fn.clone();

    let x = {
        let __cvc6 = tm.integer_sort();
        tm.mk_var(
            __cvc6,
            &crate::encode_atom_policy::adt_exhaust_var_name(adt_name),
        )
    }
    .expect("cvc5");
    let tag_x = tm
        .mk_term(cvc5::Kind::ApplyUf, &[tag_fn.clone(), x.clone()])
        .expect("cvc5");

    let tag_eqs: Vec<cvc5::Term> = adt_def
        .constructors
        .iter()
        .map(|c| {
            {
                let __cvc5 = tm.mk_integer(c.tag);
                tm.mk_term(cvc5::Kind::Equal, &[tag_x.clone(), __cvc5])
            }
            .expect("cvc5")
        })
        .collect();
    let exhaustive = if tag_eqs.len() == 1 {
        tag_eqs[0].clone()
    } else {
        tm.mk_term(cvc5::Kind::Or, &tag_eqs).expect("cvc5")
    };
    let bound_list = tm
        .mk_term(cvc5::Kind::VariableList, std::slice::from_ref(&x))
        .expect("cvc5");
    let forall_exhaustive = tm
        .mk_term(cvc5::Kind::Forall, &[bound_list, exhaustive])
        .expect("cvc5");
    solver.assert_formula(forall_exhaustive).expect("cvc5");

    for ctor in &adt_def.constructors {
        let a = {
            let __cvc4 = tm.integer_sort();
            tm.mk_var(
                __cvc4,
                &crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'a'),
            )
        }
        .expect("cvc5");
        let b = {
            let __cvc3 = tm.integer_sort();
            tm.mk_var(
                __cvc3,
                &crate::encode_atom_policy::adt_inject_var_name(adt_name, &ctor.name, 'b'),
            )
        }
        .expect("cvc5");
        let tag_a = tm
            .mk_term(cvc5::Kind::ApplyUf, &[tag_fn.clone(), a.clone()])
            .expect("cvc5");
        let tag_b = tm
            .mk_term(cvc5::Kind::ApplyUf, &[tag_fn.clone(), b.clone()])
            .expect("cvc5");
        let tag_val = tm.mk_integer(ctor.tag);

        let mut conjuncts = vec![
            tm.mk_term(cvc5::Kind::Equal, &[tag_a, tag_val.clone()])
                .expect("cvc5"),
            tm.mk_term(cvc5::Kind::Equal, &[tag_b, tag_val])
                .expect("cvc5"),
        ];

        for accessor in &ctor.accessors {
            let acc_fn_name = crate::encode_atom_policy::adt_accessor_uf_name(adt_name, accessor);
            if let Some(acc_fn_term) = symbols.acc_fns.get(&acc_fn_name) {
                let acc_a = tm
                    .mk_term(cvc5::Kind::ApplyUf, &[acc_fn_term.clone(), a.clone()])
                    .expect("cvc5");
                let acc_b = tm
                    .mk_term(cvc5::Kind::ApplyUf, &[acc_fn_term.clone(), b.clone()])
                    .expect("cvc5");
                conjuncts.push(
                    tm.mk_term(cvc5::Kind::Equal, &[acc_a, acc_b])
                        .expect("cvc5"),
                );
            }
        }

        let premise = tm.mk_term(cvc5::Kind::And, &conjuncts).expect("cvc5");
        let eq_ab = tm
            .mk_term(cvc5::Kind::Equal, &[a.clone(), b.clone()])
            .expect("cvc5");
        let implication = tm
            .mk_term(cvc5::Kind::Implies, &[premise, eq_ab])
            .expect("cvc5");
        let bound_list_ab = tm.mk_term(cvc5::Kind::VariableList, &[a, b]).expect("cvc5");
        let forall_inj = tm
            .mk_term(cvc5::Kind::Forall, &[bound_list_ab, implication])
            .expect("cvc5");
        solver.assert_formula(forall_inj).expect("cvc5");
    }

    (adt_def, symbols)
}

#[cfg(feature = "cvc5-verify")]
pub(crate) fn adt_constructor_cvc5_native(
    tm: &mut cvc5::TermManager,
    symbols: &Cvc5AdtNativeSymbols,
    ctor: &Cvc5AdtConstructor,
    args: &[cvc5::Term],
    axioms: &mut Vec<cvc5::Term>,
    fresh_counter: &mut usize,
) -> cvc5::Term {
    let val_name = crate::encode_atom_policy::adt_val_fresh_name(*fresh_counter, &ctor.name);
    *fresh_counter += 1;
    let val = {
        let __cvc2 = tm.integer_sort();
        tm.mk_const(__cvc2, &val_name)
    }
    .expect("cvc5");

    let tag_applied = tm
        .mk_term(cvc5::Kind::ApplyUf, &[symbols.tag_fn.clone(), val.clone()])
        .expect("cvc5");
    axioms.push(
        {
            let __cvc1 = tm.mk_integer(ctor.tag);
            tm.mk_term(cvc5::Kind::Equal, &[tag_applied, __cvc1])
        }
        .expect("cvc5"),
    );

    for (i, accessor) in ctor.accessors.iter().enumerate() {
        if let Some(arg) = args.get(i) {
            let acc_fn_name =
                crate::encode_atom_policy::adt_accessor_uf_name(&symbols.adt_name, accessor);
            if let Some(acc_fn) = symbols.acc_fns.get(&acc_fn_name) {
                let acc_applied = tm
                    .mk_term(cvc5::Kind::ApplyUf, &[acc_fn.clone(), val.clone()])
                    .expect("cvc5");
                axioms.push(
                    tm.mk_term(cvc5::Kind::Equal, &[acc_applied, arg.clone()])
                        .expect("cvc5"),
                );
            }
        }
    }

    val
}

#[cfg(feature = "cvc5-verify")]
#[cfg_attr(not(test), allow(dead_code, reason = "cvc5 native ADT tests"))]
pub(crate) fn adt_is_constructor_cvc5_native(
    tm: &mut cvc5::TermManager,
    symbols: &Cvc5AdtNativeSymbols,
    ctor: &Cvc5AdtConstructor,
    value: &cvc5::Term,
) -> cvc5::Term {
    let tag_val = tm
        .mk_term(
            cvc5::Kind::ApplyUf,
            &[symbols.tag_fn.clone(), value.clone()],
        )
        .expect("cvc5");
    {
        let __cvc0 = tm.mk_integer(ctor.tag);
        tm.mk_term(cvc5::Kind::Equal, &[tag_val, __cvc0])
    }
    .expect("cvc5")
}

#[cfg(feature = "cvc5-verify")]
#[cfg_attr(not(test), allow(dead_code, reason = "cvc5 native ADT tests"))]
pub(crate) fn adt_accessor_cvc5_native(
    tm: &mut cvc5::TermManager,
    symbols: &Cvc5AdtNativeSymbols,
    accessor: &str,
    value: &cvc5::Term,
) -> cvc5::Term {
    let acc_fn_name = crate::encode_atom_policy::adt_accessor_uf_name(&symbols.adt_name, accessor);
    let acc_fn = symbols
        .acc_fns
        .get(&acc_fn_name)
        .expect("accessor must be declared by define_adt_cvc5_native");
    tm.mk_term(cvc5::Kind::ApplyUf, &[acc_fn.clone(), value.clone()])
        .expect("cvc5")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_option_adt_emits_tag_decl() {
        let (def, lines) = define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);
        assert_eq!(def.name, "Option");
        assert!(lines.iter().any(|l| l.contains("__adt_tag_Option")));
    }
}
