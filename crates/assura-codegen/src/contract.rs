//! Contract, enum, proptest, and error type code generation.

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

    let tps = if c.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", c.type_params.join(", "))
    };

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

    for req in &requires_exprs {
        code.push_str(&format!("/// Requires: {req}\n"));
    }
    for eff in &effects {
        code.push_str(&format!("/// Effects: {eff}\n"));
    }
    for m in &modifies {
        code.push_str(&format!("/// Modifies: {m}\n"));
    }

    let params_s: String = input_params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");

    code.push_str(&format!(
        "pub fn check{tps}({params_s}) -> {return_type} {{\n"
    ));

    for clause in &c.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!(
                    "    let {OLD_VAR_PREFIX}{var} = {rust_expr}.clone();\n"
                ));
            }
        }
    }

    for req in &requires_exprs {
        generate_debug_assert(code, req, "requires");
    }

    // Feature-specific annotations (CORE/SEC/MEM/CONC/FMT/NUM/PLAT/PERF/TEST/MISC)
    if !feature_code.is_empty() {
        code.push_str(&feature_code);
    }

    // Check for IR-generated body to replace todo!() placeholder
    let ir_body = ir_bodies.and_then(|m| m.get(&c.name));

    if ensures_exprs.is_empty() && invariants.is_empty() {
        if let Some(body) = ir_body {
            code.push_str(body);
        } else {
            code.push_str("    todo!(\"implementation provided by AI agent\")\n");
        }
    } else {
        if let Some(body) = ir_body {
            code.push_str(body);
        } else {
            code.push_str(&format!(
                "    let {result_var}: {output_type} = todo!(\"implementation provided by AI agent\");\n",
                result_var = RESULT_VAR
            ));
        }
        // Bind the output variable name so ensures clauses can reference it
        if let Some(ref name) = output_name {
            code.push_str(&format!("    let {name} = {RESULT_VAR}.clone();\n"));
        }
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        for inv in &invariants {
            generate_debug_assert(code, inv, "invariant");
        }
        if error_enum_name.is_some() {
            code.push_str(&format!("    Ok({RESULT_VAR})\n"));
        } else {
            code.push_str(&format!("    {RESULT_VAR}\n"));
        }
    }
    code.push_str("}\n");

    if !implements.is_empty() {
        code.push_str(&format!("\npub struct {}{tps};\n\n", c.name));
        for iface in &implements {
            code.push_str(&format!("impl{tps} {iface} for {}{tps} {{\n", c.name));
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
                        code.push_str(&format!("    fn {method_name}(&self) {{ todo!() }}\n"));
                    }
                }
            }
            code.push_str("}\n");
        }
    }
}

pub(crate) fn generate_contract(
    c: &ContractDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    // Interface contracts become traits (no wrapping module needed)
    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    // Single-file mode: wrap contents in a pub mod.
    // prettyplease handles indentation, so we just emit the module wrapper.
    code.push_str(&format!(
        "/// Contract: {}\npub mod contract_{} {{\n",
        c.name,
        c.name.to_lowercase()
    ));
    generate_contract_contents(c, code, ir_bodies);
    code.push_str("}\n\n");
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

/// Try to refine a proptest strategy based on a requires constraint.
///
/// Recognizes patterns like:
///   - `x != 0` -> range that excludes zero
///   - `x > 0` / `x >= 1` -> positive range
///   - `x < N` / `x <= N` -> bounded range
///
/// Returns `Some((param_name, refined_strategy))` if the constraint can be
/// encoded as a generator, or `None` if it should remain a filter/assumption.
pub(crate) fn try_refine_strategy(requires_expr: &SpExpr) -> Option<(String, String)> {
    if let Expr::BinOp { lhs, op, rhs } = &requires_expr.node {
        let param = match &lhs.node {
            Expr::Ident(name) => name.clone(),
            _ => return None,
        };

        match (op, &rhs.node) {
            // x != 0 -> filter: use 1..=MAX for unsigned, two ranges for signed
            (BinOp::Neq, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, "1i64..=i64::MAX".to_string()))
            }
            // x > 0 -> 1..=MAX
            (BinOp::Gt, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, "1i64..=i64::MAX".to_string()))
            }
            // x >= 0 -> 0..=MAX
            (BinOp::Gte, Expr::Literal(Literal::Int(val))) if val == "0" => {
                Some((param, "0i64..=i64::MAX".to_string()))
            }
            // x >= 1 -> 1..=MAX
            (BinOp::Gte, Expr::Literal(Literal::Int(val))) if val == "1" => {
                Some((param, "1i64..=i64::MAX".to_string()))
            }
            // x < N -> MIN..N
            (BinOp::Lt, Expr::Literal(Literal::Int(val))) => {
                Some((param, format!("i64::MIN..{val}i64")))
            }
            // x <= N -> MIN..=N
            (BinOp::Lte, Expr::Literal(Literal::Int(val))) => {
                Some((param, format!("i64::MIN..={val}i64")))
            }
            _ => None,
        }
    } else {
        None
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
                requires_exprs.push(expr_to_rust(&clause.body));
                requires_ast.push(&clause.body);
            }
            ClauseKind::Ensures => {
                ensures_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Output => {
                output_name = extract_output_name(&clause.body);
            }
            ClauseKind::Effects
            | ClauseKind::Modifies
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Ordering
            | ClauseKind::Other(_) => {}
        }
    }

    if input_params.is_empty() || ensures_exprs.is_empty() {
        return;
    }

    let mut refined: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut unrefined_requires: Vec<String> = Vec::new();
    for (i, ast) in requires_ast.iter().enumerate() {
        if let Some((param, strategy)) = try_refine_strategy(ast) {
            refined.insert(param, strategy);
        } else {
            unrefined_requires.push(requires_exprs[i].clone());
        }
    }

    let fn_name = c.name.to_lowercase();

    code.push_str("#[cfg(test)]\n");
    code.push_str(&format!("mod proptest_{fn_name} {{\n"));
    code.push_str("    use proptest::prelude::*;\n\n");
    code.push_str("    proptest! {\n");
    code.push_str("        #[test]\n");

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
    code.push_str(&format!(
        "        fn test_{fn_name}({}) {{\n",
        param_strs.join(", ")
    ));

    for req in &unrefined_requires {
        code.push_str(&format!("            prop_assume!({req});\n"));
    }

    let call_args: Vec<&str> = input_params.iter().map(|(n, _)| n.as_str()).collect();
    code.push_str(&format!(
        "            let result = {check_call_path}({});\n",
        call_args.join(", ")
    ));
    // Bind the output variable name so ensures clauses can reference it
    if let Some(ref name) = output_name {
        code.push_str(&format!("            let {name} = result.clone();\n"));
    }

    for ens in &ensures_exprs {
        code.push_str(&format!("            prop_assert!({ens});\n"));
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
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
pub(crate) fn generate_interface_trait_from_contract(c: &ContractDecl, code: &mut String) {
    let tps = if c.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", c.type_params.join(", "))
    };

    // Collect extends (supertraits)
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

    if extends.is_empty() {
        code.push_str(&format!(
            "/// Interface contract: {}\npub trait {}{tps} {{\n",
            c.name, c.name
        ));
    } else {
        let bounds = extends.join(" + ");
        code.push_str(&format!(
            "/// Interface contract: {}\npub trait {}{tps}: {bounds} {{\n",
            c.name, c.name
        ));
    }

    // Generate trait methods from `method` clauses
    for clause in &c.clauses {
        if let ClauseKind::Other(k) = &clause.kind
            && k == "method"
        {
            generate_trait_method(&clause.body, code);
        }
    }

    // Generate invariant as a provided method
    for clause in &c.clauses {
        if matches!(clause.kind, ClauseKind::Invariant | ClauseKind::Ensures) {
            let expr = expr_to_rust(&clause.body);
            code.push_str(&format!(
                "    /// Interface invariant\n    fn check_invariant(&self) {{ debug_assert!({expr}); }}\n\n"
            ));
        }
    }

    code.push_str("}\n\n");
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
            // Convert TypeExpr to tokens and filter out "linear" modifier
            let tokens = param.ty.as_ref().map(|t| t.to_tokens()).unwrap_or_default();
            let filtered: Vec<String> = tokens
                .into_iter()
                .filter(|t| t.as_str() != "linear")
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
