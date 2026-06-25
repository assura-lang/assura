//! Numeric, fixed-width, and collection checks.

use assura_parser::ast::{BinOp, ClauseKind, Decl, Expr, ExprVisitor, SpExpr};

use crate::checkers::*;
use crate::domain::*;
use crate::types::*;
use crate::{Type, TypeEnv, TypeError};

// ---------------------------------------------------------------------------
// Fixed-width integer checking wiring (T055)
// ---------------------------------------------------------------------------

/// T055: Detect potential integer overflow in fixed-width arithmetic.
pub(crate) fn run_fixed_width_checks(
    source: &assura_parser::ast::SourceFile,
    type_env: &TypeEnv,
) -> Vec<TypeError> {
    let mut errors = Vec::new();
    for decl in &source.decls {
        let (params, clauses): (&[assura_parser::ast::Param], &[_]) = match &decl.node {
            Decl::Contract(c) => (&[], c.clauses.as_slice()),
            Decl::FnDef(f) => (f.params.as_slice(), f.clauses.as_slice()),
            Decl::Extern(e) => (e.params.as_slice(), e.clauses.as_slice()),
            _ => continue,
        };

        // Build a per-decl checker with declared fixed-width bindings
        let mut fw_checker = FixedWidthChecker::new();
        for param in params {
            let ty_str = param.ty.as_ref().map(|t| t.to_string()).unwrap_or_default();
            if let Some(fw_ty) = token_to_fixed_width_type(&ty_str) {
                fw_checker.declare(param.name.clone(), fw_ty);
            }
        }

        for clause in clauses {
            check_expr_fixed_width_full(
                &clause.body,
                type_env,
                &fw_checker,
                &decl.span,
                &mut errors,
            );
        }
    }
    errors
}

/// Check an expression using the full FixedWidthChecker (with bindings).
///
/// Calls `check_binop` and `check_division_by_zero` in addition to the
/// individual overflow/signedness/cast checks.
fn check_expr_fixed_width_full(
    expr: &SpExpr,
    type_env: &TypeEnv,
    fw_checker: &FixedWidthChecker,
    span: &std::ops::Range<usize>,
    errors: &mut Vec<TypeError>,
) {
    struct FixedWidthVisitor<'a> {
        type_env: &'a TypeEnv,
        fw_checker: &'a FixedWidthChecker,
        span: &'a std::ops::Range<usize>,
        errors: &'a mut Vec<TypeError>,
    }

    impl ExprVisitor for FixedWidthVisitor<'_> {
        fn visit_binop(&mut self, lhs: &SpExpr, op: &BinOp, rhs: &SpExpr) {
            // Recurse into sub-expressions first (default traversal)
            self.visit_expr(lhs);
            self.visit_expr(rhs);

            if let Some(left_type) =
                infer_fixed_width_type_ext(lhs, self.type_env, self.fw_checker)
                && let Some(right_type) =
                    infer_fixed_width_type_ext(rhs, self.type_env, self.fw_checker)
            {
                // Warn when mixing unsigned and signed in arithmetic (not just comparison)
                if op.is_arithmetic()
                    && !op.is_division_like()
                    && FixedWidthChecker::is_unsigned(&left_type)
                        != FixedWidthChecker::is_unsigned(&right_type)
                    && FixedWidthChecker::is_fixed_width(&left_type)
                    && FixedWidthChecker::is_fixed_width(&right_type)
                {
                    // already covered by check_binop's signedness check
                }
                // Use check_binop for combined overflow + signedness + div-by-zero
                for fwe in
                    self.fw_checker
                        .check_binop(op, &left_type, &right_type, rhs, self.span)
                {
                    self.errors.push(TypeError {
                        code: fwe.code,
                        message: fwe.message,
                        span: fwe.span,
                        secondary: None,
                    });
                }
            } else if let Some(left_type) =
                infer_fixed_width_type_ext(lhs, self.type_env, self.fw_checker)
            {
                // Even without right type, check division by zero
                if let Some(fwe) =
                    FixedWidthChecker::check_division_by_zero(op, rhs, &left_type, self.span)
                {
                    self.errors.push(TypeError {
                        code: fwe.code,
                        message: fwe.message,
                        span: fwe.span,
                        secondary: None,
                    });
                }
            }
        }

        fn visit_cast(&mut self, inner: &SpExpr, ty: &str) {
            // Recurse into the inner expression first (default traversal)
            self.visit_expr(inner);

            if let Some(from_type) =
                infer_fixed_width_type_ext(inner, self.type_env, self.fw_checker)
                && let Some(to_ty) = token_to_fixed_width_type(ty)
                && let Some(fwe) =
                    FixedWidthChecker::check_cast_safety(&from_type, &to_ty, self.span)
            {
                self.errors.push(TypeError {
                    code: fwe.code,
                    message: fwe.message,
                    span: fwe.span,
                    secondary: None,
                });
            }
        }
    }

    let mut visitor = FixedWidthVisitor {
        type_env,
        fw_checker,
        span,
        errors,
    };
    visitor.visit_expr(expr);
}

/// Infer fixed-width type using both type env and the checker's bindings.
fn infer_fixed_width_type_ext(
    expr: &SpExpr,
    type_env: &TypeEnv,
    fw_checker: &FixedWidthChecker,
) -> Option<Type> {
    match &expr.node {
        Expr::Ident(name) => {
            // Check checker bindings first, then type env
            if let Some(ty) = fw_checker.get_type(name)
                && FixedWidthChecker::is_fixed_width(ty)
            {
                return Some(ty.clone());
            }
            if let Some(ty) = type_env.lookup(name)
                && FixedWidthChecker::is_fixed_width(ty)
            {
                return Some(ty.clone());
            }
            None
        }
        Expr::Cast { ty, .. } => token_to_fixed_width_type(ty),
        _ => None,
    }
}

/// Convert a type name token to a fixed-width Type.
fn token_to_fixed_width_type(ty: &str) -> Option<Type> {
    match ty {
        "U8" | "u8" => Some(Type::U8),
        "U16" | "u16" => Some(Type::U16),
        "U32" | "u32" => Some(Type::U32),
        "U64" | "u64" => Some(Type::U64),
        "I8" | "i8" => Some(Type::I8),
        "I16" | "i16" => Some(Type::I16),
        "I32" | "i32" => Some(Type::I32),
        "I64" | "i64" => Some(Type::I64),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Collection contract checks (T108)
// ---------------------------------------------------------------------------

/// Validate that contracts referencing standard collection operations
/// (sort, filter, map, reverse, deduplicate) declare postconditions
/// consistent with the operation's semantics.
pub(crate) fn run_collection_contract_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let cc = CollectionContracts::new();
    let mut errors = Vec::new();

    for decl in &source.decls {
        let (name, clauses) = match &decl.node {
            Decl::Contract(c) => (&c.name, &c.clauses),
            Decl::FnDef(f) => (&f.name, &f.clauses),
            _ => continue,
        };

        // Check if the contract/function name matches a known collection op
        let lower_name = name.to_lowercase();
        if let Some(cc_def) = cc.lookup(&lower_name) {
            // Verify length-preserving operations declare it
            if cc_def.preserves_length {
                let has_length_postcondition = clauses
                    .iter()
                    .any(|c| c.kind == ClauseKind::Ensures && expr_mentions_len(&c.body));
                if !has_length_postcondition {
                    errors.push(TypeError {
                        code: "A03007".into(),
                        message: format!(
                            "collection operation `{name}` preserves length; \
                             consider adding a `len(result) == len(input)` postcondition"
                        ),
                        span: decl.span.clone(),
                        secondary: None,
                    });
                }
            }
        }
    }

    errors
}

/// Check if an expression mentions `len` (used by collection contract checks).
fn expr_mentions_len(expr: &SpExpr) -> bool {
    struct LenMentionVisitor {
        found: bool,
    }

    impl ExprVisitor for LenMentionVisitor {
        fn visit_expr(&mut self, expr: &SpExpr) {
            if !self.found {
                assura_parser::ast::walk_expr(self, expr);
            }
        }

        fn visit_ident(&mut self, name: &str) {
            if name == "len" {
                self.found = true;
            }
        }

        fn visit_raw(&mut self, tokens: &[String]) {
            if tokens.iter().any(|t| t == "len") {
                self.found = true;
            }
        }
    }

    let mut visitor = LenMentionVisitor { found: false };
    visitor.visit_expr(expr);
    visitor.found
}

pub(crate) fn run_numerical_precision_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = NumericalPrecisionChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "precision" || k == "numerical_precision" || k == "ulp_bound")
            {
                found = true;
                // Extract precision params from call syntax: precision(name, bits, ulp)
                match &clause.body.node {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = &func.as_ref().node {
                            let bits = args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_BIT_CONTAINER_BITS)
                                as u32;
                            let ulp = args
                                .get(1)
                                .and_then(extract_float_literal)
                                .unwrap_or(DEFAULT_ULP_TOLERANCE);
                            checker.declare(name.clone(), bits, ulp, decl.span.clone());
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare(
                            name.clone(),
                            DEFAULT_BIT_CONTAINER_BITS as u32,
                            1.0,
                            decl.span.clone(),
                        );
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "var")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let bits = kvs
                            .iter()
                            .find(|(k, _)| *k == "bits")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(DEFAULT_BIT_CONTAINER_BITS)
                            as u32;
                        let ulp = kvs
                            .iter()
                            .find(|(k, _)| *k == "ulp")
                            .and_then(|(_, v)| extract_float_literal(v))
                            .unwrap_or(DEFAULT_ULP_TOLERANCE);
                        checker.declare(name, bits, ulp, decl.span.clone());
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Check precision in ensures clauses for referenced variables
    let mut errors = Vec::new();
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if matches!(
                clause.kind,
                ClauseKind::Ensures | ClauseKind::Requires | ClauseKind::Invariant
            ) {
                let refs = collect_ident_references(&clause.body);
                for name in &refs {
                    // Detect narrowing operations (e.g., 64-bit -> 32-bit)
                    if clause_contains_cast(&clause.body, name) {
                        let target_bits = extract_cast_target_bits(&clause.body, name);
                        if let Some(err) = checker.check_precision_loss(name, target_bits) {
                            errors.push(err);
                        }
                    }
                    // Check ULP bound violations
                    if let Some(err) = checker.check_ulp_bound(name, 2.0)
                        && clause_contains_cast(&clause.body, name)
                    {
                        errors.push(err);
                    }
                    // Check catastrophic cancellation
                    if let Some(err) = checker.check_cancellation(name, 0.9999) {
                        errors.push(err);
                    }
                }
            }
        }
    }
    errors
}

/// Extract the target bit width from a type name (e.g., "f32" -> 32, "f16" -> 16, "Int" -> 64).
pub(crate) fn type_name_to_bits(ty: &str) -> u32 {
    match ty {
        "f16" | "Float16" => 16,
        "f32" | "Float32" => 32,
        "f64" | "Float64" | "Float" => 64,
        "i8" | "u8" | "Int8" | "Nat8" => 8,
        "i16" | "u16" | "Int16" | "Nat16" => 16,
        "i32" | "u32" | "Int32" | "Nat32" => 32,
        "i64" | "u64" | "Int" | "Nat" | "Int64" | "Nat64" => 64,
        _ => 32, // conservative default for unknown types
    }
}

/// Extract the target bit width from a cast expression involving a variable.
/// Returns the narrowest cast target found, or 32 as a default.
fn extract_cast_target_bits(expr: &SpExpr, var_name: &str) -> u32 {
    match &expr.node {
        Expr::Cast { expr: inner, ty } => {
            if let Expr::Ident(name) = &inner.node
                && name == var_name
            {
                return type_name_to_bits(ty);
            }
            extract_cast_target_bits(inner, var_name)
        }
        Expr::Call { func, args } => {
            if let Expr::Ident(fn_name) = &func.as_ref().node
                && (fn_name == "as_f32"
                    || fn_name == "as_f16"
                    || fn_name == "narrow"
                    || fn_name == "truncate")
                && args
                    .iter()
                    .any(|a| matches!(&a.node, Expr::Ident(n) if n == var_name))
            {
                return match fn_name.as_str() {
                    "as_f16" => 16,
                    "as_f32" => 32,
                    _ => 32,
                };
            }
            args.iter()
                .map(|a| extract_cast_target_bits(a, var_name))
                .min()
                .unwrap_or(DEFAULT_HASH_BITS as u32)
        }
        Expr::BinOp { lhs, rhs, .. } => {
            extract_cast_target_bits(lhs, var_name).min(extract_cast_target_bits(rhs, var_name))
        }
        _ => 32,
    }
}

/// Check if a clause body contains a cast-like expression for a variable.
fn clause_contains_cast(expr: &SpExpr, var_name: &str) -> bool {
    struct CastFinder<'a> {
        var_name: &'a str,
        found: bool,
    }

    impl ExprVisitor for CastFinder<'_> {
        fn visit_expr(&mut self, expr: &SpExpr) {
            if !self.found {
                assura_parser::ast::walk_expr(self, expr);
            }
        }

        fn visit_cast(&mut self, inner: &SpExpr, _ty: &str) {
            if let Expr::Ident(name) = &inner.node
                && name == self.var_name
            {
                self.found = true;
                return;
            }
            // Recurse into inner expression
            self.visit_expr(inner);
        }

        fn visit_call(&mut self, func: &SpExpr, args: &[SpExpr]) {
            if let Expr::Ident(fn_name) = &func.node
                && (fn_name == "as_f32" || fn_name == "narrow" || fn_name == "truncate")
                && args
                    .iter()
                    .any(|a| matches!(&a.node, Expr::Ident(n) if n == self.var_name))
            {
                self.found = true;
                return;
            }
            // Recurse into func and args (default traversal)
            self.visit_expr(func);
            for arg in args {
                self.visit_expr(arg);
            }
        }
    }

    let mut visitor = CastFinder {
        var_name,
        found: false,
    };
    visitor.visit_expr(expr);
    visitor.found
}

/// Scan for precomputed table annotations.
pub(crate) fn run_precomputed_table_checks(
    source: &assura_parser::ast::SourceFile,
) -> Vec<TypeError> {
    let mut checker = PrecomputedTableChecker::new();
    let mut found = false;
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn_block(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "precomputed_table" || k == "lookup_table" || k == "const_table")
            {
                found = true;
                // Extract table params: precomputed_table(name, size, generator)
                match &clause.body.node {
                    Expr::Call { func, args } => {
                        if let Expr::Ident(name) = &func.as_ref().node {
                            let size = args
                                .first()
                                .and_then(extract_int_literal)
                                .unwrap_or(DEFAULT_FEATURE_MAX)
                                as usize;
                            let gen_fn = args
                                .get(1)
                                .and_then(extract_ident)
                                .unwrap_or("")
                                .to_string();
                            checker.declare_table(name.clone(), size, gen_fn, decl.span.clone());
                        }
                    }
                    Expr::Ident(name) => {
                        checker.declare_table(
                            name.clone(),
                            DEFAULT_FEATURE_MAX as usize,
                            String::new(),
                            decl.span.clone(),
                        );
                    }
                    _ => {
                        let kvs = extract_kv_pairs(&clause.body);
                        let name = kvs
                            .iter()
                            .find(|(k, _)| *k == "name" || *k == "table")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("unnamed")
                            .to_string();
                        let size = kvs
                            .iter()
                            .find(|(k, _)| *k == "size" || *k == "entries")
                            .and_then(|(_, v)| extract_int_literal(v))
                            .unwrap_or(DEFAULT_FEATURE_MAX)
                            as usize;
                        let gen_fn = kvs
                            .iter()
                            .find(|(k, _)| *k == "generator" || *k == "gen")
                            .and_then(|(_, v)| extract_ident(v))
                            .unwrap_or("")
                            .to_string();
                        checker.declare_table(name, size, gen_fn, decl.span.clone());
                    }
                }
            }
        }
    }
    if !found {
        return Vec::new();
    }
    // Mark entries as verified if verification clauses exist
    for decl in &source.decls {
        let Some(clauses) = super::clauses_contract_fn_block(&decl.node) else {
            continue;
        };
        for clause in clauses {
            if let ClauseKind::Other(ref k) = clause.kind
                && (k == "verified_entries" || k == "table_verified")
                && let Some((name, args)) = extract_call(&clause.body)
            {
                let count = args
                    .first()
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_PARAM_ZERO) as usize;
                checker.mark_entries_verified(name, count);
            }
        }
    }
    let mut errors = checker.check_coverage();
    errors.extend(checker.check_generator());
    errors.extend(checker.check_non_empty());
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

    // -----------------------------------------------------------------------
    // run_numerical_precision_checks
    // -----------------------------------------------------------------------

    #[test]
    fn numerical_precision_no_annotation_produces_no_errors() {
        let src = r#"contract Simple { requires { true } }"#;
        let sf = parse_source(src);
        let errors = run_numerical_precision_checks(&sf);
        assert!(
            errors.is_empty(),
            "no precision annotation should produce no errors: {errors:?}"
        );
    }

    #[test]
    fn numerical_precision_cancellation_detected() {
        // `precision x` declares a tracked variable; `ensures { x > 0 }`
        // references it, triggering the catastrophic cancellation check.
        let src = r#"contract Compute { precision x ensures { x > 0 } }"#;
        let sf = parse_source(src);
        let errors = run_numerical_precision_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A42003"),
            "expected A42003 for catastrophic cancellation, got: {errors:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run_precomputed_table_checks
    // -----------------------------------------------------------------------

    #[test]
    fn precomputed_table_no_annotation_produces_no_errors() {
        let src = r#"contract Simple { requires { true } }"#;
        let sf = parse_source(src);
        let errors = run_precomputed_table_checks(&sf);
        assert!(
            errors.is_empty(),
            "no precomputed_table annotation should produce no errors: {errors:?}"
        );
    }

    #[test]
    fn precomputed_table_no_generator_detected() {
        // `precomputed_table crc_table` declares a table with no generator
        // function, which should trigger A43002.
        let src = r#"contract Lookup { precomputed_table crc_table }"#;
        let sf = parse_source(src);
        let errors = run_precomputed_table_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A43002"),
            "expected A43002 for table without generator function, got: {errors:?}"
        );
    }

    #[test]
    fn precomputed_table_also_flags_coverage() {
        // A bare `precomputed_table name` also gets default size (256) with
        // 0 verified entries, so A43001 (incomplete coverage) is expected too.
        let src = r#"contract Lookup { precomputed_table crc_table }"#;
        let sf = parse_source(src);
        let errors = run_precomputed_table_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A43001"),
            "expected A43001 for incomplete table coverage, got: {errors:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run_collection_contract_checks
    // -----------------------------------------------------------------------

    #[test]
    fn collection_no_known_operation_produces_no_errors() {
        let src = r#"contract Unrelated { requires { true } ensures { true } }"#;
        let sf = parse_source(src);
        let errors = run_collection_contract_checks(&sf);
        assert!(
            errors.is_empty(),
            "non-collection contract should produce no errors: {errors:?}"
        );
    }

    #[test]
    fn collection_sort_without_len_postcondition_detected() {
        // A contract named `sort` (length-preserving op) without an ensures
        // clause mentioning `len` should produce A03007.
        let src = r#"
            contract Sort {
                requires { true }
                ensures { true }
            }
        "#;
        let sf = parse_source(src);
        let errors = run_collection_contract_checks(&sf);
        assert!(
            errors.iter().any(|e| e.code == "A03007"),
            "sort without len postcondition should produce A03007: {errors:?}"
        );
    }

    #[test]
    fn collection_sort_with_len_postcondition_no_error() {
        // A contract named `sort` WITH an ensures clause mentioning `len`
        // should not produce A03007.
        let src = r#"
            contract Sort {
                input(items: List<Int>)
                ensures { len(items) == len(items) }
            }
        "#;
        let sf = parse_source(src);
        let errors = run_collection_contract_checks(&sf);
        assert!(
            !errors.iter().any(|e| e.code == "A03007"),
            "sort with len postcondition should not produce A03007: {errors:?}"
        );
    }

    // -----------------------------------------------------------------------
    // run_fixed_width_checks
    // -----------------------------------------------------------------------

    #[test]
    fn fixed_width_no_fw_params_produces_no_errors() {
        let src = r#"contract Simple { requires { true } }"#;
        let sf = parse_source(src);
        let env = TypeEnv::new();
        let errors = run_fixed_width_checks(&sf, &env);
        assert!(
            errors.is_empty(),
            "contract without fixed-width params should produce no errors: {errors:?}"
        );
    }

    #[test]
    fn fixed_width_overflow_on_u8_addition() {
        // An extern fn with two U8 params and an ensures clause adding them
        // should detect potential overflow (A10101).
        let src = r#"
            extern fn add_bytes(a: U8, b: U8) -> U8
                ensures { a + b > 0 }
        "#;
        let sf = parse_source(src);
        let env = TypeEnv::new();
        let errors = run_fixed_width_checks(&sf, &env);
        assert!(
            errors.iter().any(|e| e.code == "A10101"),
            "U8 + U8 should flag potential overflow A10101: {errors:?}"
        );
    }
}
