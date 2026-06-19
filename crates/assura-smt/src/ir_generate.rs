//! Heuristic contract-to-Implementation-IR generation.
//!
//! Analyzes `ensures` clauses to produce IR bodies richer than identity stubs.
//! Falls back to `stub_ir_sidecar_text` when no pattern matches.

use assura_parser::ast::{BinOp, Clause, ClauseKind, Expr, Literal};
use std::collections::HashMap;

use crate::ir::stub_ir_sidecar_text;

/// A planned IR instruction sequence for the main `fn #0` body.
#[derive(Debug, Clone, PartialEq)]
struct IrGenBody {
    lines: Vec<String>,
}

/// Generate `.ir` sidecar text from contract structure and ensures clauses.
pub fn generate_ir_sidecar_text(
    name: &str,
    params: &[(usize, String)],
    param_names: &[String],
    return_ty: &str,
    clauses: &[Clause],
) -> String {
    let requires_count = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Requires)
        .count();
    let ensures_count = clauses
        .iter()
        .filter(|c| c.kind == ClauseKind::Ensures)
        .count();

    let name_to_slot: HashMap<&str, usize> = param_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    for clause in clauses.iter().filter(|c| c.kind == ClauseKind::Ensures) {
        if let Some(body) = plan_from_ensures(&clause.body, &name_to_slot, return_ty, params.len())
        {
            return format_ir_module(
                name,
                params,
                return_ty,
                requires_count,
                ensures_count,
                &body,
            );
        }
    }

    stub_ir_sidecar_text(name, params, return_ty, requires_count, ensures_count)
}

fn plan_from_ensures(
    expr: &Expr,
    name_to_slot: &HashMap<&str, usize>,
    return_ty: &str,
    param_count: usize,
) -> Option<IrGenBody> {
    if let Some((lhs, rhs)) = equality_operands(expr) {
        if is_result_ident(lhs) {
            return plan_result_equals(rhs, name_to_slot, return_ty, param_count);
        }
        if is_result_ident(rhs) {
            return plan_result_equals(lhs, name_to_slot, return_ty, param_count);
        }
    }
    None
}

fn plan_result_equals(
    other: &Expr,
    name_to_slot: &HashMap<&str, usize>,
    return_ty: &str,
    param_count: usize,
) -> Option<IrGenBody> {
    match other {
        Expr::Ident(name) => {
            let slot = *name_to_slot.get(name.as_str())?;
            Some(single_load(slot, return_ty))
        }
        Expr::Literal(lit) => Some(single_const(&literal_to_ir_const(lit)?, return_ty)),
        Expr::BinOp { op, lhs, rhs } => {
            plan_result_arith(op.clone(), lhs, rhs, name_to_slot, return_ty)
        }
        _ => {
            if param_count == 1 {
                Some(single_load(0, return_ty))
            } else {
                None
            }
        }
    }
}

fn plan_result_arith(
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    name_to_slot: &HashMap<&str, usize>,
    return_ty: &str,
) -> Option<IrGenBody> {
    let ir_op = match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Mod => "mod",
        _ => return None,
    };

    let lhs_slot = expr_to_param_slot(lhs, name_to_slot)?;
    let rhs_slot = expr_to_param_slot(rhs, name_to_slot)?;
    let temp_slot = next_temp_slot(&[lhs_slot, rhs_slot]);

    Some(IrGenBody {
        lines: vec![
            format!("    ${temp_slot} = arith {ir_op} ${lhs_slot} ${rhs_slot} : {return_ty}"),
            format!("    $result = load ${temp_slot} : {return_ty}"),
        ],
    })
}

fn expr_to_param_slot(expr: &Expr, name_to_slot: &HashMap<&str, usize>) -> Option<usize> {
    match expr {
        Expr::Ident(name) => name_to_slot.get(name.as_str()).copied(),
        Expr::Literal(_) => None,
        _ => None,
    }
}

fn next_temp_slot(used: &[usize]) -> usize {
    used.iter().max().copied().unwrap_or(0) + 1
}

fn single_load(slot: usize, return_ty: &str) -> IrGenBody {
    IrGenBody {
        lines: vec![format!("    $result = load ${slot} : {return_ty}")],
    }
}

fn single_const(value: &str, return_ty: &str) -> IrGenBody {
    IrGenBody {
        lines: vec![format!("    $result = const {value} : {return_ty}")],
    }
}

fn literal_to_ir_const(lit: &Literal) -> Option<String> {
    match lit {
        Literal::Int(s) => Some(s.clone()),
        Literal::Float(s) => Some(s.clone()),
        Literal::Bool(b) => Some(if *b { "1".into() } else { "0".into() }),
        Literal::Str(_) => None,
    }
}

fn equality_operands(expr: &Expr) -> Option<(&Expr, &Expr)> {
    match expr {
        Expr::BinOp {
            op: BinOp::Eq,
            lhs,
            rhs,
        } => Some((lhs.as_ref(), rhs.as_ref())),
        _ => None,
    }
}

fn is_result_ident(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(name) if name == "result")
}

fn format_ir_module(
    name: &str,
    params: &[(usize, String)],
    return_ty: &str,
    requires_count: usize,
    ensures_count: usize,
    body: &IrGenBody,
) -> String {
    let module = sanitize_module_name(name);
    let param_list = params
        .iter()
        .map(|(slot, ty)| format!("${slot}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let body_text = body.lines.join("\n");
    format!(
        "// Generated IR for {name} from ensures heuristics\n\
         // Contract: {requires_count} requires, {ensures_count} ensures\n\
         module {module} {{\n\
           fn #0 : ({param_list}) -> {return_ty} ! pure\n\
           pre: true\n\
           {{\n\
         {body_text}\n\
           }}\n\
         }}\n"
    )
}

fn sanitize_module_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, ClauseKind, Expr};

    fn int_param(_name: &str, slot: usize) -> (usize, String) {
        (slot, "Int".into())
    }

    #[test]
    fn generates_load_when_ensures_result_eq_param() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("result".into())),
                rhs: Box::new(Expr::Ident("x".into())),
            },
            effect_variables: vec![],
        }];
        let text =
            generate_ir_sidecar_text("Echo", &[int_param("x", 0)], &["x".into()], "Int", &clauses);
        assert!(text.contains("$result = load $0 : Int"));
        assert!(text.contains("Generated IR"));
    }

    #[test]
    fn generates_const_when_ensures_result_eq_literal() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("result".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("42".into()))),
            },
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text("Const42", &[], &[], "Int", &clauses);
        assert!(text.contains("$result = const 42 : Int"));
    }

    #[test]
    fn generates_arith_when_ensures_result_eq_sum() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                op: BinOp::Eq,
                lhs: Box::new(Expr::Ident("result".into())),
                rhs: Box::new(Expr::BinOp {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Ident("x".into())),
                    rhs: Box::new(Expr::Ident("y".into())),
                }),
            },
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Add",
            &[int_param("x", 0), (1, "Int".into())],
            &["x".into(), "y".into()],
            "Int",
            &clauses,
        );
        assert!(text.contains("arith add $0 $1"));
        assert!(text.contains("$result = load $2"));
    }

    #[test]
    fn falls_back_to_stub_when_ensures_unrecognized() {
        let clauses = vec![Clause {
            kind: ClauseKind::Ensures,
            body: Expr::BinOp {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Ident("result".into())),
                rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
            },
            effect_variables: vec![],
        }];
        let text = generate_ir_sidecar_text(
            "Unknown",
            &[int_param("x", 0)],
            &["x".into()],
            "Int",
            &clauses,
        );
        assert!(text.contains("Stub IR"));
    }
}
