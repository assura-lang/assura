//! Bind, extern, and function definition code generation.

use super::*;

// ---------------------------------------------------------------------------
// Bind declarations (checked wrappers for existing Rust functions)
// ---------------------------------------------------------------------------

/// Generate a checked wrapper for a `bind` declaration.
///
/// A `bind` maps an existing Rust function path to an Assura contract name.
/// The generated code calls the real function and wraps it with
/// `debug_assert!` checks for `requires` and `ensures` clauses.
pub(crate) fn generate_bind(b: &BindDecl, code: &mut String) {
    let params_s: String = b
        .params
        .iter()
        .map(|p| {
            let ty_tokens = p.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            format!("{}: {}", p.name, map_type_tokens(&ty_tokens))
        })
        .collect::<Vec<_>>()
        .join(", ");

    let ret = match &b.return_ty {
        None => "()".to_string(),
        Some(te) => map_type_tokens(&te.to_tokens()),
    };

    let args_s: String = b
        .params
        .iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    let rust_path = &b.target_path;

    code.push_str(&format!(
        "/// Bind: {} -> {rust_path}\npub fn {}({params_s}) -> {ret} {{\n",
        b.name, b.name
    ));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    // Call the actual Rust function
    code.push_str(&format!(
        "    let __result: {ret} = {rust_path}({args_s});\n"
    ));

    // Generate ensures assertions on the result
    for ens in &ensures_exprs {
        generate_debug_assert(code, ens, "ensures");
    }

    code.push_str("    __result\n");
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Extern declarations
// ---------------------------------------------------------------------------

pub(crate) fn generate_extern(ex: &ExternDecl, code: &mut String) {
    let params_s: String = ex
        .params
        .iter()
        .map(|p| {
            let ty_tokens = p.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            format!("{}: {}", p.name, map_type_tokens(&ty_tokens))
        })
        .collect::<Vec<_>>()
        .join(", ");

    let ret = match &ex.return_ty {
        None => "()".to_string(),
        Some(te) => map_type_tokens(&te.to_tokens()),
    };

    // SEC.2 compile-time enforcement: determine trust boundary from clauses
    let trust_level = ex.clauses.iter().find_map(|c| {
        if matches!(&c.kind, ClauseKind::Other(k) if k == "trust" || k == "boundary") {
            match &c.body.node {
                Expr::Ident(v) => Some(v.as_str().to_string()),
                _ => None,
            }
        } else {
            None
        }
    });
    let is_untrusted = trust_level.as_deref() == Some("untrusted");
    let has_contract = ex
        .clauses
        .iter()
        .any(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures);

    // SEC.2 compile-time: extern functions generate `unsafe fn` so Rust's
    // type system enforces that callers must use an unsafe block, providing
    // compile-time visibility of FFI boundary crossings.
    let unsafe_kw = if trust_level.is_some() { "unsafe " } else { "" };

    // Generate as a function with contract assertions
    code.push_str(&format!(
        "/// Extern: {} [ffi_boundary: {}]\npub {unsafe_kw}fn {}({params_s}) -> {ret} {{\n",
        ex.name,
        trust_level.as_deref().unwrap_or("none"),
        ex.name
    ));

    // SEC.2 compile-time: untrusted externs without contracts emit compile_error!
    // so the generated Rust will not compile until contracts are added.
    if is_untrusted && !has_contract {
        code.push_str(&format!(
            "    compile_error!(\"FFI boundary violation: untrusted extern `{}` \
             has no contract; add requires/ensures\");\n",
            ex.name
        ));
    }

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    if ensures_exprs.is_empty() && (has_contract || !is_untrusted) {
        code.push_str("    todo!(\"extern function: implementation required\")\n");
    } else if !ensures_exprs.is_empty() {
        code.push_str(&format!(
            "    let __result: {ret} = todo!(\"extern function: implementation required\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        code.push_str("    __result\n");
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Function definitions
// ---------------------------------------------------------------------------

pub(crate) fn generate_fn_def(f: &FnDef, code: &mut String) {
    let params_s: String = f
        .params
        .iter()
        .map(|p| {
            let ty_tokens = p.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            format!("{}: {}", p.name, map_type_tokens(&ty_tokens))
        })
        .collect::<Vec<_>>()
        .join(", ");

    let ret_ty = match &f.return_ty {
        None => "()".to_string(),
        Some(te) => map_type_tokens(&te.to_tokens()),
    };

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&f.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", f.name);
        generate_error_enum(&f.name, &error_variants, code);
        Some(name)
    } else {
        None
    };

    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{ret_ty}, {err_name}>")
    } else {
        ret_ty.clone()
    };

    let ret_sig = if f.return_ty.is_none() && error_enum_name.is_none() {
        String::new()
    } else {
        format!(" -> {return_type}")
    };

    code.push_str(&format!("pub fn {}({params_s}){ret_sig} {{\n", f.name));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    // Feature-specific annotations (CORE/SEC/MEM/CONC/FMT/NUM/PLAT/PERF/TEST/MISC)
    {
        let mut feature_code = String::new();
        crate::features::generate_all_feature_clauses(&f.clauses, &f.name, &mut feature_code);
        if !feature_code.is_empty() {
            code.push_str(&feature_code);
        }
    }

    if ensures_exprs.is_empty() {
        code.push_str("    todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {ret_ty} = todo!(\"implementation provided by AI agent\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        if error_enum_name.is_some() {
            code.push_str("    Ok(__result)\n");
        } else {
            code.push_str("    __result\n");
        }
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;

    fn mk_param(name: &str, ty: &str) -> assura_ast::Param {
        assura_ast::Param {
            name: name.into(),
            ty: assura_ast::try_parse_type_tokens(&[ty.to_string()]),
        }
    }

    fn mk_clause(kind: ClauseKind, body: SpExpr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    // ---- generate_bind ----

    #[test]
    fn bind_no_clauses() {
        let b = BindDecl {
            name: "my_fn".into(),
            target_path: "std::fs::read".into(),
            params: vec![mk_param("path", "String")],
            return_ty: assura_ast::try_parse_type_tokens(&["Bytes".to_string()]),
            clauses: vec![],
        };
        let mut code = String::new();
        generate_bind(&b, &mut code);
        assert!(code.contains("pub fn my_fn(path: String) -> Vec<u8>"));
        assert!(code.contains("std::fs::read(path)"));
        assert!(code.contains("__result"));
    }

    #[test]
    fn bind_with_requires() {
        let b = BindDecl {
            name: "safe_div".into(),
            target_path: "math::divide".into(),
            params: vec![mk_param("a", "Int"), mk_param("b", "Int")],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![mk_clause(
                ClauseKind::Requires,
                Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                    op: BinOp::Neq,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
            )],
        };
        let mut code = String::new();
        generate_bind(&b, &mut code);
        assert!(code.contains("debug_assert!((b != 0)"));
    }

    #[test]
    fn bind_with_ensures() {
        let b = BindDecl {
            name: "abs".into(),
            target_path: "math::abs".into(),
            params: vec![mk_param("x", "Int")],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![mk_clause(
                ClauseKind::Ensures,
                Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                    op: BinOp::Gte,
                    rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                }),
            )],
        };
        let mut code = String::new();
        generate_bind(&b, &mut code);
        assert!(code.contains("ensures"), "should have ensures assertion");
        assert!(code.contains("__result"), "should use __result");
    }

    #[test]
    fn bind_no_return_type_defaults_unit() {
        let b = BindDecl {
            name: "log".into(),
            target_path: "logger::log".into(),
            params: vec![],
            return_ty: None,
            clauses: vec![],
        };
        let mut code = String::new();
        generate_bind(&b, &mut code);
        assert!(code.contains("-> ()"), "should default to ()");
    }

    // ---- generate_extern ----

    #[test]
    fn extern_basic() {
        let ex = ExternDecl {
            name: "crypto_hash".into(),
            params: vec![mk_param("data", "Bytes")],
            return_ty: assura_ast::try_parse_type_tokens(&["Bytes".to_string()]),
            clauses: vec![],
        };
        let mut code = String::new();
        generate_extern(&ex, &mut code);
        assert!(code.contains("pub fn crypto_hash(data: Vec<u8>) -> Vec<u8>"));
        assert!(code.contains("todo!"));
    }

    #[test]
    fn extern_with_trust_boundary_untrusted() {
        let ex = ExternDecl {
            name: "ffi_call".into(),
            params: vec![],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![mk_clause(
                ClauseKind::Other("trust".into()),
                Spanned::no_span(Expr::Ident("untrusted".into())),
            )],
        };
        let mut code = String::new();
        generate_extern(&ex, &mut code);
        assert!(code.contains("unsafe fn ffi_call"), "should be unsafe");
        assert!(code.contains("compile_error!"), "no contract on untrusted");
    }

    #[test]
    fn extern_untrusted_with_contract_no_compile_error() {
        let ex = ExternDecl {
            name: "ffi_call".into(),
            params: vec![mk_param("x", "Int")],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![
                mk_clause(
                    ClauseKind::Other("trust".into()),
                    Spanned::no_span(Expr::Ident("untrusted".into())),
                ),
                mk_clause(
                    ClauseKind::Requires,
                    Spanned::no_span(Expr::BinOp {
                        lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                        op: BinOp::Gt,
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                    }),
                ),
            ],
        };
        let mut code = String::new();
        generate_extern(&ex, &mut code);
        assert!(code.contains("unsafe fn"), "should be unsafe");
        assert!(!code.contains("compile_error!"), "has contract, no error");
    }

    // ---- generate_fn_def ----

    #[test]
    fn fn_def_no_return() {
        let f = FnDef {
            name: "do_work".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![],
            return_ty: None,
            clauses: vec![],
        };
        let mut code = String::new();
        generate_fn_def(&f, &mut code);
        assert!(code.contains("pub fn do_work()"));
        assert!(!code.contains(" -> "), "no return type");
        assert!(code.contains("todo!"));
    }

    #[test]
    fn fn_def_with_return_type() {
        let f = FnDef {
            name: "add".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![mk_param("a", "Int"), mk_param("b", "Int")],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![],
        };
        let mut code = String::new();
        generate_fn_def(&f, &mut code);
        assert!(code.contains("pub fn add(a: i64, b: i64) -> i64"));
    }

    #[test]
    fn fn_def_with_requires_and_ensures() {
        let f = FnDef {
            name: "safe_div".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![mk_param("a", "Int"), mk_param("b", "Int")],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![
                mk_clause(
                    ClauseKind::Requires,
                    Spanned::no_span(Expr::BinOp {
                        lhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                        op: BinOp::Neq,
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
                    }),
                ),
                mk_clause(
                    ClauseKind::Ensures,
                    Spanned::no_span(Expr::BinOp {
                        lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                        op: BinOp::Eq,
                        rhs: Box::new(Spanned::no_span(Expr::BinOp {
                            lhs: Box::new(Spanned::no_span(Expr::Ident("a".into()))),
                            op: BinOp::Div,
                            rhs: Box::new(Spanned::no_span(Expr::Ident("b".into()))),
                        })),
                    }),
                ),
            ],
        };
        let mut code = String::new();
        generate_fn_def(&f, &mut code);
        assert!(code.contains("debug_assert!((b != 0)"), "requires b != 0");
        assert!(code.contains("__result"), "result variable");
        assert!(code.contains("ensures"), "ensures assertion");
    }

    #[test]
    fn fn_def_old_expr_saved() {
        let f = FnDef {
            name: "incr".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![mk_param("x", "Int")],
            return_ty: assura_ast::try_parse_type_tokens(&["Int".to_string()]),
            clauses: vec![mk_clause(
                ClauseKind::Ensures,
                Spanned::no_span(Expr::BinOp {
                    lhs: Box::new(Spanned::no_span(Expr::Ident("result".into()))),
                    op: BinOp::Eq,
                    rhs: Box::new(Spanned::no_span(Expr::BinOp {
                        lhs: Box::new(Spanned::no_span(Expr::Old(Box::new(Spanned::no_span(
                            Expr::Ident("x".into()),
                        ))))),
                        op: BinOp::Add,
                        rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("1".into())))),
                    })),
                }),
            )],
        };
        let mut code = String::new();
        generate_fn_def(&f, &mut code);
        assert!(
            code.contains("let __old_x = x.clone()"),
            "should save old(x)"
        );
    }
}
