//! #975: encode simple Rust function bodies as Assura Implementation IR.
//!
//! Supports integer arithmetic on parameters and literals: `+`, `-`, `*`, `/`,
//! `%`, unary `-`, and nested forms (e.g. `x + y + 1`, `(x + 1) * 2`).
//! Body text is extracted with `syn` from the Rust source (co-publish-safe:
//! does not depend on new assura-rust-analyzer fields).

use assura_rust_analyzer::ParamInfo;
use quote::ToTokens;

/// Extract a simple trailing return expression for `fn_name` from Rust source.
pub(crate) fn extract_body_return(source: &str, fn_name: &str) -> Option<String> {
    let file = syn::parse_file(source).ok()?;
    for item in &file.items {
        match item {
            syn::Item::Fn(func) if func.sig.ident == fn_name => {
                return body_return_from_block(&func.block);
            }
            syn::Item::Impl(imp) => {
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(method) = impl_item
                        && method.sig.ident == fn_name
                    {
                        return body_return_from_block(&method.block);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn body_return_from_block(block: &syn::Block) -> Option<String> {
    match block.stmts.as_slice() {
        [syn::Stmt::Expr(syn::Expr::Return(ret), _)] => ret.expr.as_ref().map(|e| expr_source(e)),
        [syn::Stmt::Expr(expr, _)] => Some(expr_source(expr)),
        // Single let binding returned: `let y = e; y` → encode `e` (#986).
        [syn::Stmt::Local(local), syn::Stmt::Expr(ret, _)] => {
            let bind_name = match &local.pat {
                syn::Pat::Ident(id) => id.ident.to_string(),
                _ => return None,
            };
            let init = local.init.as_ref()?;
            let ret_name = match ret {
                syn::Expr::Path(p) if p.path.segments.len() == 1 => {
                    p.path.segments[0].ident.to_string()
                }
                _ => return None,
            };
            if bind_name != ret_name {
                return None;
            }
            Some(expr_source(&init.expr))
        }
        _ => None,
    }
}

fn expr_source(expr: &syn::Expr) -> String {
    expr.to_token_stream()
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build IR text for a function if `body_return` is a simple supported shape.
pub(crate) fn try_ir_from_rust_body(
    item_name: &str,
    params: &[ParamInfo],
    return_ty: Option<&str>,
    body_return: &str,
) -> Option<String> {
    let ret_assura = return_ty
        .map(assura_codegen::type_map::rust_type_to_assura)
        .unwrap_or_else(|| "Int".to_string());
    if !matches!(ret_assura.as_str(), "Int" | "Nat") {
        return None;
    }

    let param_names: Vec<&str> = params
        .iter()
        .filter(|p| p.name != "self")
        .map(|p| p.name.as_str())
        .collect();
    if param_names.is_empty() {
        return None;
    }

    for p in params.iter().filter(|p| p.name != "self") {
        let ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
        if !matches!(ty.as_str(), "Int" | "Nat") {
            return None;
        }
    }

    let expr: syn::Expr = syn::parse_str(body_return).ok()?;

    let mut sig_parts = Vec::new();
    for (i, p) in params.iter().filter(|p| p.name != "self").enumerate() {
        let ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
        sig_parts.push(format!("${i}: {ty}"));
    }
    let sig = sig_parts.join(", ");

    // Simple if: multi-block IR (see Clamp.ir).
    if let syn::Expr::If(if_expr) = &expr {
        return try_ir_from_if(item_name, &sig, &ret_assura, &param_names, if_expr);
    }

    let mut lines = Vec::new();
    let mut next = param_names.len();
    let result_slot = encode_syn_expr(&expr, &param_names, &mut lines, &mut next)?;
    lines.push(format!("$result = load ${result_slot} : Int"));

    let mut ir = String::new();
    ir.push_str(&format!("module {item_name} {{\n"));
    ir.push_str(&format!("  fn #0 : ({sig}) -> {ret_assura} ! pure\n"));
    ir.push_str("  {\n");
    for line in lines {
        ir.push_str("    ");
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  }\n");
    ir.push_str("}\n");
    Some(ir)
}

/// `if cond { then } else { else }` as IR blocks #1 / #2 (Clamp.ir style).
fn try_ir_from_if(
    item_name: &str,
    sig: &str,
    ret_assura: &str,
    param_names: &[&str],
    if_expr: &syn::ExprIf,
) -> Option<String> {
    let else_expr = if_expr.else_branch.as_ref()?.1.as_ref();
    // Only simple expression branches (not nested if / blocks with multi-stmt).
    let then_expr = block_as_expr(&if_expr.then_branch)?;
    let else_expr = match else_expr {
        syn::Expr::Block(b) => block_as_expr(&b.block)?,
        other => other,
    };

    let mut main_lines = Vec::new();
    let mut next = param_names.len();
    let cond_slot = encode_syn_expr(&if_expr.cond, param_names, &mut main_lines, &mut next)?;
    // Condition must be Bool (cmp produces Bool; truthy int not supported).
    main_lines.push(format!(
        "${} = if ${cond_slot} then #1 else #2 : {ret_assura}",
        next
    ));
    let if_out = next;
    next += 1;
    main_lines.push(format!("$result = load ${if_out} : {ret_assura}"));

    let mut then_lines = Vec::new();
    let mut then_next = param_names.len().max(next);
    let then_slot = encode_syn_expr(then_expr, param_names, &mut then_lines, &mut then_next)?;
    then_lines.push(format!("$result = load ${then_slot} : {ret_assura}"));

    let mut else_lines = Vec::new();
    let mut else_next = param_names.len().max(then_next);
    let else_slot = encode_syn_expr(else_expr, param_names, &mut else_lines, &mut else_next)?;
    else_lines.push(format!("$result = load ${else_slot} : {ret_assura}"));

    let mut ir = String::new();
    ir.push_str(&format!("module {item_name} {{\n"));
    ir.push_str(&format!("  fn #0 : ({sig}) -> {ret_assura} ! pure\n  {{\n"));
    for line in main_lines {
        ir.push_str("    ");
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  }\n");
    ir.push_str(&format!("  fn #1 : () -> {ret_assura} ! pure\n  {{\n"));
    for line in then_lines {
        ir.push_str("    ");
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  }\n");
    ir.push_str(&format!("  fn #2 : () -> {ret_assura} ! pure\n  {{\n"));
    for line in else_lines {
        ir.push_str("    ");
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  }\n");
    ir.push_str("}\n");
    Some(ir)
}

fn block_as_expr(block: &syn::Block) -> Option<&syn::Expr> {
    match block.stmts.as_slice() {
        [syn::Stmt::Expr(e, _)] => Some(e),
        _ => None,
    }
}

/// Encode `expr` into IR lines; returns the slot holding the value.
fn encode_syn_expr(
    expr: &syn::Expr,
    param_names: &[&str],
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    match expr {
        syn::Expr::Paren(p) => encode_syn_expr(&p.expr, param_names, lines, next),
        syn::Expr::Group(g) => encode_syn_expr(&g.expr, param_names, lines, next),
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            let name = path.path.segments[0].ident.to_string();
            param_names.iter().position(|n| *n == name)
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => {
            let val = n.base10_digits();
            // Reject overly large literals that don't fit i64 for IR const.
            let _ = val.parse::<i64>().ok()?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = const {val} : Int"));
            Some(slot)
        }
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            let zero = *next;
            *next += 1;
            lines.push(format!("${zero} = const 0 : Int"));
            let inner = encode_syn_expr(&u.expr, param_names, lines, next)?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = arith sub ${zero} ${inner} : Int"));
            Some(slot)
        }
        syn::Expr::Binary(b) => {
            // Comparisons → Bool slots for `if` conditions.
            if let Some(cmp) = match &b.op {
                syn::BinOp::Lt(_) => Some("lt"),
                syn::BinOp::Gt(_) => Some("gt"),
                syn::BinOp::Le(_) => Some("le"),
                syn::BinOp::Ge(_) => Some("ge"),
                syn::BinOp::Eq(_) => Some("eq"),
                syn::BinOp::Ne(_) => Some("ne"),
                _ => None,
            } {
                let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
                let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = cmp {cmp} ${lhs} ${rhs} : Bool"));
                return Some(slot);
            }
            let ir_op = match &b.op {
                syn::BinOp::Add(_) => "add",
                syn::BinOp::Sub(_) => "sub",
                syn::BinOp::Mul(_) => "mul",
                syn::BinOp::Div(_) => "div",
                syn::BinOp::Rem(_) => "mod",
                _ => return None,
            };
            let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
            let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = arith {ir_op} ${lhs} ${rhs} : Int"));
            Some(slot)
        }
        // i64::abs / min / max method receivers (IR `call abs/min/max`).
        syn::Expr::MethodCall(m) => {
            let method = m.method.to_string();
            match (method.as_str(), m.args.len()) {
                ("abs", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${a}) : Int"));
                    Some(slot)
                }
                ("min" | "max", 1) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call {method} (${a}, ${b}) : Int"));
                    Some(slot)
                }
                _ => None,
            }
        }
        // Free-function form: abs(x), min(x, y), max(x, y)
        syn::Expr::Call(c) => {
            let syn::Expr::Path(path) = c.func.as_ref() else {
                return None;
            };
            if path.path.segments.len() != 1 {
                return None;
            }
            let name = path.path.segments[0].ident.to_string();
            match (name.as_str(), c.args.len()) {
                ("abs", 1) => {
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${a}) : Int"));
                    Some(slot)
                }
                ("min" | "max", 2) => {
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let b = encode_syn_expr(&c.args[1], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call {name} (${a}, ${b}) : Int"));
                    Some(slot)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Params / return type from an annotated item Function.
pub(crate) fn function_params_return(
    kind: &assura_rust_analyzer::AnnotatedItemKind,
) -> Option<(&[ParamInfo], Option<&str>)> {
    match kind {
        assura_rust_analyzer::AnnotatedItemKind::Function {
            params,
            return_type,
            ..
        } => Some((params.as_slice(), return_type.as_deref())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_rust_analyzer::ParamInfo;

    fn px() -> Vec<ParamInfo> {
        vec![ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        }]
    }

    #[test]
    fn extract_identity_and_add() {
        let src = r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
fn good(x: i64) -> i64 { x + 1 }
fn with_let(x: i64) -> i64 { let y = x + 1; y }
"#;
        assert_eq!(extract_body_return(src, "bad").as_deref(), Some("x"));
        assert_eq!(extract_body_return(src, "good").as_deref(), Some("x + 1"));
        assert_eq!(
            extract_body_return(src, "with_let").as_deref(),
            Some("x + 1")
        );
    }

    #[test]
    fn identity_body_ir() {
        let ir = try_ir_from_rust_body("Id", &px(), Some("i64"), "x").expect("ir");
        assert!(ir.contains("$result = load $0 : Int"), "{ir}");
    }

    #[test]
    fn add_one_body_ir() {
        let ir = try_ir_from_rust_body("Inc", &px(), Some("i64"), "x + 1").expect("ir");
        assert!(ir.contains("arith add"), "{ir}");
        assert!(ir.contains("const 1"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Inc").expect("parse");
    }

    #[test]
    fn nested_add_mul_and_unary() {
        let params = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let nest = try_ir_from_rust_body("Nest", &params, Some("i64"), "x + y + 1").expect("nest");
        assert!(nest.contains("arith add"), "{nest}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&nest, "Nest").expect("parse nest");

        let mul = try_ir_from_rust_body("Mul", &px(), Some("i64"), "x * 2").expect("mul");
        assert!(mul.contains("arith mul"), "{mul}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&mul, "Mul").expect("parse mul");

        let neg = try_ir_from_rust_body("Neg", &px(), Some("i64"), "- x").expect("neg");
        assert!(neg.contains("arith sub"), "{neg}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&neg, "Neg").expect("parse neg");

        let nested = try_ir_from_rust_body("N2", &px(), Some("i64"), "(x + 1) * 2").expect("n2");
        assert!(
            nested.contains("arith mul") && nested.contains("arith add"),
            "{nested}"
        );
    }

    #[test]
    fn two_param_add_ir() {
        let params = vec![
            ParamInfo {
                name: "a".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "b".into(),
                ty: "i64".into(),
            },
        ];
        let ir = try_ir_from_rust_body("Add", &params, Some("i64"), "a + b").expect("ir");
        assert!(ir.contains("arith add $0 $1"), "{ir}");
    }

    #[test]
    fn simple_if_body_ir() {
        let ir = try_ir_from_rust_body("Clamp0", &px(), Some("i64"), "if x > 0 { x } else { 0 }")
            .expect("if ir");
        assert!(
            ir.contains("cmp gt") && ir.contains("then #1 else #2"),
            "{ir}"
        );
        assert!(ir.contains("fn #1") && ir.contains("fn #2"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Clamp0").expect("parse if");
    }

    #[test]
    fn abs_min_max_method_and_call() {
        let abs = try_ir_from_rust_body("A", &px(), Some("i64"), "x . abs ()").expect("abs");
        assert!(abs.contains("call abs"), "{abs}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&abs, "A").expect("parse abs");

        let params = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let min = try_ir_from_rust_body("M", &params, Some("i64"), "x . min (y)").expect("min");
        assert!(min.contains("call min"), "{min}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&min, "M").expect("parse min");
    }

    #[test]
    fn unsupported_returns_none() {
        assert!(try_ir_from_rust_body("F", &px(), Some("i64"), "x && y").is_none());
        assert!(try_ir_from_rust_body("F", &px(), Some("i64"), "foo(x)").is_none());
    }
}
