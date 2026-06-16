//! Information flow, taint, and dependent type checks.

use std::ops::Range;

use assura_parser::ast::{BinOp, ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;
use crate::convert::parse_type_tokens;

pub(crate) fn run_taint_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    TaintChecker::check_file(source)
}

/// S003: Run information flow tracking on contracts and functions.
///
/// Assigns security labels to input parameters based on annotations
/// (`@secret`, `@confidential`, `@internal`) and traces information flow
/// through ensures clause expressions. Reports A08001 if secret-labeled
/// data flows to a public output, and A08004 for implicit flows through
/// branches where a secret condition influences a public assignment.
pub(crate) fn run_info_flow_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut errors = Vec::new();

    for decl in &source.decls {
        match &decl.node {
            Decl::Contract(c) => {
                errors.extend(check_contract_info_flow(c, &decl.span));
            }
            Decl::FnDef(f) => {
                errors.extend(check_fn_info_flow(f, &decl.span));
            }
            _ => {}
        }
    }

    // Run dependent type checks on type definitions with index parameters
    errors.extend(run_dependent_type_checks(source));

    errors
}

/// Check dependent type index validity on type and contract declarations.
pub(crate) fn run_dependent_type_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut dep_checker = DependentTypeChecker::new();
    let mut errors = Vec::new();

    // Pass 1: register enum types for finiteness checking
    for decl in &source.decls {
        if let Decl::EnumDef(e) = &decl.node {
            let variants: Vec<String> = e.variants.iter().map(|v| v.name.clone()).collect();
            dep_checker.register_enum(e.name.clone(), variants);
        }
    }

    // Pass 2: check type/contract declarations for dependent type annotations
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::Contract(c) => c.clauses.as_slice(),
            Decl::FnDef(f) => f.clauses.as_slice(),
            _ => continue,
        };

        for clause in clauses {
            // Look for "dep_type" or "dependent" clause annotations
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "dep_type" || k == "dependent")
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 2
            {
                let index_name = &tokens[0];
                let index_type = &tokens[1];
                // Validate the index kind
                for dte in dep_checker.validate_index(index_name, index_type, &decl.span) {
                    errors.push(TypeError {
                        code: dte.code,
                        message: dte.message,
                        span: dte.span,
                        secondary: None,
                    });
                }
                // Bind the index variable
                let dep_index = match index_type.as_str() {
                    "Nat" => DepIndex::Nat(index_name.clone()),
                    "Bool" => DepIndex::Bool(index_name.clone()),
                    other => DepIndex::Enum {
                        name: index_name.clone(),
                        enum_type: other.to_string(),
                    },
                };
                dep_checker.bind_index(index_name.clone(), dep_index.clone());

                // If there is a type expression argument, check it
                if tokens.len() >= 3 {
                    let base_type = parse_type_tokens(std::slice::from_ref(&tokens[2]));
                    let dep_type = DepType {
                        base: base_type.clone(),
                        indices: vec![dep_index],
                    };
                    dep_checker.register_dep_type(index_name.clone(), dep_type);
                }
            }

            // Check index expressions in type positions
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "index_expr"
            {
                // Find the first bound index to check the expression against
                if let Some((_, idx)) = dep_checker.index_vars_ref().iter().next() {
                    for dte in dep_checker.check_index_expr(&clause.body, idx, &decl.span) {
                        errors.push(TypeError {
                            code: dte.code,
                            message: dte.message,
                            span: dte.span,
                            secondary: None,
                        });
                    }
                }
            }

            // Check index erasure in non-ghost contexts
            if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires) {
                let ghost_context = false;
                for dte in dep_checker.check_index_erasure(&clause.body, ghost_context, &decl.span)
                {
                    errors.push(TypeError {
                        code: dte.code,
                        message: dte.message,
                        span: dte.span,
                        secondary: None,
                    });
                }
            }
        }

        // Check dependent type equality in contracts with type annotations
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && k == "dep_type_eq"
                && let Expr::Raw(tokens) = &clause.body
                && tokens.len() >= 2
            {
                let name_a = &tokens[0];
                let name_b = &tokens[1];
                if let (Some(dt_a), Some(dt_b)) = (
                    dep_checker.dep_types_ref().get(name_a),
                    dep_checker.dep_types_ref().get(name_b),
                ) {
                    let a = dt_a.clone();
                    let b = dt_b.clone();
                    for dte in dep_checker.check_dep_type_eq(&a, &b, &decl.span) {
                        errors.push(TypeError {
                            code: dte.code,
                            message: dte.message,
                            span: dte.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
    }

    errors
}

/// Check information flow for a contract declaration.
///
/// Scans input clauses for security label annotations (e.g., `secret`,
/// `confidential` in the type annotation). If any input is labeled secret,
/// ensures clauses are checked for flows to public outputs.
fn check_contract_info_flow(
    contract: &assura_parser::ast::ContractDecl,
    span: &Range<usize>,
) -> Vec<TypeError> {
    let mut checker = InfoFlowChecker::new();

    // Scan input clause params for security annotations
    for clause in &contract.clauses {
        if clause.kind == ClauseKind::Input {
            let mut _has = false;
            assign_labels_from_clause(&clause.body, &mut checker, &mut _has);
        }
        // Register purpose labels from "purpose" annotations
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "purpose"
            && let Expr::Raw(tokens) = &clause.body
            && tokens.len() >= 2
        {
            checker.declare_purpose(tokens[0].clone(), tokens[1].clone());
        }
        // Register declassify annotations
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "declassify"
        {
            let refs = collect_ident_references(&clause.body);
            for name in refs {
                checker.mark_declassify(name);
            }
        }
        // Register timing-sensitive functions
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "timing_sensitive"
        {
            let refs = collect_ident_references(&clause.body);
            for name in refs {
                checker.register_timing_sensitive(name);
            }
        }
    }

    // Only check if at least one parameter has a security label
    if !checker.has_labels() {
        return Vec::new();
    }

    let mut errors = Vec::new();

    // Check ensures clauses for information flow violations using the checker's
    // built-in expression walker (handles implicit flows and covert channels)
    for clause in &contract.clauses {
        if clause.kind == ClauseKind::Ensures {
            for err in checker.check_expr(&clause.body, span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
            // Also run the legacy per-expression check
            check_expr_info_flow(&clause.body, &checker, span, &mut errors);
        }
        // Check declassification annotations
        if clause.kind == ClauseKind::Ensures || clause.kind == ClauseKind::Requires {
            // Check for implicit declassification in assignments
            let refs = collect_ident_references(&clause.body);
            for name in &refs {
                if let Some(label) = checker.get_label(name) {
                    // Check covert channel through timing functions in ensures
                    if let Some(err) = checker.check_covert_channel(label, name, span) {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                    // Check declassification
                    if let Some(err) =
                        checker.check_declassify(label, SecurityLabel::Public, false, span)
                    {
                        errors.push(TypeError {
                            code: err.code,
                            message: err.message,
                            span: err.span,
                            secondary: None,
                        });
                    }
                }
            }
        }
        // Use get_label and get_purpose for purpose-label mismatches
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "purpose_check"
            && let Expr::Raw(tokens) = &clause.body
            && tokens.len() >= 2
        {
            let var_name = &tokens[0];
            let required_purpose = &tokens[1];
            if checker.get_label(var_name).is_some()
                && let Some(err) = checker.check_purpose_label(var_name, required_purpose, span)
            {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
            // Also validate against registered purpose
            if let Some(purpose) = checker.get_purpose(var_name)
                && purpose != required_purpose.as_str()
            {
                errors.push(TypeError {
                    code: "A08003".into(),
                    message: format!(
                        "purpose mismatch for `{var_name}`: registered as `{purpose}`, \
                             required `{required_purpose}`"
                    ),
                    span: span.clone(),
                    secondary: None,
                });
            }
        }
    }

    errors
}

/// Check information flow for a function definition.
fn check_fn_info_flow(fn_def: &assura_parser::ast::FnDef, span: &Range<usize>) -> Vec<TypeError> {
    let mut checker = InfoFlowChecker::new();

    // Scan clause params for security annotations
    for clause in &fn_def.clauses {
        if clause.kind == ClauseKind::Input {
            let mut _has = false;
            assign_labels_from_clause(&clause.body, &mut checker, &mut _has);
        }
        // Register purpose, declassify, and timing-sensitive annotations
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "purpose"
            && let Expr::Raw(tokens) = &clause.body
            && tokens.len() >= 2
        {
            checker.declare_purpose(tokens[0].clone(), tokens[1].clone());
        }
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "declassify"
        {
            for name in collect_ident_references(&clause.body) {
                checker.mark_declassify(name);
            }
        }
        if let ClauseKind::Other(ref k) = clause.kind
            && k == "timing_sensitive"
        {
            for name in collect_ident_references(&clause.body) {
                checker.register_timing_sensitive(name);
            }
        }
    }

    // Also check function params for label annotations in type names
    for param in &fn_def.params {
        let label = infer_label_from_type_tokens(&param.ty);
        if label > SecurityLabel::Public {
            checker.declare(param.name.clone(), label);
        }
    }

    if !checker.has_labels() {
        return Vec::new();
    }

    let mut errors = Vec::new();

    for clause in &fn_def.clauses {
        if clause.kind == ClauseKind::Ensures {
            // Use the checker's built-in expression walker
            for err in checker.check_expr(&clause.body, span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
            check_expr_info_flow(&clause.body, &checker, span, &mut errors);
        }
    }

    errors
}

/// Assign security labels from an input clause body.
///
/// Looks for patterns like `secret key: Bytes`, `confidential password: String`
/// where the security label is a keyword before the parameter name.
fn assign_labels_from_clause(expr: &Expr, checker: &mut InfoFlowChecker, has_any: &mut bool) {
    match expr {
        Expr::Raw(tokens) => {
            // Scan for label keywords followed by a param name
            let mut i = 0;
            while i < tokens.len() {
                let label = match tokens[i].as_str() {
                    "secret" | "restricted" => Some(SecurityLabel::Restricted),
                    "confidential" => Some(SecurityLabel::Confidential),
                    "internal" => Some(SecurityLabel::Internal),
                    "public" => Some(SecurityLabel::Public),
                    _ => None,
                };
                if let Some(label) = label
                    && label > SecurityLabel::Public
                    && let Some(name) = tokens.get(i + 1)
                    && name != ":"
                {
                    checker.declare(name.clone(), label);
                    *has_any = true;
                }
                i += 1;
            }
        }
        Expr::Block(items) => {
            for item in items {
                assign_labels_from_clause(item, checker, has_any);
            }
        }
        Expr::Call { args, .. } => {
            for arg in args {
                assign_labels_from_clause(arg, checker, has_any);
            }
        }
        _ => {}
    }
}

/// Infer a security label from type annotation tokens.
///
/// If the type annotation contains `secret`, `confidential`, or `internal`
/// as a modifier, returns the corresponding label.
fn infer_label_from_type_tokens(tokens: &[String]) -> SecurityLabel {
    for tok in tokens {
        match tok.as_str() {
            "secret" | "restricted" => return SecurityLabel::Restricted,
            "confidential" => return SecurityLabel::Confidential,
            "internal" => return SecurityLabel::Internal,
            _ => {}
        }
    }
    SecurityLabel::Public
}

/// Check an expression for information flow violations.
///
/// If a sub-expression has a high security label and it contributes to
/// a value that should be public (e.g., the `result` variable in an ensures
/// clause), report A08001.
fn check_expr_info_flow(
    expr: &Expr,
    checker: &InfoFlowChecker,
    span: &Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    // Check if `result` is being assigned a value derived from secret data
    if let Expr::BinOp {
        lhs,
        rhs,
        op: BinOp::Eq,
        ..
    } = expr
    {
        // Pattern: result == expr or expr == result
        let (target, source) = if is_result_expr(lhs) {
            ("result", rhs.as_ref())
        } else if is_result_expr(rhs) {
            ("result", lhs.as_ref())
        } else {
            return;
        };

        let source_label = checker.infer_label(source);
        if source_label > SecurityLabel::Public
            && let Some(err) = checker.check_assignment(SecurityLabel::Public, source_label, span)
        {
            errors.push(TypeError {
                code: err.code,
                message: format!("information flow violation in `{target}`: {}", err.message),
                span: err.span,
                secondary: None,
            });
        }
    }

    // Check for implicit flows through if conditions
    if let Expr::If {
        cond, then_branch, ..
    } = expr
    {
        let cond_label = checker.infer_label(cond);
        if cond_label > SecurityLabel::Public {
            // Check if the branch body assigns to result or a public variable
            let branch_label = infer_branch_target_label(then_branch, checker);
            if let Some(err) = checker.check_implicit_flow(cond_label, branch_label, span) {
                errors.push(TypeError {
                    code: err.code,
                    message: err.message,
                    span: err.span,
                    secondary: None,
                });
            }
        }
    }
}

/// Check if an expression is `result` (the return value variable).
fn is_result_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(name) if name == "result")
}

/// Infer the security label of a branch target.
///
/// If the branch references `result`, the target is Public (since result
/// flows out). Otherwise, use the checker's label inference.
fn infer_branch_target_label(expr: &Expr, checker: &InfoFlowChecker) -> SecurityLabel {
    // If the branch affects `result`, the target is public
    if contains_result_ref(expr) {
        SecurityLabel::Public
    } else {
        checker.infer_label(expr)
    }
}

/// Check if an expression tree contains a reference to `result`.
fn contains_result_ref(expr: &Expr) -> bool {
    match expr {
        Expr::Ident(name) => name == "result",
        Expr::BinOp { lhs, rhs, .. } => contains_result_ref(lhs) || contains_result_ref(rhs),
        Expr::Field(inner, _) | Expr::Old(inner) | Expr::Paren(inner) => contains_result_ref(inner),
        Expr::Call { func, args } => {
            contains_result_ref(func) || args.iter().any(contains_result_ref)
        }
        Expr::MethodCall { receiver, args, .. } => {
            contains_result_ref(receiver) || args.iter().any(contains_result_ref)
        }
        Expr::If {
            cond,
            then_branch,
            else_branch,
        } => {
            contains_result_ref(cond)
                || contains_result_ref(then_branch)
                || else_branch.as_ref().is_some_and(|e| contains_result_ref(e))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    #[test]
    fn taint_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_taint_checks(&sf).is_empty());
    }

    #[test]
    fn info_flow_no_annotation_no_errors() {
        let sf = parse_source(r#"contract Simple { requires { true } }"#);
        assert!(run_info_flow_checks(&sf).is_empty());
    }
}
