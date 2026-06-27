//! Verification clause quality warnings.
//!
//! Warns about ensures clauses that reference unconstrained outputs (#617)
//! and about feature_max constants used in verification clauses (#619).

use assura_parser::ast::{ClauseKind, Decl, Expr, SpExpr};

use crate::TypeError;

// ---------------------------------------------------------------------------
// #617: Unconstrained output warning
// ---------------------------------------------------------------------------

/// Collect all identifier names from an expression tree.
fn collect_idents(expr: &SpExpr, out: &mut Vec<(String, std::ops::Range<usize>)>) {
    match &expr.node {
        Expr::Ident(name) => {
            out.push((name.clone(), expr.span.clone()));
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_idents(lhs, out);
            collect_idents(rhs, out);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Cast { expr: e, .. }
        | Expr::Ghost(e) => {
            collect_idents(e, out);
        }
        Expr::Call { func, args } => {
            collect_idents(func, out);
            for a in args {
                collect_idents(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_idents(receiver, out);
            for a in args {
                collect_idents(a, out);
            }
        }
        Expr::Field(recv, _) => {
            collect_idents(recv, out);
        }
        Expr::Index { expr: e, index } => {
            collect_idents(e, out);
            collect_idents(index, out);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_idents(cond, out);
            collect_idents(then_branch, out);
            if let Some(eb) = else_branch {
                collect_idents(eb, out);
            }
        }
        Expr::Forall { body, .. } | Expr::Exists { body, .. } => {
            collect_idents(body, out);
        }
        Expr::Let { value, body, .. } => {
            collect_idents(value, out);
            collect_idents(body, out);
        }
        Expr::Match { scrutinee, arms } => {
            collect_idents(scrutinee, out);
            for arm in arms {
                collect_idents(&arm.body, out);
            }
        }
        Expr::Tuple(items) | Expr::List(items) | Expr::Block(items) => {
            for item in items {
                collect_idents(item, out);
            }
        }
        Expr::Raw(tokens) => {
            for tok in tokens {
                if !tok.is_empty()
                    && tok
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    out.push((tok.clone(), expr.span.clone()));
                }
            }
        }
        Expr::Literal(_) | Expr::Apply { .. } => {}
    }
}

/// Check if an expression is the `result` identifier.
fn is_result_ident(expr: &SpExpr) -> bool {
    matches!(&expr.node, Expr::Ident(n) if n == "result")
}

/// Check if an expression references `result` as a method call receiver
/// with `.length()`. This is safe because the SMT encoder adds a background
/// axiom for `length >= 0`.
fn is_result_length_pattern(expr: &SpExpr) -> bool {
    matches!(
        &expr.node,
        Expr::MethodCall { receiver, method, args }
        if method == "length"
            && args.is_empty()
            && is_result_ident(receiver)
    )
}

/// Check if the entire ensures clause is a comparison involving `result.length()`.
/// E.g. `result.length() >= 0`.
fn is_result_length_comparison(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::BinOp { lhs, rhs, .. } => {
            is_result_length_pattern(lhs) || is_result_length_pattern(rhs)
        }
        _ => is_result_length_pattern(expr),
    }
}

/// Warn when `ensures` clauses reference `result` or output variables
/// that are unconstrained free variables in SMT.
pub(crate) fn run_unconstrained_output_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut warnings = Vec::new();

    for decl in &source.decls {
        // Collect input parameter names
        let (name, clauses, params) = match &decl.node {
            Decl::Contract(c) => (
                c.name.as_str(),
                c.clauses.as_slice(),
                c.fn_params.as_slice(),
            ),
            Decl::FnDef(f) => (f.name.as_str(), f.clauses.as_slice(), f.params.as_slice()),
            Decl::Extern(e) => (e.name.as_str(), e.clauses.as_slice(), e.params.as_slice()),
            _ => continue,
        };

        let is_extern = matches!(&decl.node, Decl::Extern(_));

        // Build input set: function params + input clause params
        let mut input_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for p in params {
            input_names.insert(p.name.clone());
        }
        for clause in clauses {
            if clause.kind == ClauseKind::Input {
                // Input clause body may contain param-like expressions
                let mut idents = Vec::new();
                collect_idents(&clause.body, &mut idents);
                for (id, _) in &idents {
                    input_names.insert(id.clone());
                }
            }
        }

        // Check ensures clauses
        for clause in clauses {
            if clause.kind != ClauseKind::Ensures {
                continue;
            }

            // Exception: result.length() >= 0 pattern (background axiom)
            if is_extern && is_result_length_comparison(&clause.body) {
                continue;
            }

            // Check for `result` keyword
            if expr_references_result(&clause.body) {
                warnings.push(TypeError {
                    code: "A04008".into(),
                    message: format!(
                        "`{name}`: ensures clause references `result` which is \
                         unconstrained in SMT; Z3 can assign it any value"
                    ),
                    span: clause.body.span.clone(),
                    secondary: None,
                });
            }

            // Check for output params referenced in ensures
            let mut idents = Vec::new();
            collect_idents(&clause.body, &mut idents);
            for (id, span) in &idents {
                if id != "result" && !input_names.contains(id) && is_output_param(id, clauses) {
                    warnings.push(TypeError {
                        code: "A04008".into(),
                        message: format!(
                            "`{name}`: ensures clause references output parameter `{id}` \
                             which is unconstrained in SMT"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }
        }
    }

    warnings
}

/// Check if `result` appears in an expression.
fn expr_references_result(expr: &SpExpr) -> bool {
    match &expr.node {
        Expr::Ident(name) => name == "result",
        Expr::BinOp { lhs, rhs, .. } => expr_references_result(lhs) || expr_references_result(rhs),
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Cast { expr: e, .. }
        | Expr::Ghost(e) => expr_references_result(e),
        Expr::Call { func, args } => {
            expr_references_result(func) || args.iter().any(expr_references_result)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_references_result(receiver) || args.iter().any(expr_references_result)
        }
        Expr::Field(recv, _) => expr_references_result(recv),
        Expr::Index { expr: e, index } => {
            expr_references_result(e) || expr_references_result(index)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            expr_references_result(cond)
                || expr_references_result(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|e| expr_references_result(e))
        }
        Expr::Forall { body, .. } | Expr::Exists { body, .. } => expr_references_result(body),
        Expr::Let { value, body, .. } => {
            expr_references_result(value) || expr_references_result(body)
        }
        Expr::Match { scrutinee, arms } => {
            expr_references_result(scrutinee)
                || arms.iter().any(|a| expr_references_result(&a.body))
        }
        Expr::Tuple(items) | Expr::List(items) | Expr::Block(items) => {
            items.iter().any(expr_references_result)
        }
        Expr::Raw(tokens) => tokens.iter().any(|t| t == "result"),
        Expr::Literal(_) | Expr::Apply { .. } => false,
    }
}

/// Check if an identifier is declared as an output parameter.
fn is_output_param(name: &str, clauses: &[assura_parser::ast::Clause]) -> bool {
    for clause in clauses {
        if clause.kind == ClauseKind::Output {
            let mut idents = Vec::new();
            collect_idents(&clause.body, &mut idents);
            if idents.iter().any(|(id, _)| id == name) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// #619: feature_max in verification clause warning
// ---------------------------------------------------------------------------

/// Warn when `feature_max` constants are used in verification clauses.
/// The SMT encoder treats them as unconstrained integer variables,
/// not their defined values.
pub(crate) fn run_feature_max_in_clause_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    // Collect all feature_max constant names
    let mut feature_max_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for decl in &source.decls {
        if let Decl::Block {
            kind: assura_parser::ast::BlockKind::FeatureMax,
            name,
            ..
        } = &decl.node
        {
            feature_max_names.insert(name.clone());
        }
    }

    if feature_max_names.is_empty() {
        return Vec::new();
    }

    let mut warnings = Vec::new();

    for decl in &source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (c.name.as_str(), c.clauses.as_slice()),
            Decl::FnDef(f) => (f.name.as_str(), f.clauses.as_slice()),
            Decl::Extern(e) => (e.name.as_str(), e.clauses.as_slice()),
            _ => continue,
        };

        for clause in clauses {
            // Only check verification-relevant clauses
            if !matches!(
                clause.kind,
                ClauseKind::Requires | ClauseKind::Ensures | ClauseKind::Invariant
            ) {
                continue;
            }

            let mut idents = Vec::new();
            collect_idents(&clause.body, &mut idents);

            for (id, span) in &idents {
                if feature_max_names.contains(id) {
                    let kind_str = match clause.kind {
                        ClauseKind::Requires => "requires",
                        ClauseKind::Ensures => "ensures",
                        ClauseKind::Invariant => "invariant",
                        _ => "clause",
                    };
                    warnings.push(TypeError {
                        code: "A04009".into(),
                        message: format!(
                            "`{name}`: feature_max constant `{id}` used in {kind_str} clause; \
                             SMT treats this as unconstrained (inline the value instead)"
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::*;

    fn make_ensures_clause(body: SpExpr) -> Clause {
        Clause {
            kind: ClauseKind::Ensures,
            body,
            effect_variables: vec![],
        }
    }

    fn make_requires_clause(body: SpExpr) -> Clause {
        Clause {
            kind: ClauseKind::Requires,
            body,
            effect_variables: vec![],
        }
    }

    fn ident(name: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(name.into()))
    }

    fn result_expr() -> SpExpr {
        Spanned::no_span(Expr::Ident("result".into()))
    }

    fn binop(lhs: SpExpr, op: BinOp, rhs: SpExpr) -> SpExpr {
        Spanned::no_span(Expr::BinOp {
            lhs: Box::new(lhs),
            op,
            rhs: Box::new(rhs),
        })
    }

    fn int_lit(n: &str) -> SpExpr {
        Spanned::no_span(Expr::Literal(Literal::Int(n.into())))
    }

    fn method_call(receiver: SpExpr, method: &str, args: Vec<SpExpr>) -> SpExpr {
        Spanned::no_span(Expr::MethodCall {
            receiver: Box::new(receiver),
            method: method.into(),
            args,
        })
    }

    // --- unconstrained output tests ---

    #[test]
    fn warns_on_result_in_ensures() {
        let source = SourceFile {
            decls: vec![Spanned::no_span(Decl::Contract(ContractDecl {
                name: "SafeAdd".into(),
                fn_params: vec![Param {
                    name: "x".into(),
                    ty: None,
                }],
                clauses: vec![
                    make_requires_clause(binop(ident("x"), BinOp::Gte, int_lit("0"))),
                    make_ensures_clause(binop(result_expr(), BinOp::Gte, int_lit("0"))),
                ],
                type_params: vec![],
            }))],
            project: None,
            module: None,
            imports: vec![],
        };
        let warnings = run_unconstrained_output_checks(&source);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].code == "A04008");
        assert!(warnings[0].message.contains("result"));
    }

    #[test]
    fn no_warning_on_input_only_ensures() {
        let source = SourceFile {
            decls: vec![Spanned::no_span(Decl::Contract(ContractDecl {
                name: "InputOnly".into(),
                fn_params: vec![Param {
                    name: "x".into(),
                    ty: None,
                }],
                clauses: vec![
                    make_requires_clause(binop(ident("x"), BinOp::Gte, int_lit("0"))),
                    make_ensures_clause(binop(ident("x"), BinOp::Gte, int_lit("0"))),
                ],
                type_params: vec![],
            }))],
            project: None,
            module: None,
            imports: vec![],
        };
        let warnings = run_unconstrained_output_checks(&source);
        assert!(warnings.is_empty());
    }

    #[test]
    fn no_warning_for_extern_result_length() {
        let source = SourceFile {
            decls: vec![Spanned::no_span(Decl::Extern(ExternDecl {
                name: "read_data".into(),
                params: vec![],
                clauses: vec![make_ensures_clause(binop(
                    method_call(result_expr(), "length", vec![]),
                    BinOp::Gte,
                    int_lit("0"),
                ))],
                return_ty: None,
            }))],
            project: None,
            module: None,
            imports: vec![],
        };
        let warnings = run_unconstrained_output_checks(&source);
        assert!(warnings.is_empty());
    }

    // --- feature_max in clause tests ---

    #[test]
    fn warns_on_feature_max_in_requires() {
        let source = SourceFile {
            decls: vec![
                Spanned::no_span(Decl::Block {
                    kind: BlockKind::FeatureMax,
                    name: "HEADER_SIZE".into(),
                    value: Some(vec!["3".into()]),
                    body: vec![],
                }),
                Spanned::no_span(Decl::Contract(ContractDecl {
                    name: "CheckLen".into(),
                    fn_params: vec![Param {
                        name: "data".into(),
                        ty: None,
                    }],
                    clauses: vec![make_requires_clause(binop(
                        ident("data"),
                        BinOp::Gte,
                        ident("HEADER_SIZE"),
                    ))],
                    type_params: vec![],
                })),
            ],
            project: None,
            module: None,
            imports: vec![],
        };
        let warnings = run_feature_max_in_clause_checks(&source);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].code == "A04009");
        assert!(warnings[0].message.contains("HEADER_SIZE"));
    }

    #[test]
    fn no_warning_when_no_feature_max() {
        let source = SourceFile {
            decls: vec![Spanned::no_span(Decl::Contract(ContractDecl {
                name: "NoMax".into(),
                fn_params: vec![Param {
                    name: "x".into(),
                    ty: None,
                }],
                clauses: vec![make_requires_clause(binop(
                    ident("x"),
                    BinOp::Gte,
                    int_lit("0"),
                ))],
                type_params: vec![],
            }))],
            project: None,
            module: None,
            imports: vec![],
        };
        let warnings = run_feature_max_in_clause_checks(&source);
        assert!(warnings.is_empty());
    }
}
