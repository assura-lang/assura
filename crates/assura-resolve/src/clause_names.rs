//! Expression-level name resolution in clause bodies.
//!
//! Walks clause bodies (requires, ensures, invariant, etc.) and checks
//! that `Expr::Ident` references resolve to a known name in scope.

use assura_parser::ast::{ClauseKind, Expr, ServiceItem, SourceFile, SpExpr, Span};

use crate::BUILTIN_VALUE_NAMES;
use crate::errors::ResolutionError;
use crate::imports::ResolvedImport;
use crate::symbols::SymbolTable;
use crate::type_refs::{
    TYPE_SYNTAX_TOKENS, find_scope_for, find_similar_name, is_type_name_candidate,
    should_be_lenient,
};

/// Walk all clause bodies (requires, ensures, invariant, etc.) and check
/// that `Expr::Ident` references resolve to a known name in scope.
///
/// This catches typos in contract bodies like `requires { c > 0 }` when the
/// input clause only declares `a` and `b`. In lenient mode (files with
/// imports/modules/projects), unknown names are skipped since they may
/// come from imported modules.
pub(crate) fn resolve_clause_body_names(
    source: &SourceFile,
    table: &SymbolTable,
    imports: &[ResolvedImport],
    module_scope: usize,
    errors: &mut Vec<ResolutionError>,
) {
    let lenient = should_be_lenient(source, imports);

    use assura_parser::ast::{
        BindDecl, BlockKind, Clause, ContractDecl, DeclVisitor, ExternDecl, FnDef, ServiceDecl,
    };

    struct ClauseBodyNames<'a> {
        table: &'a SymbolTable,
        imports_lenient: bool,
        module_scope: usize,
        errors: &'a mut Vec<ResolutionError>,
        decl_span: assura_parser::ast::Span,
    }

    impl ClauseBodyNames<'_> {
        fn check_clauses(
            &mut self,
            scope_name: &str,
            clauses: &[assura_parser::ast::Clause],
            span: &Span,
        ) {
            let scope = find_scope_for(self.table, scope_name, self.module_scope)
                .unwrap_or(self.module_scope);
            for clause in clauses {
                if is_body_clause(&clause.kind) {
                    check_expr_idents(
                        &clause.body,
                        self.table,
                        scope,
                        span,
                        self.imports_lenient,
                        &mut Vec::new(),
                        self.errors,
                    );
                }
            }
        }
    }

    impl DeclVisitor for ClauseBodyNames<'_> {
        fn visit_contract(&mut self, c: &ContractDecl) {
            let span = self.decl_span.clone();
            self.check_clauses(&c.name, &c.clauses, &span);
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            let span = self.decl_span.clone();
            self.check_clauses(&f.name, &f.clauses, &span);
        }
        fn visit_extern(&mut self, ex: &ExternDecl) {
            let span = self.decl_span.clone();
            self.check_clauses(&ex.name, &ex.clauses, &span);
        }
        fn visit_bind(&mut self, b: &BindDecl) {
            let span = self.decl_span.clone();
            self.check_clauses(&b.name, &b.clauses, &span);
        }
        fn visit_service(&mut self, s: &ServiceDecl) {
            let svc_scope =
                find_scope_for(self.table, &s.name, self.module_scope).unwrap_or(self.module_scope);
            for item in &s.items {
                match item {
                    ServiceItem::Operation { name, clauses, .. }
                    | ServiceItem::Query { name, clauses, .. } => {
                        let op_scope =
                            find_scope_for(self.table, name, svc_scope).unwrap_or(svc_scope);
                        for clause in clauses {
                            if is_body_clause(&clause.kind) {
                                check_expr_idents(
                                    &clause.body,
                                    self.table,
                                    op_scope,
                                    &Span::default(),
                                    self.imports_lenient,
                                    &mut Vec::new(),
                                    self.errors,
                                );
                            }
                        }
                    }
                    ServiceItem::Invariant(expr) => {
                        check_expr_idents(
                            expr,
                            self.table,
                            svc_scope,
                            &Span::default(),
                            self.imports_lenient,
                            &mut Vec::new(),
                            self.errors,
                        );
                    }
                    ServiceItem::Other { body, .. } => {
                        check_expr_idents(
                            body,
                            self.table,
                            svc_scope,
                            &Span::default(),
                            self.imports_lenient,
                            &mut Vec::new(),
                            self.errors,
                        );
                    }
                    ServiceItem::TypeDef(_) | ServiceItem::EnumDef(_) | ServiceItem::States(_) => {}
                }
            }
        }
        fn visit_block(
            &mut self,
            _kind: &BlockKind,
            _name: &str,
            _value: &Option<Vec<String>>,
            body: &[Clause],
        ) {
            let span = self.decl_span.clone();
            for clause in body {
                if is_body_clause(&clause.kind) {
                    check_expr_idents(
                        &clause.body,
                        self.table,
                        self.module_scope,
                        &span,
                        self.imports_lenient,
                        &mut Vec::new(),
                        self.errors,
                    );
                }
            }
        }
        // TypeDef, EnumDef, Prophecy, CodecRegistry: default no-op (no clause exprs here)
    }

    // walk_decls does not pass spans; walk manually to keep decl.span accuracy.
    for decl in &source.decls {
        let mut visitor = ClauseBodyNames {
            table,
            imports_lenient: lenient,
            module_scope,
            errors,
            decl_span: decl.span.clone(),
        };
        visitor.visit_decl(&decl.node);
    }
}

/// Returns `true` for clause kinds whose bodies contain expressions that
/// should be checked for name resolution (predicates, not declarations).
pub(crate) fn is_body_clause(kind: &ClauseKind) -> bool {
    matches!(
        kind,
        ClauseKind::Requires
            | ClauseKind::Ensures
            | ClauseKind::Invariant
            | ClauseKind::Modifies
            | ClauseKind::Decreases
    )
}

/// Recursively check `Expr::Ident` references in an expression tree.
///
/// The `locals` parameter tracks locally-bound names (quantifier variables,
/// let bindings) that are valid within their subtree.
fn check_expr_idents(
    expr: &SpExpr,
    table: &SymbolTable,
    scope_id: usize,
    span: &Span,
    lenient: bool,
    locals: &mut Vec<String>,
    errors: &mut Vec<ResolutionError>,
) {
    match &expr.node {
        Expr::Ident(name) => {
            // Skip if it resolves in the symbol table
            if table.lookup(name, scope_id).is_some() {
                return;
            }
            // Skip if it's a locally-bound variable (quantifier/let)
            if locals.contains(name) {
                return;
            }
            // Skip if it's a built-in value/function name
            if BUILTIN_VALUE_NAMES.contains(&name.as_str()) {
                return;
            }
            // Skip numeric-looking tokens
            if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                return;
            }
            // In lenient mode, skip all unknown names
            if lenient {
                return;
            }
            let suggestion = find_similar_name(name, table, scope_id);
            errors.push(ResolutionError {
                code: "A02001".into(),
                message: format!("undefined name `{name}` in clause body"),
                span: span.clone(),
                secondary: None,
                suggestion,
            });
        }
        Expr::Field(receiver, _field) => {
            // Only check the receiver; the field name is resolved structurally
            check_expr_idents(receiver, table, scope_id, span, lenient, locals, errors);
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_expr_idents(receiver, table, scope_id, span, lenient, locals, errors);
            for arg in args {
                check_expr_idents(arg, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Call { func, args } => {
            check_expr_idents(func, table, scope_id, span, lenient, locals, errors);
            for arg in args {
                check_expr_idents(arg, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Index { expr: base, index } => {
            check_expr_idents(base, table, scope_id, span, lenient, locals, errors);
            check_expr_idents(index, table, scope_id, span, lenient, locals, errors);
        }
        Expr::BinOp { lhs, rhs, .. } => {
            check_expr_idents(lhs, table, scope_id, span, lenient, locals, errors);
            check_expr_idents(rhs, table, scope_id, span, lenient, locals, errors);
        }
        Expr::UnaryOp { expr: inner, .. } | Expr::Old(inner) | Expr::Ghost(inner) => {
            check_expr_idents(inner, table, scope_id, span, lenient, locals, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            check_expr_idents(cond, table, scope_id, span, lenient, locals, errors);
            check_expr_idents(then_branch, table, scope_id, span, lenient, locals, errors);
            if let Some(e) = else_branch {
                check_expr_idents(e, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Forall {
            var, domain, body, ..
        }
        | Expr::Exists {
            var, domain, body, ..
        } => {
            check_expr_idents(domain, table, scope_id, span, lenient, locals, errors);
            locals.push(var.clone());
            check_expr_idents(body, table, scope_id, span, lenient, locals, errors);
            locals.pop();
        }
        Expr::Let { name, value, body } => {
            check_expr_idents(value, table, scope_id, span, lenient, locals, errors);
            locals.push(name.clone());
            check_expr_idents(body, table, scope_id, span, lenient, locals, errors);
            locals.pop();
        }
        Expr::Match { scrutinee, arms } => {
            check_expr_idents(scrutinee, table, scope_id, span, lenient, locals, errors);
            for arm in arms {
                let mut arm_locals = locals.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_locals);
                check_expr_idents(
                    &arm.body,
                    table,
                    scope_id,
                    span,
                    lenient,
                    &mut arm_locals,
                    errors,
                );
            }
        }
        Expr::Apply { lemma_name, args } => {
            // The lemma name should resolve as a function/declaration
            if table.lookup(lemma_name, scope_id).is_none()
                && !locals.contains(lemma_name)
                && !BUILTIN_VALUE_NAMES.contains(&lemma_name.as_str())
                && !lenient
            {
                let suggestion = find_similar_name(lemma_name, table, scope_id);
                errors.push(ResolutionError {
                    code: "A02001".into(),
                    message: format!("undefined lemma `{lemma_name}`"),
                    span: span.clone(),
                    secondary: None,
                    suggestion,
                });
            }
            for arg in args {
                check_expr_idents(arg, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Cast { expr: inner, .. } => {
            check_expr_idents(inner, table, scope_id, span, lenient, locals, errors);
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                check_expr_idents(item, table, scope_id, span, lenient, locals, errors);
            }
        }
        Expr::Raw(tokens) => {
            // For raw tokens, check identifiers that look like value references
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && table.lookup(tok, scope_id).is_none()
                    && !locals.contains(tok)
                    && !BUILTIN_VALUE_NAMES.contains(&tok.as_str())
                    && !TYPE_SYNTAX_TOKENS.contains(&tok.as_str())
                    && !is_type_name_candidate(tok)
                    && !lenient
                {
                    let suggestion = find_similar_name(tok, table, scope_id);
                    errors.push(ResolutionError {
                        code: "A02001".into(),
                        message: format!("undefined name `{tok}` in clause body"),
                        span: span.clone(),
                        secondary: None,
                        suggestion,
                    });
                }
            }
        }
        Expr::Literal(_) => {}
    }
}

/// Collect names bound by a pattern (for match arm local scope).
pub(crate) fn collect_pattern_bindings(
    pattern: &assura_parser::ast::Pattern,
    locals: &mut Vec<String>,
) {
    use assura_parser::ast::Pattern;
    match pattern {
        Pattern::Ident(name) if name != "_" => {
            locals.push(name.clone());
        }
        Pattern::Constructor { fields, .. } => {
            for f in fields {
                collect_pattern_bindings(f, locals);
            }
        }
        Pattern::Tuple(pats) => {
            for p in pats {
                collect_pattern_bindings(p, locals);
            }
        }
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::Ident(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Pattern;

    #[test]
    fn is_body_clause_requires() {
        assert!(is_body_clause(&ClauseKind::Requires));
    }

    #[test]
    fn is_body_clause_ensures() {
        assert!(is_body_clause(&ClauseKind::Ensures));
    }

    #[test]
    fn is_body_clause_invariant() {
        assert!(is_body_clause(&ClauseKind::Invariant));
    }

    #[test]
    fn is_body_clause_non_body() {
        assert!(!is_body_clause(&ClauseKind::Input));
        assert!(!is_body_clause(&ClauseKind::Output));
        assert!(!is_body_clause(&ClauseKind::Effects));
    }

    #[test]
    fn collect_pattern_bindings_ident() {
        let mut locals = Vec::new();
        collect_pattern_bindings(&Pattern::Ident("x".into()), &mut locals);
        assert_eq!(locals, vec!["x"]);
    }

    #[test]
    fn collect_pattern_bindings_wildcard() {
        let mut locals = Vec::new();
        collect_pattern_bindings(&Pattern::Wildcard, &mut locals);
        assert!(locals.is_empty());
    }

    #[test]
    fn collect_pattern_bindings_underscore_ident() {
        let mut locals = Vec::new();
        collect_pattern_bindings(&Pattern::Ident("_".into()), &mut locals);
        assert!(locals.is_empty(), "underscore should not bind");
    }

    #[test]
    fn collect_pattern_bindings_constructor() {
        let mut locals = Vec::new();
        let pat = Pattern::Constructor {
            name: "Some".into(),
            fields: vec![Pattern::Ident("val".into())],
        };
        collect_pattern_bindings(&pat, &mut locals);
        assert_eq!(locals, vec!["val"]);
    }

    #[test]
    fn collect_pattern_bindings_tuple() {
        let mut locals = Vec::new();
        let pat = Pattern::Tuple(vec![Pattern::Ident("a".into()), Pattern::Ident("b".into())]);
        collect_pattern_bindings(&pat, &mut locals);
        assert_eq!(locals, vec!["a", "b"]);
    }

    #[test]
    fn resolve_clause_body_accepts_params() {
        // A contract referencing its own input param should resolve fine
        let src = "contract C { input(a: Int) requires { a > 0 } }";
        let source = assura_parser::parse_unwrap(src);
        let result = crate::resolve(&source);
        assert!(
            result.is_ok(),
            "expected resolution to succeed, got: {result:?}"
        );
    }

    #[test]
    fn resolve_fn_clause_body_accepts_params() {
        let src = "fn f(x: Int) -> Int { requires { x > 0 } }";
        let source = assura_parser::parse_unwrap(src);
        let result = crate::resolve(&source);
        assert!(
            result.is_ok(),
            "expected resolution to succeed, got: {result:?}"
        );
    }
}
