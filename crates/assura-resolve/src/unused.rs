//! Unused import detection (A02007).

use std::collections::HashSet;

use assura_parser::ast::{Decl, Expr, FnDef, ServiceItem, SourceFile, TypeBody};

use crate::errors::ResolutionError;
use crate::imports::{ImportStatus, ResolvedImport};
use crate::type_refs::TYPE_SYNTAX_TOKENS;

/// Collect all identifier-like names referenced in the AST. This includes
/// type annotations, expression identifiers, and field/param type tokens.
pub(crate) fn collect_referenced_names(source: &SourceFile) -> HashSet<String> {
    let mut names = HashSet::new();
    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(t) => {
                collect_type_body_names(&t.body, &mut names);
            }
            Decl::FnDef(f) => {
                collect_fn_names(f, &mut names);
            }
            Decl::Extern(ex) => {
                for p in &ex.params {
                    collect_type_token_names(&p.ty, &mut names);
                }
                collect_type_token_names(&ex.return_ty, &mut names);
                for clause in &ex.clauses {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
            Decl::Bind(b) => {
                for p in &b.params {
                    collect_type_token_names(&p.ty, &mut names);
                }
                collect_type_token_names(&b.return_ty, &mut names);
                for clause in &b.clauses {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
            Decl::Contract(c) => {
                for clause in &c.clauses {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
            Decl::Service(s) => {
                for item in &s.items {
                    match item {
                        ServiceItem::TypeDef(t) => collect_type_body_names(&t.body, &mut names),
                        ServiceItem::EnumDef(e) => {
                            for v in &e.variants {
                                for f in &v.fields {
                                    names.insert(f.clone());
                                }
                            }
                        }
                        ServiceItem::Operation { clauses, .. }
                        | ServiceItem::Query { clauses, .. } => {
                            for clause in clauses {
                                collect_expr_names(&clause.body, &mut names);
                            }
                        }
                        ServiceItem::Invariant(expr) => collect_expr_names(expr, &mut names),
                        ServiceItem::Other { body, .. } => collect_expr_names(body, &mut names),
                        // States don't contribute expression names.
                        ServiceItem::States(_) => {}
                    }
                }
            }
            Decl::EnumDef(e) => {
                for v in &e.variants {
                    for f in &v.fields {
                        names.insert(f.clone());
                    }
                }
            }
            Decl::Prophecy(p) => {
                // Prophecy type tokens may reference user-defined types
                for tok in &p.ty_tokens {
                    if tok.chars().next().is_some_and(|c| c.is_uppercase()) {
                        names.insert(tok.clone());
                    }
                }
            }
            Decl::CodecRegistry(cr) => {
                // Output type tokens may reference user-defined types
                for tok in &cr.output_type {
                    if tok.chars().next().is_some_and(|c| c.is_uppercase()) {
                        names.insert(tok.clone());
                    }
                }
                for codec in &cr.codecs {
                    for clause in &codec.contracts {
                        collect_expr_names(&clause.body, &mut names);
                    }
                }
            }
            Decl::Block { body, .. } => {
                for clause in body {
                    collect_expr_names(&clause.body, &mut names);
                }
            }
        }
    }
    names
}

fn collect_type_body_names(body: &TypeBody, names: &mut HashSet<String>) {
    match body {
        TypeBody::Struct(fields) => {
            for f in fields {
                collect_type_token_names(&f.ty, names);
            }
        }
        TypeBody::Alias(tokens) | TypeBody::Refined(tokens) => {
            collect_type_token_names(tokens, names);
        }
        TypeBody::Empty => {}
    }
}

fn collect_fn_names(f: &FnDef, names: &mut HashSet<String>) {
    for p in &f.params {
        collect_type_token_names(&p.ty, names);
    }
    collect_type_token_names(&f.return_ty, names);
    for clause in &f.clauses {
        collect_expr_names(&clause.body, names);
    }
}

fn collect_type_token_names(tokens: &[String], names: &mut HashSet<String>) {
    for tok in tokens {
        if !TYPE_SYNTAX_TOKENS.contains(&tok.as_str())
            && !tok.starts_with(|c: char| c.is_ascii_digit())
        {
            names.insert(tok.clone());
        }
    }
}

fn collect_expr_names(expr: &Expr, names: &mut HashSet<String>) {
    match expr {
        Expr::Ident(name) => {
            names.insert(name.clone());
        }
        Expr::Field(receiver, field) => {
            collect_expr_names(receiver, names);
            names.insert(field.clone());
        }
        Expr::BinOp { lhs, rhs, .. } => {
            collect_expr_names(lhs, names);
            collect_expr_names(rhs, names);
        }
        Expr::UnaryOp { expr: inner, .. }
        | Expr::Paren(inner)
        | Expr::Old(inner)
        | Expr::Ghost(inner) => {
            collect_expr_names(inner, names);
        }
        Expr::Call { func, args } => {
            collect_expr_names(func, names);
            for arg in args {
                collect_expr_names(arg, names);
            }
        }
        Expr::MethodCall {
            receiver,
            args,
            method,
        } => {
            collect_expr_names(receiver, names);
            names.insert(method.clone());
            for arg in args {
                collect_expr_names(arg, names);
            }
        }
        Expr::Index { expr: base, index } => {
            collect_expr_names(base, names);
            collect_expr_names(index, names);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            collect_expr_names(cond, names);
            collect_expr_names(then_branch, names);
            if let Some(e) = else_branch {
                collect_expr_names(e, names);
            }
        }
        Expr::Forall {
            var, domain, body, ..
        }
        | Expr::Exists {
            var, domain, body, ..
        } => {
            names.insert(var.clone());
            collect_expr_names(domain, names);
            collect_expr_names(body, names);
        }
        Expr::List(items) | Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                collect_expr_names(item, names);
            }
        }
        Expr::Cast { expr: inner, ty } => {
            collect_expr_names(inner, names);
            names.insert(ty.clone());
        }
        Expr::Apply { lemma_name, args } => {
            names.insert(lemma_name.clone());
            for arg in args {
                collect_expr_names(arg, names);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_expr_names(scrutinee, names);
            for arm in arms {
                collect_expr_names(&arm.body, names);
            }
        }
        Expr::Let { name, value, body } => {
            names.insert(name.clone());
            collect_expr_names(value, names);
            collect_expr_names(body, names);
        }
        Expr::Raw(tokens) => {
            for tok in tokens {
                if tok
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphabetic() || c == '_')
                {
                    names.insert(tok.clone());
                }
            }
        }
        Expr::Literal(_) => {}
    }
}

/// Check which imports introduced names that are never referenced in the AST.
///
/// An import is "unused" when none of the names it introduces appear in
/// the set of referenced names collected from the AST.
pub(crate) fn check_unused_imports(
    imports: &[ResolvedImport],
    referenced: &HashSet<String>,
    errors: &mut Vec<ResolutionError>,
) {
    for imp in imports {
        if imp.status == ImportStatus::Circular {
            continue;
        }
        let introduced: Vec<&str> = if !imp.items.is_empty() {
            imp.items.iter().map(|s| s.as_str()).collect()
        } else if let Some(alias) = &imp.alias {
            vec![alias.as_str()]
        } else if let Some(last) = imp.path.last() {
            vec![last.as_str()]
        } else {
            continue;
        };

        // An import is unused if none of its introduced names appear in references
        if introduced.iter().all(|name| !referenced.contains(*name)) {
            let path_str = imp.path.join(".");
            errors.push(ResolutionError {
                code: "A02007".into(),
                message: format!("unused import `{path_str}`"),
                span: imp.span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::imports::{ImportStatus, ResolvedImport};

    fn make_import(path: &[&str], items: &[&str]) -> ResolvedImport {
        ResolvedImport {
            path: path.iter().map(|s| s.to_string()).collect(),
            items: items.iter().map(|s| s.to_string()).collect(),
            alias: None,
            status: ImportStatus::Unresolved,
            span: 0..1,
        }
    }

    #[test]
    fn collect_referenced_names_empty() {
        let source = assura_parser::parse_unwrap("");
        let names = collect_referenced_names(&source);
        assert!(names.is_empty());
    }

    #[test]
    fn collect_referenced_names_contract() {
        let source = assura_parser::parse_unwrap("contract C { input(n: Nat) requires { n > 0 } }");
        let names = collect_referenced_names(&source);
        assert!(names.contains("n"));
    }

    #[test]
    fn collect_referenced_names_fn_types_and_body() {
        let source = assura_parser::parse_unwrap("fn f(x: Int) -> Bool { ensures { x > 0 } }");
        let names = collect_referenced_names(&source);
        // Type tokens from params/return are collected
        assert!(names.contains("Int"));
        assert!(names.contains("Bool"));
    }

    #[test]
    fn check_unused_imports_used_name() {
        let mut referenced = HashSet::new();
        referenced.insert("Map".to_string());
        let imports = vec![make_import(&["std", "collections"], &["Map"])];
        let mut errors = Vec::new();
        check_unused_imports(&imports, &referenced, &mut errors);
        assert!(errors.is_empty());
    }

    #[test]
    fn check_unused_imports_unused_name() {
        let referenced = HashSet::new();
        let imports = vec![make_import(&["std", "math"], &[])];
        let mut errors = Vec::new();
        check_unused_imports(&imports, &referenced, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "A02007");
        assert!(errors[0].message.contains("std.math"));
    }

    #[test]
    fn check_unused_imports_circular_skipped() {
        let referenced = HashSet::new();
        let imports = vec![ResolvedImport {
            path: vec!["self_ref".into()],
            items: vec![],
            alias: None,
            status: ImportStatus::Circular,
            span: 0..1,
        }];
        let mut errors = Vec::new();
        check_unused_imports(&imports, &referenced, &mut errors);
        assert!(errors.is_empty(), "circular imports should be skipped");
    }

    #[test]
    fn check_unused_imports_alias_used() {
        let mut referenced = HashSet::new();
        referenced.insert("M".to_string());
        let imports = vec![ResolvedImport {
            path: vec!["std".into(), "math".into()],
            items: vec![],
            alias: Some("M".into()),
            status: ImportStatus::Unresolved,
            span: 0..1,
        }];
        let mut errors = Vec::new();
        check_unused_imports(&imports, &referenced, &mut errors);
        assert!(errors.is_empty());
    }
}
