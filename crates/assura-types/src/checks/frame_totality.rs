//! Frame and totality checks.

use assura_parser::ast::{ClauseKind, Decl, SpExpr};

use crate::checkers::*;
use crate::{TypeEnv, TypeError};

// ---------------------------------------------------------------------------
// Frame checking wiring (T045)
// ---------------------------------------------------------------------------

/// T045: Validate modifies clause structure.
///
/// The FrameChecker's scope validation (check_scope) is deferred until
/// expression-level name resolution is implemented, as the current type
/// environment doesn't contain local variables or clause-declared params,
/// causing false positives on valid code. The FrameChecker is already
/// used by the SMT crate's verify_clauses() for frame axiom injection,
/// which is its primary purpose.
pub(crate) fn run_frame_checks(
    source: &assura_parser::ast::SourceFile,
    _type_env: &TypeEnv,
    _symbols: &assura_resolve::SymbolTable,
) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        let clauses = decl.node.clauses();
        if clauses.is_empty() {
            continue;
        }
        let modifies_bodies: Vec<&SpExpr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Modifies)
            .map(|c| &c.body)
            .collect();
        if modifies_bodies.is_empty() {
            continue;
        }
        let checker = FrameChecker::new(&modifies_bodies);
        // Validate that modifies clauses are non-empty (structural check)
        if checker.modified_set().is_empty() && !modifies_bodies.is_empty() {
            errors.push(TypeError {
                code: "A14001".into(),
                message: "empty modifies clause; list the variables this function may change"
                    .into(),
                span: decl.span.clone(),
                secondary: None,
            });
        }
        // A14002: Check ensures clauses for implicit modifications to
        // variables not in the modifies set. Frame equality patterns
        // (x == old(x), old(x) == x) are excluded as frame assertions.
        let ensures_bodies: Vec<&SpExpr> = clauses
            .iter()
            .filter(|c| c.kind == ClauseKind::Ensures)
            .map(|c| &c.body)
            .collect();
        for ensures_body in &ensures_bodies {
            errors.extend(checker.check_ensures_modifications(ensures_body, &decl.span));
        }
    }
    errors
}

// ---------------------------------------------------------------------------
// Totality checking wiring (T053)
// ---------------------------------------------------------------------------

/// T053: Check termination of recursive functions via decreases measures.
///
/// Returns syntactically detected errors and pending SMT checks for cases
/// where the syntactic checker is inconclusive. The caller (CLI pipeline)
/// dispatches pending checks to assura-smt.
pub(crate) fn run_totality_checks(
    source: &assura_parser::ast::SourceFile,
) -> (Vec<TypeError>, Vec<PendingDecreaseCheck>) {
    let mut checker = TotalityChecker::new();
    let mut errors = Vec::new();
    let mut pending_smt = Vec::new();

    // Pre-register functions annotated as partial
    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node
            && f.clauses
                .iter()
                .any(|c| matches!(&c.kind, ClauseKind::Other(s) if s == "partial"))
        {
            checker.mark_partial(f.name.clone());
        }
    }

    // Collect all function definitions for mutual recursion checking
    let mut fn_defs: Vec<(&assura_parser::ast::FnDef, &std::ops::Range<usize>)> = Vec::new();

    for decl in &source.decls {
        if let Decl::FnDef(f) = &decl.node {
            fn_defs.push((f, &decl.span));
            let (te_errors, te_pending) = checker.check_function_totality(f, &decl.span);
            for te in te_errors {
                errors.push(TypeError {
                    code: te.code,
                    message: te.message,
                    span: te.span,
                    secondary: None,
                });
            }
            pending_smt.extend(te_pending);
        }
    }

    // Check for mutual recursion across all function pairs
    if fn_defs.len() >= 2 {
        for te in checker.check_mutual_recursion(&fn_defs) {
            errors.push(TypeError {
                code: te.code,
                message: te.message,
                span: te.span,
                secondary: None,
            });
        }
    }

    (errors, pending_smt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_source(src: &str) -> assura_parser::ast::SourceFile {
        let (sf, errs) = assura_parser::parse(src);
        assert!(errs.is_empty(), "parse errors: {errs:?}");
        sf.unwrap()
    }

    // --- Frame checks ---

    #[test]
    fn frame_no_modifies_no_errors() {
        let sf = parse_source("contract Simple { requires { true } }");
        let env = TypeEnv::new();
        let r = assura_resolve::resolve(&sf).unwrap();
        assert!(run_frame_checks(&r.source, &env, &r.symbols).is_empty());
    }

    #[test]
    fn frame_modifies_with_variable_no_errors() {
        let sf = parse_source("contract C { modifies { x } }");
        let env = TypeEnv::new();
        let r = assura_resolve::resolve(&sf).unwrap();
        assert!(run_frame_checks(&r.source, &env, &r.symbols).is_empty());
    }

    // --- A14002: ensures modification detection ---

    #[test]
    fn frame_a14002_frame_assertion_no_error() {
        // ensures { y == old(y) } with modifies { x } is a frame assertion, NOT A14002
        let sf = parse_source("contract C {\n    modifies { x }\n    ensures { y == old(y) }\n}");
        let env = TypeEnv::new();
        let r = assura_resolve::resolve(&sf).unwrap();
        let errs = run_frame_checks(&r.source, &env, &r.symbols);
        assert!(
            !errs.iter().any(|e| e.code.as_ref() == "A14002"),
            "frame assertion y == old(y) should not trigger A14002: {errs:?}"
        );
    }

    #[test]
    fn frame_a14002_modification_detected() {
        // ensures { y > old(y) } with modifies { x } implies y is modified
        // but y is not in modifies set => A14002
        let sf = parse_source("contract C {\n    modifies { x }\n    ensures { y > old(y) }\n}");
        let env = TypeEnv::new();
        let r = assura_resolve::resolve(&sf).unwrap();
        let errs = run_frame_checks(&r.source, &env, &r.symbols);
        assert!(
            errs.iter().any(|e| e.code.as_ref() == "A14002"),
            "y > old(y) with modifies {{ x }} should trigger A14002: {errs:?}"
        );
    }

    #[test]
    fn frame_a14002_modified_var_no_error() {
        // ensures { x > old(x) } with modifies { x } is fine: x IS modified
        let sf = parse_source("contract C {\n    modifies { x }\n    ensures { x > old(x) }\n}");
        let env = TypeEnv::new();
        let r = assura_resolve::resolve(&sf).unwrap();
        let errs = run_frame_checks(&r.source, &env, &r.symbols);
        assert!(
            !errs.iter().any(|e| e.code.as_ref() == "A14002"),
            "x > old(x) with modifies {{ x }} should not trigger A14002: {errs:?}"
        );
    }

    // --- Totality checks ---

    #[test]
    fn totality_non_recursive_no_errors() {
        let sf = parse_source("fn add(a: Int, b: Int) -> Int\n    requires { a > 0 }");
        let (errs, _pending) = run_totality_checks(&sf);
        assert!(
            errs.is_empty(),
            "non-recursive fn should have no totality errors: {errs:?}"
        );
    }

    #[test]
    fn totality_partial_function_no_errors() {
        let src = "fn diverge(x: Int) -> Int\n    partial\n    requires { x > 0 }";
        let sf = parse_source(src);
        let (errs, _pending) = run_totality_checks(&sf);
        assert!(
            errs.is_empty(),
            "partial fn should not trigger totality error: {errs:?}"
        );
    }
}
