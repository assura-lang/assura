//! FFI and error propagation checks.

use assura_parser::ast::{ClauseKind, Decl, Expr};

use crate::TypeError;
use crate::checkers::*;

pub(crate) fn run_ffi_checks(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
    let mut checker = FfiBoundaryChecker::new();
    let mut externs = Vec::new();
    let mut has_any_boundary = false;

    for decl in &source.decls {
        if let Decl::Extern(e) = &decl.node {
            let has_boundary = e.clauses.iter().any(
                |c| matches!(c.kind, ClauseKind::Other(ref k) if k == "trust" || k == "boundary"),
            );
            if has_boundary {
                has_any_boundary = true;
            }
            let has_contract = !e.clauses.is_empty();
            externs.push((
                e.name.clone(),
                has_boundary,
                has_contract,
                decl.span.clone(),
            ));

            // Register extern with trust boundary classification
            let boundary = if e.clauses.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "trust")
                    && matches!(&c.body.node, Expr::Ident(v) if v == "trusted")
            }) {
                TrustBoundary::Trusted
            } else if e.clauses.iter().any(|c| {
                matches!(&c.kind, ClauseKind::Other(k) if k == "trust")
                    && matches!(&c.body.node, Expr::Ident(v) if v == "audited")
            }) {
                TrustBoundary::Audited
            } else {
                TrustBoundary::Untrusted
            };
            checker.register_extern(e.name.clone(), boundary);

            // Mark externs with requires/ensures as contracted
            let has_requires = e.clauses.iter().any(|c| c.kind == ClauseKind::Requires);
            let has_ensures = e.clauses.iter().any(|c| c.kind == ClauseKind::Ensures);
            if has_requires || has_ensures {
                checker.mark_contracted(e.name.clone());
            }
        }
    }

    // Only check if at least one extern uses boundary annotations
    if !has_any_boundary {
        return Vec::new();
    }

    let mut errors: Vec<TypeError> = checker
        .check_file(&externs)
        .into_iter()
        .map(|fe| TypeError {
            code: fe.code,
            message: fe.message,
            span: fe.span,
            secondary: None,
        })
        .collect();

    // Additional check: externs calling into unsafe without any contract clauses
    for decl in &source.decls {
        if let Decl::Extern(e) = &decl.node {
            let has_requires = e.clauses.iter().any(|c| c.kind == ClauseKind::Requires);
            let has_ensures = e.clauses.iter().any(|c| c.kind == ClauseKind::Ensures);
            // Externs with boundary annotations but no requires/ensures
            let has_boundary = e.clauses.iter().any(
                |c| matches!(c.kind, ClauseKind::Other(ref k) if k == "trust" || k == "boundary"),
            );
            if has_boundary && !has_requires && !has_ensures {
                errors.push(TypeError {
                    code: "A11005".into(),
                    message: format!(
                        "extern `{}` has trust boundary but no requires/ensures contracts; \
                         add preconditions and postconditions to validate the trust boundary",
                        e.name
                    ),
                    span: decl.span.clone(),
                    secondary: None,
                });
            }

            // Check unsafe confinement: functions with "unsafe" annotation
            let has_unsafe_ann = e
                .clauses
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "unsafe"));
            let is_ffi_wrapper = has_boundary;
            for err in checker.check_unsafe_confinement(
                &e.name,
                is_ffi_wrapper,
                has_unsafe_ann,
                &decl.span,
            ) {
                errors.push(err.into());
            }
        }
    }

    // Check FFI call sites in function/contract clause bodies
    for decl in &source.decls {
        let clauses = match &decl.node {
            Decl::FnDef(f) => f.clauses.as_slice(),
            Decl::Contract(c) => c.clauses.as_slice(),
            _ => continue,
        };
        for clause in clauses {
            if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires) {
                let refs = collect_ident_references(&clause.body);
                for callee in &refs {
                    // A reference to the callee in an ensures clause suggests
                    // the result is being validated (used in a postcondition).
                    let result_validated = clauses.iter().any(|c| {
                        c.kind == ClauseKind::Ensures && expr_references_var(&c.body, callee)
                    });
                    for err in checker.check_ffi_call(callee, result_validated, &decl.span) {
                        errors.push(err.into());
                    }
                }
            }
        }
    }

    errors
}

/// T064: Run error propagation checks on functions that return error types.
pub(crate) fn run_error_propagation_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = ErrorPropagationChecker::new();
    let mut errors = Vec::new();

    // Pass 1: discover error policies from contracts with must_propagate annotations
    for decl in &source.decls {
        if let Decl::Contract(c) = &decl.node {
            let mut policy = ErrorPolicy::default();
            for clause in &c.clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_propagate"
                {
                    match &clause.body.node {
                        Expr::Raw(tokens) => policy.must_propagate.extend(tokens.iter().cloned()),
                        Expr::Ident(name) => policy.must_propagate.push(name.clone()),
                        _ => {}
                    }
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_check"
                    && let Expr::Raw(tokens) = &clause.body.node
                {
                    policy.must_check.extend(tokens.iter().cloned());
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_not_mask"
                    && let Expr::Raw(tokens) = &clause.body.node
                    && tokens.len() >= 2
                {
                    policy
                        .must_not_mask
                        .push((tokens[0].clone(), tokens[1].clone()));
                }
                if clause.kind == ClauseKind::MustNot
                    && let Expr::Raw(tokens) = &clause.body.node
                    && tokens.len() >= 2
                {
                    policy
                        .must_not_mask
                        .push((tokens[0].clone(), tokens[1].clone()));
                }
                if let ClauseKind::Other(ref k) = clause.kind
                    && k == "must_preserve_detail"
                    && let Expr::Raw(tokens) = &clause.body.node
                {
                    policy.must_preserve_detail.extend(tokens.iter().cloned());
                }
            }
            if !policy.must_propagate.is_empty()
                || !policy.must_check.is_empty()
                || !policy.must_not_mask.is_empty()
                || !policy.must_preserve_detail.is_empty()
            {
                checker.register_policy(c.name.clone(), policy);
            }
        }
    }

    // Pass 2: check functions that catch errors for propagation violations
    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node {
            // Check if return type is an error type
            let rt_tokens = f
                .return_ty
                .as_ref()
                .map(|t| t.to_tokens())
                .unwrap_or_default();
            let returns_error = rt_tokens.iter().any(|t| t == "Result" || t == "Error");
            if returns_error {
                for clause in &f.clauses {
                    if clause.kind == ClauseKind::Errors
                        && let Expr::Raw(tokens) = &clause.body.node
                    {
                        for error_code in tokens {
                            if checker.is_must_propagate(error_code) {
                                errors.push(TypeError {
                                    code: "A64001".into(),
                                    message: format!(
                                        "error code `{error_code}` in function `{}` must be \
                                         propagated, not caught",
                                        f.name
                                    ),
                                    span: decl.span.clone(),
                                    secondary: None,
                                });
                            }
                        }
                    }

                    // Check "catch" clauses for error action violations
                    if let ClauseKind::Other(ref k) = clause.kind
                        && k == "catch"
                        && let Expr::Raw(tokens) = &clause.body.node
                    {
                        let error_code = tokens.first().cloned().unwrap_or_default();
                        let action_kw = tokens.get(1).map(|s| s.as_str()).unwrap_or("");
                        let action = match action_kw {
                            "swallow" | "ignore" => ErrorAction::Swallow,
                            "translate" | "translate_to" => {
                                let target = tokens.get(2).cloned().unwrap_or_default();
                                ErrorAction::TranslateTo(target)
                            }
                            "propagate" | "rethrow" => ErrorAction::Propagate,
                            _ => ErrorAction::Handle,
                        };
                        if let Some(te) =
                            checker.validate_catch(&error_code, action, decl.span.clone())
                        {
                            errors.push(TypeError {
                                code: te.code,
                                message: te.message,
                                span: te.span,
                                secondary: None,
                            });
                        }
                    }

                    // Check function calls in ensures/requires for unchecked returns
                    if matches!(clause.kind, ClauseKind::Ensures | ClauseKind::Requires) {
                        let refs = collect_ident_references(&clause.body);
                        for fn_ref in &refs {
                            if let Some(te) =
                                checker.validate_unchecked_call(fn_ref, decl.span.clone())
                            {
                                errors.push(TypeError {
                                    code: te.code,
                                    message: te.message,
                                    span: te.span,
                                    secondary: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    // --- FFI boundary checks ---

    #[test]
    fn ffi_no_externs_no_errors() {
        let sf = parse_source("contract Simple { requires { true } }");
        assert!(run_ffi_checks(&sf).is_empty());
    }

    #[test]
    fn ffi_extern_without_boundary_no_errors() {
        let sf = parse_source("extern fn malloc(size: Nat) -> Nat");
        assert!(run_ffi_checks(&sf).is_empty());
    }

    #[test]
    fn ffi_extern_boundary_without_contract_emits_a11005() {
        let sf = parse_source("extern fn malloc(size: Nat) -> Nat\n    boundary untrusted");
        let errs = run_ffi_checks(&sf);
        assert!(
            errs.iter().any(|e| e.code == "A11005"),
            "expected A11005: {errs:?}"
        );
    }

    #[test]
    fn ffi_extern_with_boundary_and_requires_no_a11005() {
        let src = "extern fn malloc(size: Nat) -> Nat\n    \
                   boundary untrusted\n    requires { size > 0 }";
        let sf = parse_source(src);
        let errs = run_ffi_checks(&sf);
        assert!(
            !errs.iter().any(|e| e.code == "A11005"),
            "should not emit A11005 when extern has requires: {errs:?}"
        );
    }

    // --- Error propagation checks ---

    #[test]
    fn error_propagation_no_annotations_no_errors() {
        let sf = parse_source("contract Simple { requires { true } }");
        assert!(run_error_propagation_checks(&sf).is_empty());
    }

    #[test]
    fn error_propagation_fn_without_result_return_no_errors() {
        let src = "fn handler(x: Int) -> Int\n    requires { x > 0 }";
        let sf = parse_source(src);
        assert!(
            run_error_propagation_checks(&sf).is_empty(),
            "fn without Result return should not trigger error propagation checks"
        );
    }
}
