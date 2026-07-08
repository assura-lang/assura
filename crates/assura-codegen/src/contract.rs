//! Contract, enum, proptest, and error type code generation.

use std::fmt::Write;

use super::*;

// ---------------------------------------------------------------------------
// Enum definitions
// ---------------------------------------------------------------------------

pub(crate) fn generate_enum_def(e: &EnumDef, code: &mut String) {
    let items = crate::hir::build_enum_def(e);
    for item in &items {
        code.push_str(&crate::hir::render_item_raw(item));
    }
}

// ---------------------------------------------------------------------------
// Contract declarations
// ---------------------------------------------------------------------------

/// Generate the body of a contract as standalone module contents (no `pub mod`
/// wrapper). Used in multi-file mode where each contract gets its own `.rs` file.
///
/// When `ir_bodies` is provided and contains a body for this contract's name,
/// the IR-generated Rust code replaces the `todo!()` placeholder.
pub(crate) fn generate_contract_contents(
    c: &ContractDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    generate_contract_contents_opts(c, code, ir_bodies, false);
}

pub(crate) fn generate_contract_contents_opts(
    c: &ContractDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
    runtime_checks: bool,
) {
    use crate::hir::*;

    // Interface contracts become traits even in multi-file mode
    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    let implements: Vec<String> = c
        .clauses
        .iter()
        .filter(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "implements"))
        .filter_map(|cl| match &cl.body.node {
            Expr::Ident(name) => Some(name.clone()),
            Expr::Raw(tokens) if tokens.len() == 1 => Some(tokens[0].clone()),
            _ => None,
        })
        .collect();

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut output_name: Option<String> = None;
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut effects: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
                output_name = extract_output_name(&clause.body);
            }
            ClauseKind::Requires => requires_exprs.push(expr_to_rust(&clause.body)),
            ClauseKind::Ensures => ensures_exprs.push(expr_to_rust(&clause.body)),
            ClauseKind::Effects => effects.push(expr_to_rust(&clause.body)),
            ClauseKind::Modifies => modifies.push(expr_to_rust(&clause.body)),
            ClauseKind::Invariant => invariants.push(expr_to_rust(&clause.body)),
            ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Ordering
            | ClauseKind::Other(_) => {}
        }
    }

    // Collect feature-specific annotation code (CORE/SEC/MEM/CONC/FMT/etc.)
    let mut feature_code = String::new();
    crate::features::generate_all_feature_clauses(&c.clauses, &c.name, &mut feature_code);

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&c.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", c.name);
        generate_error_enum(&c.name, &error_variants, code);
        Some(name)
    } else {
        None
    };

    // Determine return type: wrap in Result when errors are declared
    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{output_type}, {err_name}>")
    } else {
        output_type.clone()
    };

    // Build doc comments
    let mut doc: Vec<String> = Vec::new();
    for req in &requires_exprs {
        doc.push(format!("Requires: {req}"));
    }
    for eff in &effects {
        doc.push(format!("Effects: {eff}"));
    }
    for m in &modifies {
        doc.push(format!("Modifies: {m}"));
    }

    // Build params
    let params: Vec<RustParam> = input_params
        .iter()
        .map(|(name, ty)| RustParam {
            name: name.clone(),
            ty: RustType::Raw(ty.clone()),
        })
        .collect();

    let ret = if return_type == "()" {
        None
    } else {
        Some(RustType::Raw(return_type.clone()))
    };

    // Build function body
    let mut body: Vec<RustStmt> = Vec::new();

    // old() variable snapshots for ensures clauses
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                body.push(RustStmt::Raw(format!(
                    "let {OLD_VAR_PREFIX}{var} = {rust_expr}.clone();"
                )));
            }
        }
    }

    // Requires assertions
    for req in &requires_exprs {
        body.push(RustStmt::Assert {
            cond: req.clone(),
            label: "requires".into(),
        });
    }

    // Feature-specific annotations
    if !feature_code.is_empty() {
        body.push(RustStmt::Raw(feature_code));
    }

    // Check for IR-generated body to replace todo!() placeholder
    let ir_body = ir_bodies.and_then(|m| m.get(&c.name));

    if ensures_exprs.is_empty() && invariants.is_empty() {
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Expr(RustExpr::Todo(
                "implementation provided by AI agent".into(),
            )));
        }
    } else {
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Raw(
                crate::metadata::implementation_guidance_comment(c),
            ));
            body.push(RustStmt::Raw(format!(
                "let {RESULT_VAR}: {output_type} = todo!(\"implementation provided by AI agent\");"
            )));
        }
        if let Some(ref name) = output_name {
            body.push(RustStmt::Raw(format!("let {name} = {RESULT_VAR}.clone();")));
        }
        for ens in &ensures_exprs {
            body.push(RustStmt::Assert {
                cond: ens.clone(),
                label: "ensures".into(),
            });
        }
        for inv in &invariants {
            body.push(RustStmt::Assert {
                cond: inv.clone(),
                label: "invariant".into(),
            });
        }
        if error_enum_name.is_some() {
            body.push(RustStmt::Expr(RustExpr::Ok(Box::new(RustExpr::Ident(
                RESULT_VAR.into(),
            )))));
        } else {
            body.push(RustStmt::Expr(RustExpr::Ident(RESULT_VAR.into())));
        }
    }

    let check_fn = RustFn {
        name: "check".into(),
        type_params: c.type_params.clone(),
        params,
        ret,
        body,
        doc,
        ..RustFn::default()
    };
    if runtime_checks {
        let opts = crate::hir::RenderOpts {
            runtime_checks: true,
            contract_name: c.name.clone(),
        };
        code.push_str(&crate::hir::render_item_raw_with_opts(
            &RustItem::Fn(check_fn),
            &opts,
        ));
    } else {
        code.push_str(&render_item_raw(&RustItem::Fn(check_fn)));
    }

    // Generate implements blocks
    if !implements.is_empty() {
        code.push_str(&render_item_raw(&RustItem::Struct(RustStruct {
            name: c.name.clone(),
            type_params: c.type_params.clone(),
            derives: vec![],
            ..RustStruct::default()
        })));

        for iface in &implements {
            let mut impl_methods: Vec<RustFn> = Vec::new();
            for clause in &c.clauses {
                if let ClauseKind::Other(k) = &clause.kind
                    && k == "method"
                {
                    let method_name = match &clause.body.node {
                        Expr::Ident(n) => Some(n.as_str()),
                        Expr::Raw(tokens) if tokens.len() == 1 => Some(tokens[0].as_str()),
                        _ => None,
                    };
                    if let Some(method_name) = method_name {
                        impl_methods.push(RustFn {
                            name: method_name.to_string(),
                            params: vec![RustParam {
                                name: "&self".into(),
                                ty: RustType::Raw("&Self".into()),
                            }],
                            body: vec![RustStmt::Expr(RustExpr::Todo(String::new()))],
                            is_pub: false,
                            ..RustFn::default()
                        });
                    }
                }
            }
            code.push_str(&render_item_raw(&RustItem::Impl(RustImpl {
                trait_name: Some(iface.clone()),
                target: c.name.clone(),
                type_params: c.type_params.clone(),
                methods: impl_methods,
            })));
        }
    }
}

pub(crate) fn generate_contract_runtime(
    c: &ContractDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    use crate::hir::*;

    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    let mut inner = String::new();
    generate_contract_contents_opts(c, &mut inner, ir_bodies, true);

    let m = RustItem::Mod(RustMod {
        name: format!("contract_{}", c.name.to_lowercase()),
        items: vec![RustItem::Raw(inner)],
        is_pub: true,
        doc: vec![format!("Contract: {}", c.name)],
    });
    code.push_str(&render_item_raw(&m));
}

pub(crate) fn generate_contract(
    c: &ContractDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    use crate::hir::*;

    // Interface contracts become traits (no wrapping module needed)
    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    // Single-file mode: wrap contents in a pub mod
    let mut inner = String::new();
    generate_contract_contents(c, &mut inner, ir_bodies);

    let m = RustItem::Mod(RustMod {
        name: format!("contract_{}", c.name.to_lowercase()),
        items: vec![RustItem::Raw(inner)],
        is_pub: true,
        doc: vec![format!("Contract: {}", c.name)],
    });
    code.push_str(&render_item_raw(&m));
}

// ---------------------------------------------------------------------------
// S009: Proptest generation from contracts
// ---------------------------------------------------------------------------

/// Map a Rust type to a proptest strategy expression.
pub(crate) fn proptest_strategy_for_type(rust_type: &str) -> String {
    match rust_type {
        "i64" => "proptest::prelude::any::<i64>()".to_string(),
        "u64" => "proptest::prelude::any::<u64>()".to_string(),
        "i32" => "proptest::prelude::any::<i32>()".to_string(),
        "u32" => "proptest::prelude::any::<u32>()".to_string(),
        "i16" => "proptest::prelude::any::<i16>()".to_string(),
        "u16" => "proptest::prelude::any::<u16>()".to_string(),
        "i8" => "proptest::prelude::any::<i8>()".to_string(),
        "u8" => "proptest::prelude::any::<u8>()".to_string(),
        "f64" => "proptest::prelude::any::<f64>()".to_string(),
        "f32" => "proptest::prelude::any::<f32>()".to_string(),
        "bool" => "proptest::prelude::any::<bool>()".to_string(),
        "usize" => "proptest::prelude::any::<usize>()".to_string(),
        "isize" => "proptest::prelude::any::<isize>()".to_string(),
        _ => format!("proptest::prelude::any::<{rust_type}>()"),
    }
}

/// A single bound extracted from a requires constraint.
#[derive(Debug, Clone)]
pub(crate) enum ParamBound {
    /// x >= val (inclusive lower)
    GteVal(i64),
    /// x > val  (exclusive lower, stored as val+1 inclusive)
    GtVal(i64),
    /// x <= val (inclusive upper)
    LteVal(i64),
    /// x < val  (exclusive upper, stored as val-1 inclusive)
    LtVal(i64),
    /// x != val (non-equality; currently only x != 0 uses lower bound 1)
    NeqZero,
}

/// Try to extract a bound on a parameter from a requires constraint.
///
/// Recognizes patterns like:
///   - `x != 0` -> lower bound 1
///   - `x > 0` / `x >= 1` -> lower bound
///   - `x < N` / `x <= N` -> upper bound
///
/// Returns `Some((param_name, bound))` if the constraint is a simple
/// comparison on a single param, or `None` if it should remain a
/// filter/assumption.
pub(crate) fn try_extract_bound(requires_expr: &SpExpr) -> Option<(String, ParamBound)> {
    if let Expr::BinOp { lhs, op, rhs } = &requires_expr.node {
        let param = match &lhs.node {
            Expr::Ident(name) => name.clone(),
            _ => return None,
        };

        match (op, &rhs.node) {
            (BinOp::Neq, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, ParamBound::NeqZero))
            }
            (BinOp::Gt, Expr::Literal(Literal::Int(val))) => {
                let v = val.parse::<i64>().ok()?;
                Some((param, ParamBound::GtVal(v)))
            }
            (BinOp::Gte, Expr::Literal(Literal::Int(val))) => {
                let v = val.parse::<i64>().ok()?;
                Some((param, ParamBound::GteVal(v)))
            }
            (BinOp::Lt, Expr::Literal(Literal::Int(val))) => {
                let v = val.parse::<i64>().ok()?;
                Some((param, ParamBound::LtVal(v)))
            }
            (BinOp::Lte, Expr::Literal(Literal::Int(val))) => {
                let v = val.parse::<i64>().ok()?;
                Some((param, ParamBound::LteVal(v)))
            }
            _ => None,
        }
    } else {
        None
    }
}

/// Collected lower and upper bounds for a single parameter.
#[derive(Debug, Default)]
struct ParamRange {
    lower: Option<i64>,
    upper: Option<i64>,
    neq_zero: bool,
}

impl ParamRange {
    fn apply(&mut self, bound: &ParamBound) {
        match bound {
            ParamBound::GteVal(v) => {
                self.lower = Some(self.lower.map_or(*v, |cur| cur.max(*v)));
            }
            ParamBound::GtVal(v) => {
                let inclusive = v.saturating_add(1);
                self.lower = Some(self.lower.map_or(inclusive, |cur| cur.max(inclusive)));
            }
            ParamBound::LteVal(v) => {
                self.upper = Some(self.upper.map_or(*v, |cur| cur.min(*v)));
            }
            ParamBound::LtVal(v) => {
                let inclusive = v.saturating_sub(1);
                self.upper = Some(self.upper.map_or(inclusive, |cur| cur.min(inclusive)));
            }
            ParamBound::NeqZero => {
                self.neq_zero = true;
            }
        }
    }

    fn to_strategy(&self) -> String {
        // #709: contradictory bounds (lo > hi) fall back to any()
        let base = match (self.lower, self.upper) {
            (Some(lo), Some(hi)) if lo > hi => "proptest::prelude::any::<i64>()".to_string(),
            (Some(lo), Some(hi)) => format!("({lo}i64..={hi}i64)"),
            (Some(lo), None) => format!("({lo}i64..=i64::MAX)"),
            (None, Some(hi)) => format!("(i64::MIN..={hi}i64)"),
            (None, None) => "proptest::prelude::any::<i64>()".to_string(),
        };
        // #710: neq_zero uses a filter to preserve both positive and negative domain
        if self.neq_zero {
            format!("{base}.prop_filter(\"!= 0\", |&v| v != 0)")
        } else {
            base
        }
    }

    fn has_bounds(&self) -> bool {
        self.lower.is_some() || self.upper.is_some() || self.neq_zero
    }
}

/// Check if a contract has testable content (inputs + ensures/requires).
pub(crate) fn contract_is_testable(c: &ContractDecl) -> bool {
    let has_input = c
        .clauses
        .iter()
        .any(|cl| matches!(cl.kind, ClauseKind::Input));
    let has_ensures = c
        .clauses
        .iter()
        .any(|cl| matches!(cl.kind, ClauseKind::Ensures));
    has_input && has_ensures
}

/// Generate proptest property-based tests for a contract.
///
/// For each contract with input params and ensures clauses, generates a
/// `proptest!` block that:
/// - Uses the contract's input types as proptest strategies
/// - Refines strategies based on requires constraints where possible
/// - Falls back to `prop_assume!` for complex requires constraints
/// - Asserts ensures clauses with `prop_assert!`
pub(crate) fn generate_proptest_for_contract(c: &ContractDecl, code: &mut String) {
    // Single-file mode: call path is super::contract_<name>::check()
    let fn_name = c.name.to_lowercase();
    generate_proptest_impl(c, code, &format!("super::contract_{fn_name}::check"));
}

/// Generate proptest for a contract in multi-file mode (the test module
/// is inside the contract's own .rs file, so the call is `super::check()`).
pub(crate) fn generate_proptest_for_contract_contents(c: &ContractDecl, code: &mut String) {
    generate_proptest_impl(c, code, "super::check");
}

/// Shared proptest generation. `check_call_path` is the path to the
/// contract's check function from inside the test module.
fn generate_proptest_impl(c: &ContractDecl, code: &mut String, check_call_path: &str) {
    use crate::hir::*;

    if !contract_is_testable(c) {
        return;
    }

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut requires_ast: Vec<&SpExpr> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut output_name: Option<String> = None;

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Requires => {
                requires_exprs.push(expr_to_rust_static(&clause.body));
                requires_ast.push(&clause.body);
            }
            ClauseKind::Ensures => {
                ensures_exprs.push(expr_to_rust_static(&clause.body));
            }
            ClauseKind::Output => {
                output_name = extract_output_name(&clause.body);
            }
            _ => {}
        }
    }

    if input_params.is_empty() || ensures_exprs.is_empty() {
        return;
    }

    // Collect all bounds per parameter, then merge into one range each.
    let mut param_ranges: std::collections::HashMap<String, ParamRange> =
        std::collections::HashMap::new();
    let mut unrefined_requires: Vec<String> = Vec::new();
    for (i, ast) in requires_ast.iter().enumerate() {
        if let Some((param, bound)) = try_extract_bound(ast) {
            param_ranges.entry(param).or_default().apply(&bound);
        } else {
            unrefined_requires.push(requires_exprs[i].clone());
        }
    }
    let refined: std::collections::HashMap<String, String> = param_ranges
        .iter()
        .filter(|(_, range)| range.has_bounds())
        .map(|(param, range)| (param.clone(), range.to_strategy()))
        .collect();

    let fn_name = c.name.to_lowercase();

    // Build the proptest macro body as raw code since proptest! is a macro
    // and not representable as a plain RustFn
    let param_strs: Vec<String> = input_params
        .iter()
        .map(|(name, ty)| {
            if let Some(strategy) = refined.get(name) {
                format!("{name} in {strategy}")
            } else {
                let strategy = proptest_strategy_for_type(ty);
                format!("{name} in {strategy}")
            }
        })
        .collect();

    let mut test_body = String::new();
    for req in &unrefined_requires {
        let _ = writeln!(test_body, "            prop_assume!({req});");
    }

    let call_args: Vec<&str> = input_params.iter().map(|(n, _)| n.as_str()).collect();
    let _ = writeln!(
        test_body,
        "            let result = {check_call_path}({});",
        call_args.join(", ")
    );
    if let Some(ref name) = output_name {
        let _ = writeln!(test_body, "            let {name} = result.clone();");
    }
    for (i, ens) in ensures_exprs.iter().enumerate() {
        // Evaluate ensures to a bool first so braces in `if { } else { }`
        // expressions do not break prop_assert!'s format-string expansion.
        let _ = writeln!(test_body, "            let __ensures_{i} = {ens};");
        let _ = writeln!(
            test_body,
            "            prop_assert!(__ensures_{i}, \"ensures clause {i} failed\");"
        );
    }

    // Emit as a RustMod with #[cfg(test)] + raw proptest! macro inside
    let inner_raw = format!(
        "use proptest::prelude::*;\n\n\
         proptest! {{\n\
         {indent}#[test]\n\
         {indent}fn test_{fn_name}({params}) {{\n\
         {test_body}\
         {indent}}}\n\
         }}\n",
        indent = "    ",
        params = param_strs.join(", "),
    );

    code.push_str(&render_item_raw(&RustItem::Raw(
        "#[cfg(test)]\n".to_string(),
    )));
    code.push_str(&render_item_raw(&RustItem::Mod(RustMod {
        name: format!("proptest_{fn_name}"),
        items: vec![RustItem::Raw(inner_raw)],
        is_pub: false,
        doc: vec![],
    })));
}

/// Check if any contract in the source is testable (needs proptest).
/// Check if any declaration has an `errors` clause that will generate error types.
pub(crate) fn source_has_error_types(source: &assura_ast::SourceFile) -> bool {
    use assura_ast::{ContractDecl, DeclVisitor, FnDef};

    struct HasErrors(bool);
    impl DeclVisitor for HasErrors {
        fn visit_contract(&mut self, c: &ContractDecl) {
            if c.clauses.iter().any(|cl| cl.kind == ClauseKind::Errors) {
                self.0 = true;
            }
        }
        fn visit_fn_def(&mut self, f: &FnDef) {
            if f.clauses.iter().any(|cl| cl.kind == ClauseKind::Errors) {
                self.0 = true;
            }
        }
    }
    let mut v = HasErrors(false);
    assura_ast::walk_decls(&mut v, &source.decls);
    v.0
}

pub(crate) fn source_has_testable_contracts(source: &assura_ast::SourceFile) -> bool {
    use assura_ast::{ContractDecl, DeclVisitor};

    struct HasTestable(bool);
    impl DeclVisitor for HasTestable {
        fn visit_contract(&mut self, c: &ContractDecl) {
            if contract_is_testable(c) {
                self.0 = true;
            }
        }
    }
    let mut v = HasTestable(false);
    assura_ast::walk_decls(&mut v, &source.decls);
    v.0
}

/// Generate a Rust trait from a contract that has an `interface` clause.
///
/// Delegates to the shared `generate_interface_trait` which already builds
/// a `RustItem::Trait` from clause bodies.
pub(crate) fn generate_interface_trait_from_contract(c: &ContractDecl, code: &mut String) {
    generate_interface_trait(&c.name, &c.clauses, code);
}

/// Extract `(name, rust_type)` pairs from an input clause body.
///
/// Uses the shared `extract_clause_params` from assura-parser, then maps
/// Assura type tokens to Rust types via `map_type_token`/`map_type_tokens`.
pub(crate) fn extract_input_params(body: &SpExpr, params: &mut Vec<(String, String)>) {
    use assura_ast::extract_clause_params;
    for param in extract_clause_params(body) {
        let rust_ty = if param.ty.is_none() {
            "i64".to_string()
        } else {
            // Convert TypeExpr to tokens and filter out type qualifiers
            // (linear, secret, tainted, etc.) that have no Rust equivalent
            let tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            let filtered: Vec<String> = tokens
                .into_iter()
                .filter(|t| {
                    !matches!(
                        t.as_str(),
                        "linear" | "secret" | "tainted" | "taint" | "untrusted" | "validated"
                    )
                })
                .collect();
            if filtered.is_empty() {
                "i64".to_string()
            } else if filtered.len() == 1 {
                map_type_token(&filtered[0]).to_string()
            } else {
                map_type_tokens(&filtered)
            }
        };
        params.push((param.name, rust_ty));
    }
}

/// Extract the Rust return type from an output clause body.
pub(crate) fn extract_output_type(body: &SpExpr) -> String {
    match &body.node {
        Expr::Call { args, .. } => {
            // output(result: Int) => parse the cast or ident in args
            for arg in args {
                match &arg.node {
                    Expr::Cast { ty, .. } => return map_type_token(ty).to_string(),
                    Expr::Ident(name) => return map_type_token(name).to_string(),
                    _ => {
                        let ty = extract_output_type(arg);
                        if ty != "()" {
                            return ty;
                        }
                    }
                }
            }
            "()".to_string()
        }
        Expr::Cast { ty, .. } => map_type_token(ty).to_string(),
        Expr::Ident(name) => map_type_token(name).to_string(),
        Expr::Tuple(items) | Expr::Block(items) => {
            // First typed element wins (e.g., (result: Int) parsed as tuple)
            for item in items {
                let ty = extract_output_type(item);
                if ty != "()" {
                    return ty;
                }
            }
            "()".to_string()
        }
        Expr::Raw(tokens) => {
            // Look for the type after ":" or "as"
            for (i, tok) in tokens.iter().enumerate() {
                if (tok == ":" || tok == "as") && i + 1 < tokens.len() {
                    let type_tokens = &tokens[i + 1..];
                    return map_type_tokens(type_tokens);
                }
            }
            if tokens.len() == 1 {
                return map_type_token(&tokens[0]).to_string();
            }
            "()".to_string()
        }
        // Expressions that can carry type info through structure
        Expr::If { then_branch, .. } => extract_output_type(then_branch),
        Expr::Let { body, .. } => extract_output_type(body),
        Expr::Match { arms, .. } => {
            if let Some(arm) = arms.first() {
                extract_output_type(&arm.body)
            } else {
                "()".to_string()
            }
        }
        Expr::Old(inner) | Expr::Ghost(inner) | Expr::UnaryOp { expr: inner, .. } => {
            extract_output_type(inner)
        }
        // These expression forms do not carry type annotations;
        // the output clause type cannot be determined from them.
        Expr::Literal(_)
        | Expr::Field(_, _)
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::BinOp { .. }
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::List(_)
        | Expr::Apply { .. } => "()".to_string(),
    }
}

/// Extract the variable name from an output clause body.
///
/// Given `output(value: Nat)`, the AST has a `Call { args: [Cast { expr: Ident("value"), .. }] }`.
/// Returns `Some("value")` if a name is found and it differs from `result`, which is already
/// aliased to the compiler-generated result variable by codegen. Returns `None` if the output clause has no named
/// binding or uses `result`.
pub(crate) fn extract_output_name(body: &SpExpr) -> Option<String> {
    match &body.node {
        Expr::Call { args, .. } => {
            for arg in args {
                if let Some(name) = extract_output_name(arg) {
                    return Some(name);
                }
            }
            None
        }
        Expr::Cast { expr, .. } => {
            // output(value: Nat) parses as Cast { expr: Ident("value"), ty: "Nat" }
            if let Expr::Ident(name) = &expr.node
                && name != "result"
            {
                return Some(name.clone());
            }
            None
        }
        Expr::Tuple(items) | Expr::Block(items) => {
            for item in items {
                if let Some(name) = extract_output_name(item) {
                    return Some(name);
                }
            }
            None
        }
        Expr::Raw(tokens) => {
            // Look for "name : Type" pattern
            for (i, tok) in tokens.iter().enumerate() {
                if (tok == ":" || tok == "as") && i > 0 {
                    let name = &tokens[i - 1];
                    if name != "result" {
                        return Some(name.clone());
                    }
                }
            }
            None
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Error type generation (P004)
// ---------------------------------------------------------------------------

/// Extract error variant names from an `errors` clause body.
///
/// The errors clause body may be:
/// - `Expr::Raw(["DivByZero", ",", "Overflow"])` -> vec!["DivByZero", "Overflow"]
/// - `Expr::Ident("DivByZero")` -> vec!["DivByZero"]
/// - `Expr::Tuple([Ident("A"), Ident("B")])` -> vec!["A", "B"]
pub(crate) fn extract_error_variants(body: &SpExpr) -> Vec<String> {
    match &body.node {
        Expr::Ident(name) => vec![name.clone()],
        Expr::Tuple(items) | Expr::List(items) | Expr::Block(items) => {
            items.iter().flat_map(extract_error_variants).collect()
        }
        Expr::Raw(tokens) => tokens
            .iter()
            .filter(|t| {
                let s = t.as_str();
                s != "," && s != "(" && s != ")" && s != "{" && s != "}"
            })
            .cloned()
            .collect(),
        Expr::Ghost(inner) | Expr::Old(inner) => extract_error_variants(inner),
        Expr::Call { args, .. } => args.iter().flat_map(extract_error_variants).collect(),
        // These expression forms cannot meaningfully contain error variant names
        Expr::Literal(_)
        | Expr::Field(_, _)
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::BinOp { .. }
        | Expr::UnaryOp { .. }
        | Expr::Cast { .. }
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::If { .. }
        | Expr::Let { .. }
        | Expr::Match { .. }
        | Expr::Apply { .. } => vec![],
    }
}

/// Collect all error variants from a set of clauses.
pub(crate) fn collect_error_variants(clauses: &[Clause]) -> Vec<String> {
    let mut errors = Vec::new();
    for clause in clauses {
        if clause.kind == ClauseKind::Errors {
            errors.extend(extract_error_variants(&clause.body));
        }
    }
    errors
}

/// Generate a `#[derive(Debug, thiserror::Error)]` enum for contract errors.
pub(crate) fn generate_error_enum(contract_name: &str, variants: &[String], code: &mut String) {
    let item = crate::hir::build_error_enum(contract_name, variants);
    code.push_str(&crate::hir::render_item_raw(&item));
}
#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
