//! T089: STOR.4 Transactional rollback.

use assura_parser::ast::{ClauseKind, Expr, SpExpr};

use crate::TypeError;
use crate::checkers::*;

#[derive(Debug, Clone)]
pub(crate) struct RollbackChecker {
    savepoints: Vec<String>,
    resources_acquired: Vec<String>,
    rolled_back: bool,
}

impl RollbackChecker {
    pub fn new() -> Self {
        Self {
            savepoints: Vec::new(),
            resources_acquired: Vec::new(),
            rolled_back: false,
        }
    }

    pub fn create_savepoint(&mut self, name: String) {
        self.savepoints.push(name);
    }

    pub fn acquire_resource(&mut self, name: String) {
        self.resources_acquired.push(name);
    }

    pub fn release_resource(&mut self, name: &str) {
        self.resources_acquired.retain(|r| r != name);
    }

    pub fn rollback_to(&mut self, savepoint: &str) -> Option<TypeError> {
        if !self.savepoints.contains(&savepoint.to_string()) {
            return Some(TypeError {
                code: "A36001".into(),
                message: format!("rollback to unknown savepoint `{savepoint}`"),
                span: 0..1,
                secondary: None,
                suggestion: None,
            });
        }
        self.rolled_back = true;
        if let Some(pos) = self.savepoints.iter().position(|s| s == savepoint) {
            self.savepoints.truncate(pos + 1);
        }
        None
    }

    pub fn check_resource_leak(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        if self.rolled_back {
            for r in &self.resources_acquired {
                errors.push(TypeError {
                    code: "A36002".into(),
                    message: format!("resource `{r}` not released after rollback"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    pub fn check_savepoint_nesting(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for sp in &self.savepoints {
            if !seen.insert(sp.clone()) {
                errors.push(TypeError {
                    code: "A36003".into(),
                    message: format!("duplicate savepoint name `{sp}`"),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }
}

impl RollbackChecker {
    /// Scan an expression for rollback operations.
    fn scan_expr(expr: &SpExpr, checker: &mut RollbackChecker) -> Vec<TypeError> {
        let mut scan_errors = Vec::new();
        if let Some((name, args)) = extract_call(expr) {
            match name {
                "savepoint" | "create_savepoint" => {
                    let sp_name = args
                        .first()
                        .and_then(extract_ident)
                        .unwrap_or("default")
                        .to_string();
                    checker.create_savepoint(sp_name);
                }
                "acquire" | "acquire_resource" | "lock" => {
                    let res_name = args
                        .first()
                        .and_then(extract_ident)
                        .unwrap_or("resource")
                        .to_string();
                    checker.acquire_resource(res_name);
                }
                "release" | "release_resource" | "unlock" => {
                    let res_name = args.first().and_then(extract_ident).unwrap_or("resource");
                    checker.release_resource(res_name);
                }
                "rollback" | "rollback_to" => {
                    let sp_name = args.first().and_then(extract_ident).unwrap_or("default");
                    if let Some(err) = checker.rollback_to(sp_name) {
                        scan_errors.push(err);
                    }
                }
                _ => {}
            }
        }
        if let Expr::Ident(name) = &expr.node {
            checker.create_savepoint(name.clone());
        }
        match &expr.node {
            Expr::Block(exprs) | Expr::List(exprs) => {
                for e in exprs {
                    scan_errors.extend(Self::scan_expr(e, checker));
                }
            }
            Expr::BinOp { lhs, rhs, .. } => {
                scan_errors.extend(Self::scan_expr(lhs, checker));
                scan_errors.extend(Self::scan_expr(rhs, checker));
            }
            Expr::Call { func, args } => {
                scan_errors.extend(Self::scan_expr(func, checker));
                for a in args {
                    scan_errors.extend(Self::scan_expr(a, checker));
                }
            }
            _ => {}
        }
        scan_errors
    }

    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = Self::new();
        let mut scan_errors = Vec::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::checks::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "rollback" || k == "savepoint" || k == "transactional")
                {
                    found = true;
                    scan_errors.extend(Self::scan_expr(&clause.body, &mut checker));
                }
                if found && matches!(clause.kind, ClauseKind::Requires | ClauseKind::Ensures) {
                    scan_errors.extend(Self::scan_expr(&clause.body, &mut checker));
                }
            }
        }
        if !found {
            return Vec::new();
        }
        let mut errors = scan_errors;
        errors.extend(checker.check_resource_leak());
        errors.extend(checker.check_savepoint_nesting());
        errors
    }
}

impl Default for RollbackChecker {
    fn default() -> Self {
        Self::new()
    }
}
