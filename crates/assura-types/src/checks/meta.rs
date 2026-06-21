//! Meta-level checks.
//!
//! Match exhaustiveness, interface, structural invariant,
//! complexity bounds, behavioral equivalence, refinement,
//! incremental contracts, scoped invariants, composition, libraries.

use assura_parser::ast::{BlockKind, ClauseKind, Decl, Expr, SpExpr};

use crate::checkers::*;
use crate::convert::parse_type_tokens;
use crate::domain::*;
use crate::types::*;
use crate::{Type, TypeError};

// ---------------------------------------------------------------------------
// Pattern exhaustiveness checking (T017)
// ---------------------------------------------------------------------------

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
/// Implements a simplified Maranget-style coverage check: collects the set of
/// variant names covered by the patterns (a `Wildcard` covers everything) and
/// compares against `enum_variants`.
///
/// Returns `None` if the patterns are exhaustive, or `Some(missing)` with the
/// list of uncovered variant names. The missing list preserves the declaration
/// order from `enum_variants`.
///
/// # Error code
///
/// When this returns `Some(_)`, the caller should report error **A10001**
/// (non-exhaustive match) and include the missing variants in the diagnostic.
pub(crate) fn check_exhaustiveness(
    patterns: &[Pattern],
    enum_variants: &[String],
) -> Option<Vec<String>> {
    // A wildcard covers all variants immediately.
    if patterns.iter().any(|p| matches!(p, Pattern::Wildcard)) {
        return None;
    }

    // Collect the set of variant names explicitly covered.
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

// ---------------------------------------------------------------------------
// Match exhaustiveness wiring (T017)
// ---------------------------------------------------------------------------

/// Walk all expressions in the source file and check match expressions
/// for exhaustiveness against known enum types in the symbol table.
pub(crate) fn run_match_exhaustiveness_checks(
    source: &assura_parser::ast::SourceFile,
    symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut errors = Vec::new();

    // Build a map of enum name -> variant names
    let mut enum_variants: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for decl in &source.decls {
        if let Decl::EnumDef(e) = &decl.node {
            enum_variants.insert(
                e.name.clone(),
                e.variants.iter().map(|v| v.name.clone()).collect(),
            );
        }
    }

    // Walk all clause bodies looking for match expressions
    for decl in &source.decls {
        let clauses: &[assura_parser::ast::Clause] = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Extern(e) => &e.clauses,
            _ => continue,
        };
        for clause in clauses {
            check_match_exhaustiveness_expr(
                &clause.body,
                &decl.span,
                &enum_variants,
                symbols,
                &mut errors,
            );
        }
    }

    errors
}

/// Recursively walk an expression looking for match expressions.
fn check_match_exhaustiveness_expr(
    expr: &SpExpr,
    span: &std::ops::Range<usize>,
    enum_variants: &std::collections::HashMap<String, Vec<String>>,
    _symbols: &assura_resolve::SymbolTable,
    errors: &mut Vec<TypeError>,
) {
    match &expr.node {
        Expr::Match { scrutinee, arms } => {
            // Recurse into scrutinee and arm bodies
            check_match_exhaustiveness_expr(scrutinee, span, enum_variants, _symbols, errors);
            for arm in arms {
                check_match_exhaustiveness_expr(&arm.body, span, enum_variants, _symbols, errors);
            }

            // Try to determine the enum type from the scrutinee
            if let Expr::Ident(name) = &scrutinee.node
                && let Some(variants) = enum_variants.get(name)
            {
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

                if let Some(missing) = check_exhaustiveness(&patterns, variants) {
                    // NOTE: Spec Section 7.2 assigns A09001 to non-exhaustive
                    // patterns, but A09001 is used for totality/decreases in
                    // this implementation. Using A10001 instead.
                    errors.push(TypeError {
                        code: "A10001".into(),
                        message: format!(
                            "non-exhaustive match: missing variants {}",
                            missing.join(", ")
                        ),
                        span: span.clone(),
                        secondary: None,
                    });
                }
            }

            // Even without known enum type, check that there is at least
            // a wildcard if we cannot determine the type
            let has_wildcard = arms
                .iter()
                .any(|arm| matches!(arm.pattern, assura_parser::ast::Pattern::Wildcard));
            let has_enum_coverage = if let Expr::Ident(name) = &scrutinee.node {
                enum_variants.contains_key(name)
            } else {
                false
            };
            if !has_wildcard && !has_enum_coverage && !arms.is_empty() {
                // Warn about match without wildcard on unknown scrutinee type
                errors.push(TypeError {
                    code: "A10002".into(),
                    message: "match expression on unknown type has no wildcard `_` arm; \
                              consider adding a catch-all pattern"
                        .into(),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
        // Recurse into sub-expressions
        Expr::BinOp { lhs, rhs, .. } => {
            check_match_exhaustiveness_expr(lhs, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(rhs, span, enum_variants, _symbols, errors);
        }
        Expr::UnaryOp { expr: e, .. }
        | Expr::Old(e)
        | Expr::Ghost(e)
        | Expr::Field(e, _)
        | Expr::Cast { expr: e, .. } => {
            check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
        }
        Expr::Call { func, args } => {
            check_match_exhaustiveness_expr(func, span, enum_variants, _symbols, errors);
            for a in args {
                check_match_exhaustiveness_expr(a, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Apply { args, .. } => {
            for a in args {
                check_match_exhaustiveness_expr(a, span, enum_variants, _symbols, errors);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            check_match_exhaustiveness_expr(receiver, span, enum_variants, _symbols, errors);
            for a in args {
                check_match_exhaustiveness_expr(a, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Index { expr: e, index } => {
            check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(index, span, enum_variants, _symbols, errors);
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            check_match_exhaustiveness_expr(cond, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(then_branch, span, enum_variants, _symbols, errors);
            if let Some(e) = else_branch {
                check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Forall { domain, body, .. } | Expr::Exists { domain, body, .. } => {
            check_match_exhaustiveness_expr(domain, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(body, span, enum_variants, _symbols, errors);
        }
        Expr::Block(exprs) | Expr::List(exprs) => {
            for e in exprs {
                check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Let { value, body, .. } => {
            check_match_exhaustiveness_expr(value, span, enum_variants, _symbols, errors);
            check_match_exhaustiveness_expr(body, span, enum_variants, _symbols, errors);
        }
        Expr::Tuple(elems) => {
            for e in elems {
                check_match_exhaustiveness_expr(e, span, enum_variants, _symbols, errors);
            }
        }
        Expr::Ident(_) | Expr::Literal(_) | Expr::Raw(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Interface contracts wiring (T062)
// ---------------------------------------------------------------------------

/// Scan for contracts with `implements` clauses and validate that all
/// required interface methods are present with correct signatures.
/// Extract an interface method declaration from a clause body expression.
///
/// Handles several forms:
/// - `Ident("method_name")` -> name only, no params/return
/// - `Call { func: Ident("f"), args }` -> name + param types from args
/// - `Raw(["f", "(", "Int", ")", "->", "Bool"])` -> name + parsed types
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
            // Each arg in a method decl is typically a type identifier
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
            // Try to extract method name from first token
            let name = tokens.first()?.clone();
            // Look for parameter types in parentheses
            let mut param_types = Vec::new();
            let mut return_type = Type::Unknown;
            if let Some(paren_start) = tokens.iter().position(|t| t == "(")
                && let Some(paren_end) = tokens.iter().position(|t| t == ")")
            {
                // Parse param types between ( and )
                let param_tokens = &tokens[paren_start + 1..paren_end];
                for chunk in param_tokens.split(|t| t == ",") {
                    if !chunk.is_empty() {
                        let owned: Vec<String> = chunk.to_vec();
                        param_types.push(parse_type_tokens(&owned));
                    }
                }
                // Look for -> return type after )
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

pub(crate) fn run_interface_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = InterfaceChecker::new();
    let mut errors = Vec::new();

    // First pass: register all contracts that look like interfaces
    // (have `interface` as a clause kind or are named with Interface suffix).
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

    // Second pass: check implementations
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

                    // Register the implementation
                    checker.register_impl(c.name.clone(), iface_name.clone(), method_names.clone());

                    for err in checker.check_impl(&c.name, iface_name, &method_names, &decl.span) {
                        errors.push(err.into());
                    }

                    // Check method signatures against the interface
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

                        // Check reentrancy restrictions
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

// ---------------------------------------------------------------------------
// Structural invariants wiring (T063)
// ---------------------------------------------------------------------------

/// Scan for types with structural invariant annotations and validate
/// that the invariant kind is applicable to the type's structure.
pub(crate) fn run_structural_invariant_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = StructuralInvariantChecker::new();
    let mut errors = Vec::new();

    for decl in &source.decls {
        match &decl.node {
            Decl::TypeDef(td) => {
                // Detect recursive types by checking if any field references
                // the type name itself.
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
            }
            Decl::Contract(c) => {
                // Look for structural_invariant clauses
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

                        // Register the invariant for operation-preservation checking
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

                    // Check that operations preserve registered invariants
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
            _ => {}
        }
    }

    errors
}

pub(crate) fn run_complexity_bound_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = ComplexityBoundChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };
        let name = match &decl.node {
            Decl::FnDef(f) => f.name.clone(),
            Decl::Contract(c) => c.name.clone(),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "complexity" || k == "time_complexity" || k == "big_o")
            {
                found = true;
                if let Expr::Ident(class_name) = &clause.body.node {
                    let class = match class_name.as_str() {
                        "constant" | "O1" => ComplexityClass::Constant,
                        "logarithmic" | "O_log_n" => ComplexityClass::Logarithmic,
                        "linear" | "On" => ComplexityClass::Linear,
                        "nlogn" | "O_n_log_n" => ComplexityClass::NLogN,
                        "quadratic" | "On2" => ComplexityClass::Quadratic,
                        "cubic" | "On3" => ComplexityClass::Cubic,
                        "exponential" | "O2n" => ComplexityClass::Exponential,
                        _ => ComplexityClass::Linear,
                    };
                    checker.declare_bound(name.clone(), class, decl.span.clone());
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Record measured complexity from annotations
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => &f.clauses,
            Decl::Contract(c) => &c.clauses,
            _ => continue,
        };
        let name = match &decl.node {
            Decl::FnDef(f) => f.name.as_str(),
            Decl::Contract(c) => c.name.as_str(),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "measured_complexity" || k == "actual_complexity")
                && let Expr::Ident(class_name) = &clause.body.node
            {
                let class = match class_name.as_str() {
                    "constant" | "O1" => ComplexityClass::Constant,
                    "logarithmic" | "O_log_n" => ComplexityClass::Logarithmic,
                    "linear" | "On" => ComplexityClass::Linear,
                    "nlogn" | "O_n_log_n" => ComplexityClass::NLogN,
                    "quadratic" | "On2" => ComplexityClass::Quadratic,
                    "cubic" | "On3" => ComplexityClass::Cubic,
                    "exponential" | "O2n" => ComplexityClass::Exponential,
                    _ => ComplexityClass::Linear,
                };
                checker.record_measured(name, class);
            }
        }
    }
    let mut errors = checker.check_bounds();
    errors.extend(checker.check_unverified());
    errors.extend(checker.check_expensive());
    errors
}

/// Scan for behavioral equivalence annotations.
pub(crate) fn run_behavioral_equivalence_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = BehavioralEquivalenceChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let (clauses, parent_name) = match &decl.node {
            Decl::Contract(c) => (&c.clauses, c.name.as_str()),
            Decl::FnDef(f) => (&f.clauses, f.name.as_str()),
            Decl::Block { body, name, .. } => (body, name.as_str()),
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "equivalent" || k == "behavioral_equiv" || k == "equiv")
            {
                found = true;
                if let Expr::BinOp { lhs, rhs, .. } = &clause.body.node
                    && let (Expr::Ident(a), Expr::Ident(b)) =
                        (&lhs.as_ref().node, &rhs.as_ref().node)
                {
                    checker.declare(
                        format!("{a}_equiv_{b}"),
                        a.clone(),
                        b.clone(),
                        parent_name.to_string(),
                        decl.span.clone(),
                    );
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Mark equivalences as verified if proof clauses exist
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "verified_equiv" || k == "equiv_proved")
                && let Expr::Ident(name) = &clause.body.node
            {
                checker.mark_verified(name);
            }
        }
    }
    let mut errors = checker.check_unverified();
    errors.extend(checker.check_self_equivalence());
    errors.extend(checker.check_contract_ref());
    errors
}

/// Scan for multi-pass refinement annotations.
pub(crate) fn run_multi_pass_refinement_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = MultiPassRefinementChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "refinement_pass" || k == "multi_pass" || k == "refine")
            {
                found = true;
                // Extract pass params: refine(name, from_level, to_level, order)
                match &clause.body.node {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = &func.as_ref().node {
                            let from = args
                                .first()
                                .and_then(extract_ident)
                                .unwrap_or("abstract")
                                .to_string();
                            let to = args
                                .get(1)
                                .and_then(extract_ident)
                                .unwrap_or("concrete")
                                .to_string();
                            let order = args
                                .get(2)
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as usize;
                            checker.add_pass(name.clone(), from, to, order, decl.span.clone());
                        }
                    }
                    Expr::Ident(name) => {
                        checker.add_pass(
                            name.clone(),
                            "abstract".into(),
                            "concrete".into(),
                            1,
                            decl.span.clone(),
                        );
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "pass")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let from = kvs
                            .iter()
                            .find(|(k, _)| *k == "from" || *k == "source")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("abstract")
                            .to_string();
                        let to = kvs
                            .iter()
                            .find(|(k, _)| *k == "to" || *k == "target")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("concrete")
                            .to_string();
                        let order =
                            kvs.iter()
                                .find(|(k, _)| *k == "order")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                        checker.add_pass(name, from, to, order, decl.span.clone());
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Discharge refinement obligations from proof annotations
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "discharge_pass" || k == "pass_proved")
                && let Some((name, args)) = extract_call(&clause.body)
            {
                let count = args
                    .first()
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ONE) as usize;
                checker.discharge(name, count);
            }
        }
    }
    let mut errors = checker.check_complete();
    errors.extend(checker.check_chain());
    errors.extend(checker.check_non_trivial());
    errors
}

/// Scan for incremental contract version annotations.
pub(crate) fn run_incremental_contract_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = IncrementalContractChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        // Count requires/ensures clauses for this declaration
        let requires_count = clauses
            .iter()
            .filter(|c| matches!(c.kind, ClauseKind::Requires))
            .count();
        let ensures_count = clauses
            .iter()
            .filter(|c| matches!(c.kind, ClauseKind::Ensures))
            .count();
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "version" || k == "incremental" || k == "contract_version")
            {
                found = true;
                // Extract version: version(name, major, minor, patch)
                match &clause.body.node {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = &func.as_ref().node {
                            let major = args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ONE)
                                as u32;
                            let minor = args
                                .get(1)
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as u32;
                            let patch = args
                                .get(2)
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_PARAM_ZERO)
                                as u32;
                            let version = major * 10000 + minor * 100 + patch;
                            checker.add_version(
                                name.clone(),
                                version,
                                requires_count,
                                ensures_count,
                            );
                        }
                    }
                    Expr::Ident(name) => {
                        checker.add_version(name.clone(), 10000, requires_count, ensures_count);
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "contract")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let major =
                            kvs.iter()
                                .find(|(k, _)| *k == "major")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ONE) as u32;
                        let minor =
                            kvs.iter()
                                .find(|(k, _)| *k == "minor")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO) as u32;
                        let patch =
                            kvs.iter()
                                .find(|(k, _)| *k == "patch")
                                .and_then(|(_, v)| extract_int_literal(v))
                                .unwrap_or(DEFAULT_PARAM_ZERO) as u32;
                        let version = major * 10000 + minor * 100 + patch;
                        checker.add_version(name, version, requires_count, ensures_count);
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_precondition_weakening();
    errors.extend(checker.check_postcondition_strengthening());
    errors.extend(checker.check_version_continuity());
    errors
}

/// Scan for scoped invariant suspension annotations.
pub(crate) fn run_scoped_invariant_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = ScopedInvariantChecker::new();
    let mut errors = Vec::new();
    let mut found = false;
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind {
                if k == "suspend_invariant" || k == "scoped_invariant" {
                    found = true;
                    if let Expr::Ident(name) = &clause.body.node {
                        checker.declare_invariant(name.clone());
                        if let Some(err) = checker.suspend(name) {
                            errors.push(err);
                        }
                    }
                }
                if (k == "restore_invariant" || k == "restore")
                    && let Expr::Ident(name) = &clause.body.node
                    && let Some(err) = checker.restore(name)
                {
                    errors.push(err);
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check individual invariant suspension status in clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => &c.clauses,
            Decl::FnDef(f) => &f.clauses,
            Decl::Block { body, .. } => body,
            _ => continue,
        };
        for clause in clauses {
            if clause.kind == ClauseKind::Requires || clause.kind == ClauseKind::Ensures {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    if checker.is_suspended(name) {
                        errors.push(TypeError {
                            code: "A52001".into(),
                            message: format!(
                                "invariant `{name}` is suspended in active clause context"
                            ),
                            span: decl.span.clone(),
                            secondary: None,
                        });
                    }
                }
            }
        }
    }
    errors.extend(checker.check_all_restored());
    errors
}

/// Scan for contract composition (extends) and validate.
pub(crate) fn run_contract_composition_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = ContractCompositionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let extends: Vec<String> = c
                .clauses
                .iter()
                .filter(|cl| {
                    matches!(&cl.kind, ClauseKind::Other(k) if k == "extends" || k == "inherits")
                })
                .filter_map(|cl| {
                    if let Expr::Ident(name) = &cl.body.node {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            if !extends.is_empty() {
                found = true;
            }
            checker.declare(c.name.clone(), extends, c.clauses.len());
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_extends();
    errors.extend(checker.check_circular());
    errors.extend(checker.check_diamond());
    errors.extend(checker.check_empty_contracts());
    errors
}

/// Scan for contract library packaging declarations.
pub(crate) fn run_contract_library_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = ContractLibraryChecker::new();
    let mut found = false;
    for decl in &source.decls {
        match &decl.node {
            Decl::Block {
                kind, name, body, ..
            } if *kind == BlockKind::Library => {
                found = true;
                checker.declare_library(name.clone(), "0.1.0".into());
                for clause in body {
                    if let ClauseKind::Other(ref k) = clause.kind {
                        if (k == "export" || k == "exports")
                            && let Expr::Ident(contract_name) = &clause.body.node
                        {
                            checker.add_export(name, contract_name.clone());
                        }
                        if (k == "depends" || k == "dependency")
                            && let Expr::Ident(dep_name) = &clause.body.node
                        {
                            checker.add_dependency(
                                name,
                                LibraryDep {
                                    name: dep_name.clone(),
                                    version_req: "*".into(),
                                },
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    if !found {
        return Vec::new();
    }
    let mut errors = checker.check_empty_exports();
    errors.extend(checker.check_circular_deps());
    errors.extend(checker.check_duplicates());
    errors.extend(checker.check_version_compat());
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: parse source text into a SourceFile, panicking on errors.
    fn parse_ok(src: &str) -> assura_parser::ast::SourceFile {
        assura_parser::parse_unwrap(src)
    }

    // -----------------------------------------------------------------------
    // run_complexity_bound_checks
    // -----------------------------------------------------------------------

    #[test]
    fn complexity_no_annotation_produces_no_errors() {
        let source = parse_ok("contract Plain { requires { true } }");
        let errs = run_complexity_bound_checks(&source);
        assert!(
            errs.is_empty(),
            "no complexity clause should yield no errors"
        );
    }

    #[test]
    fn complexity_linear_unverified_produces_a48002() {
        let source =
            parse_ok(r#"contract Search { complexity linear requires { true } ensures { true } }"#);
        let errs = run_complexity_bound_checks(&source);
        assert!(
            errs.iter().any(|e| e.code == "A48002"),
            "expected A48002 for unverified complexity bound, got: {errs:?}"
        );
    }

    #[test]
    fn complexity_verified_does_not_produce_a48002() {
        // Declare a bound AND supply a measured_complexity annotation to discharge it.
        let source = parse_ok(
            r#"contract Sort {
                complexity linear
                measured_complexity linear
                requires { true }
            }"#,
        );
        let errs = run_complexity_bound_checks(&source);
        assert!(
            !errs.iter().any(|e| e.code == "A48002"),
            "verified bound should not emit A48002, got: {errs:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run_contract_composition_checks
    // -----------------------------------------------------------------------

    #[test]
    fn composition_no_extends_produces_no_errors() {
        let source = parse_ok("contract Standalone { requires { true } }");
        let errs = run_contract_composition_checks(&source);
        assert!(
            errs.is_empty(),
            "no extends clause should yield no errors, got: {errs:?}"
        );
    }

    #[test]
    fn composition_extends_unknown_produces_a54001() {
        let source = parse_ok(r#"contract Child { extends NonExistent requires { true } }"#);
        let errs = run_contract_composition_checks(&source);
        assert!(
            errs.iter().any(|e| e.code == "A54001"),
            "expected A54001 for extends unknown contract, got: {errs:?}"
        );
    }

    #[test]
    fn composition_extends_known_does_not_produce_a54001() {
        let source = parse_ok(
            r#"
            contract Parent { requires { true } }
            contract Child { extends Parent requires { true } }
            "#,
        );
        let errs = run_contract_composition_checks(&source);
        assert!(
            !errs.iter().any(|e| e.code == "A54001"),
            "extending a known contract should not emit A54001, got: {errs:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run_scoped_invariant_checks
    // -----------------------------------------------------------------------

    #[test]
    fn scoped_invariant_no_annotation_produces_no_errors() {
        let source = parse_ok("contract Clean { requires { true } }");
        let errs = run_scoped_invariant_checks(&source);
        assert!(
            errs.is_empty(),
            "no suspend_invariant should yield no errors, got: {errs:?}"
        );
    }

    #[test]
    fn scoped_invariant_suspended_ref_in_ensures_produces_a52001() {
        // suspend_invariant marks "sorted" as suspended, then ensures references it.
        let source = parse_ok(
            r#"contract Maintenance { suspend_invariant sorted requires { sorted > 0 } }"#,
        );
        let errs = run_scoped_invariant_checks(&source);
        assert!(
            errs.iter().any(|e| e.code == "A52001"),
            "expected A52001 for suspended invariant referenced in clause, got: {errs:?}"
        );
    }

    #[test]
    fn scoped_invariant_restored_does_not_produce_a52001() {
        // suspend_invariant then restore_invariant before the requires clause.
        let source = parse_ok(
            r#"contract Maintenance {
                suspend_invariant sorted
                restore_invariant sorted
                requires { sorted > 0 }
            }"#,
        );
        let errs = run_scoped_invariant_checks(&source);
        // After restore, references in requires should not emit A52001.
        assert!(
            !errs.iter().any(|e| e.code == "A52001"),
            "restored invariant should not emit A52001, got: {errs:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run_behavioral_equivalence_checks
    // -----------------------------------------------------------------------

    #[test]
    fn behavioral_equivalence_no_annotation_produces_no_errors() {
        let source = parse_ok("contract Simple { requires { true } }");
        let errs = run_behavioral_equivalence_checks(&source);
        assert!(
            errs.is_empty(),
            "no equivalent clause should yield no errors, got: {errs:?}"
        );
    }

    #[test]
    fn behavioral_equivalence_unverified_produces_a49001() {
        let source =
            parse_ok(r#"contract Equiv { equivalent impl_a == impl_b requires { true } }"#);
        let errs = run_behavioral_equivalence_checks(&source);
        assert!(
            errs.iter().any(|e| e.code == "A49001"),
            "expected A49001 for unverified behavioral equivalence, got: {errs:?}"
        );
    }

    #[test]
    fn behavioral_equivalence_self_equiv_produces_a49002() {
        // Declaring equivalence where both sides are the same triggers A49002.
        let source = parse_ok(r#"contract Equiv { equivalent same == same requires { true } }"#);
        let errs = run_behavioral_equivalence_checks(&source);
        assert!(
            errs.iter().any(|e| e.code == "A49002"),
            "expected A49002 for trivial self-equivalence, got: {errs:?}"
        );
    }
}
