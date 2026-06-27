//! Numeric-related domain checkers.
//!
//! NumericalPrecisionChecker, PrecomputedTableChecker, and source-level
//! check wiring moved from `checks/numeric.rs`.

use assura_parser::ast::{BinOp, ClauseKind, Expr, ExprVisitor, SpExpr};

use crate::checkers::*;
use crate::domain::CollectionContracts;
use crate::types::*;
use crate::{Type, TypeEnv, TypeError};

// ===========================================================================
// T095: NUM.1 Numerical precision
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct NumericalPrecisionChecker {
    variables: std::collections::HashMap<String, PrecisionInfo>,
}

#[derive(Debug, Clone)]
pub(crate) struct PrecisionInfo {
    pub bits: u32,
    pub min_ulp: f64,
    pub span: std::ops::Range<usize>,
}

impl NumericalPrecisionChecker {
    pub fn new() -> Self {
        Self {
            variables: std::collections::HashMap::new(),
        }
    }

    pub fn declare(&mut self, name: String, bits: u32, min_ulp: f64, span: std::ops::Range<usize>) {
        self.variables.insert(
            name,
            PrecisionInfo {
                bits,
                min_ulp,
                span,
            },
        );
    }

    pub fn check_precision_loss(&self, name: &str, result_bits: u32) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && result_bits < info.bits
        {
            return Some(TypeError {
                code: "A42001".into(),
                message: format!(
                    "precision loss: `{name}` requires {}-bit but operation produces {result_bits}-bit",
                    info.bits
                ),
                span: info.span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_ulp_bound(&self, name: &str, actual_ulp: f64) -> Option<TypeError> {
        if let Some(info) = self.variables.get(name)
            && actual_ulp > info.min_ulp
        {
            return Some(TypeError {
                code: "A42002".into(),
                message: format!(
                    "ULP violation: `{name}` requires ULP <= {} but got {actual_ulp}",
                    info.min_ulp
                ),
                span: info.span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }

    pub fn check_cancellation(&self, name: &str, operand_ratio: f64) -> Option<TypeError> {
        if operand_ratio > 0.999
            && let Some(info) = self.variables.get(name)
        {
            return Some(TypeError {
                code: "A42003".into(),
                message: format!(
                    "potential catastrophic cancellation in `{name}` (operand ratio: {operand_ratio})"
                ),
                span: info.span.clone(),
                secondary: None,
                suggestion: None,
            });
        }
        None
    }
}

impl Default for NumericalPrecisionChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// T096: NUM.2 Precomputed table verification
// ===========================================================================

#[derive(Debug, Clone)]
pub(crate) struct PrecomputedTableChecker {
    tables: Vec<TableDecl>,
}

#[derive(Debug, Clone)]
pub(crate) struct TableDecl {
    pub name: String,
    pub size: usize,
    pub verified_entries: usize,
    pub generator_fn: String,
    pub span: std::ops::Range<usize>,
}

impl PrecomputedTableChecker {
    pub fn new() -> Self {
        Self { tables: Vec::new() }
    }

    pub fn declare_table(
        &mut self,
        name: String,
        size: usize,
        generator_fn: String,
        span: std::ops::Range<usize>,
    ) {
        self.tables.push(TableDecl {
            name,
            size,
            verified_entries: 0,
            generator_fn,
            span,
        });
    }

    pub fn mark_entries_verified(&mut self, name: &str, count: usize) {
        if let Some(t) = self.tables.iter_mut().find(|t| t.name == name) {
            t.verified_entries = count;
        }
    }

    pub fn check_coverage(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.verified_entries < t.size)
            .map(|t| TypeError {
                code: "A43001".into(),
                message: format!(
                    "table `{}` has only {}/{} entries verified",
                    t.name, t.verified_entries, t.size
                ),
                span: t.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_generator(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.generator_fn.is_empty())
            .map(|t| TypeError {
                code: "A43002".into(),
                message: format!("table `{}` has no generator function", t.name),
                span: t.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    pub fn check_non_empty(&self) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| t.size == 0)
            .map(|t| TypeError {
                code: "A43003".into(),
                message: format!("table `{}` has zero size", t.name),
                span: t.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    /// Validate that declared generator functions exist in the source.
    pub fn check_generator_exists(&self, fn_names: &[String]) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| !t.generator_fn.is_empty() && !fn_names.contains(&t.generator_fn))
            .map(|t| TypeError {
                code: "A43004".into(),
                message: format!(
                    "table `{}` references generator function `{}` which is not defined",
                    t.name, t.generator_fn
                ),
                span: t.span.clone(),
                secondary: None,
                suggestion: None,
            })
            .collect()
    }

    /// Check that declared table sizes match expected domain sizes.
    ///
    /// Common domains: byte (256), nibble (16), ascii (128), unicode_bmp (65536).
    /// Reports A43005 when the table size does not match any standard domain.
    pub fn check_domain_size(&self, expected_domain_size: Option<usize>) -> Vec<TypeError> {
        self.tables
            .iter()
            .filter(|t| {
                if let Some(expected) = expected_domain_size {
                    t.size != expected
                } else {
                    // No explicit domain: check against standard sizes
                    !STANDARD_DOMAIN_SIZES.contains(&t.size)
                }
            })
            .map(|t| {
                let msg = if let Some(expected) = expected_domain_size {
                    format!(
                        "table `{}` has {} entries but the declared domain requires {}",
                        t.name, t.size, expected
                    )
                } else {
                    format!(
                        "table `{}` has {} entries which does not match any standard domain \
                         size (16, 128, 256, 65536)",
                        t.name, t.size
                    )
                };
                TypeError {
                    code: "A43005".into(),
                    message: msg,
                    span: t.span.clone(),
                    secondary: None,
                    suggestion: None,
                }
            })
            .collect()
    }

    /// Generate SMT verification obligations for table correctness.
    ///
    /// For each table with a generator function, produces an obligation:
    /// `forall i in 0..size: table[i] == generator_fn(i)`
    ///
    /// Returns `(table_name, generator_fn, size)` tuples that the pipeline
    /// can dispatch to the Layer 2 verifier.
    pub fn smt_obligations(&self) -> Vec<TableSmtObligation> {
        self.tables
            .iter()
            .filter(|t| !t.generator_fn.is_empty() && t.size > 0)
            .map(|t| TableSmtObligation {
                table_name: t.name.clone(),
                generator_fn: t.generator_fn.clone(),
                domain_size: t.size,
                span: t.span.clone(),
            })
            .collect()
    }
}

/// Standard domain sizes for precomputed tables.
const STANDARD_DOMAIN_SIZES: &[usize] = &[16, 128, 256, 65536];

/// An SMT verification obligation for a precomputed table.
///
/// Represents the proof goal: `forall i in 0..domain_size: table[i] == generator_fn(i)`
#[derive(Debug, Clone)]
pub struct TableSmtObligation {
    pub table_name: String,
    pub generator_fn: String,
    pub domain_size: usize,
    pub span: std::ops::Range<usize>,
}

impl Default for PrecomputedTableChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl PrecomputedTableChecker {
    /// Run all precomputed-table checks on the source.
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = PrecomputedTableChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "precomputed_table" || k == "lookup_table" || k == "const_table")
                {
                    found = true;
                    parse_table_decl(&mut checker, &clause.body, &decl.span);
                }
            }
        }
        if !found {
            return Vec::new();
        }
        // Mark entries as verified if verification clauses exist
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn_block(&decl.node) else {
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
        // Validate generator functions exist in the source
        let fn_names: Vec<String> = source
            .decls
            .iter()
            .filter_map(|d| match &d.node {
                assura_parser::ast::Decl::FnDef(f) => Some(f.name.clone()),
                _ => None,
            })
            .collect();
        let mut errors = checker.check_coverage();
        errors.extend(checker.check_generator());
        errors.extend(checker.check_non_empty());
        errors.extend(checker.check_generator_exists(&fn_names));
        errors.extend(checker.check_domain_size(None));
        errors
    }

    /// Collect SMT verification obligations for precomputed tables.
    pub fn collect_smt_obligations(
        source: &assura_parser::ast::SourceFile,
    ) -> Vec<TableSmtObligation> {
        let mut checker = PrecomputedTableChecker::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn_block(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "precomputed_table" || k == "lookup_table" || k == "const_table")
                {
                    parse_table_decl(&mut checker, &clause.body, &decl.span);
                }
            }
        }
        checker.smt_obligations()
    }
}

/// Shared helper: parse a table declaration clause body into the checker.
fn parse_table_decl(
    checker: &mut PrecomputedTableChecker,
    body: &SpExpr,
    span: &std::ops::Range<usize>,
) {
    match &body.node {
        Expr::Call { func, args } => {
            if let Expr::Ident(name) = &func.as_ref().node {
                let size = args
                    .first()
                    .and_then(extract_int_literal)
                    .unwrap_or(DEFAULT_FEATURE_MAX) as usize;
                let gen_fn = args
                    .get(1)
                    .and_then(extract_ident)
                    .unwrap_or("")
                    .to_string();
                checker.declare_table(name.clone(), size, gen_fn, span.clone());
            }
        }
        Expr::Ident(name) => {
            checker.declare_table(
                name.clone(),
                DEFAULT_FEATURE_MAX as usize,
                String::new(),
                span.clone(),
            );
        }
        _ => {
            let kvs = extract_kv_pairs(body);
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
                .unwrap_or(DEFAULT_FEATURE_MAX) as usize;
            let gen_fn = kvs
                .iter()
                .find(|(k, _)| *k == "generator" || *k == "gen")
                .and_then(|(_, v)| extract_ident(v))
                .unwrap_or("")
                .to_string();
            checker.declare_table(name, size, gen_fn, span.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Fixed-width source-level checking (moved from checks/numeric.rs)
// ---------------------------------------------------------------------------

/// Namespace struct for the fixed-width source-level check.
pub(crate) struct FixedWidthSourceChecker;

impl FixedWidthSourceChecker {
    pub fn check_source(
        source: &assura_parser::ast::SourceFile,
        type_env: &TypeEnv,
    ) -> Vec<TypeError> {
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some((clauses, params)) = crate::runtime_decl_clauses_params(&decl.node) else {
                continue;
            };
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
}

/// Check an expression using the full FixedWidthChecker (with bindings).
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
            self.visit_expr(lhs);
            self.visit_expr(rhs);

            if let Some(left_type) = infer_fixed_width_type_ext(lhs, self.type_env, self.fw_checker)
                && let Some(right_type) =
                    infer_fixed_width_type_ext(rhs, self.type_env, self.fw_checker)
            {
                if op.is_arithmetic()
                    && !op.is_division_like()
                    && FixedWidthChecker::is_unsigned(&left_type)
                        != FixedWidthChecker::is_unsigned(&right_type)
                    && FixedWidthChecker::is_fixed_width(&left_type)
                    && FixedWidthChecker::is_fixed_width(&right_type)
                {
                    // already covered by check_binop's signedness check
                }
                for fwe in self
                    .fw_checker
                    .check_binop(op, &left_type, &right_type, rhs, self.span)
                {
                    self.errors.push(TypeError {
                        code: fwe.code,
                        message: fwe.message,
                        span: fwe.span,
                        secondary: None,
                        suggestion: None,
                    });
                }
            } else if let Some(left_type) =
                infer_fixed_width_type_ext(lhs, self.type_env, self.fw_checker)
                && let Some(fwe) =
                    FixedWidthChecker::check_division_by_zero(op, rhs, &left_type, self.span)
            {
                self.errors.push(TypeError {
                    code: fwe.code,
                    message: fwe.message,
                    span: fwe.span,
                    secondary: None,
                    suggestion: None,
                });
            }
        }

        fn visit_cast(&mut self, inner: &SpExpr, ty: &str) {
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
                    suggestion: None,
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
// Collection contract source-level check (moved from checks/numeric.rs)
// ---------------------------------------------------------------------------

impl CollectionContracts {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let cc = CollectionContracts::new();
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some((name, clauses)) = crate::fn_or_contract_name_clauses(&decl.node) else {
                continue;
            };
            let lower_name = name.to_lowercase();
            if let Some(cc_def) = cc.lookup(&lower_name)
                && cc_def.preserves_length
            {
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
                        suggestion: None,
                    });
                }
            }
        }
        errors
    }
}

/// Check if an expression mentions `len`.
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

// ---------------------------------------------------------------------------
// Numerical precision source-level check (moved from checks/numeric.rs)
// ---------------------------------------------------------------------------

impl NumericalPrecisionChecker {
    pub fn check_source(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
        let mut checker = NumericalPrecisionChecker::new();
        let mut found = false;
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if let ClauseKind::Other(ref k) = clause.kind
                    && (k == "precision" || k == "numerical_precision" || k == "ulp_bound")
                {
                    found = true;
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
        let mut errors = Vec::new();
        for decl in &source.decls {
            let Some(clauses) = crate::clauses_contract_fn(&decl.node) else {
                continue;
            };
            for clause in clauses {
                if matches!(
                    clause.kind,
                    ClauseKind::Ensures | ClauseKind::Requires | ClauseKind::Invariant
                ) {
                    let refs = collect_ident_references(&clause.body);
                    for name in &refs {
                        if clause_contains_cast(&clause.body, name) {
                            let target_bits = extract_cast_target_bits(&clause.body, name);
                            if let Some(err) = checker.check_precision_loss(name, target_bits) {
                                errors.push(err);
                            }
                        }
                        if let Some(err) = checker.check_ulp_bound(name, 2.0)
                            && clause_contains_cast(&clause.body, name)
                        {
                            errors.push(err);
                        }
                        if let Some(err) = checker.check_cancellation(name, 0.9999) {
                            errors.push(err);
                        }
                    }
                }
            }
        }
        errors
    }
}

/// Extract the target bit width from a type name.
pub(crate) fn type_name_to_bits(ty: &str) -> u32 {
    match ty {
        "f16" | "Float16" => 16,
        "f32" | "Float32" => 32,
        "f64" | "Float64" | "Float" => 64,
        "i8" | "u8" | "Int8" | "Nat8" => 8,
        "i16" | "u16" | "Int16" | "Nat16" => 16,
        "i32" | "u32" | "Int32" | "Nat32" => 32,
        "i64" | "u64" | "Int" | "Nat" | "Int64" | "Nat64" => 64,
        _ => 32,
    }
}

/// Extract the target bit width from a cast expression involving a variable.
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
