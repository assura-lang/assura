//! #975: encode simple Rust function bodies as Assura Implementation IR.
//!
//! Supports identity and integer `+` / `-` on parameters and integer literals.
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

    let expr = body_return.trim();
    let body_lines = encode_expr_to_ir_lines(expr, &param_names)?;

    let mut sig_parts = Vec::new();
    for (i, p) in params.iter().filter(|p| p.name != "self").enumerate() {
        let ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
        if !matches!(ty.as_str(), "Int" | "Nat") {
            return None;
        }
        sig_parts.push(format!("${i}: {ty}"));
    }
    let sig = sig_parts.join(", ");

    let mut ir = String::new();
    ir.push_str(&format!("module {item_name} {{\n"));
    ir.push_str(&format!("  fn #0 : ({sig}) -> {ret_assura} ! pure\n"));
    ir.push_str("  {\n");
    for line in body_lines {
        ir.push_str("    ");
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  }\n");
    ir.push_str("}\n");
    Some(ir)
}

fn encode_expr_to_ir_lines(expr: &str, param_names: &[&str]) -> Option<Vec<String>> {
    let mut e = expr.trim().to_string();
    while e.starts_with('(') && e.ends_with(')') && e.len() > 2 {
        e = e[1..e.len() - 1].trim().to_string();
    }

    if let Some(idx) = param_names.iter().position(|n| *n == e) {
        return Some(vec![format!("$result = load ${idx} : Int")]);
    }

    if e.parse::<i64>().is_ok() {
        return Some(vec![
            format!("$1 = const {e} : Int"),
            "$result = load $1 : Int".into(),
        ]);
    }

    for op in ["+", "-"] {
        if let Some((left, right)) = split_binary(&e, op) {
            let left = left.trim();
            let right = right.trim();
            let mut lines = Vec::new();
            let mut next = param_names.len();
            let lname = materialize_atom(left, param_names, &mut lines, &mut next)?;
            let rname = materialize_atom(right, param_names, &mut lines, &mut next)?;
            let arith = if op == "+" { "add" } else { "sub" };
            let out = format!("${next}");
            lines.push(format!("{out} = arith {arith} {lname} {rname} : Int"));
            lines.push(format!("$result = load {out} : Int"));
            return Some(lines);
        }
    }

    None
}

fn split_binary<'a>(expr: &'a str, op: &str) -> Option<(&'a str, &'a str)> {
    let mut depth = 0i32;
    let bytes = expr.as_bytes();
    let op_b = op.as_bytes()[0];
    for i in (0..bytes.len()).rev() {
        match bytes[i] {
            b')' => depth += 1,
            b'(' => depth -= 1,
            c if c == op_b && depth == 0 && i > 0 && i + 1 < bytes.len() => {
                let left = expr[..i].trim();
                let right = expr[i + 1..].trim();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, right));
                }
            }
            _ => {}
        }
    }
    None
}

fn materialize_atom(
    atom: &str,
    param_names: &[&str],
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<String> {
    if let Some(idx) = param_names.iter().position(|n| *n == atom) {
        return Some(format!("${idx}"));
    }
    if atom.parse::<i64>().is_ok() {
        let use_slot = format!("${next}");
        *next += 1;
        lines.push(format!("{use_slot} = const {atom} : Int"));
        return Some(use_slot);
    }
    None
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

    #[test]
    fn extract_identity_and_add() {
        let src = r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
fn good(x: i64) -> i64 { x + 1 }
"#;
        assert_eq!(extract_body_return(src, "bad").as_deref(), Some("x"));
        assert_eq!(extract_body_return(src, "good").as_deref(), Some("x + 1"));
    }

    #[test]
    fn identity_body_ir() {
        let params = vec![ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        }];
        let ir = try_ir_from_rust_body("Id", &params, Some("i64"), "x").expect("ir");
        assert!(ir.contains("$result = load $0 : Int"), "{ir}");
    }

    #[test]
    fn add_one_body_ir() {
        let params = vec![ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        }];
        let ir = try_ir_from_rust_body("Inc", &params, Some("i64"), "x + 1").expect("ir");
        assert!(ir.contains("arith add"), "{ir}");
        assert!(ir.contains("const 1"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Inc").expect("parse");
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
    fn unsupported_returns_none() {
        let params = vec![ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        }];
        assert!(try_ir_from_rust_body("F", &params, Some("i64"), "x * 2").is_none());
    }
}
