//! Clause body type checking.
//!
//! Handles parameter extraction from input clauses, output type inference,
//! and type-checking clause bodies against their expected types.

use assura_parser::ast::{ClauseKind, Decl, Expr, ServiceItem};

use crate::{
    Type, TypeEnv, TypeError, check_ghost_fn_effects, check_lemma_fn_effects, infer_expr,
    infer_expr_spanned, parse_type_tokens, type_from_hir_type,
};

// ---------------------------------------------------------------------------
// Clause body type checking
// ---------------------------------------------------------------------------

/// Walk all clause bodies in a source file, infer expression types, and
/// collect type errors. Lenient: errors involving `Unknown` are suppressed.
/// Create a copy of the type environment with `result` bound to the given type.
/// Register parameter types from an input clause body into the type environment.
///
/// Input clauses are expressions like `input(a: Int, b: String)` which parse
/// as `Call { func: Ident("input"), args: [...] }` or raw token sequences.
/// This extracts `(name, type)` pairs and inserts them as bindings.
///
/// Uses the shared `extract_clause_params` from assura-parser.
pub(crate) fn register_input_clause_params(body: &Expr, env: &mut TypeEnv) {
    use assura_parser::ast::extract_clause_params;
    for param in extract_clause_params(body) {
        if param.ty.is_empty() {
            if env.lookup(&param.name).is_none() {
                env.insert(param.name, Type::Unknown);
            }
        } else {
            let parsed = parse_type_tokens(&param.ty);
            env.insert(param.name, parsed);
        }
    }
}

/// Collect parameter types from an input clause body (types only, no env mutation).
///
/// Used by service operation/query type enrichment to build the parameter
/// type list for `Type::Fn`. Mirrors `register_input_clause_params` but
/// returns types instead of inserting into a `TypeEnv`.
///
/// Uses the shared `extract_clause_params` from assura-parser.
pub(crate) fn collect_input_param_types(body: &Expr, out: &mut Vec<Type>) {
    use assura_parser::ast::extract_clause_params;
    for param in extract_clause_params(body) {
        if param.ty.is_empty() {
            out.push(Type::Unknown);
        } else {
            out.push(parse_type_tokens(&param.ty));
        }
    }
}

/// Bind pattern variables into a type environment.
///
/// For `Ident` patterns, the variable is bound to the scrutinee type.
/// For `Constructor` patterns, nested fields get `Unknown` (we don't
/// know field types without full ADT info). For `Tuple` patterns, elements
/// get `Unknown`. Wildcards and literals don't bind variables.
pub(crate) fn bind_pattern_vars(
    pattern: &assura_parser::ast::Pattern,
    scrutinee_ty: &Type,
    env: &mut TypeEnv,
) {
    match pattern {
        assura_parser::ast::Pattern::Ident(name) => {
            // Bind the pattern variable to the scrutinee type
            env.insert(name.clone(), scrutinee_ty.clone());
        }
        assura_parser::ast::Pattern::Constructor { name, fields } => {
            // Look up the constructor in the environment.  Enum variant
            // constructors are registered as Fn { params, ret }, so we
            // can use the param types to type the sub-patterns.
            let param_types: Vec<Type> = match env.lookup(name) {
                Some(Type::Fn { params, .. }) => params.clone(),
                _ => Vec::new(),
            };
            for (i, field) in fields.iter().enumerate() {
                let field_ty = param_types.get(i).cloned().unwrap_or(Type::Unknown);
                bind_pattern_vars(field, &field_ty, env);
            }
        }
        assura_parser::ast::Pattern::Tuple(pats) => {
            if let Type::Tuple(elem_tys) = scrutinee_ty {
                for (i, pat) in pats.iter().enumerate() {
                    let elem_ty = elem_tys.get(i).cloned().unwrap_or(Type::Unknown);
                    bind_pattern_vars(pat, &elem_ty, env);
                }
            } else {
                for pat in pats {
                    bind_pattern_vars(pat, &Type::Unknown, env);
                }
            }
        }
        assura_parser::ast::Pattern::Wildcard | assura_parser::ast::Pattern::Literal(_) => {}
    }
}

pub(crate) fn env_with_result(env: &TypeEnv, result_ty: &Type) -> TypeEnv {
    let mut new_env = env.clone();
    new_env.insert("result".to_string(), result_ty.clone());
    new_env
}

/// Extract the output type from a contract's output clause.
///
/// Looks for the first `output` clause and infers the type of its body
/// expression. For `output(result: Int)`, the body is parsed as an
/// expression; we extract the type annotation from the Ident or the
/// clause body tokens. Falls back to `Unknown` if no output clause.
/// Extract a type annotation from an output clause body.
///
/// The output body can appear as:
/// - `Expr::Cast { expr: Ident("result"), ty: "Nat" }` (expression-parsed)
/// - `Expr::Raw(["result", ":", "Nat"])` (raw tokens from `output(result: Nat)`)
/// - `Expr::Call { args: [Cast { ... }] }` (wrapped call)
///
/// Returns the declared output type, or `Type::Unknown` if not extractable.
/// Treats `Type::Error` as "not found" for the purposes of extraction.
pub(crate) fn extract_output_type_from_body(body: &Expr) -> Type {
    match body {
        Expr::Cast { ty, .. } => parse_type_tokens(std::slice::from_ref(ty)),
        Expr::Raw(tokens) => {
            // Look for "name : Type" pattern
            if let Some(colon_pos) = tokens.iter().position(|t| t == ":") {
                let type_tokens: Vec<String> = tokens[colon_pos + 1..].to_vec();
                if !type_tokens.is_empty() {
                    let ty = parse_type_tokens(&type_tokens);
                    if !ty.is_indeterminate() {
                        return ty;
                    }
                }
            }
            Type::Unknown
        }
        Expr::Call { args, .. } => {
            // output(result: Int) parsed as Call with Cast args
            for arg in args {
                let ty = extract_output_type_from_body(arg);
                if !ty.is_indeterminate() {
                    return ty;
                }
            }
            Type::Unknown
        }
        _ => {
            // Fall back to inference
            let env = TypeEnv::new();
            if let Ok(ty) = infer_expr(body, &env) {
                ty
            } else {
                Type::Unknown
            }
        }
    }
}

pub(crate) fn extract_contract_output_type(c: &assura_parser::ast::ContractDecl) -> Type {
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Output {
            let ty = extract_output_type_from_body(&clause.body);
            if !ty.is_indeterminate() {
                return ty;
            }
        }
    }
    Type::Unknown
}

pub(crate) fn check_clause_bodies(
    source: &assura_parser::ast::SourceFile,
    env: &TypeEnv,
) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        let span = &decl.span;
        match &decl.node {
            Decl::Contract(c) => {
                // Extract the output type from the contract's output clause
                // to bind `result` in ensures clauses.
                let output_ty = extract_contract_output_type(c);
                // Build a contract-scoped env with all declared params
                let mut contract_env = env.clone();
                for clause in &c.clauses {
                    if clause.kind == ClauseKind::Requires
                        || clause.kind == ClauseKind::Input
                        || clause.kind == ClauseKind::Ensures
                    {
                        register_input_clause_params(&clause.body, &mut contract_env);
                    }
                }
                // Register inline fn params with their declared types
                for p in &c.fn_params {
                    if !p.ty.is_empty() {
                        contract_env.insert(p.name.clone(), parse_type_tokens(&p.ty));
                    }
                }
                let ensures_env = env_with_result(&contract_env, &output_ty);
                for clause in &c.clauses {
                    let clause_env = if clause.kind == ClauseKind::Ensures {
                        &ensures_env
                    } else {
                        &contract_env
                    };
                    check_clause_expr(&clause.kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            Decl::FnDef(f) => {
                // T043 CORE.1: ghost functions must have pure effects
                if f.is_ghost {
                    check_ghost_fn_effects(f, span, &mut errors);
                }
                // T044 CORE.2: lemma functions must have pure effects
                if f.is_lemma {
                    check_lemma_fn_effects(f, span, &mut errors);
                }
                // Build a scoped env with `result` bound to the return type
                // so ensures clauses can type-check `result` correctly.
                let ret_ty = if f.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&f.return_ty)
                };
                let fn_env = env_with_result(env, &ret_ty);
                for clause in &f.clauses {
                    let clause_env = if clause.kind == ClauseKind::Ensures {
                        &fn_env
                    } else {
                        env
                    };
                    check_clause_expr(&clause.kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            Decl::Extern(ex) => {
                let ret_ty = if ex.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&ex.return_ty)
                };
                let ext_env = env_with_result(env, &ret_ty);
                for clause in &ex.clauses {
                    let clause_env = if clause.kind == ClauseKind::Ensures {
                        &ext_env
                    } else {
                        env
                    };
                    check_clause_expr(&clause.kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            Decl::Bind(b) => {
                let ret_ty = if b.return_ty.is_empty() {
                    Type::Unit
                } else {
                    parse_type_tokens(&b.return_ty)
                };
                let bind_env = env_with_result(env, &ret_ty);
                for clause in &b.clauses {
                    let clause_env = if clause.kind == ClauseKind::Ensures {
                        &bind_env
                    } else {
                        env
                    };
                    check_clause_expr(&clause.kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            Decl::Service(s) => {
                // Build a service-scoped env with `self` bound to the service type
                let mut svc_env = env.clone();
                svc_env.insert("self".to_string(), Type::Named(s.name.clone()));

                for item in &s.items {
                    let clauses = match item {
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => clauses.as_slice(),
                        ServiceItem::Invariant(expr) => {
                            // Service-level invariants are always Bool-typed
                            check_clause_expr(
                                &ClauseKind::Invariant,
                                expr,
                                &svc_env,
                                &mut errors,
                                span,
                            );
                            continue;
                        }
                        ServiceItem::Other { body, .. } => {
                            collect_expr_errors(body, &svc_env, &mut errors, span);
                            continue;
                        }
                        _ => continue,
                    };

                    // Build operation-scoped env: register input clause params
                    // and bind `result` for ensures clauses
                    let mut op_env = svc_env.clone();
                    let mut output_ty = Type::Unit;
                    for clause in clauses {
                        if clause.kind == ClauseKind::Input {
                            register_input_clause_params(&clause.body, &mut op_env);
                        }
                        if clause.kind == ClauseKind::Output {
                            let ty = extract_output_type_from_body(&clause.body);
                            if !ty.is_indeterminate() {
                                output_ty = ty;
                            }
                        }
                    }
                    let ensures_env = env_with_result(&op_env, &output_ty);

                    for clause in clauses {
                        let clause_env = if clause.kind == ClauseKind::Ensures {
                            &ensures_env
                        } else {
                            &op_env
                        };
                        check_clause_expr(
                            &clause.kind,
                            &clause.body,
                            clause_env,
                            &mut errors,
                            span,
                        );
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    check_clause_expr(&clause.kind, &clause.body, env, &mut errors, span);
                }
            }
            // TypeDef, EnumDef, Prophecy, and CodecRegistry don't have direct expression bodies
            Decl::TypeDef(_) | Decl::EnumDef(_) | Decl::Prophecy(_) | Decl::CodecRegistry(_) => {}
        }
    }

    errors
}

/// Walk all clause bodies in an HIR file, converting clause bodies back to
/// AST `Expr` for inference. Uses structured `HirType` for return types
/// instead of raw token parsing.
pub(crate) fn check_clause_bodies_hir(hir: &assura_hir::HirFile, env: &TypeEnv) -> Vec<TypeError> {
    use assura_hir::{HirClauseKind, HirDeclKind, HirServiceItem as HirSI};

    let mut errors = Vec::new();
    let dummy_span = 0..0usize;

    for decl in &hir.decls {
        let span = &dummy_span;
        match &decl.kind {
            HirDeclKind::Contract(c) => {
                // Find output type from clauses
                let output_ty = hir_extract_contract_output_type(c);
                // Build a contract-scoped env with params from input/output
                // clauses AND inline fn definitions.
                let mut contract_env = env.clone();
                // Find the matching AST contract to get fn_params and input clauses
                if let Some(ast_contract) = find_ast_contract(hir, &c.name) {
                    for clause in &ast_contract.clauses {
                        if clause.kind == assura_parser::ast::ClauseKind::Input
                            || clause.kind == assura_parser::ast::ClauseKind::Output
                            || clause.kind == assura_parser::ast::ClauseKind::Requires
                            || clause.kind == assura_parser::ast::ClauseKind::Ensures
                        {
                            register_input_clause_params(&clause.body, &mut contract_env);
                        }
                    }
                    for p in &ast_contract.fn_params {
                        if !p.ty.is_empty() {
                            contract_env.insert(p.name.clone(), parse_type_tokens(&p.ty));
                        }
                    }
                }
                let ensures_env = env_with_result(&contract_env, &output_ty);
                for clause in &c.clauses {
                    let clause_env = if clause.kind == HirClauseKind::Ensures {
                        &ensures_env
                    } else {
                        &contract_env
                    };
                    let ast_kind = hir_clause_kind_to_ast(&clause.kind);
                    check_clause_hir_expr(&ast_kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            HirDeclKind::FnDef(f) => {
                // Ghost/lemma checks still use AST (these examine clause structure)
                if let Some(ast_fn) = find_ast_fn_def(hir, &f.name) {
                    if f.is_ghost {
                        check_ghost_fn_effects(ast_fn, span, &mut errors);
                    }
                    if f.is_lemma {
                        check_lemma_fn_effects(ast_fn, span, &mut errors);
                    }
                }
                let ret_ty = type_from_hir_type(&f.return_ty);
                let fn_env = env_with_result(env, &ret_ty);
                for clause in &f.clauses {
                    let clause_env = if clause.kind == HirClauseKind::Ensures {
                        &fn_env
                    } else {
                        env
                    };
                    let ast_kind = hir_clause_kind_to_ast(&clause.kind);
                    check_clause_hir_expr(&ast_kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            HirDeclKind::Extern(e) => {
                let ret_ty = type_from_hir_type(&e.return_ty);
                let ext_env = env_with_result(env, &ret_ty);
                for clause in &e.clauses {
                    let clause_env = if clause.kind == HirClauseKind::Ensures {
                        &ext_env
                    } else {
                        env
                    };
                    let ast_kind = hir_clause_kind_to_ast(&clause.kind);
                    check_clause_hir_expr(&ast_kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            HirDeclKind::Bind(b) => {
                let ret_ty = type_from_hir_type(&b.return_ty);
                let bind_env = env_with_result(env, &ret_ty);
                for clause in &b.clauses {
                    let clause_env = if clause.kind == HirClauseKind::Ensures {
                        &bind_env
                    } else {
                        env
                    };
                    let ast_kind = hir_clause_kind_to_ast(&clause.kind);
                    check_clause_hir_expr(&ast_kind, &clause.body, clause_env, &mut errors, span);
                }
            }
            HirDeclKind::Service(s) => {
                let mut svc_env = env.clone();
                svc_env.insert("self".to_string(), Type::Named(s.name.clone()));

                for item in &s.items {
                    let (clauses,) = match item {
                        HirSI::Operation { clauses, .. } | HirSI::Query { clauses, .. } => {
                            (clauses,)
                        }
                        HirSI::Invariant(expr) => {
                            check_clause_hir_expr(
                                &ClauseKind::Invariant,
                                expr,
                                &svc_env,
                                &mut errors,
                                span,
                            );
                            continue;
                        }
                        _ => continue,
                    };

                    let mut op_env = svc_env.clone();
                    let mut output_ty = Type::Unit;
                    for clause in clauses {
                        if clause.kind == HirClauseKind::Input {
                            let ast_clause = clause.to_ast_clause();
                            register_input_clause_params(&ast_clause.body, &mut op_env);
                        }
                        if clause.kind == HirClauseKind::Output {
                            let ast_clause = clause.to_ast_clause();
                            let ty = extract_output_type_from_body(&ast_clause.body);
                            if !ty.is_indeterminate() {
                                output_ty = ty;
                            }
                        }
                    }
                    let ensures_env = env_with_result(&op_env, &output_ty);

                    for clause in clauses {
                        let clause_env = if clause.kind == HirClauseKind::Ensures {
                            &ensures_env
                        } else {
                            &op_env
                        };
                        let ast_kind = hir_clause_kind_to_ast(&clause.kind);
                        check_clause_hir_expr(
                            &ast_kind,
                            &clause.body,
                            clause_env,
                            &mut errors,
                            span,
                        );
                    }
                }
            }
            HirDeclKind::Block(b) => {
                for clause in &b.clauses {
                    let ast_kind = hir_clause_kind_to_ast(&clause.kind);
                    check_clause_hir_expr(&ast_kind, &clause.body, env, &mut errors, span);
                }
            }
            // Prophecy, CodecRegistry don't have clause bodies to check here.
            HirDeclKind::Prophecy(_) | HirDeclKind::CodecRegistry(_) => {}
            HirDeclKind::TypeDef(_) | HirDeclKind::EnumDef(_) => {}
        }
    }

    errors
}

/// Convert HirClauseKind to parser ClauseKind.
fn hir_clause_kind_to_ast(kind: &assura_hir::HirClauseKind) -> ClauseKind {
    use assura_hir::HirClauseKind;
    match kind {
        HirClauseKind::Requires => ClauseKind::Requires,
        HirClauseKind::Ensures => ClauseKind::Ensures,
        HirClauseKind::Effects => ClauseKind::Effects,
        HirClauseKind::Invariant => ClauseKind::Invariant,
        HirClauseKind::Modifies => ClauseKind::Modifies,
        HirClauseKind::Input => ClauseKind::Input,
        HirClauseKind::Output => ClauseKind::Output,
        HirClauseKind::Errors => ClauseKind::Errors,
        HirClauseKind::Rule => ClauseKind::Rule,
        HirClauseKind::DataFlow => ClauseKind::DataFlow,
        HirClauseKind::MustNot => ClauseKind::MustNot,
        HirClauseKind::Decreases => ClauseKind::Decreases,
        HirClauseKind::Ordering => ClauseKind::Ordering,
        HirClauseKind::Other(s) => ClauseKind::Other(s.clone()),
    }
}

/// Extract the output type from a contract's HIR clauses.
fn hir_extract_contract_output_type(c: &assura_hir::HirContract) -> Type {
    for clause in &c.clauses {
        if clause.kind == assura_hir::HirClauseKind::Output {
            let ast_clause = clause.to_ast_clause();
            let ty = extract_output_type_from_body(&ast_clause.body);
            if !ty.is_indeterminate() {
                return ty;
            }
        }
    }
    Type::Unit
}

/// Find a parser FnDef by name in the resolved source file (for ghost/lemma checks).
fn find_ast_fn_def<'a>(
    hir: &'a assura_hir::HirFile,
    name: &str,
) -> Option<&'a assura_parser::ast::FnDef> {
    for decl in &hir.resolved().source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.name == name
        {
            return Some(f);
        }
    }
    None
}

fn find_ast_contract<'a>(
    hir: &'a assura_hir::HirFile,
    name: &str,
) -> Option<&'a assura_parser::ast::ContractDecl> {
    for decl in &hir.resolved().source.decls {
        if let Decl::Contract(c) = &decl.node
            && c.name == name
        {
            return Some(c);
        }
    }
    None
}

/// Try to infer the type of an expression; if a type error occurs, push
/// it into the collector. Uses `ctx_span` to replace placeholder `0..0`
/// spans with the declaration's actual source span.
fn collect_expr_errors(
    expr: &Expr,
    env: &TypeEnv,
    errors: &mut Vec<TypeError>,
    ctx_span: &std::ops::Range<usize>,
) {
    match infer_expr_spanned(expr, env, ctx_span.clone()) {
        Ok(_) => {}
        Err(e) => {
            errors.push(e);
        }
    }
}

/// Returns `true` if the clause kind requires a Bool-typed body.
fn clause_requires_bool(kind: &ClauseKind) -> bool {
    matches!(
        kind,
        ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant | ClauseKind::Rule
    )
}

/// Human-readable label for a clause kind (used in error messages).
fn clause_kind_label(kind: &ClauseKind) -> &'static str {
    match kind {
        ClauseKind::Requires => "requires",
        ClauseKind::Ensures => "ensures",
        ClauseKind::Invariant => "invariant",
        ClauseKind::Rule => "rule",
        _ => "clause",
    }
}

/// Check a single clause expression. Infer its type, push any inference
/// errors, and additionally emit A03006 if the clause kind demands Bool
/// but the body has a definitively non-Bool type.
pub(crate) fn check_clause_expr(
    kind: &ClauseKind,
    body: &Expr,
    env: &TypeEnv,
    errors: &mut Vec<TypeError>,
    ctx_span: &std::ops::Range<usize>,
) {
    match infer_expr_spanned(body, env, ctx_span.clone()) {
        Ok(ty) => {
            if clause_requires_bool(kind) && !ty.is_indeterminate() && ty != Type::Bool {
                errors.push(TypeError {
                    code: "A03006".into(),
                    message: format!(
                        "{} clause must be Bool, found `{ty}`",
                        clause_kind_label(kind),
                    ),
                    span: ctx_span.clone(),
                    secondary: None,
                });
            }
        }
        Err(e) => {
            errors.push(e);
        }
    }
}

/// HIR variant of `check_clause_expr` that accepts `HirExpr` directly,
/// eliminating the need for `to_ast_expr()` bridge conversions.
pub(crate) fn check_clause_hir_expr(
    kind: &ClauseKind,
    body: &assura_hir::HirExpr,
    env: &TypeEnv,
    errors: &mut Vec<TypeError>,
    ctx_span: &std::ops::Range<usize>,
) {
    match crate::infer_hir_expr_spanned(body, env, ctx_span.clone()) {
        Ok(ty) => {
            if clause_requires_bool(kind) && !ty.is_indeterminate() && ty != Type::Bool {
                errors.push(TypeError {
                    code: "A03006".into(),
                    message: format!(
                        "{} clause must be Bool, found `{ty}`",
                        clause_kind_label(kind),
                    ),
                    span: ctx_span.clone(),
                    secondary: None,
                });
            }
        }
        Err(e) => {
            errors.push(e);
        }
    }
}
