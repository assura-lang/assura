//! T109: CRUD patterns and auth contracts.

use assura_parser::ast::{ClauseKind, Decl, Expr, ServiceItem, SpExpr};

use crate::TypeError;

/// Standard CRUD and authentication contract patterns.
#[derive(Debug, Clone)]
pub(crate) struct CrudAuthContracts {
    crud_ops: Vec<CrudOperation>,
    auth_policies: Vec<AuthPolicy>,
}

#[derive(Debug, Clone)]
pub(crate) struct CrudOperation {
    pub name: String,
    pub op_type: CrudType,
    pub requires_auth: bool,
    pub preconditions: Vec<String>,
    pub postconditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CrudType {
    Create,
    Read,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub(crate) struct AuthPolicy {
    pub name: String,
    pub required_role: String,
    pub allow_self: bool,
}

impl CrudAuthContracts {
    pub fn new() -> Self {
        Self {
            crud_ops: Vec::new(),
            auth_policies: Vec::new(),
        }
    }

    pub fn add_crud(&mut self, name: String, op_type: CrudType, requires_auth: bool) {
        self.crud_ops.push(CrudOperation {
            name,
            op_type,
            requires_auth,
            preconditions: Vec::new(),
            postconditions: Vec::new(),
        });
    }

    pub fn add_auth_policy(&mut self, name: String, required_role: String, allow_self: bool) {
        self.auth_policies.push(AuthPolicy {
            name,
            required_role,
            allow_self,
        });
    }

    pub fn check_auth_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.requires_auth {
                let has_policy = self.auth_policies.iter().any(|p| p.name == op.name);
                if !has_policy {
                    errors.push(TypeError {
                        code: "A53001".into(),
                        message: format!(
                            "CRUD operation `{}` requires auth but has no policy",
                            op.name
                        ),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    pub fn check_delete_protection(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if op.op_type == CrudType::Delete && !op.requires_auth {
                errors.push(TypeError {
                    code: "A53002".into(),
                    message: format!(
                        "delete operation `{}` should require authentication",
                        op.name
                    ),
                    span: 0..1,
                    secondary: None,
                    suggestion: None,
                });
            }
        }
        errors
    }

    /// Check that CRUD operations with preconditions have matching policies.
    pub fn check_precondition_coverage(&self) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for op in &self.crud_ops {
            if !op.preconditions.is_empty() || !op.postconditions.is_empty() {
                let has_policy = self
                    .auth_policies
                    .iter()
                    .any(|p| p.name == op.name && (!p.required_role.is_empty() || p.allow_self));
                if !has_policy && op.requires_auth {
                    errors.push(TypeError {
                        code: "A53003".into(),
                        message: format!(
                            "CRUD operation `{}` has contracts but no matching auth policy",
                            op.name
                        ),
                        span: 0..1,
                        secondary: None,
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }

    /// AST-walking entry point: scan services for CRUD operations and check auth.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for decl in &source.decls {
            if let Decl::Service(s) = &decl.node {
                let mut checker = CrudAuthContracts::new();
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        let has_auth = clauses.iter().any(|c| {
                            matches!(c.kind, ClauseKind::Other(ref k) if k == "auth" || k == "requires_auth")
                        });
                        let crud_type = if name.starts_with("create") || name.starts_with("add") {
                            CrudType::Create
                        } else if name.starts_with("read")
                            || name.starts_with("get")
                            || name.starts_with("list")
                        {
                            CrudType::Read
                        } else if name.starts_with("update") || name.starts_with("set") {
                            CrudType::Update
                        } else if name.starts_with("delete") || name.starts_with("remove") {
                            CrudType::Delete
                        } else {
                            continue;
                        };
                        checker.add_crud(name.clone(), crud_type, has_auth);
                    }
                }
                for item in &s.items {
                    if let ServiceItem::Operation { name, clauses } = item {
                        for clause in clauses {
                            if let ClauseKind::Other(ref k) = clause.kind
                                && (k == "auth_policy" || k == "role")
                            {
                                let role = extract_ident_from_expr(&clause.body)
                                    .unwrap_or("user")
                                    .to_string();
                                let allow_self = clauses.iter().any(
                                    |c| matches!(&c.kind, ClauseKind::Other(k2) if k2 == "allow_self"),
                                );
                                checker.add_auth_policy(name.clone(), role, allow_self);
                            }
                        }
                    }
                }
                errors.extend(checker.check_auth_coverage());
                errors.extend(checker.check_delete_protection());
                errors.extend(checker.check_precondition_coverage());
            }
        }
        errors
    }
}

impl Default for CrudAuthContracts {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_ident_from_expr(expr: &SpExpr) -> Option<&str> {
    match &expr.node {
        Expr::Ident(s) => Some(s.as_str()),
        Expr::Raw(tokens) => tokens.first().map(|s| s.as_str()),
        _ => None,
    }
}
