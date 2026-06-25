use super::*;

// Frame condition checking (T045 - CORE.3)
// ---------------------------------------------------------------------------

/// Extract the set of variable/field names from a `modifies` clause body.
///
/// The modifies clause body is typically:
/// - `Expr::Ident("x")` for a single variable
/// - `Expr::Block([Expr::Ident("x"), Expr::Ident("y")])` for multiple
/// - `Expr::Field(Expr::Ident("obj"), "field")` for `obj.field`
/// - `Expr::List([...])` for comma-separated list
///
/// Returns a set of string representations (e.g., `"x"`, `"node.keys"`).
pub(crate) fn extract_modifies_targets(expr: &SpExpr) -> Vec<String> {
    let mut targets = Vec::new();
    collect_modifies_targets(expr, &mut targets);
    targets
}

/// Recursively collect modifies targets from an expression.
fn collect_modifies_targets(expr: &SpExpr, targets: &mut Vec<String>) {
    match &expr.node {
        Expr::Ident(name) => {
            targets.push(name.clone());
        }
        Expr::Field(receiver, field) => {
            // Build dotted path: "obj.field"
            let mut path = String::new();
            build_field_path(receiver, &mut path);
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            targets.push(path);
        }
        Expr::Block(exprs) => {
            for e in exprs {
                collect_modifies_targets(e, targets);
            }
        }
        Expr::List(items) => {
            for item in items {
                collect_modifies_targets(item, targets);
            }
        }
        Expr::Raw(tokens) => {
            // Parse comma-separated identifiers from raw tokens
            for tok in tokens {
                let trimmed = tok.trim();
                if !trimmed.is_empty() && trimmed != "," {
                    targets.push(trimmed.to_string());
                }
            }
        }
        // Other expression types are not valid modifies targets
        _ => {}
    }
}

/// Build a dotted field path from nested Field expressions.
fn build_field_path(expr: &SpExpr, path: &mut String) {
    match &expr.node {
        Expr::Ident(name) => {
            path.push_str(name);
        }
        Expr::Field(receiver, field) => {
            build_field_path(receiver, path);
            path.push('.');
            path.push_str(field);
        }
        _ => {}
    }
}

/// Collect all variable names referenced via `old(expr)` in an expression.
///
/// Walks the expression tree and whenever it finds `Expr::Old(inner)`,
/// extracts the variable/field name from `inner`. This is used to find
/// which pre-state variables an `ensures` clause references.
pub(crate) fn collect_old_references(expr: &SpExpr) -> Vec<String> {
    struct OldRefCollector(Vec<String>);
    impl ExprVisitor for OldRefCollector {
        fn visit_old(&mut self, inner: &SpExpr) {
            match &inner.node {
                Expr::Ident(name) => self.0.push(name.clone()),
                Expr::Field(receiver, field) => {
                    let mut path = String::new();
                    build_field_path(receiver, &mut path);
                    if !path.is_empty() {
                        path.push('.');
                    }
                    path.push_str(field);
                    self.0.push(path);
                }
                _ => {}
            }
            // Also recurse into the inner expression
            self.visit_expr(inner);
        }
    }
    let mut c = OldRefCollector(Vec::new());
    c.visit_expr(expr);
    c.0
}

/// Collect all identifier names referenced in an expression (non-recursive
/// into old()).
///
/// Used to find which variables an ensures clause mentions so we can
/// determine which frame axioms to inject.
pub(crate) fn collect_ident_references(expr: &SpExpr) -> Vec<String> {
    struct IdentRefCollector(Vec<String>);
    impl ExprVisitor for IdentRefCollector {
        fn visit_ident(&mut self, name: &str) {
            if name != "true" && name != "false" && name != "result" && name != "self" {
                self.0.push(name.to_string());
            }
        }
        fn visit_field(&mut self, base: &SpExpr, field: &str) {
            let mut path = String::new();
            build_field_path(base, &mut path);
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(field);
            self.0.push(path);
            // Continue walking into the base expression
            self.visit_expr(base);
        }
    }
    let mut c = IdentRefCollector(Vec::new());
    c.visit_expr(expr);
    c.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Spanned;

    fn ident(s: &str) -> SpExpr {
        Spanned::no_span(Expr::Ident(s.to_string()))
    }

    #[test]
    fn extract_modifies_single_ident() {
        let expr = ident("x");
        let targets = extract_modifies_targets(&expr);
        assert_eq!(targets, vec!["x"]);
    }

    #[test]
    fn extract_modifies_block_of_idents() {
        let expr = Spanned::no_span(Expr::Block(vec![
            ident("a"),
            ident("b"),
            ident("c"),
        ]));
        let targets = extract_modifies_targets(&expr);
        assert_eq!(targets, vec!["a", "b", "c"]);
    }

    #[test]
    fn extract_modifies_field_path() {
        let expr = Spanned::no_span(Expr::Field(Box::new(ident("obj")), "field".into()));
        let targets = extract_modifies_targets(&expr);
        assert_eq!(targets, vec!["obj.field"]);
    }

    #[test]
    fn collect_old_references_ident() {
        let expr = Spanned::no_span(Expr::Old(Box::new(ident("x"))));
        let refs = collect_old_references(&expr);
        assert!(refs.contains(&"x".to_string()));
    }

    #[test]
    fn collect_old_references_field() {
        let inner = Spanned::no_span(Expr::Field(Box::new(ident("obj")), "val".into()));
        let expr = Spanned::no_span(Expr::Old(Box::new(inner)));
        let refs = collect_old_references(&expr);
        assert!(refs.contains(&"obj.val".to_string()));
    }

    #[test]
    fn collect_ident_references_skips_builtins() {
        let expr = Spanned::no_span(Expr::Block(vec![
            ident("x"),
            ident("result"),
            ident("true"),
            ident("self"),
            ident("y"),
        ]));
        let refs = collect_ident_references(&expr);
        assert!(refs.contains(&"x".to_string()));
        assert!(refs.contains(&"y".to_string()));
        assert!(!refs.contains(&"result".to_string()));
        assert!(!refs.contains(&"true".to_string()));
        assert!(!refs.contains(&"self".to_string()));
    }
}

// ---------------------------------------------------------------------------
