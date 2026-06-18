use super::*;
use assura_parser::ast::{BinOp, Clause, ClauseKind, Literal, Pattern, UnaryOp};
use std::collections::HashSet;

// =========================================================================
// Native CVC5 API backend (feature = "cvc5-verify")
// =========================================================================

#[cfg(feature = "cvc5-verify")]
use std::collections::HashMap;

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

    // Assert requires as assumptions
    for req in requires {
        if let Some(term) = encode_expr_cvc5(&tm, req, &var_map) {
            solver.assert_formula(term);
        }
    }

    // Encode the clause body
    let body_term = match encode_expr_cvc5(&tm, ensures_body, &var_map) {
        Some(t) => t,
        None => {
            return VerificationResult::Unknown {
                clause_desc: desc.to_string(),
                reason: "could not encode clause to CVC5 terms".into(),
            };
        }
    };

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
#[cfg(feature = "cvc5-verify")]
fn encode_expr_cvc5<'a>(
    tm: &'a cvc5::TermManager,
    expr: &Expr,
    vars: &HashMap<String, cvc5::Term<'a>>,
) -> Option<cvc5::Term<'a>> {
    match expr {
        Expr::Literal(Literal::Int(n)) => {
            let val: i64 = n.parse().ok()?;
            Some(tm.mk_integer(val))
        }
        Expr::Literal(Literal::Bool(b)) => Some(tm.mk_boolean(*b)),
        Expr::Literal(Literal::Float(_)) | Expr::Literal(Literal::Str(_)) => None,
        Expr::Ident(name) => {
            let key = if name == "result" {
                "__result".to_string()
            } else {
                sanitize_smtlib_name(name)
            };
            vars.get(&key).cloned().or_else(|| {
                // Create a fresh constant for unknown variables
                Some(tm.mk_const(tm.integer_sort(), &key))
            })
        }
        Expr::BinOp { op, lhs, rhs } => {
            let l = encode_expr_cvc5(tm, lhs, vars)?;
            let r = encode_expr_cvc5(tm, rhs, vars)?;
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
                BinOp::Range | BinOp::In | BinOp::NotIn | BinOp::Concat => return None,
            };
            Some(tm.mk_term(kind, &[l, r]))
        }
        Expr::UnaryOp { op, expr: inner } => {
            let e = encode_expr_cvc5(tm, inner, vars)?;
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
            let c = encode_expr_cvc5(tm, cond, vars)?;
            let t = encode_expr_cvc5(tm, then_branch, vars)?;
            if let Some(e) = else_branch {
                let e = encode_expr_cvc5(tm, e, vars)?;
                Some(tm.mk_term(cvc5::Kind::Ite, &[c, t, e]))
            } else {
                Some(tm.mk_term(cvc5::Kind::Implies, &[c, t]))
            }
        }
        Expr::Forall { var, body, .. } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name, bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &local_vars)?;
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var]);
            Some(tm.mk_term(cvc5::Kind::Forall, &[bound_list, b]))
        }
        Expr::Exists { var, body, .. } => {
            let v_name = sanitize_smtlib_name(var);
            let bound_var = tm.mk_var(tm.integer_sort(), &v_name);
            let mut local_vars = vars.clone();
            local_vars.insert(v_name, bound_var.clone());
            let b = encode_expr_cvc5(tm, body, &local_vars)?;
            let bound_list = tm.mk_term(cvc5::Kind::VariableList, &[bound_var]);
            Some(tm.mk_term(cvc5::Kind::Exists, &[bound_list, b]))
        }
        Expr::Call { func, args } => {
            // Uninterpreted function: create a function sort and apply
            if let Expr::Ident(name) = func.as_ref() {
                let f_name = sanitize_smtlib_name(name);
                if args.is_empty() {
                    // Treat as a constant
                    vars.get(&f_name)
                        .cloned()
                        .or_else(|| Some(tm.mk_const(tm.integer_sort(), &f_name)))
                } else {
                    // Encode arguments; return None if any fail
                    let encoded_args: Option<Vec<cvc5::Term>> =
                        args.iter().map(|a| encode_expr_cvc5(tm, a, vars)).collect();
                    let encoded_args = encoded_args?;
                    // Create uninterpreted function sort: (Int, ..., Int) -> Int
                    let domain: Vec<cvc5::Sort> =
                        (0..encoded_args.len()).map(|_| tm.integer_sort()).collect();
                    let func_sort = tm.mk_fun_sort(&domain, tm.integer_sort());
                    let func_const = tm.mk_const(func_sort, &f_name);
                    let mut apply_args = vec![func_const];
                    apply_args.extend(encoded_args);
                    Some(tm.mk_term(cvc5::Kind::ApplyUf, &apply_args))
                }
            } else {
                None
            }
        }
        Expr::Old(inner) | Expr::Paren(inner) | Expr::Ghost(inner) => {
            encode_expr_cvc5(tm, inner, vars)
        }
        Expr::Cast { expr: inner, .. } => encode_expr_cvc5(tm, inner, vars),
        Expr::Let {
            name, value, body, ..
        } => {
            // Encode let as substitution: evaluate value, bind in scope
            let v = encode_expr_cvc5(tm, value, vars)?;
            let mut local_vars = vars.clone();
            local_vars.insert(sanitize_smtlib_name(name), v);
            encode_expr_cvc5(tm, body, &local_vars)
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            if arms.is_empty() {
                return None;
            }
            let s = encode_expr_cvc5(tm, scrutinee, vars)?;
            let mut result: Option<cvc5::Term> = None;
            for arm in arms.iter().rev() {
                let body = encode_expr_cvc5(tm, &arm.body, vars)?;
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
        // Types we cannot encode
        Expr::Field(_, _)
        | Expr::Index { .. }
        | Expr::Block(_)
        | Expr::Raw(_)
        | Expr::Tuple(_)
        | Expr::MethodCall { .. }
        | Expr::List(_)
        | Expr::Apply { .. } => None,
    }
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
    if return_ty.len() == 1 && return_ty[0] == "Nat" && vars.contains("__result") {
        script.push_str("(assert (>= __result 0))\n");
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
        Expr::Literal(Literal::Str(_)) => None, // strings not easily supported
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
                BinOp::Range => return None, // ranges not directly encodable
                BinOp::In | BinOp::NotIn => return None,
                BinOp::Concat => return None,
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
        Expr::Forall {
            var,
            domain: _,
            body,
        } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            Some(format!("(forall (({v} Int)) {b})"))
        }
        Expr::Exists {
            var,
            domain: _,
            body,
        } => {
            let v = sanitize_smtlib_name(var);
            let b = expr_to_smtlib(body)?;
            Some(format!("(exists (({v} Int)) {b})"))
        }
        Expr::Call { func, args } => {
            // func is Box<Expr>, extract name from Ident
            let f = match func.as_ref() {
                Expr::Ident(name) => sanitize_smtlib_name(name),
                _ => return None,
            };
            if args.is_empty() {
                Some(f)
            } else {
                let arg_strs: Option<Vec<String>> = args.iter().map(expr_to_smtlib).collect();
                let arg_strs = arg_strs?;
                Some(format!("({f} {})", arg_strs.join(" ")))
            }
        }
        Expr::Old(inner) => expr_to_smtlib(inner), // old(x) = x for SMT
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
        Expr::Field(_, _) => None, // SMT-LIB cannot represent field access
        Expr::Index { .. } => None, // SMT-LIB cannot represent indexing
        Expr::Block(_) => None,    // SMT-LIB cannot represent block expressions
        Expr::Raw(_) => None,      // SMT-LIB cannot represent raw token sequences
        Expr::Tuple(_) => None,    // SMT-LIB cannot represent tuple expressions
        Expr::MethodCall { .. } => None, // SMT-LIB cannot represent method calls
        Expr::List(_) => None,     // SMT-LIB cannot represent list literals
        Expr::Apply { .. } => None, // SMT-LIB cannot represent apply expressions
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
    use assura_parser::ast::{BinOp, Literal, UnaryOp};

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
    fn test_smtlib_string_returns_none() {
        let expr = Expr::Literal(Literal::Str("hello".into()));
        assert_eq!(expr_to_smtlib(&expr), None);
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
    fn test_smtlib_binop_range_returns_none() {
        let expr = Expr::BinOp {
            op: BinOp::Range,
            lhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            rhs: Box::new(Expr::Literal(Literal::Int("10".into()))),
        };
        assert_eq!(expr_to_smtlib(&expr), None);
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
    fn test_smtlib_forall() {
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
            Some("(forall ((i Int)) (>= i 0))".into())
        );
    }

    #[test]
    fn test_smtlib_exists() {
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
            Some("(exists ((x Int)) (= x 0))".into())
        );
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
    fn test_smtlib_old_transparent() {
        let expr = Expr::Old(Box::new(Expr::Ident("x".into())));
        assert_eq!(expr_to_smtlib(&expr), Some("x".into()));
    }

    #[test]
    fn test_smtlib_paren_transparent() {
        let expr = Expr::Paren(Box::new(Expr::Literal(Literal::Int("5".into()))));
        assert_eq!(expr_to_smtlib(&expr), Some("5".into()));
    }

    #[test]
    fn test_smtlib_raw_returns_none() {
        let expr = Expr::Raw(vec!["foo".into()]);
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
