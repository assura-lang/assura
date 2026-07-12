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
    use crate::hir::*;

    let params: Vec<RustParam> = b
        .params
        .iter()
        .map(|p| {
            let ty_tokens = p.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            RustParam {
                name: p.name.clone(),
                ty: RustType::Raw(map_type_tokens(&ty_tokens)),
            }
        })
        .collect();

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

    let mut body: Vec<RustStmt> = Vec::new();

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                body.push(RustStmt::Raw(format!(
                    "let {OLD_VAR_PREFIX}{var} = {rust_expr}.clone();"
                )));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Requires assertions
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            body.push(RustStmt::Assert {
                cond: expr,
                label: "requires".into(),
            });
        }
    }

    // Call the actual Rust function
    body.push(RustStmt::Raw(format!(
        "let {RESULT_VAR}: {ret} = {rust_path}({args_s});"
    )));

    // Ensures assertions
    for ens in &ensures_exprs {
        body.push(RustStmt::Assert {
            cond: ens.clone(),
            label: "ensures".into(),
        });
    }

    body.push(RustStmt::Expr(RustExpr::Ident(RESULT_VAR.into())));

    let item = RustItem::Fn(RustFn {
        name: b.name.clone(),
        params,
        ret: Some(RustType::Raw(ret)),
        body,
        doc: vec![format!("Bind: {} -> {rust_path}", b.name)],
        ..RustFn::default()
    });
    code.push_str(&render_item_raw(&item));
}

// ---------------------------------------------------------------------------
// Extern declarations
// ---------------------------------------------------------------------------

pub(crate) fn generate_extern(ex: &ExternDecl, code: &mut String) {
    use crate::hir::*;

    let params: Vec<RustParam> = ex
        .params
        .iter()
        .map(|p| {
            let ty_tokens = p.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            RustParam {
                name: p.name.clone(),
                ty: RustType::Raw(map_type_tokens(&ty_tokens)),
            }
        })
        .collect();

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

    let mut body: Vec<RustStmt> = Vec::new();

    // SEC.2 compile-time: untrusted externs without contracts emit compile_error!
    if is_untrusted && !has_contract {
        body.push(RustStmt::Raw(format!(
            "compile_error!(\"FFI boundary violation: untrusted extern `{}` \
             has no contract; add requires/ensures\");",
            ex.name
        )));
    }

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                body.push(RustStmt::Raw(format!(
                    "let {OLD_VAR_PREFIX}{var} = {rust_expr}.clone();"
                )));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Requires assertions
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            body.push(RustStmt::Assert {
                cond: expr,
                label: "requires".into(),
            });
        }
    }

    if ensures_exprs.is_empty() && (has_contract || !is_untrusted) {
        body.push(RustStmt::Expr(RustExpr::Todo(
            "extern function: implementation required".into(),
        )));
    } else if !ensures_exprs.is_empty() {
        body.push(RustStmt::Raw(format!(
            "let {RESULT_VAR}: {ret} = todo!(\"extern function: implementation required\");"
        )));
        for ens in &ensures_exprs {
            body.push(RustStmt::Assert {
                cond: ens.clone(),
                label: "ensures".into(),
            });
        }
        body.push(RustStmt::Expr(RustExpr::Ident(RESULT_VAR.into())));
    }

    let item = RustItem::Fn(RustFn {
        name: ex.name.clone(),
        params,
        ret: Some(RustType::Raw(ret)),
        body,
        is_unsafe: trust_level.is_some(),
        doc: vec![format!(
            "Extern: {} [ffi_boundary: {}]",
            ex.name,
            trust_level.as_deref().unwrap_or("none")
        )],
        ..RustFn::default()
    });
    code.push_str(&render_item_raw(&item));
}

// ---------------------------------------------------------------------------
// Function definitions
// ---------------------------------------------------------------------------

pub(crate) fn generate_fn_def(
    f: &FnDef,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    use crate::hir::*;

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&f.clauses);
    if !error_variants.is_empty() {
        let err_item = build_error_enum(&f.name, &error_variants);
        code.push_str(&render_item_raw(&err_item));
    }
    let error_enum_name = if !error_variants.is_empty() {
        Some(format!("{}Error", f.name))
    } else {
        None
    };

    let params: Vec<RustParam> = f
        .params
        .iter()
        .map(|p| {
            let ty_tokens = p.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            RustParam {
                name: p.name.clone(),
                ty: RustType::Raw(map_type_tokens(&ty_tokens)),
            }
        })
        .collect();

    let ret_ty = match &f.return_ty {
        None => "()".to_string(),
        Some(te) => map_type_tokens(&te.to_tokens()),
    };

    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{ret_ty}, {err_name}>")
    } else {
        ret_ty.clone()
    };

    let ret = if f.return_ty.is_none() && error_enum_name.is_none() {
        None
    } else {
        Some(RustType::Raw(return_type))
    };

    let mut body: Vec<RustStmt> = Vec::new();

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                body.push(RustStmt::Raw(format!(
                    "let {OLD_VAR_PREFIX}{var} = {rust_expr}.clone();"
                )));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Requires assertions
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            body.push(RustStmt::Assert {
                cond: expr,
                label: "requires".into(),
            });
        }
    }

    // Feature-specific annotations (CORE/SEC/MEM/CONC/FMT/NUM/PLAT/PERF/TEST/MISC)
    {
        let mut feature_code = String::new();
        crate::features::generate_all_feature_clauses(&f.clauses, &f.name, &mut feature_code);
        if !feature_code.is_empty() {
            body.push(RustStmt::Raw(feature_code));
        }
    }

    let ir_body = ir_bodies.and_then(|m| m.get(&f.name));

    if ensures_exprs.is_empty() {
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Expr(RustExpr::Todo(
                "implementation provided by AI agent".into(),
            )));
        }
    } else if let Some(ir) = ir_body {
        body.push(RustStmt::Raw(ir.clone()));
    } else {
        body.push(RustStmt::Raw(format!(
            "let {RESULT_VAR}: {ret_ty} = todo!(\"implementation provided by AI agent\");"
        )));
        for ens in &ensures_exprs {
            body.push(RustStmt::Assert {
                cond: ens.clone(),
                label: "ensures".into(),
            });
        }
        if error_enum_name.is_some() {
            body.push(RustStmt::Expr(RustExpr::Ok(Box::new(RustExpr::Ident(
                RESULT_VAR.into(),
            )))));
        } else {
            body.push(RustStmt::Expr(RustExpr::Ident(RESULT_VAR.into())));
        }
    }

    let item = RustItem::Fn(RustFn {
        name: f.name.clone(),
        params,
        ret,
        body,
        ..RustFn::default()
    });
    code.push_str(&render_item_raw(&item));
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;

    fn mk_param(name: &str, ty: &str) -> assura_ast::Param {
        assura_ast::Param {
            name: name.into(),
            ty: Some(assura_ast::TypeExpr::named(ty)),
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
            return_ty: Some(assura_ast::TypeExpr::named("Bytes")),
            clauses: vec![],
        };
        let mut code = String::new();
        generate_bind(&b, &mut code);
        assert!(code.contains("pub fn my_fn(path: String) -> Vec<u8>"));
        assert!(code.contains("std::fs::read(path)"));
        assert!(code.contains(RESULT_VAR));
    }

    #[test]
    fn bind_with_requires() {
        let b = BindDecl {
            name: "safe_div".into(),
            target_path: "math::divide".into(),
            params: vec![mk_param("a", "Int"), mk_param("b", "Int")],
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
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
        assert!(
            code.contains("debug_assert!((i128::from(b) != i128::from(0))"),
            "bind requires: {code}"
        );
    }

    #[test]
    fn bind_with_ensures() {
        let b = BindDecl {
            name: "abs".into(),
            target_path: "math::abs".into(),
            params: vec![mk_param("x", "Int")],
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
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
        assert!(code.contains(RESULT_VAR), "should use result var");
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
            return_ty: Some(assura_ast::TypeExpr::named("Bytes")),
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
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
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
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
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
        generate_fn_def(&f, &mut code, None);
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
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
            clauses: vec![],
        };
        let mut code = String::new();
        generate_fn_def(&f, &mut code, None);
        assert!(code.contains("pub fn add(a: i64, b: i64) -> i64"));
    }

    #[test]
    fn fn_def_with_requires_and_ensures() {
        let f = FnDef {
            name: "safe_div".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![mk_param("a", "Int"), mk_param("b", "Int")],
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
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
        generate_fn_def(&f, &mut code, None);
        assert!(
            code.contains("debug_assert!((i128::from(b) != i128::from(0))"),
            "requires b != 0: {code}"
        );
        assert!(code.contains(RESULT_VAR), "result variable");
        assert!(code.contains("ensures"), "ensures assertion");
    }

    #[test]
    fn fn_def_old_expr_saved() {
        let f = FnDef {
            name: "incr".into(),
            is_ghost: false,
            is_lemma: false,
            params: vec![mk_param("x", "Int")],
            return_ty: Some(assura_ast::TypeExpr::named("Int")),
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
        generate_fn_def(&f, &mut code, None);
        assert!(
            code.contains(&format!("let {OLD_VAR_PREFIX}x = x.clone()")),
            "should save old(x)"
        );
    }
}
