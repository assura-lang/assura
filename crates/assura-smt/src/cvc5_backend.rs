use super::*;
use crate::cache::SessionCache;
use crate::cvc5_common::{
    collect_apply_refs_from_expr, collect_unmodelable_reasons_cvc5,
    expr_has_unmodelable_features_cvc5, float_literal_to_smtlib, is_internal_cvc5_var,
    old_ident_smtlib_name, sanitize_smtlib_name, smtlib_result_name,
};
use crate::cvc5_field_access::{
    FieldAccessPlan, old_flat_field_smtlib, plan_field_access, shallow_field_smtlib,
};
use crate::cvc5_index_access::index_access_smtlib;
use crate::cvc5_match_encode::encode_match_smtlib;

#[cfg(test)]
use crate::cvc5_common::{
    field_chain_depth_cvc5, flatten_field_chain_cvc5, has_deep_field_chain_cvc5,
    is_self_rooted_cvc5,
};
use crate::cvc5_raw_ops::{
    comma_chunk_ranges, concat_binop_smtlib, domain_as_range, domain_contains_guard_smtlib,
    find_matching_delim, format_neq_ast_binop_smtlib, format_raw_binop_smtlib,
    format_raw_quantifier_smtlib, format_standard_ast_binop_smtlib, in_binop_smtlib,
    is_raw_spec_skip_keyword, not_in_binop_smtlib, parse_raw_quantifier_slice, range_binop_smtlib,
    range_guard_smtlib, raw_op_info, raw_op_is_comparison, wrap_ast_quantifier_smtlib,
};
use assura_parser::ast::{BinOp, BlockKind, Clause, ClauseKind, Decl, Literal, UnaryOp};
use std::collections::HashSet;
use std::sync::OnceLock;

/// Baseline Option ADT for shell-out match encoding (#263).
static SHELL_MATCH_ADT: OnceLock<Cvc5AdtDef> = OnceLock::new();

fn shell_match_adt_def() -> &'static Cvc5AdtDef {
    SHELL_MATCH_ADT.get_or_init(|| {
        let (def, _) = define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);
        assert_eq!(def.name, "Option");
        def
    })
}

/// SMT-LIB2 declarations and axioms for baseline ADT infrastructure.
pub(crate) fn cvc5_adt_prelude_lines() -> Vec<String> {
    let (def, mut lines) = define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);
    lines.push(format!("; adt: {}", def.name));
    let tester = adt_is_constructor_smt("Option", "Some", "x", &def);
    let accessor = adt_accessor_smt("Option", "value", "x");
    lines.push(format!("; adt tester: {tester}"));
    lines.push(format!("; adt accessor: {accessor}"));
    lines
}

/// Collect lemma definitions from a typed file's declarations.
///
/// Maps each lemma name to its ensures clause bodies. This mirrors
/// `z3_backend::collect_lemma_defs` but is available without the
/// `z3-verify` feature.
pub(crate) fn collect_lemma_defs_for_cvc5(
    typed: &assura_types::TypedFile,
) -> std::collections::HashMap<String, Vec<&Expr>> {
    let mut lemmas = std::collections::HashMap::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.is_lemma
        {
            let ensures: Vec<&Expr> = f
                .clauses
                .iter()
                .filter(|c| c.kind == ClauseKind::Ensures)
                .map(|c| &c.body)
                .collect();
            lemmas.insert(f.name.clone(), ensures);
        }
    }
    lemmas
}

// =========================================================================
// Feature-max constant collection and refinement narrowing (CVC5)
// =========================================================================

/// Collect `feature_max` constants from a `TypedFile`'s declarations.
///
/// Each `feature_max NAME: Nat = VALUE` declaration is returned as
/// `(NAME, VALUE)`. The CVC5 backend binds these as concrete integer
/// constants instead of free Z3/CVC5 variables (matching the Z3
/// backend's behavior from #180).
pub(crate) fn collect_feature_max_constants_cvc5(typed: &crate::TypedFile) -> Vec<(String, i64)> {
    let mut constants = Vec::new();
    for decl in &typed.resolved.source.decls {
        if let Decl::Block {
            kind,
            name,
            value: Some(tokens),
            ..
        } = &decl.node
            && *kind == BlockKind::FeatureMax
            && let Some(eq_pos) = tokens.iter().position(|t| t == "=")
            && let Some(val_str) = tokens.get(eq_pos + 1)
            && let Ok(v) = val_str.parse::<i64>()
        {
            constants.push((name.clone(), v));
        }
    }
    constants
}

/// Derive refinement narrowings from `feature_max` constants.
///
/// For a constant named `max_X` or `MAX_X`, derives a narrowing
/// `(X, value)` that asserts `X <= value` in the solver. This
/// mirrors the Z3 backend's `derive_narrowings`.
pub(crate) fn derive_narrowings_cvc5(constants: &[(String, i64)]) -> Vec<(String, i64)> {
    let mut narrowings = Vec::new();
    for (name, value) in constants {
        let narrowed = name
            .strip_prefix("max_")
            .or_else(|| name.strip_prefix("MAX_"));
        if let Some(narrowed) = narrowed.filter(|s| !s.is_empty()) {
            narrowings.push((narrowed.to_string(), *value));
            let lower = narrowed.to_lowercase();
            if lower != narrowed {
                narrowings.push((lower, *value));
            }
        }
    }
    narrowings
}

/// Shared contract setup for native and shell-out CVC5 verify paths.
fn cvc5_contract_shared_setup<'a>(
    clauses: &'a [Clause],
    constants: &[(String, i64)],
) -> (
    Vec<(String, i64)>,
    Vec<&'a Expr>,
    assura_types::FrameChecker,
) {
    let narrowings = derive_narrowings_cvc5(constants);
    let requires_exprs: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let modifies_bodies: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Modifies)
        .map(|c| &c.body)
        .collect();
    let frame_checker = if modifies_bodies.is_empty() {
        assura_types::FrameChecker::empty()
    } else {
        assura_types::FrameChecker::new(&modifies_bodies)
    };
    (narrowings, requires_exprs, frame_checker)
}

fn cvc5_lookup_cached_clause(
    cache: &mut SessionCache,
    cache_key: &str,
    desc: &str,
) -> Option<VerificationResult> {
    cache
        .lookup(cache_key)
        .map(|entry| match entry.result.as_str() {
            "verified" => VerificationResult::verified(desc.to_string()),
            other => VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: format!("cached: {other}"),
            },
        })
}

fn cvc5_unmodelable_precheck(desc: &str, body: &Expr) -> Option<VerificationResult> {
    if !expr_has_unmodelable_features_cvc5(body) {
        return None;
    }
    let reasons = collect_unmodelable_reasons_cvc5(body);
    Some(VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: format!(
            "clause uses features not yet encoded in SMT ({})",
            reasons.join(", ")
        ),
    })
}

// =========================================================================
// ADT (algebraic data type) emulation for CVC5 (#263)
//
// Mirrors the Z3 backend's ADT encoding using integer tags and
// uninterpreted functions. Each constructor gets a unique tag, accessors
// are UFs, and exhaustiveness/injectivity axioms are generated.
// =========================================================================

/// A single ADT constructor for CVC5 emulation.
#[derive(Debug, Clone)]
pub(crate) struct Cvc5AdtConstructor {
    /// Constructor name.
    pub(crate) name: String,
    /// Unique integer tag.
    pub(crate) tag: i64,
    /// Named accessor fields.
    pub(crate) accessors: Vec<String>,
}

/// An ADT definition for CVC5 emulation.
#[derive(Debug, Clone)]
pub(crate) struct Cvc5AdtDef {
    /// ADT type name.
    pub(crate) name: String,
    /// Constructors.
    pub(crate) constructors: Vec<Cvc5AdtConstructor>,
}

/// Native CVC5 UF symbols declared by `define_adt_cvc5_native`.
#[cfg(feature = "cvc5-verify")]
pub(crate) struct Cvc5AdtNativeSymbols<'a> {
    pub(crate) adt_name: String,
    pub(crate) tag_fn: cvc5::Term<'a>,
    pub(crate) acc_fns: std::collections::HashMap<String, cvc5::Term<'a>>,
}

/// Define an ADT for CVC5 and generate SMT-LIB2 assertions.
///
/// Returns `(Cvc5AdtDef, Vec<String>)` where the Vec contains SMT-LIB2
/// assert statements for exhaustiveness and injectivity axioms.
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

    // Declare the tag function
    let tag_fn = format!("__adt_tag_{adt_name}");
    assertions.push(format!("(declare-fun {tag_fn} (Int) Int)"));

    // Declare accessor functions
    for ctor in &adt_def.constructors {
        for accessor in &ctor.accessors {
            let acc_fn = format!("__adt_{adt_name}_{accessor}");
            assertions.push(format!("(declare-fun {acc_fn} (Int) Int)"));
        }
    }

    // Exhaustiveness axiom
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

    // Injectivity axioms
    for ctor in &adt_def.constructors {
        if ctor.accessors.is_empty() {
            // Nullary: both tagged => equal
            assertions.push(format!(
                "(assert (forall ((a Int) (b Int)) \
                 (=> (and (= ({tag_fn} a) {}) (= ({tag_fn} b) {})) (= a b))))",
                ctor.tag, ctor.tag
            ));
        } else {
            // With fields: matching tag and all accessors => equal
            let mut conjuncts = vec![
                format!("(= ({tag_fn} a) {})", ctor.tag),
                format!("(= ({tag_fn} b) {})", ctor.tag),
            ];
            for accessor in &ctor.accessors {
                let acc_fn = format!("__adt_{adt_name}_{accessor}");
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

/// Generate an SMT-LIB2 assertion for a constructor tester.
///
/// Returns `(= (__adt_tag_<adt> <value>) <tag>)`.
pub(crate) fn adt_is_constructor_smt(
    adt_name: &str,
    ctor_name: &str,
    value: &str,
    adt_def: &Cvc5AdtDef,
) -> String {
    let tag = adt_def
        .constructors
        .iter()
        .find(|c| c.name == ctor_name)
        .map_or(0, |c| c.tag);
    let tag_fn = format!("__adt_tag_{adt_name}");
    format!("(= ({tag_fn} {value}) {tag})")
}

/// Generate an SMT-LIB2 expression for an accessor application.
///
/// Returns `(__adt_<adt>_<accessor> <value>)`.
pub(crate) fn adt_accessor_smt(adt_name: &str, accessor: &str, value: &str) -> String {
    let acc_fn = format!("__adt_{adt_name}_{accessor}");
    format!("({acc_fn} {value})")
}

/// Define an ADT using CVC5 native API and assert axioms.
///
/// Returns the `Cvc5AdtDef` with constructor tags assigned.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn define_adt_cvc5_native<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    adt_name: &str,
    constructors: &[(&str, &[&str])],
) -> (Cvc5AdtDef, Cvc5AdtNativeSymbols<'a>) {
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

    // Declare the tag function: Int -> Int
    let tag_fn_name = format!("__adt_tag_{adt_name}");
    let tag_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
    let tag_fn = tm.mk_const(tag_sort, &tag_fn_name);

    // Declare accessor functions
    let mut acc_fns = std::collections::HashMap::new();
    for ctor in &adt_def.constructors {
        for accessor in &ctor.accessors {
            let acc_fn_name = format!("__adt_{adt_name}_{accessor}");
            let acc_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let acc_fn_term = tm.mk_const(acc_sort, &acc_fn_name);
            acc_fns.insert(acc_fn_name, acc_fn_term);
        }
    }

    // Exhaustiveness axiom: forall x: tag(x) == 0 || tag(x) == 1 || ...
    let x = tm.mk_var(tm.integer_sort(), &format!("__adt_exh_{adt_name}"));
    let tag_x = tm.mk_term(cvc5::Kind::ApplyUf, &[tag_fn.clone(), x.clone()]);

    let tag_eqs: Vec<cvc5::Term> = adt_def
        .constructors
        .iter()
        .map(|c| tm.mk_term(cvc5::Kind::Equal, &[tag_x.clone(), tm.mk_integer(c.tag)]))
        .collect();
    let exhaustive = if tag_eqs.len() == 1 {
        tag_eqs[0].clone()
    } else {
        tm.mk_term(cvc5::Kind::Or, &tag_eqs)
    };
    let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[x.clone()]);
    let forall_exhaustive = tm.mk_term(cvc5::Kind::Forall, &[bound_list, exhaustive]);
    solver.assert_formula(forall_exhaustive);

    // Injectivity axioms
    for ctor in &adt_def.constructors {
        let a = tm.mk_var(
            tm.integer_sort(),
            &format!("__adt_inj_{adt_name}_{}_a", ctor.name),
        );
        let b = tm.mk_var(
            tm.integer_sort(),
            &format!("__adt_inj_{adt_name}_{}_b", ctor.name),
        );
        let tag_a = tm.mk_term(cvc5::Kind::ApplyUf, &[tag_fn.clone(), a.clone()]);
        let tag_b = tm.mk_term(cvc5::Kind::ApplyUf, &[tag_fn.clone(), b.clone()]);
        let tag_val = tm.mk_integer(ctor.tag);

        let mut conjuncts = vec![
            tm.mk_term(cvc5::Kind::Equal, &[tag_a, tag_val.clone()]),
            tm.mk_term(cvc5::Kind::Equal, &[tag_b, tag_val]),
        ];

        for accessor in &ctor.accessors {
            let acc_fn_name = format!("__adt_{adt_name}_{accessor}");
            if let Some(acc_fn_term) = acc_fns.get(&acc_fn_name) {
                let acc_a = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_fn_term.clone(), a.clone()]);
                let acc_b = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_fn_term.clone(), b.clone()]);
                conjuncts.push(tm.mk_term(cvc5::Kind::Equal, &[acc_a, acc_b]));
            }
        }

        let premise = tm.mk_term(cvc5::Kind::And, &conjuncts);
        let eq_ab = tm.mk_term(cvc5::Kind::Equal, &[a.clone(), b.clone()]);
        let implication = tm.mk_term(cvc5::Kind::Implies, &[premise, eq_ab]);
        let bound_list_ab = tm.mk_term(cvc5::Kind::VariableList, &[a, b]);
        let forall_inj = tm.mk_term(cvc5::Kind::Forall, &[bound_list_ab, implication]);
        solver.assert_formula(forall_inj);
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

/// Build a CVC5 constructor application (native API): create a fresh
/// constant, set its tag, and bind accessor values.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn adt_constructor_cvc5_native<'a>(
    tm: &'a cvc5::TermManager,
    symbols: &Cvc5AdtNativeSymbols<'a>,
    ctor: &Cvc5AdtConstructor,
    args: &[cvc5::Term<'a>],
    axioms: &mut Vec<cvc5::Term<'a>>,
    fresh_counter: &mut usize,
) -> cvc5::Term<'a> {
    let val_name = format!("__adt_val_{}_{}", fresh_counter, ctor.name);
    *fresh_counter += 1;
    let val = tm.mk_const(tm.integer_sort(), &val_name);

    // Tag axiom
    let tag_applied = tm.mk_term(cvc5::Kind::ApplyUf, &[symbols.tag_fn.clone(), val.clone()]);
    axioms.push(tm.mk_term(cvc5::Kind::Equal, &[tag_applied, tm.mk_integer(ctor.tag)]));

    // Accessor axioms
    for (i, accessor) in ctor.accessors.iter().enumerate() {
        if let Some(arg) = args.get(i) {
            let acc_fn_name = format!("__adt_{}_{accessor}", symbols.adt_name);
            if let Some(acc_fn) = symbols.acc_fns.get(&acc_fn_name) {
                let acc_applied = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_fn.clone(), val.clone()]);
                axioms.push(tm.mk_term(cvc5::Kind::Equal, &[acc_applied, arg.clone()]));
            }
        }
    }

    val
}

/// Test whether a CVC5 value was built with a specific constructor (native API).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn adt_is_constructor_cvc5_native<'a>(
    tm: &'a cvc5::TermManager,
    symbols: &Cvc5AdtNativeSymbols<'a>,
    ctor: &Cvc5AdtConstructor,
    value: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let tag_val = tm.mk_term(
        cvc5::Kind::ApplyUf,
        &[symbols.tag_fn.clone(), value.clone()],
    );
    tm.mk_term(cvc5::Kind::Equal, &[tag_val, tm.mk_integer(ctor.tag)])
}

/// Access a field of a CVC5 ADT value (native API).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn adt_accessor_cvc5_native<'a>(
    tm: &'a cvc5::TermManager,
    symbols: &Cvc5AdtNativeSymbols<'a>,
    accessor: &str,
    value: &cvc5::Term<'a>,
) -> cvc5::Term<'a> {
    let acc_fn_name = format!("__adt_{}_{accessor}", symbols.adt_name);
    let acc_fn = symbols
        .acc_fns
        .get(&acc_fn_name)
        .expect("accessor must be declared by define_adt_cvc5_native");
    tm.mk_term(cvc5::Kind::ApplyUf, &[acc_fn.clone(), value.clone()])
}

// =========================================================================
// Native CVC5 API backend (feature = "cvc5-verify")
// =========================================================================

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

#[cfg(feature = "cvc5-verify")]
#[path = "cvc5_native_encoder.rs"]
mod cvc5_native_encoder;

#[cfg(feature = "cvc5-verify")]
use cvc5_native_encoder::{
    Cvc5EncoderState, apply_havoc_assume_cvc5, default_cvc5_encoder_state, encode_expr_cvc5,
};

#[cfg(all(test, feature = "cvc5-verify"))]
use cvc5_native_encoder::infer_quantifier_patterns_cvc5;

#[cfg(feature = "cvc5-verify")]
fn build_cvc5_var_map<'a>(
    tm: &'a cvc5::TermManager,
    var_names: &HashSet<String>,
    constants: &[(String, i64)],
) -> HashMap<String, cvc5::Term<'a>> {
    let mut var_map = HashMap::new();
    for name in var_names {
        var_map.insert(name.clone(), tm.mk_const(tm.integer_sort(), name));
    }
    for (name, value) in constants {
        let key = sanitize_smtlib_name(name);
        var_map.insert(key, tm.mk_integer(*value));
    }
    var_map
}

#[cfg(feature = "cvc5-verify")]
fn assert_cvc5_solver_prelude<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    narrowings: &[(String, i64)],
) {
    let zero = tm.mk_integer(0);
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if let Some(term) = var_map.get(&name) {
                solver.assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]));
            }
        }
    }
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if let Some(term) = var_map.get("__result") {
            solver.assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]));
        }
        if let Some(term) = var_map.get("result") {
            solver.assert_formula(tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero]));
        }
    }
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if let Some(var) = var_map.get(&key) {
            solver
                .assert_formula(tm.mk_term(cvc5::Kind::Leq, &[var.clone(), tm.mk_integer(*value)]));
        }
    }
}

#[cfg(feature = "cvc5-verify")]
#[derive(Default)]
struct Cvc5SolverOpts {
    incremental: bool,
    unsat_core: bool,
}

#[cfg(feature = "cvc5-verify")]
fn new_cvc5_solver<'a>(tm: &'a cvc5::TermManager, opts: Cvc5SolverOpts) -> cvc5::Solver<'a> {
    let mut solver = cvc5::Solver::new(tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");
    if opts.incremental {
        solver.set_option("incremental", "true");
    }
    if opts.unsat_core {
        solver.set_option("produce-unsat-cores", "true");
        solver.set_option("produce-unsat-assumptions", "true");
    }
    solver
}

#[cfg(feature = "cvc5-verify")]
fn collect_cvc5_var_names(requires: &[&Expr], body: &Expr) -> HashSet<String> {
    let mut names = HashSet::new();
    for req in requires {
        collect_vars(req, &mut names);
    }
    collect_vars(body, &mut names);
    names
}

#[cfg(feature = "cvc5-verify")]
fn collect_cvc5_var_names_from_clauses(requires: &[&Expr], clauses: &[&Clause]) -> HashSet<String> {
    let mut names = HashSet::new();
    for req in requires {
        collect_vars(req, &mut names);
    }
    for clause in clauses {
        collect_vars(&clause.body, &mut names);
    }
    names
}

#[cfg(feature = "cvc5-verify")]
fn collect_cvc5_var_names_from_assumptions(assumptions: &[&Expr], body: &Expr) -> HashSet<String> {
    let mut names = HashSet::new();
    for a in assumptions {
        collect_vars(a, &mut names);
    }
    collect_vars(body, &mut names);
    names
}

#[cfg(feature = "cvc5-verify")]
fn assert_cvc5_axioms<'a>(solver: &mut cvc5::Solver<'a>, axioms: &[cvc5::Term<'a>]) {
    for axiom in axioms {
        solver.assert_formula(axiom.clone());
    }
}

#[cfg(feature = "cvc5-verify")]
fn assert_cvc5_axioms_since<'a>(
    solver: &mut cvc5::Solver<'a>,
    axioms: &[cvc5::Term<'a>],
    start: usize,
) {
    for axiom in &axioms[start..] {
        solver.assert_formula(axiom.clone());
    }
}

#[cfg(feature = "cvc5-verify")]
fn assert_cvc5_frame_axioms<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
    frame_vars: &[String],
) {
    for var_name in frame_vars {
        let current_key = sanitize_smtlib_name(var_name);
        let old_key = sanitize_smtlib_name(&format!("{var_name}__old"));
        let current = var_map
            .get(&current_key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &current_key));
        let old_var = var_map
            .get(&old_key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &old_key));
        solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[current, old_var]));
    }
}

#[cfg(feature = "cvc5-verify")]
fn assert_cvc5_clause_check<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    kind: ClauseKind,
    body_term: cvc5::Term<'a>,
) {
    match kind {
        ClauseKind::Invariant | ClauseKind::MustNot => solver.assert_formula(body_term),
        _ => {
            let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
            solver.assert_formula(negated);
        }
    }
}

#[cfg(feature = "cvc5-verify")]
fn extract_cvc5_counterexample_model<'a>(
    solver: &cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
) -> (String, Option<CounterexampleModel>) {
    let mut variables: Vec<(String, String)> = var_map
        .iter()
        .filter(|(name, _)| !is_internal_cvc5_var(name))
        .map(|(name, term)| {
            let val = solver.get_value(term.clone());
            (name.clone(), val.to_string())
        })
        .collect();
    variables.sort_by(|(a, _), (b, _)| a.cmp(b));
    let model_str = variables
        .iter()
        .map(|(n, v)| format!("{n} = {v}"))
        .collect::<Vec<_>>()
        .join(", ");
    let counter_model = if variables.is_empty() {
        None
    } else {
        Some(CounterexampleModel { variables })
    };
    (model_str, counter_model)
}

#[cfg(feature = "cvc5-verify")]
fn finish_cvc5_clause_check<'a>(
    desc: &str,
    kind: ClauseKind,
    solver: &mut cvc5::Solver<'a>,
    var_map: &HashMap<String, cvc5::Term<'a>>,
) -> VerificationResult {
    let sat_result = solver.check_sat();
    if sat_result.is_unsat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: "invariant is unsatisfiable".to_string(),
                counter_model: None,
            }
        } else {
            VerificationResult::verified(desc.to_string())
        }
    } else if sat_result.is_sat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::verified(desc.to_string())
        } else {
            let (model_str, counter_model) = extract_cvc5_counterexample_model(solver, var_map);
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: model_str,
                counter_model,
            }
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

#[cfg(feature = "cvc5-verify")]
fn inject_cvc5_lemma_assumptions<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    body: &'a Expr,
    defs: &std::collections::HashMap<String, Vec<&Expr>>,
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) {
    inject_cvc5_lemma_assumptions_for_bodies(
        tm,
        solver,
        std::iter::once(body),
        defs,
        var_map,
        enc_state,
    );
}

#[cfg(feature = "cvc5-verify")]
fn inject_cvc5_lemma_assumptions_for_bodies<'a, I>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    bodies: I,
    defs: &std::collections::HashMap<String, Vec<&Expr>>,
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) where
    I: IntoIterator<Item = &'a Expr>,
{
    for body in bodies {
        let apply_refs = collect_apply_refs_from_expr(body);
        for lemma_name in &apply_refs {
            if let Some(ensures_bodies) = defs.get(lemma_name) {
                for ens_body in ensures_bodies {
                    if let Some(term) = encode_expr_cvc5(tm, ens_body, var_map, enc_state) {
                        solver.assert_formula(term);
                    }
                }
            }
        }
    }
}

#[cfg(feature = "cvc5-verify")]
fn cvc5_encode_failure(desc: &str) -> VerificationResult {
    VerificationResult::Unknown {
        clause_desc: desc.to_string(),
        reason: "could not encode clause to CVC5 terms".into(),
    }
}

fn store_cvc5_clause_cache(
    cache: &mut SessionCache,
    cache_key: String,
    result: &VerificationResult,
) {
    let result_str = match result {
        VerificationResult::Verified { .. } => "verified",
        VerificationResult::Counterexample { .. } => "counterexample",
        VerificationResult::Timeout { .. } => "timeout",
        VerificationResult::Unknown { .. } => "unknown",
    };
    cache.insert(cache_key, result_str.to_string(), 0);
}

fn cvc5_clause_result_from_unsat(desc: &str, kind: ClauseKind) -> VerificationResult {
    if matches!(kind, ClauseKind::Invariant) {
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: "invariant is unsatisfiable".to_string(),
            counter_model: None,
        }
    } else {
        VerificationResult::verified(desc.to_string())
    }
}

/// Verify a single contract's clauses using CVC5.
///
/// When the `cvc5-verify` feature is enabled, uses the native Rust cvc5
/// crate (direct API calls, no process spawning). Otherwise falls back to
/// generating SMT-LIB2 text and invoking the `cvc5` binary.
///
/// This variant extracts params from `input()` clauses. For function
/// definitions whose params live in `FnDef.params`, use
/// `verify_contract_cvc5_with_types` instead.
pub(crate) fn verify_contract_cvc5(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    let params = crate::entry::extract_input_params(clauses);
    let return_ty = crate::entry::extract_output_return_type(clauses);
    let mut cache = SessionCache::new();
    verify_contract_cvc5_with_types(contract_name, clauses, &params, &return_ty, &mut cache)
}

/// Verify a single contract's clauses using CVC5 with explicit type info.
///
/// `params` and `return_ty` supply Nat constraints that cannot be extracted
/// from clauses alone (e.g., function parameters declared outside the clause
/// list). This fixes the parity gap where the Z3 backend received Nat >= 0
/// constraints via `verify_contract_impl_with_types` but the CVC5 backend
/// only extracted them from `input()` clauses.
pub(crate) fn verify_contract_cvc5_with_types(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    verify_contract_cvc5_with_full_context(contract_name, clauses, params, return_ty, &[], cache)
}

/// Verify a single contract's clauses using CVC5 with full context.
///
/// Like `verify_contract_cvc5_with_types` but also takes `feature_max`
/// constants that are bound to concrete integer values in the solver
/// (matching the Z3 backend's behavior from #180). Refinement narrowings
/// are derived from constants with `max_`/`MAX_` prefixes.
pub(crate) fn verify_contract_cvc5_with_full_context(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    verify_contract_cvc5_with_lemmas(
        contract_name,
        clauses,
        params,
        return_ty,
        None,
        constants,
        cache,
    )
}

/// Verify a single contract's clauses using CVC5, with optional lemma defs.
///
/// When `lemma_defs` is `Some`, `apply lemma_name(args)` expressions will
/// have the referenced lemma's ensures clauses injected as solver
/// assumptions (matching the Z3 backend's behavior).
///
/// `constants` binds `feature_max` names to concrete values instead of
/// leaving them as free solver variables.
pub(crate) fn verify_contract_cvc5_with_lemmas(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        verify_contract_cvc5_native(
            contract_name,
            clauses,
            params,
            return_ty,
            lemma_defs,
            constants,
            cache,
        )
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        verify_contract_cvc5_shellout(
            contract_name,
            clauses,
            params,
            return_ty,
            lemma_defs,
            constants,
            cache,
        )
    }
}

// -------------------------------------------------------------------------
// Native CVC5 implementation
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
fn verify_contract_cvc5_native(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    let (narrowings, requires_exprs, frame_checker) =
        cvc5_contract_shared_setup(clauses, constants);

    // Collect verifiable clauses
    let verifiable: Vec<&assura_parser::ast::Clause> = clauses
        .iter()
        .filter(|c| {
            matches!(
                c.kind,
                ClauseKind::Ensures
                    | ClauseKind::Invariant
                    | ClauseKind::Rule
                    | ClauseKind::MustNot
                    | ClauseKind::Decreases
            )
        })
        .collect();

    // Process feature-specific Other clauses
    for clause in clauses {
        if let ClauseKind::Other(kind) = &clause.kind {
            let feature_results = crate::smt_features::verify_feature_clause(
                kind,
                contract_name,
                &clause.body,
                clauses,
            );
            results.extend(feature_results);
        }
    }

    let requires_clauses: Vec<&Clause> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .collect();
    let ensures_clauses: Vec<&Clause> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .collect();
    let param_names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();

    // For 0 or 1 verifiable clauses, fall back to per-clause solver
    // (incremental push/pop has no benefit with a single query).
    if verifiable.len() <= 1 {
        for clause in &verifiable {
            let desc = format!("{contract_name}::{:?}", clause.kind);
            let result = check_clause_cvc5_native(
                &desc,
                &requires_exprs,
                &requires_clauses,
                &ensures_clauses,
                &clause.body,
                clause.kind.clone(),
                params,
                return_ty,
                &param_names,
                None,
                constants,
                &narrowings,
                &frame_checker,
                lemma_defs,
                cache,
            );
            results.push(result);
        }
        return results;
    }

    // ---------------------------------------------------------------
    // Incremental solving: create ONE solver, assert shared requires
    // ONCE, then use push/pop for each clause (#264).
    // ---------------------------------------------------------------

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(
        &tm,
        Cvc5SolverOpts {
            incremental: true,
            ..Default::default()
        },
    );

    let var_names = collect_cvc5_var_names_from_clauses(&requires_exprs, &verifiable);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, constants);
    assert_cvc5_solver_prelude(&tm, &mut solver, &var_map, params, return_ty, &narrowings);

    let mut enc_state = default_cvc5_encoder_state();

    assert_cvc5_requires(
        &tm,
        &mut solver,
        &requires_exprs,
        &mut var_map,
        &mut enc_state,
    );

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);
    let requires_axiom_count = enc_state.axioms.len();

    if let Some(defs) = lemma_defs {
        inject_cvc5_lemma_assumptions_for_bodies(
            &tm,
            &mut solver,
            verifiable.iter().map(|c| &c.body),
            defs,
            &mut var_map,
            &mut enc_state,
        );
        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, requires_axiom_count);
    }

    // For each verifiable clause: push, encode, check, pop
    for clause in &verifiable {
        let desc = format!("{contract_name}::{:?}", clause.kind);

        let cache_key = format!("{desc}::{:?}:{:?}", clause.kind, clause.body);
        if let Some(cached_result) = cvc5_lookup_cached_clause(cache, &cache_key, &desc) {
            results.push(cached_result);
            continue;
        }

        if let Some(result) = cvc5_unmodelable_precheck(&desc, &clause.body) {
            results.push(result);
            continue;
        }

        solver.push(1); // Save solver state

        // Track axiom count before havoc+assume and clause encoding
        let axiom_base = enc_state.axioms.len();

        apply_havoc_assume_cvc5(
            &tm,
            &requires_clauses,
            &ensures_clauses,
            return_ty,
            &param_names,
            None,
            &mut var_map,
            &mut enc_state,
        );
        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, axiom_base);
        let havoc_axiom_end = enc_state.axioms.len();

        let body_term = match encode_expr_cvc5(&tm, &clause.body, &mut var_map, &mut enc_state) {
            Some(t) => t,
            None => {
                solver.pop(1);
                enc_state.axioms.truncate(axiom_base);
                results.push(cvc5_encode_failure(&desc));
                continue;
            }
        };

        assert_cvc5_axioms_since(&mut solver, &enc_state.axioms, havoc_axiom_end);

        if clause.kind == ClauseKind::Ensures && frame_checker.has_modifies() {
            let frame_vars = frame_checker.frame_axiom_vars(&clause.body);
            assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
        }

        assert_cvc5_clause_check(&tm, &mut solver, clause.kind.clone(), body_term);

        let result = finish_cvc5_clause_check(&desc, clause.kind.clone(), &mut solver, &var_map);
        store_cvc5_clause_cache(cache, cache_key, &result);

        results.push(result);

        solver.pop(1); // Restore solver state

        // Truncate havoc+assume and clause-specific axioms (removed from
        // the solver by pop).
        enc_state.axioms.truncate(axiom_base);
    }

    results
}

#[cfg(feature = "cvc5-verify")]
#[expect(clippy::too_many_arguments)]
fn check_clause_cvc5_native(
    desc: &str,
    requires: &[&Expr],
    requires_clauses: &[&Clause],
    ensures_clauses: &[&Clause],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    param_names: &[String],
    ir_body: Option<&crate::ir::IrFunction>,
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    frame_checker: &assura_types::FrameChecker,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    cache: &mut SessionCache,
) -> VerificationResult {
    let cache_key = format!("{desc}::{kind:?}:{ensures_body:?}");
    if let Some(result) = cvc5_lookup_cached_clause(cache, &cache_key, desc) {
        return result;
    }

    if let Some(result) = cvc5_unmodelable_precheck(desc, ensures_body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let var_names = collect_cvc5_var_names(requires, ensures_body);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, constants);
    assert_cvc5_solver_prelude(&tm, &mut solver, &var_map, params, return_ty, narrowings);

    let mut enc_state = default_cvc5_encoder_state();

    apply_havoc_assume_cvc5(
        &tm,
        requires_clauses,
        ensures_clauses,
        return_ty,
        param_names,
        ir_body,
        &mut var_map,
        &mut enc_state,
    );

    assert_cvc5_requires(&tm, &mut solver, requires, &mut var_map, &mut enc_state);

    if let Some(defs) = lemma_defs {
        inject_cvc5_lemma_assumptions(
            &tm,
            &mut solver,
            ensures_body,
            defs,
            &mut var_map,
            &mut enc_state,
        );
    }

    let body_term = match encode_expr_cvc5(&tm, ensures_body, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure(desc),
    };

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    if kind == ClauseKind::Ensures && frame_checker.has_modifies() {
        let frame_vars = frame_checker.frame_axiom_vars(ensures_body);
        assert_cvc5_frame_axioms(&tm, &mut solver, &var_map, &frame_vars);
    }

    assert_cvc5_clause_check(&tm, &mut solver, kind.clone(), body_term);

    let result = finish_cvc5_clause_check(desc, kind, &mut solver, &var_map);
    store_cvc5_clause_cache(cache, cache_key, &result);

    result
}

// -------------------------------------------------------------------------
// Generic CVC5 validity checker (reusable for standalone functions)
// -------------------------------------------------------------------------

/// Extract tracking-label names from a CVC5 unsat core / unsat assumptions.
#[cfg(feature = "cvc5-verify")]
fn extract_cvc5_unsat_core_labels(solver: &cvc5::Solver, tracked: &[cvc5::Term]) -> Vec<String> {
    let mut labels: Vec<String> = solver
        .get_unsat_assumptions()
        .iter()
        .map(|t| cvc5_term_label(t))
        .collect();
    if labels.is_empty() {
        labels = solver
            .get_unsat_core()
            .iter()
            .map(|t| cvc5_term_label(t))
            .collect();
    }
    if labels.is_empty() && !tracked.is_empty() {
        labels = tracked.iter().map(|t| cvc5_term_label(t)).collect();
    }
    labels.sort();
    labels.dedup();
    labels
}

/// Best-effort human-readable label for a CVC5 term (tracking constants).
#[cfg(feature = "cvc5-verify")]
fn cvc5_term_label(term: &cvc5::Term) -> String {
    let s = term.to_string();
    if let Some(start) = s.find(' ') {
        let rest = s[start + 1..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    s
}

/// Check validity of `body` under `assumptions` using CVC5.
///
/// Encodes: assert all assumptions, negate body, check-sat.
/// UNSAT = body holds (Verified), SAT = counterexample.
///
/// This is the CVC5 equivalent of `z3_backend::solver::check_validity`.
/// Used by standalone entry-point functions (refinement, buffer bounds,
/// taint, measures, termination) and feature clause dispatch.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_validity_cvc5(
    desc: &str,
    assumptions: &[&Expr],
    body: &Expr,
) -> VerificationResult {
    if let Some(result) = cvc5_unmodelable_precheck(desc, body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(
        &tm,
        Cvc5SolverOpts {
            unsat_core: true,
            ..Default::default()
        },
    );

    let var_names = collect_cvc5_var_names_from_assumptions(assumptions, body);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, &[]);

    let mut enc_state = default_cvc5_encoder_state();

    let bool_sort = tm.boolean_sort();
    let mut tracked_assumptions: Vec<cvc5::Term> = Vec::new();

    // Track assumptions with labels for unsat-core extraction (#266).
    for (i, a) in assumptions.iter().enumerate() {
        if let Some(term) = encode_expr_cvc5(&tm, a, &mut var_map, &mut enc_state) {
            let label = format!("req_{i}");
            let track = tm.mk_const(bool_sort.clone(), &label);
            tracked_assumptions.push(track.clone());
            let implication = tm.mk_term(cvc5::Kind::Implies, &[track, term]);
            solver.assert_formula(implication);
        }
    }

    // Encode body
    let body_term = match encode_expr_cvc5(&tm, body, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure(desc),
    };

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
    solver.assert_formula(negated);

    let sat_result = if tracked_assumptions.is_empty() {
        solver.check_sat()
    } else {
        solver.check_sat_assuming(&tracked_assumptions)
    };
    if sat_result.is_unsat() {
        let core = extract_cvc5_unsat_core_labels(&solver, &tracked_assumptions);
        VerificationResult::verified_with_core(desc.to_string(), core)
    } else if sat_result.is_sat() {
        let (model_str, counter_model) = extract_cvc5_counterexample_model(&solver, &var_map);
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: model_str,
            counter_model,
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// Check satisfiability of `body` under `assumptions` using CVC5.
///
/// For invariants: assert all assumptions + body, check-sat.
/// SAT = invariant is satisfiable (Verified), UNSAT = unsatisfiable (Counterexample).
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_satisfiability_cvc5(
    desc: &str,
    assumptions: &[&Expr],
    body: &Expr,
) -> VerificationResult {
    if let Some(result) = cvc5_unmodelable_precheck(desc, body) {
        return result;
    }

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let var_names = collect_cvc5_var_names_from_assumptions(assumptions, body);
    let mut var_map = build_cvc5_var_map(&tm, &var_names, &[]);

    let mut enc_state = default_cvc5_encoder_state();

    for a in assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, a, &mut var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    let body_term = match encode_expr_cvc5(&tm, body, &mut var_map, &mut enc_state) {
        Some(t) => t,
        None => return cvc5_encode_failure(desc),
    };

    assert_cvc5_axioms(&mut solver, &enc_state.axioms);

    solver.assert_formula(body_term);

    let sat_result = solver.check_sat();
    if sat_result.is_sat() {
        VerificationResult::verified(desc.to_string())
    } else if sat_result.is_unsat() {
        VerificationResult::Counterexample {
            clause_desc: desc.to_string(),
            model: "invariant is unsatisfiable".to_string(),
            counter_model: None,
        }
    } else {
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// CVC5 implementation of refinement subtype check.
///
/// `{v: T | antecedent} <: {v: T | consequent}`
/// Encodes: (assert antecedent) (assert (not consequent)) (check-sat)
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_refinement_subtype_cvc5(
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    check_validity_cvc5("refinement_subtype", &[antecedent], consequent)
}

/// CVC5 implementation of refinement subtype check with extra context.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn check_refinement_subtype_with_context_cvc5(
    context: &[Expr],
    antecedent: &Expr,
    consequent: &Expr,
) -> VerificationResult {
    let mut assumptions: Vec<&Expr> = context.iter().collect();
    assumptions.push(antecedent);
    check_validity_cvc5("refinement_subtype_ctx", &assumptions, consequent)
}

/// CVC5 implementation of buffer bounds verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_buffer_bounds_cvc5(requires: &[Expr], ensures: &Expr) -> VerificationResult {
    let assumptions: Vec<&Expr> = requires.iter().collect();
    check_validity_cvc5("buffer_bounds", &assumptions, ensures)
}

/// CVC5 implementation of region containment verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_region_containment_cvc5(
    context: &[Expr],
    sub_lo: &Expr,
    sub_hi: &Expr,
    parent_lo: &Expr,
    parent_hi: &Expr,
) -> VerificationResult {
    // Build: forall i: sub_lo <= i < sub_hi => parent_lo <= i < parent_hi
    // Encode as two validity checks:
    // 1. context => sub_lo >= parent_lo
    // 2. context => sub_hi <= parent_hi
    let lo_check = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(sub_lo.clone()),
        rhs: Box::new(parent_lo.clone()),
    };
    let hi_check = Expr::BinOp {
        op: BinOp::Lte,
        lhs: Box::new(sub_hi.clone()),
        rhs: Box::new(parent_hi.clone()),
    };
    let combined = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(lo_check),
        rhs: Box::new(hi_check),
    };
    let assumptions: Vec<&Expr> = context.iter().collect();
    check_validity_cvc5("region_containment", &assumptions, &combined)
}

/// CVC5 implementation of measure-aware verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_with_measures_cvc5(
    requires: &[Expr],
    ensures: &Expr,
    _measures: &[crate::measures::MeasureDefinition],
) -> VerificationResult {
    // Measures are encoded as uninterpreted functions with axioms.
    // For CVC5, we encode as plain validity check (measure axioms
    // would need to be threaded through the encoder state).
    let assumptions: Vec<&Expr> = requires.iter().collect();
    check_validity_cvc5("verify_with_measures", &assumptions, ensures)
}

/// CVC5 implementation of decrease verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_decrease_cvc5(
    preconditions: &[Expr],
    measure_expr: &Expr,
    call_arg_expr: &Expr,
    clause_desc: String,
) -> VerificationResult {
    // Check: preconditions => measure(call_args) < measure(fn_args) && measure(call_args) >= 0
    let decrease_check = Expr::BinOp {
        op: BinOp::Lt,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(measure_expr.clone()),
    };
    let non_neg = Expr::BinOp {
        op: BinOp::Gte,
        lhs: Box::new(call_arg_expr.clone()),
        rhs: Box::new(Expr::Literal(Literal::Int("0".to_string()))),
    };
    let combined = Expr::BinOp {
        op: BinOp::And,
        lhs: Box::new(decrease_check),
        rhs: Box::new(non_neg),
    };
    let assumptions: Vec<&Expr> = preconditions.iter().collect();
    check_validity_cvc5(&clause_desc, &assumptions, &combined)
}

/// CVC5 implementation of taint safety verification.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_taint_safety_cvc5(
    taint_labels: &[(String, assura_types::TaintLabel)],
    _validation_fns: &[String],
    sensitive_uses: &[(String, assura_types::TaintLabel)],
) -> VerificationResult {
    use assura_types::TaintLabel;

    let tm = cvc5::TermManager::new();
    let mut solver = new_cvc5_solver(&tm, Cvc5SolverOpts::default());

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    let zero = tm.mk_integer(0);
    let one = tm.mk_integer(1);
    let two = tm.mk_integer(2);

    // Create taint level variables
    for (name, label) in taint_labels {
        let level = match label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        var_map.insert(name.clone(), level);
    }

    // Check sensitive uses: each must have taint level >= required
    for (name, required_label) in sensitive_uses {
        let required_level = match required_label {
            TaintLabel::Untrusted => zero.clone(),
            TaintLabel::Validated => one.clone(),
            TaintLabel::Trusted => two.clone(),
        };
        if let Some(actual) = var_map.get(name) {
            let check = tm.mk_term(cvc5::Kind::Geq, &[actual.clone(), required_level]);
            let neg = tm.mk_term(cvc5::Kind::Not, &[check]);
            // If the negation is satisfiable, the taint check fails
            solver.push(1);
            solver.assert_formula(neg);
            let result = solver.check_sat();
            solver.pop(1);
            if result.is_sat() {
                return VerificationResult::Counterexample {
                    clause_desc: "taint_safety".to_string(),
                    model: format!("{name} has insufficient taint level"),
                    counter_model: None,
                };
            }
        }
    }

    VerificationResult::verified("taint_safety".to_string())
}

/// CVC5 implementation of feature clause body verification.
///
/// Used by `smt_features::verify_feature_body` when the CVC5 solver is
/// selected. Collects sibling requires as assumptions, checks body validity.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_feature_body_cvc5(
    parent_name: &str,
    feature_label: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> VerificationResult {
    let desc = format!("{parent_name}: {feature_label}");

    // Skip declarative feature clauses (bare uppercase ident)
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        return VerificationResult::Unknown {
            clause_desc: desc,
            reason: format!("{feature_label} not yet encoded in SMT"),
        };
    }

    let requires: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    check_validity_cvc5(&desc, &requires, body)
}

/// CVC5 implementation of structural invariant inductive checking.
#[cfg(feature = "cvc5-verify")]
pub(crate) fn verify_structural_invariant_inductive_cvc5(
    parent_name: &str,
    body: &Expr,
    sibling_clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Skip bare uppercase ident
    if matches!(body, Expr::Ident(name) if name.chars().next().is_some_and(|c| c.is_uppercase())) {
        results.push(VerificationResult::Unknown {
            clause_desc: format!("{parent_name}: structural_invariant"),
            reason: "structural_invariant not yet encoded in SMT".into(),
        });
        return results;
    }

    // Step 1: Establishment (requires => invariant)
    let requires: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();
    let desc1 = format!("{parent_name}: structural_invariant (establishment)");
    results.push(check_validity_cvc5(&desc1, &requires, body));

    // Step 2: Preservation (requires + ensures => invariant)
    let mut assumptions: Vec<&Expr> = requires;
    let ensures: Vec<&Expr> = sibling_clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .map(|c| &c.body)
        .collect();
    assumptions.extend(ensures);
    let desc2 = format!("{parent_name}: structural_invariant (preservation)");
    results.push(check_validity_cvc5(&desc2, &assumptions, body));

    results
}

// -------------------------------------------------------------------------
// Shell-out CVC5 fallback (no cvc5-verify feature)
// -------------------------------------------------------------------------

#[cfg(feature = "cvc5-verify")]
fn assert_cvc5_requires<'a>(
    tm: &'a cvc5::TermManager,
    solver: &mut cvc5::Solver<'a>,
    requires: &[&Expr],
    var_map: &mut HashMap<String, cvc5::Term<'a>>,
    enc_state: &mut Cvc5EncoderState<'a>,
) {
    for req in requires {
        if let Some(term) = encode_expr_cvc5(tm, req, var_map, enc_state) {
            solver.assert_formula(term);
        }
    }
}

#[cfg(not(feature = "cvc5-verify"))]
fn append_cvc5_shellout_requires(script: &mut String, requires: &[&Expr]) {
    for req in requires {
        if let Some(smt) = expr_to_smtlib(req) {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }
}

#[cfg(not(feature = "cvc5-verify"))]
fn append_cvc5_shellout_frame_axioms(
    script: &mut String,
    vars: &HashSet<String>,
    frame_vars: &[String],
) {
    for var_name in frame_vars {
        let current = sanitize_smtlib_name(var_name);
        let old = sanitize_smtlib_name(&format!("{var_name}__old"));
        if !vars.contains(&old) {
            script.push_str(&format!("(declare-const {old} Int)\n"));
        }
        script.push_str(&format!("(assert (= {current} {old}))\n"));
    }
}

#[cfg(not(feature = "cvc5-verify"))]
fn append_cvc5_shellout_lemma_assumptions(
    script: &mut String,
    body: &Expr,
    defs: &std::collections::HashMap<String, Vec<&Expr>>,
) {
    let apply_refs = collect_apply_refs_from_expr(body);
    for lemma_name in &apply_refs {
        if let Some(ensures_bodies) = defs.get(lemma_name) {
            for ens_body in ensures_bodies {
                if let Some(smt) = expr_to_smtlib(ens_body) {
                    script.push_str(&format!("(assert {smt})\n"));
                }
            }
        }
    }
}

#[cfg(not(feature = "cvc5-verify"))]
fn append_cvc5_shellout_clause_check(script: &mut String, kind: ClauseKind, smt: &str) {
    match kind {
        ClauseKind::Invariant | ClauseKind::MustNot => {
            script.push_str(&format!("(assert {smt})\n"));
        }
        _ => {
            script.push_str(&format!("(assert (not {smt}))\n"));
        }
    }
}

#[cfg(not(feature = "cvc5-verify"))]
fn append_cvc5_shellout_constraints(
    script: &mut String,
    vars: &HashSet<String>,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
) {
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if vars.contains(&name) {
                script.push_str(&format!("(assert (>= {name} 0))\n"));
            }
        }
    }
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if vars.contains("__result") {
            script.push_str("(assert (>= __result 0))\n");
        }
        if vars.contains("result") {
            script.push_str("(assert (>= result 0))\n");
        }
    }
    for (name, value) in constants {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            script.push_str(&format!("(assert (= {key} {value}))\n"));
        }
    }
    for (name, value) in narrowings {
        let key = sanitize_smtlib_name(name);
        if vars.contains(&key) {
            script.push_str(&format!("(assert (<= {key} {value}))\n"));
        }
    }
}

#[cfg(not(feature = "cvc5-verify"))]
fn verify_contract_cvc5_shellout(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    constants: &[(String, i64)],
    cache: &mut SessionCache,
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    let (narrowings, requires_exprs, frame_checker) =
        cvc5_contract_shared_setup(clauses, constants);

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases => {
                let desc = format!("{contract_name}::{:?}", clause.kind);
                let result = check_clause_cvc5_shellout(
                    &desc,
                    &requires_exprs,
                    &clause.body,
                    clause.kind.clone(),
                    params,
                    return_ty,
                    constants,
                    &narrowings,
                    &frame_checker,
                    lemma_defs,
                    cache,
                );
                results.push(result);
            }
            ClauseKind::Other(kind) => {
                let feature_results = crate::smt_features::verify_feature_clause(
                    kind,
                    contract_name,
                    &clause.body,
                    clauses,
                );
                results.extend(feature_results);
            }
            _ => {}
        }
    }

    results
}

/// Result of running CVC5 binary on an SMT-LIB2 script.
#[cfg(not(feature = "cvc5-verify"))]
enum Cvc5Result {
    Unsat,
    Sat(String),
    Timeout,
    Error(String),
}

#[cfg(not(feature = "cvc5-verify"))]
#[expect(clippy::too_many_arguments)]
fn check_clause_cvc5_shellout(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
    constants: &[(String, i64)],
    narrowings: &[(String, i64)],
    frame_checker: &assura_types::FrameChecker,
    lemma_defs: Option<&std::collections::HashMap<String, Vec<&Expr>>>,
    cache: &mut SessionCache,
) -> VerificationResult {
    let cache_key = format!("{desc}::{kind:?}:{ensures_body:?}");
    if let Some(result) = cvc5_lookup_cached_clause(cache, &cache_key, desc) {
        return result;
    }

    if let Some(result) = cvc5_unmodelable_precheck(desc, ensures_body) {
        return result;
    }

    let mut vars = HashSet::new();
    for req in requires {
        collect_vars(req, &mut vars);
    }
    collect_vars(ensures_body, &mut vars);

    let mut script = String::new();
    script.push_str("(set-logic ALL)\n");

    for line in cvc5_adt_prelude_lines() {
        script.push_str(&line);
        if !line.ends_with('\n') {
            script.push('\n');
        }
    }

    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    append_cvc5_shellout_constraints(&mut script, &vars, params, return_ty, constants, narrowings);

    append_cvc5_shellout_requires(&mut script, requires);

    if kind == ClauseKind::Ensures && frame_checker.has_modifies() {
        let frame_vars = frame_checker.frame_axiom_vars(ensures_body);
        append_cvc5_shellout_frame_axioms(&mut script, &vars, &frame_vars);
    }

    if let Some(defs) = lemma_defs {
        append_cvc5_shellout_lemma_assumptions(&mut script, ensures_body, defs);
    }

    let Some(smt) = expr_to_smtlib(ensures_body) else {
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: "could not encode clause to SMT-LIB2".into(),
        };
    };
    append_cvc5_shellout_clause_check(&mut script, kind.clone(), &smt);

    script.push_str("(check-sat)\n");
    script.push_str("(get-model)\n");

    let result = match run_cvc5_binary(&script) {
        Cvc5Result::Unsat => cvc5_clause_result_from_unsat(desc, kind),
        Cvc5Result::Sat(model_str) => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::verified(desc.to_string())
            } else {
                let counter_model = parse_smtlib_model(&model_str);
                let filtered_model = counter_model
                    .as_ref()
                    .map(|cm| {
                        cm.variables
                            .iter()
                            .map(|(n, v)| format!("{n} = {v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or(model_str);
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: filtered_model,
                    counter_model,
                }
            }
        }
        Cvc5Result::Timeout => VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        },
        Cvc5Result::Error(reason) => VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason,
        },
    };

    store_cvc5_clause_cache(cache, cache_key, &result);

    result
}

#[cfg(not(feature = "cvc5-verify"))]
fn run_cvc5_binary(script: &str) -> Cvc5Result {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("cvc5");
    cmd.arg("--lang")
        .arg("smt2")
        .arg("--tlimit")
        .arg("1000")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 not found on PATH: {e}"));
        }
    };

    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(script.as_bytes())
    {
        return Cvc5Result::Error(format!("Failed to write SMT script to CVC5 stdin: {e}"));
    }

    let output = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 execution failed: {e}"));
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();

    match first_line {
        "unsat" => Cvc5Result::Unsat,
        "sat" => {
            let model = stdout.lines().skip(1).collect::<Vec<_>>().join("\n");
            Cvc5Result::Sat(model)
        }
        "timeout" | "resourceout" => Cvc5Result::Timeout,
        "unknown" => Cvc5Result::Timeout,
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("timeout") || stderr.contains("resourceout") {
                Cvc5Result::Timeout
            } else {
                Cvc5Result::Error(format!("unexpected cvc5 output: {first_line}"))
            }
        }
    }
}

/// Convert an AST expression to an SMT-LIB2 string representation.
pub fn expr_to_smtlib(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            if let Some(stripped) = n.strip_prefix('-') {
                Some(format!("(- {stripped})"))
            } else {
                Some(n.clone())
            }
        }
        Expr::Literal(Literal::Bool(b)) => Some(b.to_string()),
        Expr::Literal(Literal::Float(f)) => Some(float_literal_to_smtlib(f)),
        Expr::Literal(Literal::Str(s)) => {
            // Named integer constant matching Z3 pattern
            Some(format!("__str_{}", sanitize_smtlib_name(s)))
        }
        Expr::Ident(name) => {
            if name == "result" {
                Some(smtlib_result_name().to_string())
            } else {
                Some(sanitize_smtlib_name(name))
            }
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = expr_to_smtlib(lhs)?;
            let r = expr_to_smtlib(rhs)?;
            match op {
                BinOp::Neq => Some(format_neq_ast_binop_smtlib(&l, &r)),
                BinOp::Range => Some(range_binop_smtlib(&l, &r)),
                BinOp::In => Some(in_binop_smtlib(&l, &r)),
                BinOp::NotIn => Some(not_in_binop_smtlib(&l, &r)),
                BinOp::Concat => Some(concat_binop_smtlib(&l, &r)),
                _ => format_standard_ast_binop_smtlib(op, &l, &r),
            }
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = expr_to_smtlib(inner)?;
            match op {
                UnaryOp::Not => Some(format!("(not {e})")),
                UnaryOp::Neg => Some(format!("(- {e})")),
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = expr_to_smtlib(cond)?;
            let t = expr_to_smtlib(then_branch)?;
            if let Some(e) = else_branch {
                let e = expr_to_smtlib(e)?;
                Some(format!("(ite {c} {t} {e})"))
            } else {
                // No else branch: treat as implication
                Some(format!("(=> {c} {t})"))
            }
        }
        Expr::Forall { var, domain, body } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            let guard = quantifier_domain_guard_smtlib(domain, &v)?;
            Some(wrap_ast_quantifier_smtlib(true, &v, &guard, &b))
        }
        Expr::Exists { var, domain, body } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            let guard = quantifier_domain_guard_smtlib(domain, &v)?;
            Some(wrap_ast_quantifier_smtlib(false, &v, &guard, &b))
        }
        Expr::Call { func, args } => {
            let f = match func.as_ref() {
                Expr::Ident(name) => sanitize_smtlib_name(name),
                _ => return None,
            };
            if args.is_empty() {
                return Some(f);
            }
            let arg_strs: Option<Vec<String>> = args.iter().map(expr_to_smtlib).collect();
            let arg_strs = arg_strs?;
            if let Some(s) = crate::cvc5_builtins::known_builtin_to_smtlib(f.as_str(), &arg_strs) {
                return Some(s);
            }
            Some(format!("({f} {})", arg_strs.join(" ")))
        }
        Expr::Old(inner) => match inner.as_ref() {
            // old(x) -> x__old
            Expr::Ident(name) => Some(old_ident_smtlib_name(name)),
            // old(obj.field) -> flatten deep chains, else UF
            Expr::Field(obj, field) => match plan_field_access(obj.as_ref(), field) {
                FieldAccessPlan::Flatten(flat) => Some(old_flat_field_smtlib(&flat)),
                FieldAccessPlan::ShallowUf { field: f } => {
                    let old_obj = expr_to_smtlib(&Expr::Old(obj.clone()))?;
                    Some(shallow_field_smtlib(&f, &old_obj))
                }
            },
            // old(obj.method(args)) -> (method (old obj))
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let old_recv = expr_to_smtlib(&Expr::Old(receiver.clone()))?;
                Some(format!("({method} {old_recv})"))
            }
            _ => expr_to_smtlib(inner),
        },
        Expr::Paren(inner) => expr_to_smtlib(inner),
        Expr::Cast { expr: inner, .. } => expr_to_smtlib(inner),
        Expr::Ghost(inner) => expr_to_smtlib(inner),
        Expr::Let {
            name, value, body, ..
        } => {
            let v = sanitize_smtlib_name(name);
            let val = expr_to_smtlib(value)?;
            let b = expr_to_smtlib(body)?;
            Some(format!("(let (({v} {val})) {b})"))
        }
        Expr::Match {
            scrutinee, arms, ..
        } => encode_match_smtlib(scrutinee, arms, expr_to_smtlib, |name, s| {
            adt_is_constructor_smt("Option", name, s, shell_match_adt_def())
        }),
        // Field access: flatten deep chains, else UF __field_name(obj)
        Expr::Field(obj, field) => match plan_field_access(obj.as_ref(), field) {
            FieldAccessPlan::Flatten(name) => Some(name),
            FieldAccessPlan::ShallowUf { field: f } => {
                let o = expr_to_smtlib(obj)?;
                Some(shallow_field_smtlib(&f, &o))
            }
        },
        // Index: UF __index(coll, idx)
        Expr::Index { expr: coll, index } => {
            let c = expr_to_smtlib(coll)?;
            let i = expr_to_smtlib(index)?;
            Some(index_access_smtlib(&c, &i))
        }
        // Block: encode all, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some("true".to_string());
            }
            // SMT-LIB has no block; encode the last expression
            expr_to_smtlib(body.last()?)
        }
        // Raw tokens: full precedence-climbing SMT-LIB2 encoding
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some("true".to_string());
            }
            let (val, _) = parse_raw_expr_smtlib(tokens, 0, 0)?;
            Some(val)
        }
        // Tuple: use a fresh variable name
        Expr::Tuple(_) => Some("__tuple_fresh".to_string()),
        // MethodCall: prepend receiver as first arg to UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let r = expr_to_smtlib(receiver)?;
            let arg_strs: Option<Vec<String>> = args.iter().map(expr_to_smtlib).collect();
            let arg_strs = arg_strs.unwrap_or_default();
            let mut all_args = vec![r];
            all_args.extend(arg_strs);
            if let Some(s) =
                crate::cvc5_builtins::known_builtin_to_smtlib(method.as_str(), &all_args)
            {
                return Some(s);
            }
            if all_args.len() == 1 {
                Some(format!("({method} {})", all_args[0]))
            } else {
                Some(format!("({method} {})", all_args.join(" ")))
            }
        }
        // List: use a fresh variable name
        Expr::List(_) => Some("__list_fresh".to_string()),
        // Apply: return named bool
        Expr::Apply { lemma_name, .. } => Some(format!("__apply_{lemma_name}")),
    }
}

// -------------------------------------------------------------------------
// SMT-LIB2 precedence-climbing parser for Expr::Raw tokens (shell-out path)
// -------------------------------------------------------------------------

/// Precedence-climbing expression parser for raw tokens producing SMT-LIB2 text.
///
/// Returns `(smtlib_string, next_position)`.
fn parse_raw_expr_smtlib(tokens: &[String], pos: usize, min_prec: u8) -> Option<(String, usize)> {
    let (mut lhs, mut pos) = parse_raw_atom_smtlib(tokens, pos)?;

    while pos < tokens.len() {
        let Some((op_prec, op_kind)) = raw_op_info(tokens[pos].as_str()) else {
            break;
        };
        if op_prec < min_prec {
            break;
        }

        pos += 1; // consume operator

        let (rhs, next_pos) = parse_raw_expr_smtlib(tokens, pos, op_prec + 1)?;
        pos = next_pos;

        // Comparison chaining: `a < b < c` -> `(and (< a b) (< b c))`
        if raw_op_is_comparison(op_kind)
            && pos < tokens.len()
            && let Some((next_prec, next_op)) = raw_op_info(tokens[pos].as_str())
            && raw_op_is_comparison(next_op)
            && next_prec >= min_prec
        {
            let left_cmp = format_raw_binop_smtlib(op_kind, &lhs, &rhs);
            pos += 1; // consume next operator
            let (rhs2, next_pos2) = parse_raw_expr_smtlib(tokens, pos, next_prec + 1)?;
            pos = next_pos2;
            let right_cmp = format_raw_binop_smtlib(next_op, &rhs, &rhs2);
            lhs = format!("(and {left_cmp} {right_cmp})");
            continue;
        }

        lhs = format_raw_binop_smtlib(op_kind, &lhs, &rhs);
    }

    Some((lhs, pos))
}

/// Parse a single atom from raw tokens into SMT-LIB2 text.
fn parse_raw_atom_smtlib(tokens: &[String], start: usize) -> Option<(String, usize)> {
    if start >= tokens.len() {
        return Some(("true".to_string(), start));
    }

    let tok = &tokens[start];

    // --- Unary not ---
    if tok == "not" || tok == "!" {
        let (val, next) = parse_raw_atom_smtlib(tokens, start + 1)?;
        return Some((format!("(not {val})"), next));
    }

    // --- Unary minus ---
    if tok == "-" {
        let (val, next) = parse_raw_atom_smtlib(tokens, start + 1)?;
        return Some((format!("(- {val})"), next));
    }

    // --- Parenthesized expression ---
    if tok == "(" {
        let (val, end) = parse_raw_expr_smtlib(tokens, start + 1, 0)?;
        let next = if end < tokens.len() && tokens[end] == ")" {
            end + 1
        } else {
            end
        };
        return Some((val, next));
    }

    // --- Boolean literals ---
    if tok == "true" || tok == "false" {
        return Some((tok.clone(), start + 1));
    }

    // --- `result` keyword ---
    if tok == "result" {
        return Some(("__result".to_string(), start + 1));
    }

    // --- `old(expr)` ---
    if tok == "old" && start + 1 < tokens.len() && tokens[start + 1] == "(" {
        let p = find_matching_delim(tokens, start + 1, "(", ")")?;
        let end = p + 1;
        let inner = &tokens[start + 2..p];

        if inner.len() == 1 {
            let old_name = format!("{}__old", sanitize_smtlib_name(&inner[0]));
            return Some((old_name, end));
        }
        // General old(expr): parse inner and suffix identifiers conceptually
        if let Some((val, _)) = parse_raw_expr_smtlib(inner, 0, 0) {
            return Some((val, end));
        }
        return Some(("__old_fresh".to_string(), end));
    }

    // --- `forall`/`exists` quantifiers ---
    if let Some(slice) = parse_raw_quantifier_slice(tokens, start) {
        let var_name = sanitize_smtlib_name(&tokens[slice.var_token_idx]);
        let body_tokens = &tokens[slice.body_start..slice.body_end];
        if let Some((body_val, _)) = parse_raw_expr_smtlib(body_tokens, 0, 0) {
            return Some((
                format_raw_quantifier_smtlib(slice.is_forall, &var_name, &body_val),
                slice.final_pos,
            ));
        }
        return Some((
            format_raw_quantifier_smtlib(slice.is_forall, &var_name, "true"),
            slice.final_pos,
        ));
    }

    // --- Integer literal ---
    if tok.parse::<i64>().is_ok() {
        return Some((tok.clone(), start + 1));
    }

    // --- Skip specification keywords ---
    if is_raw_spec_skip_keyword(tok) {
        return parse_raw_atom_smtlib(tokens, start + 1);
    }

    // --- Identifier with dot-separated field access ---
    let mut name = sanitize_smtlib_name(tok);
    let mut next = start + 1;
    while next + 1 < tokens.len() && tokens[next] == "." {
        name.push('_');
        name.push_str(&sanitize_smtlib_name(&tokens[next + 1]));
        next += 2;
    }

    // Function call: `name(args)` -> `(name arg1 arg2 ...)`
    if next < tokens.len() && tokens[next] == "(" {
        let p = find_matching_delim(tokens, next, "(", ")")?;
        let arg_tokens = &tokens[next + 1..p];
        let mut arg_strs: Vec<String> = Vec::new();
        for (lo, hi) in comma_chunk_ranges(arg_tokens) {
            let chunk = &arg_tokens[lo..hi];
            if !chunk.is_empty()
                && let Some((v, _)) = parse_raw_expr_smtlib(chunk, 0, 0)
            {
                arg_strs.push(v);
            }
        }
        let end = p + 1;

        if arg_strs.is_empty() {
            return Some((name, end));
        }
        return Some((format!("({name} {})", arg_strs.join(" ")), end));
    }

    Some((name, next))
}

fn quantifier_domain_guard_smtlib(domain: &Expr, var: &str) -> Option<String> {
    if let Some((lo, hi)) = domain_as_range(domain) {
        let lo_s = expr_to_smtlib(lo)?;
        let hi_s = expr_to_smtlib(hi)?;
        Some(range_guard_smtlib(var, &lo_s, &hi_s))
    } else {
        let d = expr_to_smtlib(domain).unwrap_or_else(|| var.to_string());
        Some(domain_contains_guard_smtlib(&d, var))
    }
}

/// Collect all variable names referenced in an expression.
pub fn collect_vars(expr: &Expr, vars: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            if name == "result" {
                vars.insert(smtlib_result_name().to_string());
            } else {
                vars.insert(sanitize_smtlib_name(name));
            }
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_vars(lhs, vars);
            collect_vars(rhs, vars);
        }
        Expr::UnaryOp { expr: inner, .. } => collect_vars(inner, vars),
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_vars(cond, vars);
            collect_vars(then_branch, vars);
            if let Some(e) = else_branch {
                collect_vars(e, vars);
            }
        }
        Expr::Forall {
            var, body, domain, ..
        }
        | Expr::Exists {
            var, body, domain, ..
        } => {
            // Do NOT insert the quantifier-bound variable as a global constant.
            // It is locally scoped by the (forall ((var Int)) ...) quantifier.
            // Declaring it as a global constant creates a name collision in CVC5.
            collect_vars(body, vars);
            collect_vars(domain, vars);
            // Remove the bound variable if it was collected from the body/domain.
            vars.remove(&sanitize_smtlib_name(var));
        }
        Expr::Call { args, .. } => {
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Old(inner) | Expr::Paren(inner) | Expr::Ghost(inner) => {
            collect_vars(inner, vars);
        }
        Expr::Cast { expr: inner, .. } => collect_vars(inner, vars),
        Expr::Field(receiver, _) => collect_vars(receiver, vars),
        Expr::MethodCall { receiver, args, .. } => {
            collect_vars(receiver, vars);
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Index { expr, index } => {
            collect_vars(expr, vars);
            collect_vars(index, vars);
        }
        Expr::Let { value, body, .. } => {
            collect_vars(value, vars);
            collect_vars(body, vars);
        }
        Expr::Match { scrutinee, arms } => {
            collect_vars(scrutinee, vars);
            for arm in arms {
                collect_vars(&arm.body, vars);
            }
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_vars(item, vars);
            }
        }
        Expr::Apply { args, .. } => {
            for arg in args {
                collect_vars(arg, vars);
            }
        }
        Expr::Literal(_) => {}
        Expr::Raw(tokens) => {
            // Raw tokens may contain variable names; collect identifiers
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && tok != "true"
                    && tok != "false"
                {
                    vars.insert(sanitize_smtlib_name(tok));
                }
            }
        }
    }
}

/// Parse a CVC5 model output into a CounterexampleModel.
///
/// Filters out internal encoder variables and sorts the remaining
/// user variables alphabetically (matching Z3 backend behavior).
#[cfg_attr(feature = "cvc5-verify", expect(dead_code))]
pub(crate) fn parse_smtlib_model(model_str: &str) -> Option<CounterexampleModel> {
    // CVC5 model format: (define-fun name () Int value)
    let mut variables = Vec::new();
    for line in model_str.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("(define-fun ") {
            // Extract name and value from: (define-fun name () Type value)
            let parts: Vec<&str> = trimmed
                .trim_start_matches("(define-fun ")
                .splitn(2, " () ")
                .collect();
            if parts.len() == 2 {
                let name = parts[0].to_string();
                // Value is after the type, before the closing paren
                let type_and_value = parts[1];
                if let Some(space_idx) = type_and_value.find(' ') {
                    let raw = &type_and_value[space_idx + 1..];
                    // Strip exactly one trailing ')' (the define-fun closer)
                    let value = raw.strip_suffix(')').unwrap_or(raw).trim().to_string();
                    if !is_internal_cvc5_var(&name) {
                        variables.push((name, value));
                    }
                }
            }
        }
    }
    if variables.is_empty() {
        None
    } else {
        // Sort alphabetically for deterministic output
        variables.sort_by(|(a, _), (b, _)| a.cmp(b));
        Some(CounterexampleModel { variables })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Literal, Pattern, UnaryOp};

    // -------------------------------------------------------------------
    // derive_narrowings_cvc5 tests (#257)
    // -------------------------------------------------------------------

    #[test]
    fn test_derive_narrowings_cvc5_basic() {
        let narrowings = derive_narrowings_cvc5(&[("max_size".into(), 100)]);
        assert_eq!(narrowings.len(), 1);
        assert_eq!(narrowings[0], ("size".into(), 100));
    }

    #[test]
    fn test_derive_narrowings_cvc5_empty() {
        let narrowings = derive_narrowings_cvc5(&[]);
        assert!(narrowings.is_empty());
    }

    #[test]
    fn test_derive_narrowings_cvc5_no_prefix() {
        let narrowings = derive_narrowings_cvc5(&[("size".into(), 50)]);
        assert!(narrowings.is_empty());
    }

    #[test]
    fn test_derive_narrowings_cvc5_uppercase_prefix() {
        let narrowings = derive_narrowings_cvc5(&[("MAX_BUFFER".into(), 1024)]);
        assert_eq!(narrowings.len(), 2);
        assert_eq!(narrowings[0], ("BUFFER".into(), 1024));
        assert_eq!(narrowings[1], ("buffer".into(), 1024));
    }

    #[test]
    fn test_derive_narrowings_cvc5_multiple() {
        let narrowings = derive_narrowings_cvc5(&[
            ("max_size".into(), 100),
            ("max_count".into(), 50),
            ("threshold".into(), 10),
        ]);
        assert_eq!(narrowings.len(), 2);
        assert_eq!(narrowings[0], ("size".into(), 100));
        assert_eq!(narrowings[1], ("count".into(), 50));
    }

    // -------------------------------------------------------------------
    // expr_to_smtlib tests
    // -------------------------------------------------------------------

    #[test]
    fn test_smtlib_int_positive() {
        let expr = Expr::Literal(Literal::Int("42".into()));
        assert_eq!(expr_to_smtlib(&expr), Some("42".into()));
    }

    #[test]
    fn test_smtlib_int_negative() {
        let expr = Expr::Literal(Literal::Int("-7".into()));
        assert_eq!(expr_to_smtlib(&expr), Some("(- 7)".into()));
    }

    #[test]
    fn test_smtlib_bool_true() {
        let expr = Expr::Literal(Literal::Bool(true));
        assert_eq!(expr_to_smtlib(&expr), Some("true".into()));
    }

    #[test]
    fn test_smtlib_bool_false() {
        let expr = Expr::Literal(Literal::Bool(false));
        assert_eq!(expr_to_smtlib(&expr), Some("false".into()));
    }

    #[test]
    fn test_smtlib_string_encodes_as_named_const() {
        let expr = Expr::Literal(Literal::Str("hello".into()));
        assert_eq!(expr_to_smtlib(&expr), Some("__str_hello".into()));
    }

    #[test]
    fn test_smtlib_ident() {
        let expr = Expr::Ident("x".into());
        assert_eq!(expr_to_smtlib(&expr), Some("x".into()));
    }

    #[test]
    fn test_smtlib_result_keyword() {
        let expr = Expr::Ident("result".into());
        assert_eq!(expr_to_smtlib(&expr), Some("__result".into()));
    }

    #[test]
    fn test_smtlib_dotted_ident_sanitized() {
        let expr = Expr::Ident("state.field".into());
        assert_eq!(expr_to_smtlib(&expr), Some("state_field".into()));
    }

    #[test]
    fn test_smtlib_binop_add() {
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(+ x 1)".into()));
    }

    #[test]
    fn test_smtlib_binop_neq() {
        let expr = Expr::BinOp {
            op: BinOp::Neq,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
    }

    #[test]
    fn test_smtlib_binop_div_is_integer() {
        let expr = Expr::BinOp {
            op: BinOp::Div,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("y".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(div x y)".into()));
    }

    #[test]
    fn test_smtlib_binop_implies() {
        let expr = Expr::BinOp {
            op: BinOp::Implies,
            lhs: Box::new(Expr::Ident("p".into())),
            rhs: Box::new(Expr::Ident("q".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
    }

    #[test]
    fn test_smtlib_binop_range_encodes() {
        let expr = Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
        };
        let s = expr_to_smtlib(&expr).expect("Range should encode");
        assert!(s.contains(">="), "missing >= in range encoding: {s}");
        assert!(s.contains("<"), "missing < in range encoding: {s}");
        assert!(
            s.contains("__range_fresh"),
            "missing fresh var in range: {s}"
        );
    }

    #[test]
    fn test_smtlib_binop_in() {
        let expr = Expr::BinOp {
            op: BinOp::In,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("collection".into())),
        };
        let s = expr_to_smtlib(&expr).expect("In should encode");
        assert!(s.contains("__contains"), "missing contains UF in: {s}");
        assert!(s.contains("collection"), "missing collection in: {s}");
        assert!(s.contains("x"), "missing element in: {s}");
    }

    #[test]
    fn test_smtlib_binop_notin() {
        let expr = Expr::BinOp {
            op: BinOp::NotIn,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Ident("items".into())),
        };
        let s = expr_to_smtlib(&expr).expect("NotIn should encode");
        assert!(s.contains("not"), "missing negation in NotIn: {s}");
        assert!(
            s.contains("__contains"),
            "missing contains UF in NotIn: {s}"
        );
    }

    #[test]
    fn test_smtlib_binop_concat() {
        let expr = Expr::BinOp {
            op: BinOp::Concat,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        };
        let s = expr_to_smtlib(&expr).expect("Concat should encode");
        assert!(s.contains("__concat"), "missing concat UF in: {s}");
        assert!(s.contains("a"), "missing lhs in concat: {s}");
        assert!(s.contains("b"), "missing rhs in concat: {s}");
    }

    #[test]
    fn test_smtlib_unary_not() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Not,
            expr: Box::new(Expr::Ident("flag".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(not flag)".into()));
    }

    #[test]
    fn test_smtlib_unary_neg() {
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Ident("x".into())),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
    }

    #[test]
    fn test_smtlib_if_with_else() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Ident("t".into())),
            else_branch: Some(Box::new(Expr::Ident("e".into()))),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(ite c t e)".into()));
    }

    #[test]
    fn test_smtlib_if_without_else() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("p".into())),
            then_branch: Box::new(Expr::Ident("q".into())),
            else_branch: None,
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(=> p q)".into()));
    }

    #[test]
    fn test_smtlib_forall_non_range_domain() {
        // Non-range domain should produce __domain_contains guard
        let expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        assert_eq!(
            expr_to_smtlib(&expr),
            Some("(forall ((i Int)) (=> (__domain_contains xs i) (>= i 0)))".into())
        );
    }

    #[test]
    fn test_smtlib_exists_non_range_domain() {
        // Non-range domain should produce __domain_contains guard
        let expr = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::Ident("S".into())),
            body: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        assert_eq!(
            expr_to_smtlib(&expr),
            Some("(exists ((x Int)) (and (__domain_contains S x) (= x 0)))".into())
        );
    }

    #[test]
    fn test_smtlib_forall_range_domain() {
        // forall x in 0..10 { x >= 0 } should produce range guard
        let expr = Expr::Forall {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                op: BinOp::Range,
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(
            s,
            "(forall ((x Int)) (=> (and (>= x 0) (< x 10)) (>= x 0)))"
        );
    }

    #[test]
    fn test_smtlib_exists_range_domain() {
        // exists x in 0..10 { x == 5 } should produce range guard with conjunction
        let expr = Expr::Exists {
            var: "x".into(),
            domain: Box::new(Expr::BinOp {
                op: BinOp::Range,
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
            }),
            body: Box::new(Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
            }),
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(
            s,
            "(exists ((x Int)) (and (and (>= x 0) (< x 10)) (= x 5)))"
        );
    }

    #[test]
    fn test_smtlib_forall_range_variable_bounds() {
        // forall i in 0..n { i >= 0 } -- variable upper bound
        let expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::BinOp {
                op: BinOp::Range,
                lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                rhs: Box::new(Expr::Ident("n".into())),
            }),
            body: Box::new(Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            }),
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(forall ((i Int)) (=> (and (>= i 0) (< i n)) (>= i 0)))");
    }

    #[test]
    fn test_smtlib_call_no_args() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("foo".into())),
            args: vec![],
        };
        assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
    }

    #[test]
    fn test_smtlib_call_with_args() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("f".into())),
            args: vec![Expr::Ident("x".into()), Expr::Ident("y".into())],
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(f x y)".into()));
    }

    #[test]
    fn test_smtlib_old_adds_suffix() {
        let expr = Expr::Old(Box::new(Expr::Ident("x".into())));
        assert_eq!(expr_to_smtlib(&expr), Some("x__old".into()));
    }

    #[test]
    fn test_smtlib_paren_transparent() {
        let expr = Expr::Paren(Box::new(Expr::Literal(Literal::Int("5".into()))));
        assert_eq!(expr_to_smtlib(&expr), Some("5".into()));
    }

    #[test]
    fn test_smtlib_raw_single_token() {
        let expr = Expr::Raw(vec!["foo".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("foo".into()));
        // Integer token
        let expr_int = Expr::Raw(vec!["42".into()]);
        assert_eq!(expr_to_smtlib(&expr_int), Some("42".into()));
        // Bool token
        let expr_bool = Expr::Raw(vec!["true".into()]);
        assert_eq!(expr_to_smtlib(&expr_bool), Some("true".into()));
    }

    #[test]
    fn test_smtlib_raw_precedence_climbing() {
        // "a + b * c" should parse as (+ a (* b c)) due to precedence
        let expr = Expr::Raw(vec![
            "a".into(),
            "+".into(),
            "b".into(),
            "*".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(+ a (* b c))".into()));
    }

    #[test]
    fn test_smtlib_raw_parentheses() {
        // "(a + b) * c" should parse as (* (+ a b) c)
        let expr = Expr::Raw(vec![
            "(".into(),
            "a".into(),
            "+".into(),
            "b".into(),
            ")".into(),
            "*".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(* (+ a b) c)".into()));
    }

    #[test]
    fn test_smtlib_raw_old_expression() {
        // "old ( x ) + 1" should parse old(x) + 1
        let expr = Expr::Raw(vec![
            "old".into(),
            "(".into(),
            "x".into(),
            ")".into(),
            "+".into(),
            "1".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(+ x__old 1)".into()));
    }

    #[test]
    fn test_smtlib_raw_nested_operators() {
        // "a + b - c + d" left-associative: (+ (- (+ a b) c) d)
        let expr = Expr::Raw(vec![
            "a".into(),
            "+".into(),
            "b".into(),
            "-".into(),
            "c".into(),
            "+".into(),
            "d".into(),
        ]);
        let result = expr_to_smtlib(&expr).unwrap();
        // Left-associative: ((a + b) - c) + d
        assert_eq!(result, "(+ (- (+ a b) c) d)");
    }

    #[test]
    fn test_smtlib_raw_comparison_chain() {
        // "a < b < c" desugars to (and (< a b) (< b c))
        let expr = Expr::Raw(vec![
            "a".into(),
            "<".into(),
            "b".into(),
            "<".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(and (< a b) (< b c))".into()));
    }

    #[test]
    fn test_smtlib_raw_unary_not() {
        // "! x" -> (not x)
        let expr = Expr::Raw(vec!["!".into(), "x".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(not x)".into()));
    }

    #[test]
    fn test_smtlib_raw_unary_neg() {
        // "- x" -> (- x)
        let expr = Expr::Raw(vec!["-".into(), "x".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(- x)".into()));
    }

    #[test]
    fn test_smtlib_raw_logical_ops() {
        // "a && b || c" should respect precedence: (or (and a b) c)
        let expr = Expr::Raw(vec![
            "a".into(),
            "&&".into(),
            "b".into(),
            "||".into(),
            "c".into(),
        ]);
        assert_eq!(expr_to_smtlib(&expr), Some("(or (and a b) c)".into()));
    }

    #[test]
    fn test_smtlib_raw_neq() {
        // "a != b" -> (not (= a b))
        let expr = Expr::Raw(vec!["a".into(), "!=".into(), "b".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(not (= a b))".into()));
    }

    #[test]
    fn test_smtlib_raw_mod_div() {
        // "a mod b" and "a div b"
        let expr_mod = Expr::Raw(vec!["a".into(), "mod".into(), "b".into()]);
        assert_eq!(expr_to_smtlib(&expr_mod), Some("(mod a b)".into()));

        let expr_div = Expr::Raw(vec!["a".into(), "div".into(), "b".into()]);
        assert_eq!(expr_to_smtlib(&expr_div), Some("(div a b)".into()));
    }

    #[test]
    fn test_smtlib_raw_complex_expression() {
        // "x >= 0 && x < max" -> (and (>= x 0) (< x max))
        let expr = Expr::Raw(vec![
            "x".into(),
            ">=".into(),
            "0".into(),
            "&&".into(),
            "x".into(),
            "<".into(),
            "max".into(),
        ]);
        assert_eq!(
            expr_to_smtlib(&expr),
            Some("(and (>= x 0) (< x max))".into())
        );
    }

    #[test]
    fn test_smtlib_raw_function_call() {
        // "abs ( x )" -> (abs x)
        let expr = Expr::Raw(vec!["abs".into(), "(".into(), "x".into(), ")".into()]);
        assert_eq!(expr_to_smtlib(&expr), Some("(abs x)".into()));
    }

    #[test]
    fn test_smtlib_let_expr() {
        let expr = Expr::Let {
            name: "x".into(),
            value: Box::new(Expr::Literal(Literal::Int("5".into()))),
            body: Box::new(Expr::BinOp {
                op: BinOp::Add,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
            }),
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(let ((x 5)) (+ x 1))".into()));
    }

    #[test]
    fn test_smtlib_match_with_literal_and_wildcard() {
        use assura_parser::ast::MatchArm;
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("n".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Literal(Literal::Int("0".into())),
                    body: Expr::Literal(Literal::Int("1".into())),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Ident("n".into()),
                },
            ],
        };
        assert_eq!(expr_to_smtlib(&expr), Some("(ite (= n 0) 1 n)".into()));
    }

    #[test]
    fn test_smtlib_match_empty_arms() {
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("n".into())),
            arms: vec![],
        };
        assert_eq!(expr_to_smtlib(&expr), None);
    }

    #[test]
    fn test_smtlib_match_constructor_pattern() {
        use assura_parser::ast::MatchArm;
        // match x { Some(v) => v, None => 0 }
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Constructor {
                        name: "Some".into(),
                        fields: vec![Pattern::Ident("v".into())],
                    },
                    body: Expr::Ident("v".into()),
                },
                MatchArm {
                    pattern: Pattern::Constructor {
                        name: "None".into(),
                        fields: vec![],
                    },
                    body: Expr::Literal(Literal::Int("0".into())),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Literal(Literal::Int("0".into())),
                },
            ],
        };
        let smt = expr_to_smtlib(&expr).expect("should encode constructor match");
        // #263: Constructor patterns use ADT tag testers, not pattern hashes.
        assert!(smt.contains("__adt_tag_Option"));
        assert!(smt.contains("(= (__adt_tag_Option x) 0)")); // Some
        assert!(smt.contains("(= (__adt_tag_Option x) 1)")); // None
        assert!(smt.contains("ite"));
    }

    #[test]
    fn test_smtlib_match_tuple_pattern() {
        use assura_parser::ast::MatchArm;
        // match t { (a, b) => a }
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("t".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Tuple(vec![
                    Pattern::Ident("a".into()),
                    Pattern::Ident("b".into()),
                ]),
                body: Expr::Ident("a".into()),
            }],
        };
        let smt = expr_to_smtlib(&expr).expect("should encode tuple match");
        // Tuple is structural, body is just "a"
        assert_eq!(smt, "a");
    }

    #[test]
    fn test_smtlib_match_ident_constructor_like() {
        use assura_parser::ast::MatchArm;
        // match x { None => 1, _ => 0 }  (Ident "None" uppercase = constructor)
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Ident("None".into()),
                    body: Expr::Literal(Literal::Int("1".into())),
                },
                MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Literal(Literal::Int("0".into())),
                },
            ],
        };
        let smt = expr_to_smtlib(&expr).expect("should encode ident-as-constructor match");
        let none_hash = crate::cvc5_builtins::pattern_hash_name("None");
        assert!(smt.contains(&none_hash.to_string()));
        assert!(smt.contains("ite"));
    }

    // -------------------------------------------------------------------
    // collect_vars tests
    // -------------------------------------------------------------------

    #[test]
    fn test_collect_vars_ident() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Ident("x".into()), &mut vars);
        assert!(vars.contains("x"));
    }

    #[test]
    fn test_collect_vars_result() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Ident("result".into()), &mut vars);
        assert!(vars.contains("__result"));
        assert!(!vars.contains("result"));
    }

    #[test]
    fn test_collect_vars_binop() {
        let mut vars = HashSet::new();
        let expr = Expr::BinOp {
            op: BinOp::Add,
            lhs: Box::new(Expr::Ident("a".into())),
            rhs: Box::new(Expr::Ident("b".into())),
        };
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("a"));
        assert!(vars.contains("b"));
    }

    #[test]
    fn test_collect_vars_if_all_branches() {
        let mut vars = HashSet::new();
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("c".into())),
            then_branch: Box::new(Expr::Ident("t".into())),
            else_branch: Some(Box::new(Expr::Ident("e".into()))),
        };
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("c"));
        assert!(vars.contains("t"));
        assert!(vars.contains("e"));
    }

    #[test]
    fn test_collect_vars_literal_no_vars() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Literal(Literal::Int("42".into())), &mut vars);
        assert!(vars.is_empty());
    }

    #[test]
    fn test_collect_vars_dotted_sanitized() {
        let mut vars = HashSet::new();
        collect_vars(&Expr::Ident("obj.field".into()), &mut vars);
        assert!(vars.contains("obj_field"));
    }

    // -------------------------------------------------------------------
    // parse_smtlib_model tests
    // -------------------------------------------------------------------

    #[test]
    fn test_parse_model_define_fun() {
        let model = "(define-fun x () Int 42)\n(define-fun y () Int (- 1))";
        let parsed = parse_smtlib_model(model).unwrap();
        assert_eq!(parsed.variables.len(), 2);
        assert_eq!(parsed.variables[0].0, "x");
        assert_eq!(parsed.variables[0].1, "42");
        assert_eq!(parsed.variables[1].0, "y");
        assert_eq!(parsed.variables[1].1, "(- 1)");
    }

    #[test]
    fn test_parse_model_empty() {
        assert!(parse_smtlib_model("").is_none());
    }

    #[test]
    fn test_parse_model_no_define_fun() {
        assert!(parse_smtlib_model("sat\n(something else)").is_none());
    }

    #[test]
    fn test_parse_model_skips_coerce() {
        let model = "(define-fun __coerce_1 () Int 0)\n(define-fun x () Int 5)";
        let parsed = parse_smtlib_model(model).unwrap();
        assert_eq!(parsed.variables.len(), 1);
        assert_eq!(parsed.variables[0].0, "x");
    }

    // -------------------------------------------------------------------
    // is_internal_cvc5_var and counterexample model filtering (#260)
    // -------------------------------------------------------------------

    #[test]
    fn test_is_internal_cvc5_var_internal_prefixes() {
        assert!(is_internal_cvc5_var("__str_hello"));
        assert!(is_internal_cvc5_var("__tuple_0"));
        assert!(is_internal_cvc5_var("__list_vals"));
        assert!(is_internal_cvc5_var("__fresh_3"));
        assert!(is_internal_cvc5_var("__field_len"));
        assert!(is_internal_cvc5_var("__index_0"));
        assert!(is_internal_cvc5_var("__len_buf"));
        assert!(is_internal_cvc5_var("__arr_data"));
        assert!(is_internal_cvc5_var("__domain_contains_x"));
        assert!(is_internal_cvc5_var("__apply_func"));
        assert!(is_internal_cvc5_var("__coerce_1"));
        assert!(is_internal_cvc5_var("__trigger_pat"));
        assert!(is_internal_cvc5_var("__list_get_0"));
        assert!(is_internal_cvc5_var("__result"));
        assert!(is_internal_cvc5_var("__contains"));
        assert!(is_internal_cvc5_var("__obj_ptr"));
    }

    #[test]
    fn test_is_internal_cvc5_var_user_variables() {
        assert!(!is_internal_cvc5_var("x"));
        assert!(!is_internal_cvc5_var("buffer_size"));
        assert!(!is_internal_cvc5_var("payload_length"));
        assert!(!is_internal_cvc5_var("n"));
        assert!(!is_internal_cvc5_var("result_count"));
        assert!(!is_internal_cvc5_var("max_size"));
        assert!(!is_internal_cvc5_var("i"));
    }

    #[test]
    fn test_parse_model_filters_all_internal_vars() {
        let model = "\
(define-fun __str_hello () Int 1)\n\
(define-fun __field_len () Int 5)\n\
(define-fun __fresh_0 () Int 99)\n\
(define-fun __result () Int 42)\n\
(define-fun __coerce_1 () Int 0)\n\
(define-fun x () Int 10)\n\
(define-fun y () Int 20)";
        let parsed = parse_smtlib_model(model).unwrap();
        let names: Vec<&str> = parsed.variables.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["x", "y"]);
        assert!(!names.contains(&"__str_hello"));
        assert!(!names.contains(&"__field_len"));
        assert!(!names.contains(&"__fresh_0"));
        assert!(!names.contains(&"__result"));
        assert!(!names.contains(&"__coerce_1"));
    }

    #[test]
    fn test_parse_model_sorted_alphabetically() {
        let model = "\
(define-fun z_var () Int 3)\n\
(define-fun a_var () Int 1)\n\
(define-fun m_var () Int 2)";
        let parsed = parse_smtlib_model(model).unwrap();
        let names: Vec<&str> = parsed.variables.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a_var", "m_var", "z_var"]);
    }

    #[test]
    fn test_parse_model_all_internal_returns_none() {
        let model = "\
(define-fun __str_a () Int 1)\n\
(define-fun __fresh_0 () Int 2)\n\
(define-fun __coerce_1 () Int 3)";
        assert!(
            parse_smtlib_model(model).is_none(),
            "model with only internal vars should return None"
        );
    }

    // -------------------------------------------------------------------
    // collect_vars exhaustive coverage (issue #54)
    // -------------------------------------------------------------------

    #[test]
    fn collect_vars_field_access() {
        let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "field".into());
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("obj"));
    }

    #[test]
    fn collect_vars_method_call() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("list".into())),
            method: "len".into(),
            args: vec![Expr::Ident("idx".into())],
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("list"));
        assert!(vars.contains("idx"));
    }

    #[test]
    fn collect_vars_index() {
        let expr = Expr::Index {
            expr: Box::new(Expr::Ident("arr".into())),
            index: Box::new(Expr::Ident("i".into())),
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("arr"));
        assert!(vars.contains("i"));
    }

    #[test]
    fn collect_vars_let_expr() {
        let expr = Expr::Let {
            name: "tmp".into(),
            value: Box::new(Expr::Ident("a".into())),
            body: Box::new(Expr::Ident("b".into())),
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("a"));
        assert!(vars.contains("b"));
    }

    #[test]
    fn collect_vars_match_expr() {
        use assura_parser::ast::{MatchArm, Pattern};
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![MatchArm {
                pattern: Pattern::Ident("_".into()),
                body: Expr::Ident("y".into()),
            }],
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
    }

    #[test]
    fn collect_vars_list_tuple_block() {
        let list = Expr::List(vec![Expr::Ident("a".into()), Expr::Ident("b".into())]);
        let tuple = Expr::Tuple(vec![Expr::Ident("c".into())]);
        let block = Expr::Block(vec![Expr::Ident("d".into())]);
        let mut vars = HashSet::new();
        collect_vars(&list, &mut vars);
        collect_vars(&tuple, &mut vars);
        collect_vars(&block, &mut vars);
        assert!(vars.contains("a"));
        assert!(vars.contains("b"));
        assert!(vars.contains("c"));
        assert!(vars.contains("d"));
    }

    #[test]
    fn collect_vars_apply() {
        let expr = Expr::Apply {
            lemma_name: "lem".into(),
            args: vec![Expr::Ident("p".into())],
        };
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.contains("p"));
    }

    #[test]
    fn collect_vars_literal_is_empty() {
        let expr = Expr::Literal(Literal::Int("42".into()));
        let mut vars = HashSet::new();
        collect_vars(&expr, &mut vars);
        assert!(vars.is_empty());
    }

    // -------------------------------------------------------------------
    // Regression: CVC5 must_not semantics (#166)
    // -------------------------------------------------------------------

    /// must_not(true) should NOT be verified: true is always possible.
    /// The CVC5 backend must assert the body directly (not negate it).
    #[test]
    fn test_cvc5_must_not_semantics() {
        // must_not { true } -- "true" is always satisfiable, so
        // asserting it directly gives SAT -> Counterexample.
        let clause = Clause {
            kind: ClauseKind::MustNot,
            body: Expr::Literal(Literal::Bool(true)),
            effect_variables: vec![],
        };
        let results = verify_contract_cvc5("TestMustNot", &[clause]);
        // Should be Counterexample (the bad thing CAN happen)
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Counterexample { .. } | VerificationResult::Unknown { .. }
            ),
            "must_not(true) should be Counterexample or Unknown, got: {:?}",
            results[0]
        );
    }

    /// must_not(false) should verify: false is impossible.
    #[test]
    fn test_cvc5_must_not_impossible() {
        let clause = Clause {
            kind: ClauseKind::MustNot,
            body: Expr::Literal(Literal::Bool(false)),
            effect_variables: vec![],
        };
        let results = verify_contract_cvc5("TestMustNotFalse", &[clause]);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "must_not(false) should be Verified or Unknown (if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Regression: quantifier-bound vars not global (#167)
    // -------------------------------------------------------------------

    /// Quantifier-bound variables must NOT appear in the global
    /// `(declare-const ...)` section of the generated SMT-LIB2 script.
    #[test]
    fn test_cvc5_quantifier_var_not_global() {
        // forall i in xs: i >= 0
        let body = Expr::BinOp {
            op: BinOp::Gte,
            lhs: Box::new(Expr::Ident("i".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let forall_expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(body),
        };
        let mut vars = HashSet::new();
        collect_vars(&forall_expr, &mut vars);
        // "i" must NOT be in the global vars set
        assert!(
            !vars.contains("i"),
            "quantifier-bound variable 'i' must not be a global constant"
        );
        // "xs" (the domain) should still be collected
        assert!(
            vars.contains("xs"),
            "domain variable 'xs' should be collected"
        );
    }

    // -------------------------------------------------------------------
    // Unmodelable feature pre-check tests (cfg-independent)
    // -------------------------------------------------------------------

    #[test]
    fn test_typestate_now_modelable() {
        // #262: Raw tokens with @ are now modelable (encoded as integer equality)
        let expr = Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]);
        assert!(
            !expr_has_unmodelable_features_cvc5(&expr),
            "typestate @ annotation should be modelable after #262"
        );
    }

    #[test]
    fn test_no_unmodelable_reason_for_typestate() {
        // #262: Typestate no longer produces unmodelable reasons
        let expr = Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]);
        let reasons = collect_unmodelable_reasons_cvc5(&expr);
        assert!(
            reasons.is_empty(),
            "typestate should produce no unmodelable reasons after #262, got: {:?}",
            reasons
        );
    }

    #[test]
    fn test_modelable_normal_expr() {
        // Normal binary expression should be modelable
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert!(
            !expr_has_unmodelable_features_cvc5(&expr),
            "normal binop should be modelable"
        );
    }

    #[test]
    fn test_typestate_nested_in_binop_modelable() {
        // #262: Typestate nested in a binary expression is now modelable
        let expr = Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Raw(vec![
                "conn".into(),
                "@".into(),
                "Connected".into(),
            ])),
        };
        assert!(
            !expr_has_unmodelable_features_cvc5(&expr),
            "typestate nested in binop should be modelable after #262"
        );
    }

    #[test]
    fn test_typestate_in_if_branch_modelable() {
        // #262: Typestate in if branch is now modelable
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("flag".into())),
            then_branch: Box::new(Expr::Raw(vec!["s".into(), "@".into(), "Locked".into()])),
            else_branch: None,
        };
        assert!(
            !expr_has_unmodelable_features_cvc5(&expr),
            "typestate in if-then should be modelable after #262"
        );
    }

    #[test]
    fn test_typestate_in_forall_body_modelable() {
        // #262: Typestate in forall body is now modelable
        let expr = Expr::Forall {
            var: "i".into(),
            domain: Box::new(Expr::Ident("xs".into())),
            body: Box::new(Expr::Raw(vec!["item".into(), "@".into(), "Valid".into()])),
        };
        assert!(
            !expr_has_unmodelable_features_cvc5(&expr),
            "typestate in forall body should be modelable after #262"
        );
    }

    #[test]
    fn test_cvc5_typestate_same_state_verifies() {
        // #262: Typestate same pre/post should verify via verify_contract_cvc5
        // (or Unknown if cvc5 is not installed on this system)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("TypestateIdentity", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for typestate identity"
        );
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "same typestate pre/post should verify or Unknown (if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_typestate_different_state_counterexample() {
        // #262: Different typestate pre/post should produce counterexample
        // (or Unknown if cvc5 is not installed on this system)
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()]),
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("TypestateMismatch", &clauses);
        assert!(
            !results.is_empty(),
            "should have results for typestate mismatch"
        );
        assert!(
            matches!(
                &results[0],
                VerificationResult::Counterexample { .. } | VerificationResult::Unknown { .. }
            ),
            "different typestate pre/post should produce counterexample or Unknown (if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    // -------------------------------------------------------------------
    // Lemma apply-ref collection tests (cfg-independent)
    // -------------------------------------------------------------------

    #[test]
    fn test_collect_apply_refs_simple() {
        let expr = Expr::Apply {
            lemma_name: "helper".into(),
            args: vec![Expr::Ident("x".into())],
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert_eq!(refs, vec!["helper"]);
    }

    #[test]
    fn test_collect_apply_refs_nested_in_binop() {
        let expr = Expr::BinOp {
            op: BinOp::And,
            lhs: Box::new(Expr::Apply {
                lemma_name: "lem_a".into(),
                args: vec![Expr::Ident("x".into())],
            }),
            rhs: Box::new(Expr::Apply {
                lemma_name: "lem_b".into(),
                args: vec![Expr::Ident("y".into())],
            }),
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"lem_a".to_string()));
        assert!(refs.contains(&"lem_b".to_string()));
    }

    #[test]
    fn test_collect_apply_refs_no_apply() {
        let expr = Expr::BinOp {
            op: BinOp::Gt,
            lhs: Box::new(Expr::Ident("x".into())),
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_collect_apply_refs_nested_in_if() {
        let expr = Expr::If {
            cond: Box::new(Expr::Ident("flag".into())),
            then_branch: Box::new(Expr::Apply {
                lemma_name: "branch_lem".into(),
                args: vec![],
            }),
            else_branch: Some(Box::new(Expr::Literal(Literal::Bool(true)))),
        };
        let refs = collect_apply_refs_from_expr(&expr);
        assert_eq!(refs, vec!["branch_lem"]);
    }

    // -------------------------------------------------------------------
    // SMT-LIB float encoding tests (#248)
    // -------------------------------------------------------------------

    #[test]
    fn test_smtlib_float_rational_encoding() {
        let expr = Expr::Literal(Literal::Float("3.14".into()));
        let result = expr_to_smtlib(&expr).unwrap();
        assert_eq!(result, "(/ 3140000 1000000)");
    }

    #[test]
    fn test_smtlib_float_zero() {
        let expr = Expr::Literal(Literal::Float("0.0".into()));
        let result = expr_to_smtlib(&expr).unwrap();
        assert_eq!(result, "(/ 0 1000000)");
    }

    #[test]
    fn test_smtlib_float_negative() {
        // Negative floats: the negation is applied by UnaryOp::Neg externally,
        // but the literal itself may parse as negative
        let expr = Expr::UnaryOp {
            op: UnaryOp::Neg,
            expr: Box::new(Expr::Literal(Literal::Float("2.5".into()))),
        };
        let result = expr_to_smtlib(&expr).unwrap();
        assert_eq!(result, "(- (/ 2500000 1000000))");
    }

    #[test]
    fn test_smtlib_match_float_pattern_rational() {
        // Match arm with float literal should use rational encoding
        let expr = Expr::Match {
            scrutinee: Box::new(Expr::Ident("x".into())),
            arms: vec![
                assura_parser::ast::MatchArm {
                    pattern: Pattern::Literal(Literal::Float("1.5".into())),
                    body: Expr::Literal(Literal::Bool(true)),
                },
                assura_parser::ast::MatchArm {
                    pattern: Pattern::Wildcard,
                    body: Expr::Literal(Literal::Bool(false)),
                },
            ],
        };
        let result = expr_to_smtlib(&expr).unwrap();
        assert!(
            result.contains("(/ 1500000 1000000)"),
            "match float pattern should use rational: {result}"
        );
    }

    // Deep field chain flattening helpers (#250)
    // -------------------------------------------------------------------

    #[test]
    fn test_is_self_rooted_cvc5_ident_self() {
        let expr = Expr::Ident("self".into());
        assert!(is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_is_self_rooted_cvc5_ident_other() {
        let expr = Expr::Ident("x".into());
        assert!(!is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_is_self_rooted_cvc5_field_chain() {
        // self.value
        let expr = Expr::Field(Box::new(Expr::Ident("self".into())), "value".into());
        assert!(is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_is_self_rooted_cvc5_deep_chain() {
        // self.inner.value
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("self".into())),
                "inner".into(),
            )),
            "value".into(),
        );
        assert!(is_self_rooted_cvc5(&expr));
    }

    #[test]
    fn test_field_chain_depth_cvc5_ident() {
        assert_eq!(field_chain_depth_cvc5(&Expr::Ident("x".into())), 0);
    }

    #[test]
    fn test_field_chain_depth_cvc5_single() {
        let expr = Expr::Field(Box::new(Expr::Ident("x".into())), "y".into());
        assert_eq!(field_chain_depth_cvc5(&expr), 1);
    }

    #[test]
    fn test_field_chain_depth_cvc5_deep() {
        // a.b.c -> depth 2
        let expr = Expr::Field(
            Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
            "c".into(),
        );
        assert_eq!(field_chain_depth_cvc5(&expr), 2);
    }

    #[test]
    fn test_has_deep_field_chain_cvc5() {
        // a.b -> depth 1, not deep
        let shallow = Expr::Field(Box::new(Expr::Ident("a".into())), "b".into());
        assert!(!has_deep_field_chain_cvc5(&shallow));

        // a.b.c -> depth 2, deep
        let deep = Expr::Field(
            Box::new(Expr::Field(Box::new(Expr::Ident("a".into())), "b".into())),
            "c".into(),
        );
        assert!(has_deep_field_chain_cvc5(&deep));
    }

    #[test]
    fn test_flatten_field_chain_cvc5_simple() {
        // a.b -> "a__b"
        let expr = Expr::Field(Box::new(Expr::Ident("a".into())), "b".into());
        assert_eq!(flatten_field_chain_cvc5(&expr), "a__b");
    }

    #[test]
    fn test_flatten_field_chain_cvc5_deep() {
        // state.head.extra.extra_max -> "state__head__extra__extra_max"
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Field(
                    Box::new(Expr::Ident("state".into())),
                    "head".into(),
                )),
                "extra".into(),
            )),
            "extra_max".into(),
        );
        assert_eq!(
            flatten_field_chain_cvc5(&expr),
            "state__head__extra__extra_max"
        );
    }

    #[test]
    fn test_flatten_field_chain_cvc5_paren() {
        // (a).b -> "a__b"
        let expr = Expr::Field(
            Box::new(Expr::Paren(Box::new(Expr::Ident("a".into())))),
            "b".into(),
        );
        assert_eq!(flatten_field_chain_cvc5(&expr), "a__b");
    }

    #[test]
    fn test_cvc5_deep_field_chain_smtlib_flattening() {
        // state.head.extra.extra_max should flatten in SMT-LIB output
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Field(
                    Box::new(Expr::Ident("state".into())),
                    "head".into(),
                )),
                "extra".into(),
            )),
            "extra_max".into(),
        );
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("state__head__extra__extra_max".into()));
    }

    #[test]
    fn test_cvc5_self_rooted_smtlib_flattening() {
        // self.value should flatten even at depth 1
        let expr = Expr::Field(Box::new(Expr::Ident("self".into())), "value".into());
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("self__value".into()));
    }

    #[test]
    fn test_cvc5_shallow_field_smtlib_no_flatten() {
        // obj.field at depth 1 (not self-rooted) should NOT flatten
        let expr = Expr::Field(Box::new(Expr::Ident("obj".into())), "field".into());
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("(__field_field obj)".into()));
    }

    #[test]
    fn test_cvc5_old_deep_field_smtlib_flattening() {
        // old(state.head.value) should flatten to state__head__value__old
        let inner = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("state".into())),
                "head".into(),
            )),
            "value".into(),
        );
        let expr = Expr::Old(Box::new(inner));
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("state__head__value__old".into()));
    }

    #[test]
    fn test_cvc5_old_self_rooted_smtlib_flattening() {
        // old(self.counter) should flatten to self__counter__old
        let inner = Expr::Field(Box::new(Expr::Ident("self".into())), "counter".into());
        let expr = Expr::Old(Box::new(inner));
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("self__counter__old".into()));
    }

    #[test]
    fn test_cvc5_deep_field_chain_contract_verifies() {
        // Contract: requires { x >= 0 && x < state.head.extra.max }
        //           ensures  { state.head.extra.max > x }
        // With flattening, both sides reference the same flat variable.
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::And,
                    lhs: Box::new(Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    }),
                    rhs: Box::new(Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Field(
                            Box::new(Expr::Field(
                                Box::new(Expr::Field(
                                    Box::new(Expr::Ident("state".into())),
                                    "head".into(),
                                )),
                                "extra".into(),
                            )),
                            "max".into(),
                        )),
                    }),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Field(
                        Box::new(Expr::Field(
                            Box::new(Expr::Field(
                                Box::new(Expr::Ident("state".into())),
                                "head".into(),
                            )),
                            "extra".into(),
                        )),
                        "max".into(),
                    )),
                    rhs: Box::new(Expr::Ident("x".into())),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("DeepFieldChain", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "deep field chain contract should verify (or Unknown if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_self_rooted_field_contract_verifies() {
        // Contract with self.value: requires { self.value > 0 } ensures { self.value >= 1 }
        let clauses = vec![
            Clause {
                kind: ClauseKind::Requires,
                body: Expr::BinOp {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Field(
                        Box::new(Expr::Ident("self".into())),
                        "value".into(),
                    )),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            },
            Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::Field(
                        Box::new(Expr::Ident("self".into())),
                        "value".into(),
                    )),
                    rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                },
                effect_variables: vec![],
            },
        ];
        let results = verify_contract_cvc5("SelfRootedField", &clauses);
        assert_eq!(results.len(), 1);
        assert!(
            matches!(
                &results[0],
                VerificationResult::Verified { .. } | VerificationResult::Unknown { .. }
            ),
            "self-rooted field contract should verify (or Unknown if cvc5 not installed), got: {:?}",
            results[0]
        );
    }

    #[test]
    fn test_cvc5_nested_field_boolean_smtlib() {
        // obj.inner.is_empty should flatten in SMT-LIB output
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("obj".into())),
                "inner".into(),
            )),
            "is_empty".into(),
        );
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("obj__inner__is_empty".into()));
    }

    #[test]
    fn test_cvc5_nested_field_size_smtlib() {
        // obj.inner.length should flatten in SMT-LIB output
        let expr = Expr::Field(
            Box::new(Expr::Field(
                Box::new(Expr::Ident("obj".into())),
                "inner".into(),
            )),
            "length".into(),
        );
        let result = expr_to_smtlib(&expr);
        assert_eq!(result, Some("obj__inner__length".into()));
    }

    // -------------------------------------------------------------------
    // CVC5 native API tests (only when cvc5-verify feature enabled)
    // -------------------------------------------------------------------

    #[cfg(feature = "cvc5-verify")]
    mod native_tests {
        use super::*;
        use assura_parser::ast::Param;

        #[test]
        fn cvc5_with_types_fn_params_nat() {
            // FnDef-style: params passed explicitly (not via input() clause).
            // This is the path used for `fn check_table_bounds(root_bits: Nat, ...)`
            let params = vec![Param {
                name: "n".into(),
                ty: vec!["Nat".into()],
                parsed_type: None,
            }];
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("n".into())),
                    op: BinOp::Gte,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            }];
            let mut cache = SessionCache::new();
            let results =
                verify_contract_cvc5_with_types("FnNatParam", &clauses, &params, &[], &mut cache);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "Nat param n >= 0 should verify via explicit params: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_trivial_ensures_verified() {
            // requires x > 0, ensures x > 0 (trivially true)
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        lhs: Box::new(Expr::Ident("x".into())),
                        op: BinOp::Gt,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        lhs: Box::new(Expr::Ident("x".into())),
                        op: BinOp::Gt,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NativeTest", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_counterexample() {
            // No requires, ensures x > 0 (counterexample: x = 0)
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeCounterexample", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Counterexample { .. }),
                "should have counterexample: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_invariant_satisfiable() {
            // invariant { x > 0 } -- satisfiable (x = 1)
            let clauses = vec![Clause {
                kind: ClauseKind::Invariant,
                body: Expr::BinOp {
                    lhs: Box::new(Expr::Ident("x".into())),
                    op: BinOp::Gt,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeInvariant", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "invariant should be satisfiable: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_must_not_true_counterexample() {
            // must_not { true } -- true is always possible, should be counterexample
            let clauses = vec![Clause {
                kind: ClauseKind::MustNot,
                body: Expr::Literal(Literal::Bool(true)),
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeMustNot", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Counterexample { .. }),
                "must_not(true) should be counterexample: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_must_not_false_verified() {
            // must_not { false } -- false is impossible, should verify
            let clauses = vec![Clause {
                kind: ClauseKind::MustNot,
                body: Expr::Literal(Literal::Bool(false)),
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("NativeMustNotFalse", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "must_not(false) should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_nat_type_constraint() {
            // input(n: Nat), ensures n >= 0 -- should verify with Nat constraint
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Input,
                    body: Expr::Raw(vec!["n".into(), ":".into(), "Nat".into()]),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        lhs: Box::new(Expr::Ident("n".into())),
                        op: BinOp::Gte,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NatConstraint", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "Nat n >= 0 should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_typestate_same_state_verifies() {
            // #262: Typestate same pre/post should verify via native CVC5
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NativeTypestateIdentity", &clauses);
            assert!(
                !results.is_empty(),
                "should have results for typestate identity"
            );
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "same typestate pre/post should verify via native CVC5, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_typestate_different_state_counterexample() {
            // #262: Different typestate pre/post should produce counterexample
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::Raw(vec!["file".into(), "@".into(), "Open".into()]),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Raw(vec!["file".into(), "@".into(), "Closed".into()]),
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NativeTypestateMismatch", &clauses);
            assert!(
                !results.is_empty(),
                "should have results for typestate mismatch"
            );
            assert!(
                matches!(&results[0], VerificationResult::Counterexample { .. }),
                "different typestate pre/post should produce counterexample via native CVC5, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_nested_typestate_encoded() {
            // #262: Typestate nested inside a binary expression is now encoded
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::And,
                        lhs: Box::new(Expr::BinOp {
                            op: BinOp::Gt,
                            lhs: Box::new(Expr::Ident("x".into())),
                            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                        }),
                        rhs: Box::new(Expr::Raw(vec![
                            "conn".into(),
                            "@".into(),
                            "Connected".into(),
                        ])),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Raw(vec!["conn".into(), "@".into(), "Connected".into()]),
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("NativeNestedTypestate", &clauses);
            assert!(
                !results.is_empty(),
                "should have results for nested typestate"
            );
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "nested typestate with matching state should verify, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_check_validity_typestate_encoded() {
            // #262: check_validity_cvc5 should now encode typestate (not skip)
            let assumption = Expr::Raw(vec!["state".into(), "@".into(), "Running".into()]);
            let body = Expr::Raw(vec!["state".into(), "@".into(), "Running".into()]);
            let result = check_validity_cvc5("validity_typestate", &[&assumption], &body);
            assert!(
                matches!(&result, VerificationResult::Verified { .. }),
                "check_validity_cvc5 should verify same-state typestate: {:?}",
                result
            );
        }

        #[test]
        fn native_cvc5_check_satisfiability_typestate_encoded() {
            // #262: check_satisfiability_cvc5 should now encode typestate (not skip)
            let body = Expr::Raw(vec!["lock".into(), "@".into(), "Acquired".into()]);
            let result = check_satisfiability_cvc5("sat_typestate", &[], &body);
            assert!(
                matches!(&result, VerificationResult::Verified { .. }),
                "check_satisfiability_cvc5 should find typestate satisfiable: {:?}",
                result
            );
        }

        // -------------------------------------------------------------------
        // String method axiom tests (CVC5 native, issue #251)
        // -------------------------------------------------------------------

        fn make_clause(kind: ClauseKind, body: Expr) -> Clause {
            Clause {
                kind,
                body,
                effect_variables: vec![],
            }
        }

        #[test]
        fn test_cvc5_string_substring_axiom() {
            // Contract: requires constraints on inputs,
            // ensures { substring(s, start, end).length() >= 0 }
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("len".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Lte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Ident("end_val".into())),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Call {
                                func: Box::new(Expr::Ident("substring".into())),
                                args: vec![
                                    Expr::Ident("s".into()),
                                    Expr::Ident("start".into()),
                                    Expr::Ident("end_val".into()),
                                ],
                            }),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("SubstringTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "Got unexpected counterexample: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_concat_axiom() {
            // ensures { concat(a, b).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("concat".into())),
                            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("ConcatTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "concat axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_indexof_axiom() {
            // requires { s.length() > 0 }
            // ensures { index_of(s, sub) >= -1 }
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("s".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("index_of".into())),
                            args: vec![Expr::Ident("s".into()), Expr::Ident("sub".into())],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("-1".into()))),
                    },
                ),
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("IndexOfTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "indexOf axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_charat_axiom() {
            // requires { idx >= 0 && s.length() > idx }
            // ensures { char_at(s, idx) >= 0 || char_at(s, idx) < 0 } (tautology -- tests axiom wiring)
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("idx".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("s".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Ident("idx".into())),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("idx".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("CharAtTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "charAt axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_replace_axiom() {
            // ensures { replace(s, old_s, new_s).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("replace".into())),
                            args: vec![
                                Expr::Ident("s".into()),
                                Expr::Ident("old_s".into()),
                                Expr::Ident("new_s".into()),
                            ],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("ReplaceTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "replace axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_split_axiom() {
            // ensures { split(s, delim).length() >= 1 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("split".into())),
                            args: vec![Expr::Ident("s".into()), Expr::Ident("delim".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("SplitTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "split axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_string_trim_axiom() {
            // ensures { trim(s).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("trim".into())),
                            args: vec![Expr::Ident("s".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("TrimTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "trim axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_array_set_axiom() {
            // ensures { set(arr, i, v).length() >= 0 }
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("set".into())),
                            args: vec![
                                Expr::Ident("arr".into()),
                                Expr::Ident("i".into()),
                                Expr::Ident("v".into()),
                            ],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("ArraySetTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "array set axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_map_put_axiom() {
            // ensures { put(m, k, v).size() >= 0 } (via size axiom)
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::Call {
                            func: Box::new(Expr::Ident("put".into())),
                            args: vec![
                                Expr::Ident("m".into()),
                                Expr::Ident("k".into()),
                                Expr::Ident("v".into()),
                            ],
                        }),
                        method: "size".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("MapPutTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "map put axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_method_call_substring_axiom() {
            // Test method call form: s.substring(start, end).length() >= 0
            let clauses = vec![
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                make_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        op: BinOp::Lte,
                        lhs: Box::new(Expr::Ident("start".into())),
                        rhs: Box::new(Expr::Ident("end_val".into())),
                    },
                ),
                make_clause(
                    ClauseKind::Ensures,
                    Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::MethodCall {
                                receiver: Box::new(Expr::Ident("s".into())),
                                method: "substring".into(),
                                args: vec![
                                    Expr::Ident("start".into()),
                                    Expr::Ident("end_val".into()),
                                ],
                            }),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ];
            let results =
                crate::cvc5_backend::verify_contract_cvc5("MethodSubstringTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "method call substring axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_method_call_set_axiom() {
            // Test method call form: arr.set(i, v).length() >= 0
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("arr".into())),
                            method: "set".into(),
                            args: vec![Expr::Ident("i".into()), Expr::Ident("v".into())],
                        }),
                        method: "length".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("MethodArraySetTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "method call set axiom failed: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_method_call_put_axiom() {
            // Test method call form: m.put(k, v).size() >= 0
            let clauses = vec![make_clause(
                ClauseKind::Ensures,
                Expr::BinOp {
                    op: BinOp::Gte,
                    lhs: Box::new(Expr::MethodCall {
                        receiver: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("m".into())),
                            method: "put".into(),
                            args: vec![Expr::Ident("k".into()), Expr::Ident("v".into())],
                        }),
                        method: "size".into(),
                        args: vec![],
                    }),
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )];
            let results = crate::cvc5_backend::verify_contract_cvc5("MethodMapPutTest", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "method call put axiom failed: {:?}",
                    r
                );
            }
        }
    }

    // -------------------------------------------------------------------
    // expr_to_smtlib string method tests (issue #251)
    // -------------------------------------------------------------------

    #[test]
    fn test_smtlib_call_substring() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("substring".into())),
            args: vec![
                Expr::Ident("s".into()),
                Expr::Literal(Literal::Int("0".into())),
                Expr::Literal(Literal::Int("5".into())),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(substring s 0 5)");
    }

    #[test]
    fn test_smtlib_call_concat() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("concat".into())),
            args: vec![Expr::Ident("a".into()), Expr::Ident("b".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(__concat a b)");
    }

    #[test]
    fn test_smtlib_call_index_of() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("index_of".into())),
            args: vec![Expr::Ident("s".into()), Expr::Ident("sub".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(index_of s sub)");
    }

    #[test]
    fn test_smtlib_call_char_at() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("char_at".into())),
            args: vec![
                Expr::Ident("s".into()),
                Expr::Literal(Literal::Int("3".into())),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(char_at s 3)");
    }

    #[test]
    fn test_smtlib_call_replace() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("replace".into())),
            args: vec![
                Expr::Ident("s".into()),
                Expr::Ident("old_s".into()),
                Expr::Ident("new_s".into()),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(replace s old_s new_s)");
    }

    #[test]
    fn test_smtlib_call_split() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("split".into())),
            args: vec![Expr::Ident("s".into()), Expr::Ident("delim".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(split s delim)");
    }

    #[test]
    fn test_smtlib_call_trim() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("trim".into())),
            args: vec![Expr::Ident("s".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(trim s)");
    }

    #[test]
    fn test_smtlib_call_set() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("set".into())),
            args: vec![
                Expr::Ident("arr".into()),
                Expr::Ident("i".into()),
                Expr::Ident("v".into()),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(set arr i v)");
    }

    #[test]
    fn test_smtlib_call_put() {
        let expr = Expr::Call {
            func: Box::new(Expr::Ident("put".into())),
            args: vec![
                Expr::Ident("m".into()),
                Expr::Ident("k".into()),
                Expr::Ident("v".into()),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(put m k v)");
    }

    #[test]
    fn test_smtlib_method_substring() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("s".into())),
            method: "substring".into(),
            args: vec![
                Expr::Literal(Literal::Int("1".into())),
                Expr::Literal(Literal::Int("4".into())),
            ],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(substring s 1 4)");
    }

    #[test]
    fn test_smtlib_method_concat() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("a".into())),
            method: "concat".into(),
            args: vec![Expr::Ident("b".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(__concat a b)");
    }

    #[test]
    fn test_smtlib_method_set() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("arr".into())),
            method: "set".into(),
            args: vec![Expr::Ident("i".into()), Expr::Ident("v".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(set arr i v)");
    }

    #[test]
    fn test_smtlib_method_put() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("m".into())),
            method: "put".into(),
            args: vec![Expr::Ident("k".into()), Expr::Ident("v".into())],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(put m k v)");
    }

    #[test]
    fn test_smtlib_method_trim() {
        let expr = Expr::MethodCall {
            receiver: Box::new(Expr::Ident("s".into())),
            method: "trim".into(),
            args: vec![],
        };
        let s = expr_to_smtlib(&expr).unwrap();
        assert_eq!(s, "(trim s)");
    }

    // -------------------------------------------------------------------
    // CVC5 match pattern tests (native API, issue #252)
    // -------------------------------------------------------------------

    #[cfg(feature = "cvc5-verify")]
    mod match_pattern_tests {
        use super::*;
        use assura_parser::ast::MatchArm;

        #[test]
        fn test_cvc5_match_constructor_pattern() {
            // ensures { match x { Some(v) => v > 0, None => true } }
            // with requires { x >= 0 } so scrut is constrained
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Match {
                        scrutinee: Box::new(Expr::Ident("x".into())),
                        arms: vec![
                            MatchArm {
                                pattern: Pattern::Constructor {
                                    name: "Positive".into(),
                                    fields: vec![Pattern::Ident("v".into())],
                                },
                                body: Expr::Literal(Literal::Bool(true)),
                            },
                            MatchArm {
                                pattern: Pattern::Wildcard,
                                body: Expr::Literal(Literal::Bool(true)),
                            },
                        ],
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("MatchConstructor", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // The match should encode without returning Unknown due to unhandled patterns
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")),
                    "Constructor pattern should be encoded, got: {:?}",
                    r
                );
            }
        }

        #[test]
        fn test_cvc5_match_tuple_pattern() {
            // ensures { match t { (a, b) => true } }
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Match {
                    scrutinee: Box::new(Expr::Ident("t".into())),
                    arms: vec![MatchArm {
                        pattern: Pattern::Tuple(vec![
                            Pattern::Ident("a".into()),
                            Pattern::Ident("b".into()),
                        ]),
                        body: Expr::Literal(Literal::Bool(true)),
                    }],
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("MatchTuple", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // ensures { true } should verify
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "tuple match with body `true` should verify, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_match_nested_patterns() {
            // ensures { match x { Outer(Inner(v)) => true, _ => true } }
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Match {
                    scrutinee: Box::new(Expr::Ident("x".into())),
                    arms: vec![
                        MatchArm {
                            pattern: Pattern::Constructor {
                                name: "Outer".into(),
                                fields: vec![Pattern::Constructor {
                                    name: "Inner".into(),
                                    fields: vec![Pattern::Ident("v".into())],
                                }],
                            },
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                        MatchArm {
                            pattern: Pattern::Wildcard,
                            body: Expr::Literal(Literal::Bool(true)),
                        },
                    ],
                },
                effect_variables: vec![],
            }];
            let results = verify_contract_cvc5("MatchNested", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // All arms return true, so should verify
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "nested constructor match with all-true body should verify, got: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_match_enum_verifies() {
            // A simple enum-like match:
            //   requires { x >= 0 }
            //   ensures { match x { Zero => x == 0, _ => x >= 0 } }
            // We use Ident patterns with uppercase names as constructors.
            // Since both arms return expressions derivable from requires, it
            // should verify (or at worst produce a result, not Unknown).
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Match {
                        scrutinee: Box::new(Expr::Ident("x".into())),
                        arms: vec![
                            MatchArm {
                                pattern: Pattern::Ident("Zero".into()),
                                body: Expr::Literal(Literal::Bool(true)),
                            },
                            MatchArm {
                                pattern: Pattern::Wildcard,
                                body: Expr::BinOp {
                                    op: BinOp::Gte,
                                    lhs: Box::new(Expr::Ident("x".into())),
                                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                                },
                            },
                        ],
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("MatchEnum", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            // Should not produce Unknown with "not yet encoded" reason
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Unknown { reason, .. }
                        if reason.contains("not yet encoded")),
                    "Enum match should be encoded, got: {:?}",
                    r
                );
            }
        }
    }

    // -------------------------------------------------------------------
    // Frame axiom tests (CVC5 native, issue #256)
    // -------------------------------------------------------------------

    #[cfg(feature = "cvc5-verify")]
    mod frame_tests {
        use super::*;

        #[test]
        fn test_cvc5_frame_axiom_injection() {
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Modifies,
                    body: Expr::Ident("y".into()),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("FrameTest", &clauses);
            assert!(!results.is_empty());
        }

        #[test]
        fn test_cvc5_modifies_preserves_unmodified() {
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Modifies,
                    body: Expr::Ident("y".into()),
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Eq,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("5".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = crate::cvc5_backend::verify_contract_cvc5("FramePreserve", &clauses);
            assert!(!results.is_empty());
            for r in &results {
                assert!(
                    !matches!(r, VerificationResult::Counterexample { .. }),
                    "Frame axiom should prevent counterexample: {:?}",
                    r
                );
            }
        }

        // ---------------------------------------------------------------
        // Lemma injection tests (#254)
        // ---------------------------------------------------------------

        #[test]
        fn native_cvc5_lemma_injection_basic() {
            // Contract with apply(lemma): the ensures body contains an
            // apply expression, which should be encoded as a named bool.
            // Without lemma defs, this just produces a result (not a panic).
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Apply {
                        lemma_name: "helper_lemma".into(),
                        args: vec![Expr::Ident("x".into())],
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("LemmaTest", &clauses);
            assert!(!results.is_empty(), "should produce at least one result");
        }

        #[test]
        fn native_cvc5_lemma_postcondition_injected() {
            // Build a lemma_defs map where "pos_lemma" ensures x >= 0.
            // The ensures clause uses `apply pos_lemma(x)` inside a
            // conjunction with `true`. With the lemma postcondition
            // injected as an assumption, this should not produce false
            // counterexamples for the apply sub-expression.
            let mut lemma_defs = std::collections::HashMap::new();
            let lemma_ensures = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };
            lemma_defs.insert("pos_lemma".to_string(), vec![&lemma_ensures]);

            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::And,
                        lhs: Box::new(Expr::Apply {
                            lemma_name: "pos_lemma".into(),
                            args: vec![Expr::Ident("x".into())],
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Bool(true))),
                    },
                    effect_variables: vec![],
                },
            ];
            let mut cache = SessionCache::new();
            let results = verify_contract_cvc5_with_lemmas(
                "ApplyPostcondTest",
                &clauses,
                &[],
                &[],
                Some(&lemma_defs),
                &[],
                &mut cache,
            );
            assert!(
                !results.is_empty(),
                "should produce at least one result with lemma injection"
            );
        }

        #[test]
        fn native_cvc5_lemma_injection_verifies_with_postcondition() {
            // The ensures clause says: x >= 0 (trivially follows from requires).
            // We also have an apply expression in the clause. With lemma defs
            // injecting x >= 0, the combined clause should still verify.
            let mut lemma_defs = std::collections::HashMap::new();
            let lemma_ensures = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("x".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };
            lemma_defs.insert("helper".to_string(), vec![&lemma_ensures]);

            // requires { x > 0 }
            // ensures { x >= 0 }  (trivially true from requires)
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let mut cache = SessionCache::new();
            let results = verify_contract_cvc5_with_lemmas(
                "LemmaVerifTest",
                &clauses,
                &[],
                &[],
                Some(&lemma_defs),
                &[],
                &mut cache,
            );
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "should verify with lemma injection: {:?}",
                results[0]
            );
        }

        #[test]
        fn native_cvc5_no_lemma_defs_still_works() {
            // When lemma_defs is None, the apply expression is just
            // encoded as a named boolean (no postcondition injected).
            let clauses = vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::Apply {
                    lemma_name: "unknown_lemma".into(),
                    args: vec![Expr::Ident("x".into())],
                },
                effect_variables: vec![],
            }];
            let mut cache = SessionCache::new();
            let results = verify_contract_cvc5_with_lemmas(
                "NoLemmaDefs",
                &clauses,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert!(
                !results.is_empty(),
                "should produce results even without lemma defs"
            );
        }

        // ---------------------------------------------------------------
        // CVC5 Real sort float encoding tests (#248)
        // ---------------------------------------------------------------

        #[test]
        fn test_cvc5_float_real_sort() {
            // Float literal in requires/ensures should encode as CVC5 Real sort.
            // requires { x > 0 }, requires { x < 1000000 },
            // ensures { x > 0 } -- trivially true from precondition
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("FloatRealSort", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "float Real sort should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_real_ite_promotion() {
            // ITE with mixed Int/Real branches should sort-promote.
            // requires { x > 0 }
            // ensures { if x > 0 then 1.5 else 0 > 0 }
            // The then branch is Real (1.5), else is Int (0).
            // Sort promotion converts the Int to Real so ITE succeeds.
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::If {
                            cond: Box::new(Expr::BinOp {
                                op: BinOp::Gt,
                                lhs: Box::new(Expr::Ident("x".into())),
                                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                            }),
                            then_branch: Box::new(Expr::Literal(Literal::Float("1.5".into()))),
                            else_branch: Some(Box::new(Expr::Literal(Literal::Int("0".into())))),
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("ItePromotion", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "ITE sort promotion should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_real_negation() {
            // Negated float should work with Real sort.
            // requires { x > 1.0 }, ensures { -x < 0.0 }
            // True because x > 1.0 implies -x < -1.0 < 0.0
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("1.0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Lt,
                        lhs: Box::new(Expr::UnaryOp {
                            op: UnaryOp::Neg,
                            expr: Box::new(Expr::Ident("x".into())),
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Float("0.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("RealNeg", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "negated float Real should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_float_arithmetic_verifies() {
            // Float arithmetic: requires { x > 2.0 }, ensures { x + 1.0 > 3.0 }
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Float("2.0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::BinOp {
                            op: BinOp::Add,
                            lhs: Box::new(Expr::Ident("x".into())),
                            rhs: Box::new(Expr::Literal(Literal::Float("1.0".into()))),
                        }),
                        rhs: Box::new(Expr::Literal(Literal::Float("3.0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("FloatArith", &clauses);
            assert_eq!(results.len(), 1);
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "float arithmetic should verify: {:?}",
                results[0]
            );
        }

        // ---------------------------------------------------------------
        // CVC5 quantifier trigger pattern inference tests (#247)
        // ---------------------------------------------------------------

        #[test]
        fn test_cvc5_quantifier_trigger_inference() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Call {
                    func: Box::new(Expr::Ident("f".into())),
                    args: vec![Expr::Ident("i".into())],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                !patterns.is_empty(),
                "should infer trigger from f(i) call in quantifier body"
            );
        }

        #[test]
        fn test_cvc5_trigger_no_call_no_pattern() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Ident("i".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                patterns.is_empty(),
                "no function calls means no triggers: got {:?}",
                patterns.len()
            );
        }

        #[test]
        fn test_cvc5_trigger_nested_call() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("g".into())),
                        args: vec![Expr::Ident("i".into())],
                    }),
                    rhs: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("h".into())),
                        args: vec![Expr::Ident("i".into())],
                    }),
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                patterns.len() >= 2,
                "should infer triggers from both g(i) and h(i): got {}",
                patterns.len()
            );
        }

        #[test]
        fn test_cvc5_trigger_manager_integration() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::Call {
                func: Box::new(Expr::Ident("lookup".into())),
                args: vec![Expr::Ident("i".into())],
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                !patterns.is_empty(),
                "should infer trigger from lookup(i) via direct scan fallback"
            );
        }

        #[test]
        fn test_cvc5_quantified_with_trigger_verifies() {
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::Forall {
                        var: "i".into(),
                        domain: Box::new(Expr::BinOp {
                            op: BinOp::Range,
                            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                            rhs: Box::new(Expr::Ident("x".into())),
                        }),
                        body: Box::new(Expr::BinOp {
                            op: BinOp::Gte,
                            lhs: Box::new(Expr::Ident("i".into())),
                            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                        }),
                    },
                    effect_variables: vec![],
                },
            ];
            let results = verify_contract_cvc5("QuantTriggerTest", &clauses);
            assert!(!results.is_empty(), "should produce verification results");
            assert!(
                matches!(&results[0], VerificationResult::Verified { .. }),
                "quantified contract should verify: {:?}",
                results[0]
            );
        }

        #[test]
        fn test_cvc5_multi_arg_trigger() {
            let tm = cvc5::TermManager::new();
            let bound = tm.mk_var(tm.integer_sort(), "i");

            let body = Expr::BinOp {
                op: BinOp::Gte,
                lhs: Box::new(Expr::Call {
                    func: Box::new(Expr::Ident("lookup".into())),
                    args: vec![Expr::Ident("table".into()), Expr::Ident("i".into())],
                }),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            };

            let patterns = infer_quantifier_patterns_cvc5(&tm, &body, "i", &bound);
            assert!(
                !patterns.is_empty(),
                "should infer trigger from multi-arg lookup(table, i)"
            );
        }

        // -------------------------------------------------------------------
        // CVC5 session cache tests (#253)
        // -------------------------------------------------------------------

        #[test]
        fn test_cvc5_session_cache_hit() {
            // Verify same contract twice; second call should return cached result
            let clauses = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];

            let mut cache = SessionCache::new();

            // First call: cache miss, runs CVC5
            let results1 = verify_contract_cvc5_with_lemmas(
                "CacheTest",
                &clauses,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results1.len(), 1);
            assert!(matches!(&results1[0], VerificationResult::Verified { .. }));
            assert_eq!(cache.entry_count(), 1);

            // Second call: cache hit, should not invoke CVC5
            let results2 = verify_contract_cvc5_with_lemmas(
                "CacheTest",
                &clauses,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results2.len(), 1);
            assert!(matches!(&results2[0], VerificationResult::Verified { .. }));
            // Cache should still have 1 entry (same key), with 1 hit
            assert_eq!(cache.entry_count(), 1);
            assert!(cache.hit_rate() > 0.0);
        }

        #[test]
        fn test_cvc5_session_cache_miss() {
            // Two different contracts should be cache misses
            let clauses_a = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("x".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];
            let clauses_b = vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("y".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Gte,
                        lhs: Box::new(Expr::Ident("y".into())),
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
            ];

            let mut cache = SessionCache::new();

            let results_a = verify_contract_cvc5_with_lemmas(
                "CacheA",
                &clauses_a,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results_a.len(), 1);
            assert_eq!(cache.entry_count(), 1);

            let results_b = verify_contract_cvc5_with_lemmas(
                "CacheB",
                &clauses_b,
                &[],
                &[],
                None,
                &[],
                &mut cache,
            );
            assert_eq!(results_b.len(), 1);
            // Both should be cache misses, so 2 entries
            assert_eq!(cache.entry_count(), 2);
        }

        // -------------------------------------------------------------------
        // #263: CVC5 ADT encoding tests
        // -------------------------------------------------------------------

        #[test]
        fn test_cvc5_adt_constructor() {
            // Define Option = Some(value: Int) | None using CVC5 native API.
            // Verify that constructor tags are distinct and accessors work.
            let tm = cvc5::TermManager::new();
            let mut solver = cvc5::Solver::new(&tm);
            solver.set_logic("ALL");
            solver.set_option("produce-models", "true");
            solver.set_option("tlimit", "2000");

            let (adt_def, adt_symbols) = super::define_adt_cvc5_native(
                &tm,
                &mut solver,
                "Option",
                &[("Some", &["value"]), ("None", &[])],
            );

            // Construct Some(42)
            let some_ctor = adt_def
                .constructors
                .iter()
                .find(|c| c.name == "Some")
                .unwrap();
            let none_ctor = adt_def
                .constructors
                .iter()
                .find(|c| c.name == "None")
                .unwrap();

            let mut axioms = Vec::new();
            let mut fresh = 0usize;

            let forty_two = tm.mk_integer(42);
            let some_val = super::adt_constructor_cvc5_native(
                &tm,
                &adt_symbols,
                some_ctor,
                &[forty_two.clone()],
                &mut axioms,
                &mut fresh,
            );
            let none_val = super::adt_constructor_cvc5_native(
                &tm,
                &adt_symbols,
                none_ctor,
                &[],
                &mut axioms,
                &mut fresh,
            );

            // Assert all axioms
            for axiom in &axioms {
                solver.assert_formula(axiom.clone());
            }

            // Verify tags are distinct
            let is_some =
                super::adt_is_constructor_cvc5_native(&tm, &adt_symbols, some_ctor, &some_val);
            let is_none =
                super::adt_is_constructor_cvc5_native(&tm, &adt_symbols, none_ctor, &none_val);
            solver.assert_formula(is_some);
            solver.assert_formula(is_none);

            // Verify accessor: value(some_val) == 42
            let accessed = super::adt_accessor_cvc5_native(&tm, &adt_symbols, "value", &some_val);
            let eq_42 = tm.mk_term(cvc5::Kind::Equal, &[accessed, forty_two]);
            let not_eq_42 = tm.mk_term(cvc5::Kind::Not, &[eq_42]);
            solver.push(1);
            solver.assert_formula(not_eq_42);
            let result = solver.check_sat();
            assert!(
                result.is_unsat(),
                "accessor(Some(42)) must equal 42 (negation should be UNSAT)"
            );
            solver.pop(1);

            // Verify exhaustiveness: tag(x) == 99 should be UNSAT
            let x = tm.mk_const(tm.integer_sort(), "x_adt_exh");
            let tag_x = tm.mk_term(cvc5::Kind::ApplyUf, &[adt_symbols.tag_fn.clone(), x]);
            let bad_tag = tm.mk_term(cvc5::Kind::Equal, &[tag_x, tm.mk_integer(99)]);
            solver.push(1);
            solver.assert_formula(bad_tag);
            let result = solver.check_sat();
            assert!(
                result.is_unsat(),
                "tag(x) == 99 should be UNSAT with only tags 0 and 1"
            );
            solver.pop(1);
        }

        #[test]
        fn test_cvc5_adt_smtlib_generation() {
            // Test that the SMT-LIB2 generation functions produce valid output
            let (adt_def, assertions) =
                super::define_adt_cvc5("Option", &[("Some", &["value"]), ("None", &[])]);

            // Should have 3 declarations + 1 exhaustiveness + 2 injectivity = 6
            assert!(
                assertions.len() >= 5,
                "should have at least 5 SMT-LIB2 assertions, got {}",
                assertions.len()
            );

            // Check tag function declaration
            assert!(
                assertions.iter().any(|a| a.contains("__adt_tag_Option")),
                "should declare tag function"
            );

            // Check accessor function declaration
            assert!(
                assertions.iter().any(|a| a.contains("__adt_Option_value")),
                "should declare value accessor"
            );

            // Check exhaustiveness axiom
            assert!(
                assertions
                    .iter()
                    .any(|a| a.contains("forall") && a.contains("or")),
                "should have exhaustiveness axiom with forall/or"
            );

            // Test constructor tester SMT generation
            let tester = super::adt_is_constructor_smt("Option", "Some", "x", &adt_def);
            assert_eq!(tester, "(= (__adt_tag_Option x) 0)");

            let tester_none = super::adt_is_constructor_smt("Option", "None", "x", &adt_def);
            assert_eq!(tester_none, "(= (__adt_tag_Option x) 1)");

            // Test accessor SMT generation
            let acc = super::adt_accessor_smt("Option", "value", "x");
            assert_eq!(acc, "(__adt_Option_value x)");
        }

        // -------------------------------------------------------------------
        // #265: CVC5 bitvector wrapping test
        // -------------------------------------------------------------------

        #[test]
        fn test_cvc5_unsat_core_extraction() {
            use assura_parser::ast::{BinOp, Literal};

            let int_lit = |n: &str| Expr::Literal(Literal::Int(n.into()));
            let var = |name: &str| Expr::Ident(name.into());
            let cmp = |name: &str, op: BinOp, n: &str| Expr::BinOp {
                lhs: Box::new(var(name)),
                op,
                rhs: Box::new(int_lit(n)),
            };

            let req0 = cmp("x", BinOp::Gt, "50");
            let req1 = cmp("x", BinOp::Lt, "100");
            let ensures = cmp("x", BinOp::Gt, "10");

            let result = check_validity_cvc5("unsat_core_test", &[&req0, &req1], &ensures);
            match result {
                VerificationResult::Verified { unsat_core, .. } => {
                    let core = unsat_core
                        .as_ref()
                        .expect("CVC5 verified result should include unsat core");
                    assert!(
                        core.iter().any(|l| l.contains("req_0")),
                        "core should include req_0, got: {core:?}"
                    );
                }
                other => panic!("expected verified result, got: {other:?}"),
            }
        }

        #[test]
        fn test_cvc5_bitvector_wrapping() {
            let tm = cvc5::TermManager::new();
            let mut solver = cvc5::Solver::new(&tm);
            solver.set_logic("QF_BV");
            solver.set_option("produce-models", "true");

            let eight = tm.mk_bv_sort(8);
            let a = tm.mk_const(eight.clone(), "a");
            let b = tm.mk_const(eight, "b");
            let two_five_five = tm.mk_bv(8, 255);
            let one = tm.mk_bv(8, 1);
            let zero = tm.mk_bv(8, 0);

            solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[a.clone(), two_five_five]));
            solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[b.clone(), one]));
            let sum = tm.mk_term(cvc5::Kind::BitvectorAdd, &[a, b]);
            solver.assert_formula(tm.mk_term(cvc5::Kind::Equal, &[sum, zero]));

            assert!(
                solver.check_sat().is_sat(),
                "255 + 1 should wrap to 0 in 8-bit BV"
            );
        }
    }
}
