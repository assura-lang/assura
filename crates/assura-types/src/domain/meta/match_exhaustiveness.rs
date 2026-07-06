use assura_parser::ast::{ClauseKind, Decl, Expr, ExprVisitor, MatchArm, SpExpr};

use crate::checkers::{
    InterfaceChecker, InterfaceContract, InterfaceMethod, InvariantKind, StructuralInvariant,
    StructuralInvariantChecker,
};
use crate::checks::clauses_contract_fn_extern;
use crate::convert::parse_type_tokens;
use crate::{Type, TypeError};

// ===========================================================================
// Pattern exhaustiveness (T017)
// ===========================================================================

/// A pattern in a match arm, used for exhaustiveness checking.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Pattern {
    /// Matches a specific enum variant by name.
    Variant(String),
    /// Wildcard `_` pattern that matches anything.
    Wildcard,
    /// Matches a specific literal value.
    Literal(assura_parser::ast::Literal),
}

/// Check whether a set of patterns exhaustively covers all variants of an enum.
///
/// Returns `None` if the patterns are exhaustive, or `Some(missing)` with the
/// list of uncovered variant names.
pub(crate) fn check_exhaustiveness(
    patterns: &[Pattern],
    enum_variants: &[String],
) -> Option<Vec<String>> {
    if patterns.iter().any(|p| matches!(p, Pattern::Wildcard)) {
        return None;
    }
    let covered: std::collections::HashSet<&str> = patterns
        .iter()
        .filter_map(|p| match p {
            Pattern::Variant(name) => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let missing: Vec<String> = enum_variants
        .iter()
        .filter(|v| !covered.contains(v.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

// ===========================================================================
// Match exhaustiveness source walking (T017)
// ===========================================================================

/// Map enum type name → variant names.
type EnumVariantMap = std::collections::HashMap<String, Vec<String>>;
/// Map parameter / binding name → enum type name (from `input(c: Color)` etc.).
type ParamTypeMap = std::collections::HashMap<String, String>;

/// Resolve the enum type name for a match scrutinee ident.
///
/// Prefer a parameter's declared type (`input(c: Color)` → `Color`), then fall
/// back to treating the ident itself as an enum type name (`match Status { … }`).
fn enum_type_for_scrutinee_ident(
    name: &str,
    enum_variants: &EnumVariantMap,
    param_types: &ParamTypeMap,
) -> Option<String> {
    if let Some(ty) = param_types.get(name)
        && enum_variants.contains_key(ty)
    {
        return Some(ty.clone());
    }
    if enum_variants.contains_key(name) {
        return Some(name.to_string());
    }
    None
}

fn simple_type_name(ty: &assura_parser::ast::TypeExpr) -> Option<String> {
    use assura_parser::ast::TypeExpr;
    match ty {
        TypeExpr::Named(n) => Some(n.clone()),
        TypeExpr::Generic(name, _) => Some(name.clone()),
        TypeExpr::Refined { base, .. } => simple_type_name(base),
        TypeExpr::Tuple(_) | TypeExpr::Fn { .. } | TypeExpr::Unit => None,
    }
}

/// Walk all expressions in the source file and check match expressions
/// for exhaustiveness against known enum types.
pub(crate) fn run_match_exhaustiveness_source(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut errors = Vec::new();
    let mut enum_variants: EnumVariantMap = std::collections::HashMap::new();
    for decl in &source.decls {
        if let Decl::EnumDef(e) = &decl.node {
            enum_variants.insert(
                e.name.clone(),
                e.variants.iter().map(|v| v.name.clone()).collect(),
            );
        }
    }
    for decl in &source.decls {
        let Some(clauses) = clauses_contract_fn_extern(&decl.node) else {
            continue;
        };
        // Build param-name → type-name from input clauses and fn_params / params.
        let mut param_types: ParamTypeMap = std::collections::HashMap::new();
        match &decl.node {
            Decl::Contract(c) => {
                for p in &c.fn_params {
                    if let Some(te) = p.ty.as_ref()
                        && let Some(tn) = simple_type_name(te)
                    {
                        param_types.insert(p.name.clone(), tn);
                    }
                }
            }
            Decl::FnDef(f) => {
                for p in &f.params {
                    if let Some(te) = p.ty.as_ref()
                        && let Some(tn) = simple_type_name(te)
                    {
                        param_types.insert(p.name.clone(), tn);
                    }
                }
            }
            Decl::Extern(e) => {
                for p in &e.params {
                    if let Some(te) = p.ty.as_ref()
                        && let Some(tn) = simple_type_name(te)
                    {
                        param_types.insert(p.name.clone(), tn);
                    }
                }
            }
            _ => {}
        }
        for clause in clauses {
            if matches!(
                &clause.kind,
                assura_parser::ast::ClauseKind::Input | assura_parser::ast::ClauseKind::Other(_)
            ) {
                // Prefer structured Input; also accept other that is input-like via extract.
                if matches!(&clause.kind, assura_parser::ast::ClauseKind::Input)
                    || matches!(&clause.kind, assura_parser::ast::ClauseKind::Other(k) if k == "input")
                {
                    for p in assura_parser::ast::extract_clause_params(&clause.body) {
                        if let Some(te) = p.ty.as_ref()
                            && let Some(tn) = simple_type_name(te)
                        {
                            param_types.insert(p.name.clone(), tn);
                        }
                    }
                }
            }
        }
        for clause in clauses {
            check_match_exhaustiveness_expr(
                &clause.body,
                &decl.span,
                &enum_variants,
                &param_types,
                symbols,
                &mut errors,
            );
        }
    }
    errors
}

fn check_match_exhaustiveness_expr(
    expr: &SpExpr,
    span: &std::ops::Range<usize>,
    enum_variants: &EnumVariantMap,
    param_types: &ParamTypeMap,
    _symbols: &assura_resolve::SymbolTable,
    errors: &mut Vec<TypeError>,
) {
    struct MatchExhaustivenessVisitor<'a> {
        span: &'a std::ops::Range<usize>,
        enum_variants: &'a EnumVariantMap,
        param_types: &'a ParamTypeMap,
        errors: &'a mut Vec<TypeError>,
    }

    impl ExprVisitor for MatchExhaustivenessVisitor<'_> {
        fn visit_match(&mut self, scrutinee: &SpExpr, arms: &[MatchArm]) {
            self.visit_expr(scrutinee);
            for arm in arms {
                self.visit_expr(&arm.body);
            }
            let patterns: Vec<Pattern> = arms
                .iter()
                .map(|arm| match &arm.pattern {
                    assura_parser::ast::Pattern::Ident(n) => Pattern::Variant(n.clone()),
                    assura_parser::ast::Pattern::Wildcard => Pattern::Wildcard,
                    assura_parser::ast::Pattern::Literal(lit) => Pattern::Literal(lit.clone()),
                    assura_parser::ast::Pattern::Constructor { name, .. } => {
                        Pattern::Variant(name.clone())
                    }
                    assura_parser::ast::Pattern::Tuple(_) => Pattern::Wildcard,
                })
                .collect();

            let enum_ty = if let Expr::Ident(name) = &scrutinee.node {
                enum_type_for_scrutinee_ident(name, self.enum_variants, self.param_types)
            } else {
                None
            };

            if let Some(ref ty_name) = enum_ty
                && let Some(variants) = self.enum_variants.get(ty_name)
                && let Some(missing) = check_exhaustiveness(&patterns, variants)
            {
                self.errors.push(TypeError {
                    code: "A10001".into(),
                    message: format!(
                        "non-exhaustive match: missing variants {}",
                        missing.join(", ")
                    ),
                    span: self.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
            let has_wildcard = arms
                .iter()
                .any(|arm| matches!(arm.pattern, assura_parser::ast::Pattern::Wildcard));
            let has_enum_coverage = enum_ty.is_some();
            if !has_wildcard && !has_enum_coverage && !arms.is_empty() {
                self.errors.push(TypeError {
                    code: "A10002".into(),
                    message: "match expression on unknown type has no wildcard `_` arm; \
                              consider adding a catch-all pattern"
                        .into(),
                    span: self.span.clone(),
                    secondary: None,
                    suggestion: None,
                });
            }
        }
    }

    let mut visitor = MatchExhaustivenessVisitor {
        span,
        enum_variants,
        param_types,
        errors,
    };
    visitor.visit_expr(expr);
}

// ===========================================================================
// Interface contracts source walking (T062)
// ===========================================================================

fn extract_interface_method(body: &SpExpr) -> Option<InterfaceMethod> {
    match &body.node {
        Expr::Ident(name) => Some(InterfaceMethod {
            name: name.clone(),
            param_types: vec![],
            return_type: Type::Unknown,
            has_requires: false,
            has_ensures: false,
            no_reentrancy: false,
        }),
        Expr::Call { func, args } => {
            let name = match &func.as_ref().node {
                Expr::Ident(n) => n.clone(),
                _ => return None,
            };
            let param_types: Vec<Type> = args
                .iter()
                .map(|arg| match &arg.node {
                    Expr::Ident(t) => parse_type_tokens(std::slice::from_ref(t)),
                    _ => Type::Unknown,
                })
                .collect();
            Some(InterfaceMethod {
                name,
                param_types,
                return_type: Type::Unknown,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            })
        }
        Expr::Raw(tokens) => {
            let name = tokens.first()?.clone();
            let mut param_types = Vec::new();
            let mut return_type = Type::Unknown;
            if let Some(paren_start) = tokens.iter().position(|t| t == "(")
                && let Some(paren_end) = tokens.iter().position(|t| t == ")")
            {
                let param_tokens = &tokens[paren_start + 1..paren_end];
                for chunk in param_tokens.split(|t| t == ",") {
                    if !chunk.is_empty() {
                        let owned: Vec<String> = chunk.to_vec();
                        param_types.push(parse_type_tokens(&owned));
                    }
                }
                if let Some(arrow_pos) = tokens[paren_end..].iter().position(|t| t == "->") {
                    let ret_tokens: Vec<String> = tokens[paren_end + arrow_pos + 1..].to_vec();
                    if !ret_tokens.is_empty() {
                        return_type = parse_type_tokens(&ret_tokens);
                    }
                }
            }
            Some(InterfaceMethod {
                name,
                param_types,
                return_type,
                has_requires: false,
                has_ensures: false,
                no_reentrancy: false,
            })
        }
        _ => None,
    }
}

impl InterfaceChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = InterfaceChecker::new();
        let mut errors = Vec::new();

        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                let is_interface = c
                    .clauses
                    .iter()
                    .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
                if is_interface {
                    let methods: Vec<InterfaceMethod> = c
                        .clauses
                        .iter()
                        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                        .filter_map(|cl| extract_interface_method(&cl.body))
                        .collect();
                    let extends: Vec<String> = c
                        .clauses
                        .iter()
                        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "extends"))
                        .filter_map(|cl| {
                            if let Expr::Ident(name) = &cl.body.node {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    checker.register_interface(InterfaceContract {
                        name: c.name.clone(),
                        methods,
                        extends,
                    });
                }
            }
        }

        for decl in &source.decls {
            if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "implements"
                        && let Expr::Ident(iface_name) = &clause.body.node
                    {
                        let impl_methods: Vec<InterfaceMethod> = c
                            .clauses
                            .iter()
                            .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "method"))
                            .filter_map(|cl| extract_interface_method(&cl.body))
                            .collect();
                        let method_names: Vec<String> =
                            impl_methods.iter().map(|m| m.name.clone()).collect();
                        checker.register_impl(
                            c.name.clone(),
                            iface_name.clone(),
                            method_names.clone(),
                        );
                        for err in
                            checker.check_impl(&c.name, iface_name, &method_names, &decl.span)
                        {
                            errors.push(err.into());
                        }
                        for method in &impl_methods {
                            for err in checker.check_method_signature(
                                iface_name,
                                &method.name,
                                &method.param_types,
                                &method.return_type,
                                &decl.span,
                            ) {
                                errors.push(err.into());
                            }
                            let is_reentrant = c.clauses.iter().any(|cl| {
                                matches!(&cl.kind, ClauseKind::Other(k) if k == "reentrant")
                                    && matches!(&cl.body.node, Expr::Ident(n) if n == &method.name)
                            });
                            for err in checker.check_reentrancy(
                                iface_name,
                                &method.name,
                                is_reentrant,
                                &decl.span,
                            ) {
                                errors.push(err.into());
                            }
                        }
                    }
                }
            }
        }

        errors
    }
}

// ===========================================================================
// Structural invariants source walking (T063)
// ===========================================================================

impl StructuralInvariantChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = StructuralInvariantChecker::new();
        let mut errors = Vec::new();

        for decl in &source.decls {
            if let Decl::TypeDef(td) = &decl.node {
                if let assura_parser::ast::TypeBody::Struct(fields) = &td.body {
                    let recursive_fields: Vec<String> = fields
                        .iter()
                        .filter(|f| {
                            let tokens = f.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
                            tokens.iter().any(|t| t == &td.name)
                        })
                        .map(|f| f.name.clone())
                        .collect();
                    if !recursive_fields.is_empty() {
                        checker.register_recursive_type(td.name.clone(), recursive_fields);
                    }
                }
            } else if let Decl::Contract(c) = &decl.node {
                for clause in &c.clauses {
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "structural_invariant"
                    {
                        let kind = match &clause.body.node {
                            Expr::Ident(name) => match name.as_str() {
                                "sorted" => InvariantKind::Sorted { descending: false },
                                "acyclic" => InvariantKind::Acyclic,
                                "bst_ordering" => InvariantKind::BstOrdering,
                                other => InvariantKind::Custom(other.to_string()),
                            },
                            Expr::Call { func, .. } => {
                                if let Expr::Ident(name) = &func.as_ref().node {
                                    match name.as_str() {
                                        "tree_balance" => {
                                            InvariantKind::TreeBalance { max_diff: 1 }
                                        }
                                        "min_heap" => {
                                            InvariantKind::HeapProperty { min_heap: true }
                                        }
                                        "max_heap" => {
                                            InvariantKind::HeapProperty { min_heap: false }
                                        }
                                        other => InvariantKind::Custom(other.to_string()),
                                    }
                                } else {
                                    InvariantKind::Custom(format!("{:?}", clause.body))
                                }
                            }
                            _ => InvariantKind::Custom(format!("{:?}", clause.body)),
                        };
                        checker.register_invariant(StructuralInvariant {
                            name: format!("{}_{}", c.name, kind),
                            type_name: c.name.clone(),
                            kind: kind.clone(),
                        });
                        for err in checker.check_invariant_applicability(&c.name, &kind, &decl.span)
                        {
                            errors.push(err.into());
                        }
                    }
                    if let ClauseKind::Other(k) = &clause.kind
                        && k == "modifies_structure"
                    {
                        let op_name = match &clause.body.node {
                            Expr::Ident(name) => name.as_str(),
                            _ => "unknown",
                        };
                        let has_preservation = c.clauses.iter().any(|cl| {
                            matches!(&cl.kind, ClauseKind::Other(k2) if k2 == "preserves_invariant")
                        });
                        for err in checker.check_operation_preserves(
                            &c.name,
                            op_name,
                            true,
                            has_preservation,
                            &decl.span,
                        ) {
                            errors.push(err.into());
                        }
                    }
                }
            }
        }

        errors
    }
}
