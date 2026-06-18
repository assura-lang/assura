use super::*;
use assura_parser::ast::{BinOp, Clause, ClauseKind, Literal, Pattern, UnaryOp};
use std::collections::HashSet;

// =========================================================================
// Native CVC5 API backend (feature = "cvc5-verify")
// =========================================================================

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

/// Encoder state for the native CVC5 backend.
/// Tracks background axioms, string constants, and fresh variable counter.
#[cfg(feature = "cvc5-verify")]
struct Cvc5EncoderState<'a> {
    axioms: Vec<cvc5::Term<'a>>,
    string_constants: Vec<String>,
    fresh_counter: usize,
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
    verify_contract_cvc5_with_types(contract_name, clauses, &params, &return_ty)
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
) -> Vec<VerificationResult> {
    #[cfg(feature = "cvc5-verify")]
    {
        verify_contract_cvc5_native(contract_name, clauses, params, return_ty)
    }
    #[cfg(not(feature = "cvc5-verify"))]
    {
        verify_contract_cvc5_shellout(contract_name, clauses, params, return_ty)
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
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    let requires_exprs: Vec<&Expr> = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .map(|c| &c.body)
        .collect();

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases => {
                let desc = format!("{contract_name}::{:?}", clause.kind);
                let result = check_clause_cvc5_native(
                    &desc,
                    &requires_exprs,
                    &clause.body,
                    clause.kind.clone(),
                    params,
                    return_ty,
                );
                results.push(result);
            }
            ClauseKind::Other(kind) => {
                // Dispatch to feature-specific verifier
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

#[cfg(feature = "cvc5-verify")]
fn check_clause_cvc5_native(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
) -> VerificationResult {
    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    // Collect all variable names
    let mut var_names = HashSet::new();
    for req in requires {
        collect_vars(req, &mut var_names);
    }
    collect_vars(ensures_body, &mut var_names);

    // Create CVC5 constants for each variable
    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    for name in &var_names {
        let term = tm.mk_const(tm.integer_sort(), name);
        var_map.insert(name.clone(), term);
    }

    // Assert type-level constraints (Nat params get >= 0)
    let zero = tm.mk_integer(0);
    for param in params {
        if param.ty.len() == 1 && param.ty[0] == "Nat" {
            let name = sanitize_smtlib_name(&param.name);
            if let Some(term) = var_map.get(&name) {
                let geq = tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]);
                solver.assert_formula(geq);
            }
        }
    }
    // Nat return type constrains result >= 0
    if return_ty.len() == 1 && return_ty[0] == "Nat" {
        if let Some(term) = var_map.get("__result") {
            let geq = tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero.clone()]);
            solver.assert_formula(geq);
        }
        // Also constrain "result" (different encoding paths use different names)
        if let Some(term) = var_map.get("result") {
            let geq = tm.mk_term(cvc5::Kind::Geq, &[term.clone(), zero]);
            solver.assert_formula(geq);
        }
    }

    let mut enc_state = Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
    };

    // Assert requires as assumptions
    for req in requires {
        if let Some(term) = encode_expr_cvc5(&tm, req, &var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    // Encode the clause body
    let body_term = match encode_expr_cvc5(&tm, ensures_body, &var_map, &mut enc_state) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

    // Assert background axioms collected during encoding
    for axiom in &enc_state.axioms {
        solver.assert_formula(axiom.clone());
    }

    // Assert clause according to verification semantics
    match kind {
        ClauseKind::Invariant => {
            // Invariant: check satisfiability (not always false)
            solver.assert_formula(body_term);
        }
        ClauseKind::MustNot => {
            // MustNot P: assert P directly; UNSAT means P is impossible
            solver.assert_formula(body_term);
        }
        _ => {
            // Ensures/rule/decreases: check validity via negation
            let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
            solver.assert_formula(negated);
        }
    }

    let sat_result = solver.check_sat();

    if sat_result.is_unsat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: "invariant is unsatisfiable".to_string(),
                counter_model: None,
            }
        } else {
            VerificationResult::Verified {
                clause_desc: desc.to_string(),
            }
        }
    } else if sat_result.is_sat() {
        if matches!(kind, ClauseKind::Invariant) {
            VerificationResult::Verified {
                clause_desc: desc.to_string(),
            }
        } else {
            // Extract counterexample model
            let mut variables = Vec::new();
            for (name, term) in &var_map {
                if !name.starts_with("__coerce") {
                    let val = solver.get_value(term.clone());
                    variables.push((name.clone(), val.to_string()));
                }
            }
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
            VerificationResult::Counterexample {
                clause_desc: desc.to_string(),
                model: model_str,
                counter_model,
            }
        }
    } else {
        // Unknown/timeout
        VerificationResult::Timeout {
            clause_desc: desc.to_string(),
        }
    }
}

/// Encode an AST expression as a CVC5 Term using the native API.
///
/// `state` collects background axioms and tracks string constants
/// so that `check_clause_cvc5_native` can assert them before check_sat.
#[cfg(feature = "cvc5-verify")]
fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            let val: i64 = n.parse().ok()?;
            Some(tm.mk_integer(val))
        }
        Expr::Literal(Literal::Bool(b)) => Some(tm.mk_boolean(*b)),
        Expr::Literal(Literal::Float(f_str)) => {
            // Rational approximation matching Z3 backend
            let f: f64 = f_str.parse().unwrap_or(0.0);
            let denom = 1_000_000i64;
            let numer = (f * denom as f64) as i64;
            // CVC5 has no direct Real sort in our integer-only encoding;
            // approximate as numer (scaled integer) for now
            Some(tm.mk_integer(numer))
        }
        Expr::Literal(Literal::Str(s)) => {
            // Named integer constant matching Z3 pattern
            let const_name = format!("__str_{s}");
            let str_val = tm.mk_const(tm.integer_sort(), &const_name);
            // Pairwise distinctness from previously seen string constants
            if !state.string_constants.contains(&const_name) {
                for prev in &state.string_constants {
                    let prev_val = tm.mk_const(tm.integer_sort(), prev);
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[str_val.clone(), prev_val]);
                    let neq = tm.mk_term(cvc5::Kind::Not, &[eq]);
                    state.axioms.push(neq);
                }
                state.string_constants.push(const_name);
            }
            // String length axiom: len("hello") == 5
            let len_name = "__field_len";
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, len_name);
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, str_val.clone()]);
            let str_len = tm.mk_integer(s.len() as i64);
            let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result, str_len]);
            state.axioms.push(len_eq);
            Some(str_val)
        }
        Expr::Ident(name) => {
            let key = if name == "result" {
                "__result".to_string()
            } else {
                sanitize_smtlib_name(name)
            };
            vars.get(&key)
                .cloned()
                .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)))
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = encode_expr_cvc5(tm, lhs, vars, state)?;
            let r = encode_expr_cvc5(tm, rhs, vars, state)?;
            let kind = match op {
                BinOp::Add => cvc5::Kind::Add,
                BinOp::Sub => cvc5::Kind::Sub,
                BinOp::Mul => cvc5::Kind::Mult,
                BinOp::Div => cvc5::Kind::IntsDivision,
                BinOp::Mod => cvc5::Kind::IntsModulus,
                BinOp::Eq => cvc5::Kind::Equal,
                BinOp::Neq => {
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[l, r]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[eq]));
                }
                BinOp::Lt => cvc5::Kind::Lt,
                BinOp::Lte => cvc5::Kind::Leq,
                BinOp::Gt => cvc5::Kind::Gt,
                BinOp::Gte => cvc5::Kind::Geq,
                BinOp::And => cvc5::Kind::And,
                BinOp::Or => cvc5::Kind::Or,
                BinOp::Implies => cvc5::Kind::Implies,
                BinOp::Range => {
                    // Range (a..b): create a fresh Int constrained to [lhs, rhs)
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let fresh = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[fresh.clone(), l]);
                    let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[fresh.clone(), r]);
                    let in_range = tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
                    state.axioms.push(in_range);
                    return Some(fresh);
                }
                BinOp::In => {
                    // In (elem in collection): UF __contains(collection, elem) -> Bool
                    let func_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
                    let contains = tm.mk_const(func_sort, "__contains");
                    return Some(tm.mk_term(cvc5::Kind::ApplyUf, &[contains, r, l]));
                }
                BinOp::NotIn => {
                    // NotIn: negation of In
                    let func_sort =
                        tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
                    let contains = tm.mk_const(func_sort, "__contains");
                    let in_result = tm.mk_term(cvc5::Kind::ApplyUf, &[contains, r, l]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[in_result]));
                }
                BinOp::Concat => {
                    // Concat (a ++ b): fresh value with length axiom
                    let fresh_name = format!("__fresh_{}", state.fresh_counter);
                    state.fresh_counter += 1;
                    let result = tm.mk_const(tm.integer_sort(), &fresh_name);
                    let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let len_func = tm.mk_const(len_sort, "__field_len");
                    let len_l = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), l]);
                    let len_r = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func.clone(), r]);
                    let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, result.clone()]);
                    let sum = tm.mk_term(cvc5::Kind::Add, &[len_l.clone(), len_r.clone()]);
                    let len_eq = tm.mk_term(cvc5::Kind::Equal, &[len_result.clone(), sum]);
                    state.axioms.push(len_eq);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_l, zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_r, zero.clone()]));
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[len_result, zero]));
                    return Some(result);
                }
            };
            Some(tm.mk_term(kind, &[l, r]))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, vars, state)?;
            match op {
                UnaryOp::Not => Some(tm.mk_term(cvc5::Kind::Not, &[e])),
                UnaryOp::Neg => Some(tm.mk_term(cvc5::Kind::Neg, &[e])),
            }
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = encode_expr_cvc5(tm, cond, vars, state)?;
            let t = encode_expr_cvc5(tm, then_branch, vars, state)?;
            if let Some(e) = else_branch {
                let e = encode_expr_cvc5(tm, e, vars, state)?;
                Some(tm.mk_term(cvc5::Kind::Ite, &[c, t, e]))
            } else {
                Some(tm.mk_term(cvc5::Kind::Implies, &[c, t]))
            }
        }
        Expr::Forall { var, domain, body } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name, bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &local_vars, state)?;
            let guarded = guard_quantifier_body_cvc5(tm, domain, &bound_var, b, true, vars, state);
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var]);
            Some(tm.mk_term(cvc5::Kind::Forall, &[bound_list, guarded]))
        }
        Expr::Exists { var, domain, body } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name, bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &local_vars, state)?;
            let guarded = guard_quantifier_body_cvc5(tm, domain, &bound_var, b, false, vars, state);
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var]);
            Some(tm.mk_term(cvc5::Kind::Exists, &[bound_list, guarded]))
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = func.as_ref() {
                let f_name = sanitize_smtlib_name(name);
                if args.is_empty() {
                    return vars
                        .get(&f_name)
                        .cloned()
                        .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)));
                }
                let encoded_args: Option<Vec<cvc5::Term>> = args
                    .iter()
                    .map(|a| encode_expr_cvc5(tm, a, vars, state))
                    .collect();
                let encoded_args = encoded_args?;
                // Built-in functions with known semantics
                match f_name.as_str() {
                    // abs(x) => ite(x >= 0, x, -x)
                    "abs" if encoded_args.len() == 1 => {
                        let x = &encoded_args[0];
                        let zero = tm.mk_integer(0);
                        let neg = tm.mk_term(cvc5::Kind::Neg, &[x.clone()]);
                        let cond = tm.mk_term(cvc5::Kind::Geq, &[x.clone(), zero]);
                        return Some(tm.mk_term(cvc5::Kind::Ite, &[cond, x.clone(), neg]));
                    }
                    // min(a, b) => ite(a <= b, a, b)
                    "min" if encoded_args.len() == 2 => {
                        let (a, b) = (&encoded_args[0], &encoded_args[1]);
                        let cond = tm.mk_term(cvc5::Kind::Leq, &[a.clone(), b.clone()]);
                        return Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]));
                    }
                    // max(a, b) => ite(a >= b, a, b)
                    "max" if encoded_args.len() == 2 => {
                        let (a, b) = (&encoded_args[0], &encoded_args[1]);
                        let cond = tm.mk_term(cvc5::Kind::Geq, &[a.clone(), b.clone()]);
                        return Some(tm.mk_term(cvc5::Kind::Ite, &[cond, a.clone(), b.clone()]));
                    }
                    _ => {}
                }
                // Boolean methods return Bool sort
                if matches!(
                    f_name.as_str(),
                    "contains"
                        | "is_empty"
                        | "is_some"
                        | "is_none"
                        | "is_ok"
                        | "is_err"
                        | "any"
                        | "all"
                        | "contains_key"
                        | "starts_with"
                        | "ends_with"
                        | "is_subset"
                        | "is_superset"
                ) {
                    let domain: Vec<cvc5::Sort> =
                        (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                    let func_sort = tm.mk_fun_sort(&domain, tm.boolean_sort());
                    let func_const = tm.mk_const(func_sort, &f_name);
                    let mut apply_args = vec![func_const];
                    apply_args.extend(encoded_args);
                    return Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args));
                }
                // Size methods get non-negativity axiom
                if matches!(
                    f_name.as_str(),
                    "len" | "length" | "size" | "count" | "capacity"
                ) {
                    let domain: Vec<cvc5::Sort> =
                        (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                    let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                    let func_const = tm.mk_const(func_sort, &f_name);
                    let mut apply_args = vec![func_const];
                    apply_args.extend(encoded_args);
                    let result = tm.mk_term(cvc5::Kind::ApplyUf, &apply_args);
                    let zero = tm.mk_integer(0);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                    return Some(result);
                }
                // Default: uninterpreted function (Int, ..., Int) -> Int
                let domain: Vec<cvc5::Sort> =
                    (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &f_name);
                let mut apply_args = vec![func_const];
                apply_args.extend(encoded_args);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args))
            } else {
                None
            }
        }
        // old(expr): add __old suffix for Ident, recurse for Field/MethodCall
        Expr::Old(inner) => match inner.as_ref() {
            Expr::Ident(name) => {
                let old_name = format!("{name}__old");
                let key = sanitize_smtlib_name(&old_name);
                Some(
                    vars.get(&key)
                        .cloned()
                        .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
                )
            }
            Expr::Field(obj, field) => {
                let old_obj = encode_expr_cvc5(tm, &Expr::Old(obj.clone()), vars, state)?;
                let func_name = format!("__field_{field}");
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_obj]))
            }
            Expr::MethodCall {
                receiver, method, ..
            } => {
                let old_recv = encode_expr_cvc5(tm, &Expr::Old(receiver.clone()), vars, state)?;
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, method);
                Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, old_recv]))
            }
            _ => encode_expr_cvc5(tm, inner, vars, state),
        },
        Expr::Paren(inner) | Expr::Ghost(inner) => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Cast { expr: inner, .. } => encode_expr_cvc5(tm, inner, vars, state),
        Expr::Let {
            name, value, body, ..
        } => {
            let v = encode_expr_cvc5(tm, value, vars, state)?;
            let mut local_vars = vars.clone();
            local_vars.insert(sanitize_smtlib_name(name), v);
            encode_expr_cvc5(tm, body, &local_vars, state)
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            if arms.is_empty() {
                return None;
            }
            let s = encode_expr_cvc5(tm, scrutinee, vars, state)?;
            let mut result: Option<cvc5::Term> = None;
            for arm in arms.iter().rev() {
                let body = encode_expr_cvc5(tm, &arm.body, vars, state)?;
                match &arm.pattern {
                    Pattern::Wildcard | Pattern::Ident(_) => {
                        result = Some(body);
                    }
                    Pattern::Literal(lit) => {
                        let lit_term = match lit {
                            Literal::Int(n) => {
                                let val: i64 = n.parse().ok()?;
                                tm.mk_integer(val)
                            }
                            Literal::Bool(b) => tm.mk_boolean(*b),
                            _ => return None,
                        };
                        let default = result.as_ref()?.clone();
                        let cond = tm.mk_term(cvc5::Kind::Equal, &[s.clone(), lit_term]);
                        result = Some(tm.mk_term(cvc5::Kind::Ite, &[cond, body, default]));
                    }
                    _ => return None,
                }
            }
            result
        }
        // Field access: UF __field_name(receiver)
        Expr::Field(obj, field) => {
            let obj_val = encode_expr_cvc5(tm, obj, vars, state)?;
            let func_name = format!("__field_{field}");
            // Boolean fields return Bool sort
            if matches!(
                field.as_str(),
                "is_empty" | "is_some" | "is_none" | "is_ok" | "is_err"
            ) {
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.boolean_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                return Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]));
            }
            // Size fields get non-negativity axiom
            if matches!(
                field.as_str(),
                "len" | "length" | "size" | "capacity" | "count"
            ) {
                let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &func_name);
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                return Some(result);
            }
            let func_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &func_name);
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[func_const, obj_val]))
        }
        // Index: UF __index(collection, index) with bounds axioms
        Expr::Index { expr: coll, index } => {
            let coll_val = encode_expr_cvc5(tm, coll, vars, state)?;
            let idx_val = encode_expr_cvc5(tm, index, vars, state)?;
            let zero = tm.mk_integer(0);
            // 0 <= index
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[idx_val.clone(), zero.clone()]));
            // len(collection) via UF
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, "__len");
            let len_val = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, coll_val.clone()]);
            // len >= 0
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Geq, &[len_val.clone(), zero]));
            // index < len
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Lt, &[idx_val.clone(), len_val]));
            // UF __index(coll, idx)
            let idx_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let idx_func = tm.mk_const(idx_sort, "__index");
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &[idx_func, coll_val, idx_val]))
        }
        // Block: encode all expressions, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            let mut result = None;
            for e in body {
                result = encode_expr_cvc5(tm, e, vars, state);
            }
            result
        }
        // Raw tokens: basic parsing (single token bools/ints/idents)
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some(tm.mk_boolean(true));
            }
            if tokens.len() == 1 {
                let t = &tokens[0];
                if t == "true" {
                    return Some(tm.mk_boolean(true));
                }
                if t == "false" {
                    return Some(tm.mk_boolean(false));
                }
                if let Ok(n) = t.parse::<i64>() {
                    return Some(tm.mk_integer(n));
                }
                let key = sanitize_smtlib_name(t);
                return vars
                    .get(&key)
                    .cloned()
                    .or_else(|| Some(tm.mk_const(tm.integer_sort(), &key)));
            }
            // Multi-token: try to parse as infix expression
            encode_raw_tokens_cvc5(tm, tokens, vars, state)
        }
        // Tuple: fresh Int with element-access axioms
        Expr::Tuple(elems) => {
            let tuple_name = format!("__tuple_{}", state.fresh_counter);
            state.fresh_counter += 1;
            let tuple_val = tm.mk_const(tm.integer_sort(), &tuple_name);
            let arity = elems.len();
            for (i, elem) in elems.iter().enumerate() {
                if let Some(elem_val) = encode_expr_cvc5(tm, elem, vars, state) {
                    let accessor_name = format!("__tuple_{arity}_{i}");
                    let acc_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
                    let acc_func = tm.mk_const(acc_sort, &accessor_name);
                    let accessed = tm.mk_term(cvc5::Kind::ApplyUf, &[acc_func, tuple_val.clone()]);
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val]));
                }
            }
            Some(tuple_val)
        }
        // MethodCall: prepend receiver, call UF
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            let recv_val = encode_expr_cvc5(tm, receiver, vars, state)?;
            let mut all_encoded = vec![recv_val];
            for arg in args {
                all_encoded.push(encode_expr_cvc5(tm, arg, vars, state)?);
            }
            let f_name = sanitize_smtlib_name(method);
            // Boolean methods return Bool sort
            if matches!(
                f_name.as_str(),
                "contains"
                    | "is_empty"
                    | "is_some"
                    | "is_none"
                    | "is_ok"
                    | "is_err"
                    | "any"
                    | "all"
                    | "contains_key"
                    | "starts_with"
                    | "ends_with"
                    | "is_subset"
                    | "is_superset"
            ) {
                let domain: Vec<cvc5::Sort> =
                    (0..all_encoded.len()).map(|_| tm.integer_sort()).collect();
                let func_sort = tm.mk_fun_sort(&domain, tm.boolean_sort());
                let func_const = tm.mk_const(func_sort, &f_name);
                let mut apply_args = vec![func_const];
                apply_args.extend(all_encoded);
                return Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args));
            }
            // Size methods get non-negativity axiom
            if matches!(
                f_name.as_str(),
                "len" | "length" | "size" | "count" | "capacity"
            ) {
                let domain: Vec<cvc5::Sort> =
                    (0..all_encoded.len()).map(|_| tm.integer_sort()).collect();
                let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                let func_const = tm.mk_const(func_sort, &f_name);
                let mut apply_args = vec![func_const];
                apply_args.extend(all_encoded);
                let result = tm.mk_term(cvc5::Kind::ApplyUf, &apply_args);
                let zero = tm.mk_integer(0);
                state
                    .axioms
                    .push(tm.mk_term(cvc5::Kind::Geq, &[result.clone(), zero]));
                return Some(result);
            }
            // Default: uninterpreted function
            let domain: Vec<cvc5::Sort> =
                (0..all_encoded.len()).map(|_| tm.integer_sort()).collect();
            let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
            let func_const = tm.mk_const(func_sort, &f_name);
            let mut apply_args = vec![func_const];
            apply_args.extend(all_encoded);
            Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args))
        }
        // List: fresh Int with element-access and length axioms
        Expr::List(elems) => {
            let list_name = format!("__list_{}", state.fresh_counter);
            state.fresh_counter += 1;
            let list_val = tm.mk_const(tm.integer_sort(), &list_name);
            let get_sort =
                tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.integer_sort());
            let get_func = tm.mk_const(get_sort, "__list_get");
            for (i, elem) in elems.iter().enumerate() {
                if let Some(elem_val) = encode_expr_cvc5(tm, elem, vars, state) {
                    let idx = tm.mk_integer(i as i64);
                    let accessed = tm.mk_term(
                        cvc5::Kind::ApplyUf,
                        &[get_func.clone(), list_val.clone(), idx],
                    );
                    state
                        .axioms
                        .push(tm.mk_term(cvc5::Kind::Equal, &[accessed, elem_val]));
                }
            }
            // Assert length
            let len_sort = tm.mk_fun_sort(&[tm.integer_sort()], tm.integer_sort());
            let len_func = tm.mk_const(len_sort, "__field_len");
            let len_result = tm.mk_term(cvc5::Kind::ApplyUf, &[len_func, list_val.clone()]);
            let expected_len = tm.mk_integer(elems.len() as i64);
            state
                .axioms
                .push(tm.mk_term(cvc5::Kind::Equal, &[len_result, expected_len]));
            Some(list_val)
        }
        // Apply: encode args for side effects, return named bool
        Expr::Apply { lemma_name, args } => {
            for arg in args {
                let _ = encode_expr_cvc5(tm, arg, vars, state);
            }
            let apply_name = format!("__apply_{lemma_name}");
            Some(tm.mk_const(tm.boolean_sort(), &apply_name))
        }
    }
}

/// Build a domain guard for quantifier bodies (CVC5 native API).
///
/// For range domains (`lo..hi`):
/// - `is_forall=true`:  `(lo <= x && x < hi) => body`
/// - `is_forall=false`: `(lo <= x && x < hi) && body`
///
/// For non-range domains (collections, identifiers), encode
/// membership as an uninterpreted `__domain_contains(domain, x)` predicate.
#[cfg(feature = "cvc5-verify")]
fn guard_quantifier_body_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    domain: &Expr,
    bound_var: &cvc5::Term<'a>,
    body: cvc5::Term<'a>,
    is_forall: bool,
    outer_vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> cvc5::Term<'a> {
    if let Expr::BinOp {
        op: BinOp::Range,
        lhs: lo,
        rhs: hi,
    } = domain
    {
        // Range domain: lo <= x && x < hi
        let lo_val =
            encode_expr_cvc5(tm, lo, outer_vars, state).unwrap_or_else(|| tm.mk_integer(0));
        let hi_val =
            encode_expr_cvc5(tm, hi, outer_vars, state).unwrap_or_else(|| tm.mk_integer(0));
        let ge_lo = tm.mk_term(cvc5::Kind::Geq, &[bound_var.clone(), lo_val]);
        let lt_hi = tm.mk_term(cvc5::Kind::Lt, &[bound_var.clone(), hi_val]);
        let in_range = tm.mk_term(cvc5::Kind::And, &[ge_lo, lt_hi]);
        if is_forall {
            tm.mk_term(cvc5::Kind::Implies, &[in_range, body])
        } else {
            tm.mk_term(cvc5::Kind::And, &[in_range, body])
        }
    } else {
        // Non-range domain: __domain_contains(domain, x) UF
        let domain_val = encode_expr_cvc5(tm, domain, outer_vars, state)
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), "__domain_unknown"));
        let contains_sort =
            tm.mk_fun_sort(&[tm.integer_sort(), tm.integer_sort()], tm.boolean_sort());
        let contains_fn = tm.mk_const(contains_sort, "__domain_contains");
        let membership = tm.mk_term(
            cvc5::Kind::ApplyUf,
            &[contains_fn, domain_val, bound_var.clone()],
        );
        if is_forall {
            tm.mk_term(cvc5::Kind::Implies, &[membership, body])
        } else {
            tm.mk_term(cvc5::Kind::And, &[membership, body])
        }
    }
}

/// Encode multi-token raw expressions for the native CVC5 backend.
/// Handles simple infix patterns: a op b.
#[cfg(feature = "cvc5-verify")]
fn encode_raw_tokens_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    tokens: &[String],
    vars: &HashMap<String, cvc5::Term<'a>>,
    state: &mut Cvc5EncoderState<'a>,
) -> Option<cvc5::Term<'a>> {
    // Three-token pattern: lhs op rhs
    if tokens.len() == 3 {
        let lhs = encode_raw_atom_cvc5(tm, &tokens[0], vars);
        let rhs = encode_raw_atom_cvc5(tm, &tokens[2], vars);
        if let (Some(l), Some(r)) = (lhs, rhs) {
            let kind = match tokens[1].as_str() {
                "+" => Some(cvc5::Kind::Add),
                "-" => Some(cvc5::Kind::Sub),
                "*" => Some(cvc5::Kind::Mult),
                "/" | "div" => Some(cvc5::Kind::IntsDivision),
                "%" | "mod" => Some(cvc5::Kind::IntsModulus),
                "=" | "==" => Some(cvc5::Kind::Equal),
                "!=" => {
                    let eq = tm.mk_term(cvc5::Kind::Equal, &[l, r]);
                    return Some(tm.mk_term(cvc5::Kind::Not, &[eq]));
                }
                "<" => Some(cvc5::Kind::Lt),
                "<=" => Some(cvc5::Kind::Leq),
                ">" => Some(cvc5::Kind::Gt),
                ">=" => Some(cvc5::Kind::Geq),
                "&&" | "and" => Some(cvc5::Kind::And),
                "||" | "or" => Some(cvc5::Kind::Or),
                "=>" | "implies" => Some(cvc5::Kind::Implies),
                _ => None,
            };
            if let Some(k) = kind {
                return Some(tm.mk_term(k, &[l, r]));
            }
        }
    }
    // Fallback: try to parse as a single atom (for tokens that
    // were split but are really one value)
    if tokens.len() == 2 && tokens[0] == "-" {
        if let Some(atom) = encode_raw_atom_cvc5(tm, &tokens[1], vars) {
            return Some(tm.mk_term(cvc5::Kind::Neg, &[atom]));
        }
    }
    if tokens.len() == 2 && (tokens[0] == "!" || tokens[0] == "not") {
        if let Some(atom) = encode_raw_atom_cvc5(tm, &tokens[1], vars) {
            return Some(tm.mk_term(cvc5::Kind::Not, &[atom]));
        }
    }
    // Cannot parse: encode args for side effects, return None
    let _ = state;
    None
}

/// Parse a single raw token as a CVC5 atom.
#[cfg(feature = "cvc5-verify")]
fn encode_raw_atom_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    token: &str,
    vars: &HashMap<String, cvc5::Term<'a>>,
) -> Option<cvc5::Term<'a>> {
    if token == "true" {
        return Some(tm.mk_boolean(true));
    }
    if token == "false" {
        return Some(tm.mk_boolean(false));
    }
    if let Ok(n) = token.parse::<i64>() {
        return Some(tm.mk_integer(n));
    }
    let key = sanitize_smtlib_name(token);
    Some(
        vars.get(&key)
            .cloned()
            .unwrap_or_else(|| tm.mk_const(tm.integer_sort(), &key)),
    )
}

// -------------------------------------------------------------------------
// Generic CVC5 validity checker (reusable for standalone functions)
// -------------------------------------------------------------------------

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
    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    let mut var_names = std::collections::HashSet::new();
    for a in assumptions {
        collect_vars(a, &mut var_names);
    }
    collect_vars(body, &mut var_names);

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    for name in &var_names {
        let term = tm.mk_const(tm.integer_sort(), name);
        var_map.insert(name.clone(), term);
    }

    let mut enc_state = Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
    };

    // Assert assumptions
    for a in assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, a, &var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    // Encode body
    let body_term = match encode_expr_cvc5(&tm, body, &var_map, &mut enc_state) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

    // Assert background axioms
    for axiom in &enc_state.axioms {
        solver.assert_formula(axiom.clone());
    }

    // Negate body, check-sat: UNSAT = valid
    let negated = tm.mk_term(cvc5::Kind::Not, &[body_term]);
    solver.assert_formula(negated);

    let sat_result = solver.check_sat();
    if sat_result.is_unsat() {
        VerificationResult::Verified {
            clause_desc: desc.to_string(),
        }
    } else if sat_result.is_sat() {
        let mut variables = Vec::new();
        for (name, term) in &var_map {
            if !name.starts_with("__coerce") {
                let val = solver.get_value(term.clone());
                variables.push((name.clone(), val.to_string()));
            }
        }
        let model_str = variables
            .iter()
            .map(|(n, v)| format!("{n} = {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        let counter_model = if variables.is_empty() {
            None
        } else {
            Some(crate::result::CounterexampleModel { variables })
        };
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
    let tm = cvc5::TermManager::new();
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

    let mut var_names = std::collections::HashSet::new();
    for a in assumptions {
        collect_vars(a, &mut var_names);
    }
    collect_vars(body, &mut var_names);

    let mut var_map: HashMap<String, cvc5::Term> = HashMap::new();
    for name in &var_names {
        let term = tm.mk_const(tm.integer_sort(), name);
        var_map.insert(name.clone(), term);
    }

    let mut enc_state = Cvc5EncoderState {
        axioms: Vec::new(),
        string_constants: Vec::new(),
        fresh_counter: 0,
    };

    for a in assumptions {
        if let Some(term) = encode_expr_cvc5(&tm, a, &var_map, &mut enc_state) {
            solver.assert_formula(term);
        }
    }

    let body_term = match encode_expr_cvc5(&tm, body, &var_map, &mut enc_state) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

    for axiom in &enc_state.axioms {
        solver.assert_formula(axiom.clone());
    }

    solver.assert_formula(body_term);

    let sat_result = solver.check_sat();
    if sat_result.is_sat() {
        VerificationResult::Verified {
            clause_desc: desc.to_string(),
        }
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
    let mut solver = cvc5::Solver::new(&tm);
    solver.set_logic("ALL");
    solver.set_option("produce-models", "true");
    solver.set_option("tlimit", "2000");

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

    VerificationResult::Verified {
        clause_desc: "taint_safety".to_string(),
    }
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

#[cfg(not(feature = "cvc5-verify"))]
fn verify_contract_cvc5_shellout(
    contract_name: &str,
    clauses: &[Clause],
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    let mut requires_exprs: Vec<&Expr> = Vec::new();
    for clause in clauses {
        if clause.kind == ClauseKind::Requires {
            requires_exprs.push(&clause.body);
        }
    }

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
fn check_clause_cvc5_shellout(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
    params: &[assura_parser::ast::Param],
    return_ty: &[String],
) -> VerificationResult {
    let mut vars = HashSet::new();
    for req in requires {
        collect_vars(req, &mut vars);
    }
    collect_vars(ensures_body, &mut vars);

    let mut script = String::new();
    script.push_str("(set-logic ALL)\n");

    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    // Assert type-level constraints (Nat params get >= 0)
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
        // Also constrain "result" (different encoding paths use different names)
        if vars.contains("result") {
            script.push_str("(assert (>= result 0))\n");
        }
    }

    for req in requires {
        if let Some(smt) = expr_to_smtlib(req) {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }

    if let Some(smt) = expr_to_smtlib(ensures_body) {
        match kind {
            ClauseKind::Invariant => {
                script.push_str(&format!("(assert {smt})\n"));
            }
            ClauseKind::MustNot => {
                script.push_str(&format!("(assert {smt})\n"));
            }
            _ => {
                script.push_str(&format!("(assert (not {smt}))\n"));
            }
        }
    } else {
        return VerificationResult::Unknown {
            clause_desc: desc.to_string(),
            reason: "could not encode clause to SMT-LIB2".into(),
        };
    }

    script.push_str("(check-sat)\n");
    script.push_str("(get-model)\n");

    match run_cvc5_binary(&script) {
        Cvc5Result::Unsat => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: "invariant is unsatisfiable".to_string(),
                    counter_model: None,
                }
            } else {
                VerificationResult::Verified {
                    clause_desc: desc.to_string(),
                }
            }
        }
        Cvc5Result::Sat(model_str) => {
            if matches!(kind, ClauseKind::Invariant) {
                VerificationResult::Verified {
                    clause_desc: desc.to_string(),
                }
            } else {
                let counter_model = parse_smtlib_model(&model_str);
                VerificationResult::Counterexample {
                    clause_desc: desc.to_string(),
                    model: model_str,
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
    }
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
        Expr::Literal(Literal::Float(f)) => Some(f.clone()),
        Expr::Literal(Literal::Str(s)) => {
            // Named integer constant matching Z3 pattern
            Some(format!("__str_{}", sanitize_smtlib_name(s)))
        }
        Expr::Ident(name) => {
            // "result" in ensures context maps to __result
            if name == "result" {
                Some("__result".to_string())
            } else {
                Some(sanitize_smtlib_name(name))
            }
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = expr_to_smtlib(lhs)?;
            let r = expr_to_smtlib(rhs)?;
            let smt_op = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "div",
                BinOp::Mod => "mod",
                BinOp::Eq => "=",
                BinOp::Neq => return Some(format!("(not (= {l} {r}))")),
                BinOp::Lt => "<",
                BinOp::Lte => "<=",
                BinOp::Gt => ">",
                BinOp::Gte => ">=",
                BinOp::And => "and",
                BinOp::Or => "or",
                BinOp::Implies => "=>",
                BinOp::Range => {
                    // Range (a..b): fresh Int constrained to [l, r)
                    return Some(format!(
                        "(let ((__range_fresh (+ {l} 0))) (and (>= __range_fresh {l}) (< __range_fresh {r})))"
                    ));
                }
                BinOp::In => {
                    // In (elem in collection): UF __contains(collection, elem)
                    return Some(format!("(__contains {r} {l})"));
                }
                BinOp::NotIn => {
                    // NotIn: negation of In
                    return Some(format!("(not (__contains {r} {l}))"));
                }
                BinOp::Concat => {
                    // Concat (a ++ b): fresh value with length axiom comment
                    // In shell-out mode we return a symbolic expression;
                    // the length axiom is implicit.
                    return Some(format!("(__concat {l} {r})"));
                }
            };
            Some(format!("({smt_op} {l} {r})"))
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
            if let Expr::BinOp {
                op: BinOp::Range,
                lhs: lo,
                rhs: hi,
            } = domain.as_ref()
            {
                let lo_s = expr_to_smtlib(lo)?;
                let hi_s = expr_to_smtlib(hi)?;
                Some(format!(
                    "(forall (({v} Int)) (=> (and (>= {v} {lo_s}) (< {v} {hi_s})) {b}))"
                ))
            } else {
                let d = expr_to_smtlib(domain).unwrap_or_else(|| v.clone());
                Some(format!(
                    "(forall (({v} Int)) (=> (__domain_contains {d} {v}) {b}))"
                ))
            }
        }
        Expr::Exists { var, domain, body } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            if let Expr::BinOp {
                op: BinOp::Range,
                lhs: lo,
                rhs: hi,
            } = domain.as_ref()
            {
                let lo_s = expr_to_smtlib(lo)?;
                let hi_s = expr_to_smtlib(hi)?;
                Some(format!(
                    "(exists (({v} Int)) (and (and (>= {v} {lo_s}) (< {v} {hi_s})) {b}))"
                ))
            } else {
                let d = expr_to_smtlib(domain).unwrap_or_else(|| v.clone());
                Some(format!(
                    "(exists (({v} Int)) (and (__domain_contains {d} {v}) {b}))"
                ))
            }
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
            // Built-in functions with known semantics
            match f.as_str() {
                "abs" if arg_strs.len() == 1 => {
                    let x = &arg_strs[0];
                    Some(format!("(ite (>= {x} 0) {x} (- {x}))"))
                }
                "min" if arg_strs.len() == 2 => {
                    let (a, b) = (&arg_strs[0], &arg_strs[1]);
                    Some(format!("(ite (<= {a} {b}) {a} {b})"))
                }
                "max" if arg_strs.len() == 2 => {
                    let (a, b) = (&arg_strs[0], &arg_strs[1]);
                    Some(format!("(ite (>= {a} {b}) {a} {b})"))
                }
                _ => Some(format!("({f} {})", arg_strs.join(" "))),
            }
        }
        Expr::Old(inner) => match inner.as_ref() {
            // old(x) -> x__old
            Expr::Ident(name) => {
                let old_name = if name == "result" {
                    "__result__old".to_string()
                } else {
                    format!("{}__old", sanitize_smtlib_name(name))
                };
                Some(old_name)
            }
            // old(obj.field) -> (__field_name (old obj))
            Expr::Field(obj, field) => {
                let old_obj = expr_to_smtlib(&Expr::Old(obj.clone()))?;
                Some(format!("(__field_{field} {old_obj})"))
            }
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
        } => {
            // Encode simple two-arm matches as nested ite chains
            if arms.is_empty() {
                return None;
            }
            let s = expr_to_smtlib(scrutinee)?;
            let mut result = None;
            for arm in arms.iter().rev() {
                let body = expr_to_smtlib(&arm.body)?;
                match &arm.pattern {
                    Pattern::Wildcard | Pattern::Ident(_) => {
                        // Default arm
                        result = Some(body);
                    }
                    Pattern::Literal(lit) => {
                        let lit_smt = match lit {
                            Literal::Int(n) => n.clone(),
                            Literal::Float(f) => f.clone(),
                            Literal::Bool(b) => b.to_string(),
                            Literal::Str(_) => return None,
                        };
                        let default = result.as_ref()?;
                        result = Some(format!("(ite (= {s} {lit_smt}) {body} {default})"));
                    }
                    _ => return None, // Complex patterns cannot be encoded
                }
            }
            result
        }
        // Field access: UF __field_name(obj)
        Expr::Field(obj, field) => {
            let o = expr_to_smtlib(obj)?;
            Some(format!("(__field_{field} {o})"))
        }
        // Index: UF __index(coll, idx)
        Expr::Index { expr: coll, index } => {
            let c = expr_to_smtlib(coll)?;
            let i = expr_to_smtlib(index)?;
            Some(format!("(__index {c} {i})"))
        }
        // Block: encode all, return last
        Expr::Block(body) => {
            if body.is_empty() {
                return Some("true".to_string());
            }
            // SMT-LIB has no block; encode the last expression
            expr_to_smtlib(body.last()?)
        }
        // Raw tokens: basic SMT-LIB encoding
        Expr::Raw(tokens) => {
            if tokens.is_empty() {
                return Some("true".to_string());
            }
            if tokens.len() == 1 {
                let t = &tokens[0];
                if t == "true" || t == "false" {
                    return Some(t.clone());
                }
                if t.parse::<i64>().is_ok() {
                    return Some(t.clone());
                }
                return Some(sanitize_smtlib_name(t));
            }
            // Three-token infix: lhs op rhs
            if tokens.len() == 3 {
                let l = &tokens[0];
                let r = &tokens[2];
                let smt_op = match tokens[1].as_str() {
                    "+" => "+",
                    "-" => "-",
                    "*" => "*",
                    "/" | "div" => "div",
                    "%" | "mod" => "mod",
                    "=" | "==" => "=",
                    "!=" => {
                        return Some(format!(
                            "(not (= {} {}))",
                            sanitize_smtlib_name(l),
                            sanitize_smtlib_name(r)
                        ));
                    }
                    "<" => "<",
                    "<=" => "<=",
                    ">" => ">",
                    ">=" => ">=",
                    "&&" | "and" => "and",
                    "||" | "or" => "or",
                    "=>" | "implies" => "=>",
                    _ => return None,
                };
                return Some(format!(
                    "({smt_op} {} {})",
                    sanitize_smtlib_name(l),
                    sanitize_smtlib_name(r)
                ));
            }
            None
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
            if args.is_empty() {
                Some(format!("({method} {r})"))
            } else {
                let arg_strs: Option<Vec<String>> = args.iter().map(expr_to_smtlib).collect();
                let arg_strs = arg_strs?;
                Some(format!("({method} {r} {})", arg_strs.join(" ")))
            }
        }
        // List: use a fresh variable name
        Expr::List(_) => Some("__list_fresh".to_string()),
        // Apply: return named bool
        Expr::Apply { lemma_name, .. } => Some(format!("__apply_{lemma_name}")),
    }
}

/// Sanitize a name for SMT-LIB2 (replace dots with underscores).
fn sanitize_smtlib_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Collect all variable names referenced in an expression.
pub fn collect_vars(expr: &Expr, vars: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            if name == "result" {
                vars.insert("__result".to_string());
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
                    if !name.starts_with("__coerce") {
                        variables.push((name, value));
                    }
                }
            }
        }
    }
    if variables.is_empty() {
        None
    } else {
        Some(CounterexampleModel { variables })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, Literal, Pattern, UnaryOp};

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
            let results = verify_contract_cvc5_with_types("FnNatParam", &clauses, &params, &[]);
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
    }
}
