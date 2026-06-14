use super::*;
use assura_parser::ast::{BinOp, Clause, ClauseKind, Literal, UnaryOp};
use std::collections::HashSet;

/// Verify a single contract's clauses using the CVC5 binary.
///
/// Generates SMT-LIB2 scripts and invokes `cvc5 --lang smt2` on each.
pub(crate) fn verify_contract_cvc5(
    contract_name: &str,
    clauses: &[Clause],
) -> Vec<VerificationResult> {
    let mut results = Vec::new();

    // Collect requires clauses as assumptions
    let mut requires_exprs: Vec<&Expr> = Vec::new();
    for clause in clauses {
        if clause.kind == ClauseKind::Requires {
            requires_exprs.push(&clause.body);
        }
    }

    // Check each ensures/invariant clause
    for clause in clauses {
        match &clause.kind {
            ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Rule
            | ClauseKind::MustNot
            | ClauseKind::Decreases => {
                let desc = format!("{contract_name}::{:?}", clause.kind);
                let result =
                    check_clause_cvc5(&desc, &requires_exprs, &clause.body, clause.kind.clone());
                results.push(result);
            }
            _ => {}
        }
    }

    results
}

/// Check a single clause by generating SMT-LIB2 and invoking CVC5.
fn check_clause_cvc5(
    desc: &str,
    requires: &[&Expr],
    ensures_body: &Expr,
    kind: ClauseKind,
) -> VerificationResult {
    // Collect all variable names from the expressions
    let mut vars = HashSet::new();
    for req in requires {
        collect_vars(req, &mut vars);
    }
    collect_vars(ensures_body, &mut vars);

    // Build SMT-LIB2 script
    let mut script = String::new();
    script.push_str("(set-logic ALL)\n");

    // Declare variables
    for var in &vars {
        script.push_str(&format!("(declare-const {var} Int)\n"));
    }

    // Assert requires
    for req in requires {
        if let Some(smt) = expr_to_smtlib(req) {
            script.push_str(&format!("(assert {smt})\n"));
        }
    }

    // Assert negation of ensures (validity check)
    if let Some(smt) = expr_to_smtlib(ensures_body) {
        match kind {
            ClauseKind::Invariant => {
                // Invariant: check satisfiability (not always false)
                script.push_str(&format!("(assert {smt})\n"));
            }
            _ => {
                // Ensures/rule/must_not/decreases: check validity via negation
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

    // Run CVC5
    match run_cvc5(&script) {
        Cvc5Result::Unsat => {
            if matches!(kind, ClauseKind::Invariant) {
                // UNSAT for invariant means it's always false (bad)
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

/// Result of running CVC5 on an SMT-LIB2 script.
enum Cvc5Result {
    Unsat,
    Sat(String),
    Timeout,
    Error(String),
}

/// Run CVC5 on an SMT-LIB2 script string.
fn run_cvc5(script: &str) -> Cvc5Result {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut cmd = Command::new("cvc5");
    cmd.arg("--lang")
        .arg("smt2")
        .arg("--tlimit")
        .arg("1000") // 1 second timeout
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Cvc5Result::Error(format!("cvc5 not found on PATH: {e}"));
        }
    };

    // Write script to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(script.as_bytes());
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
        "unknown" => Cvc5Result::Timeout, // treat unknown as timeout
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

/// Convert an Assura expression to SMT-LIB2 s-expression.
pub(crate) fn expr_to_smtlib(expr: &Expr) -> Option<String> {
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
        Expr::Field(_, _) => None,
        Expr::Index { .. } => None,
        Expr::Block(_) => None,
        Expr::Raw(_) => None,
        Expr::Tuple(_) => None,
        Expr::Match { .. } => None,
        Expr::MethodCall { .. } => None,
        Expr::List(_) => None,
        Expr::Apply { .. } => None,
        Expr::Let { .. } => None,
    }
}

/// Sanitize a name for SMT-LIB2 (replace dots with underscores).
fn sanitize_smtlib_name(name: &str) -> String {
    name.replace('.', "_")
}

/// Collect variable names from an expression.
pub(crate) fn collect_vars(expr: &Expr, vars: &mut HashSet<String>) {
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
        Expr::Forall { body, domain, .. } | Expr::Exists { body, domain, .. } => {
            collect_vars(body, vars);
            collect_vars(domain, vars);
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
        _ => {}
    }
}

/// Parse a CVC5 model output into a CounterexampleModel.
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
