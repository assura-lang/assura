//! Contract, enum, proptest, and error type code generation.

use super::*;

// ---------------------------------------------------------------------------
// Enum definitions
// ---------------------------------------------------------------------------

pub(crate) fn generate_enum_def(e: &EnumDef, code: &mut String) {
    let tps = if e.type_params.is_empty() {
        String::new()
    } else {
        format!("<{}>", e.type_params.join(", "))
    };

    code.push_str(&format!(
        "#[derive(Debug, Clone, PartialEq)]\npub enum {}{tps} {{\n",
        e.name
    ));
    for v in &e.variants {
        if v.fields.is_empty() {
            code.push_str(&format!("    {},\n", v.name));
        } else {
            let fields: Vec<String> = v
                .fields
                .iter()
                .map(|f| map_type_token(f).to_string())
                .collect();
            code.push_str(&format!("    {}({}),\n", v.name, fields.join(", ")));
        }
    }
    code.push_str("}\n\n");

    // Generate Display implementation for non-generic enums
    if e.type_params.is_empty() {
        code.push_str(&format!("impl std::fmt::Display for {} {{\n", e.name));
        code.push_str("    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n");
        code.push_str("        match self {\n");
        for v in &e.variants {
            if v.fields.is_empty() {
                code.push_str(&format!(
                    "            {}::{} => write!(f, \"{}\"),\n",
                    e.name, v.name, v.name
                ));
            } else {
                let underscores: Vec<&str> = (0..v.fields.len()).map(|_| "_").collect();
                code.push_str(&format!(
                    "            {}::{}({}) => write!(f, \"{}(...)\"),\n",
                    e.name,
                    v.name,
                    underscores.join(", "),
                    v.name
                ));
            }
        }
        code.push_str("        }\n");
        code.push_str("    }\n");
        code.push_str("}\n\n");
    }

    // Generate exhaustiveness check: a match with no wildcard arm.
    // Rust's compiler will error if a variant is added but not covered,
    // catching missing cases at compile time rather than runtime.
    if !e.variants.is_empty() && e.type_params.is_empty() {
        code.push_str(&format!(
            "/// Compile-time exhaustiveness check for `{}`.\n",
            e.name
        ));
        code.push_str(
            "/// Adding a variant without updating all match sites causes a build error.\n",
        );
        code.push_str(&format!(
            "#[allow(dead_code)]\nfn __exhaustive_check_{}(v: &{}) -> &'static str {{\n",
            e.name.to_lowercase(),
            e.name
        ));
        code.push_str("    match v {\n");
        for v in &e.variants {
            if v.fields.is_empty() {
                code.push_str(&format!(
                    "        {}::{} => \"{}\",\n",
                    e.name, v.name, v.name
                ));
            } else {
                let underscores: Vec<&str> = (0..v.fields.len()).map(|_| "_").collect();
                code.push_str(&format!(
                    "        {}::{}({}) => \"{}\",\n",
                    e.name,
                    v.name,
                    underscores.join(", "),
                    v.name
                ));
            }
        }
        code.push_str("    }\n");
        code.push_str("}\n\n");
    }
}

// ---------------------------------------------------------------------------
// Contract declarations
// ---------------------------------------------------------------------------

/// Generate the body of a contract as standalone module contents (no `pub mod`
/// wrapper). Used in multi-file mode where each contract gets its own `.rs` file.
pub(crate) fn generate_contract_contents(c: &ContractDecl, code: &mut String) {
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
        .filter_map(|cl| match &cl.body {
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
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
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

    if ensures_exprs.is_empty() && invariants.is_empty() {
        code.push_str("    todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {output_type} = todo!(\"implementation provided by AI agent\");\n"
        ));
        // Bind the output variable name so ensures clauses can reference it
        if let Some(ref name) = output_name {
            code.push_str(&format!("    let {name} = __result.clone();\n"));
        }
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        for inv in &invariants {
            generate_debug_assert(code, inv, "invariant");
        }
        if error_enum_name.is_some() {
            code.push_str("    Ok(__result)\n");
        } else {
            code.push_str("    __result\n");
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
                    let method_name = match &clause.body {
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

pub(crate) fn generate_contract(c: &ContractDecl, code: &mut String) {
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
    generate_contract_contents(c, code);
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
pub(crate) fn try_refine_strategy(requires_expr: &Expr) -> Option<(String, String)> {
    if let Expr::BinOp { lhs, op, rhs } = requires_expr {
        let param = match lhs.as_ref() {
            Expr::Ident(name) => name.clone(),
            _ => return None,
        };

        match (op, rhs.as_ref()) {
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
    let mut requires_ast: Vec<&Expr> = Vec::new();
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
pub(crate) fn source_has_error_types(source: &assura_parser::ast::SourceFile) -> bool {
    source.decls.iter().any(|decl| match &decl.node {
        Decl::Contract(c) => c.clauses.iter().any(|cl| cl.kind == ClauseKind::Errors),
        Decl::FnDef(f) => f.clauses.iter().any(|cl| cl.kind == ClauseKind::Errors),
        _ => false,
    })
}

pub(crate) fn source_has_testable_contracts(source: &assura_parser::ast::SourceFile) -> bool {
    source.decls.iter().any(|decl| {
        if let Decl::Contract(c) = &decl.node {
            contract_is_testable(c)
        } else {
            false
        }
    })
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
            if let Expr::Ident(name) = &cl.body {
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
pub(crate) fn extract_input_params(body: &Expr, params: &mut Vec<(String, String)>) {
    use assura_parser::ast::extract_clause_params;
    for param in extract_clause_params(body) {
        let rust_ty = if param.ty.is_empty() {
            "i64".to_string()
        } else {
            // Filter out "linear" modifier from type tokens
            let filtered: Vec<String> = param
                .ty
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
pub(crate) fn extract_output_type(body: &Expr) -> String {
    match body {
        Expr::Call { args, .. } => {
            // output(result: Int) => parse the cast or ident in args
            for arg in args {
                match arg {
                    Expr::Cast { ty, .. } => return map_type_token(ty).to_string(),
                    Expr::Ident(name) => return map_type_token(name).to_string(),
                    Expr::Paren(inner) => return extract_output_type(inner),
                    other => {
                        let ty = extract_output_type(other);
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
        Expr::Paren(inner) => extract_output_type(inner),
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
/// aliased to `__result` by the codegen. Returns `None` if the output clause has no named
/// binding or uses `result`.
pub(crate) fn extract_output_name(body: &Expr) -> Option<String> {
    match body {
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
            if let Expr::Ident(name) = expr.as_ref()
                && name != "result"
            {
                return Some(name.clone());
            }
            None
        }
        Expr::Paren(inner) => extract_output_name(inner),
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
pub(crate) fn extract_error_variants(body: &Expr) -> Vec<String> {
    match body {
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
        Expr::Paren(inner) | Expr::Ghost(inner) | Expr::Old(inner) => extract_error_variants(inner),
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
    let enum_name = format!("{contract_name}Error");
    code.push_str("#[derive(Debug, thiserror::Error)]\n");
    code.push_str(&format!("pub enum {enum_name} {{\n"));
    for variant in variants {
        code.push_str(&format!("    #[error(\"{variant}\")]\n    {variant},\n"));
    }
    code.push_str("}\n\n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::*;

    fn mk_clause(kind: ClauseKind, body: Expr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    fn mk_contract(name: &str, clauses: Vec<Clause>) -> ContractDecl {
        ContractDecl {
            name: name.into(),
            type_params: vec![],
            clauses,
            fn_params: vec![],
        }
    }

    // ---- generate_enum_def ----

    #[test]
    fn enum_def_unit_variants() {
        let e = EnumDef {
            name: "Color".into(),
            type_params: vec![],
            variants: vec![
                EnumVariant {
                    name: "Red".into(),
                    fields: vec![],
                },
                EnumVariant {
                    name: "Green".into(),
                    fields: vec![],
                },
            ],
        };
        let mut code = String::new();
        generate_enum_def(&e, &mut code);
        assert!(code.contains("pub enum Color {"));
        assert!(code.contains("    Red,"));
        assert!(code.contains("    Green,"));
        // Display impl
        assert!(code.contains("impl std::fmt::Display for Color"));
        // Exhaustiveness check
        assert!(code.contains("__exhaustive_check_color"));
    }

    #[test]
    fn enum_def_variant_with_fields() {
        let e = EnumDef {
            name: "Shape".into(),
            type_params: vec![],
            variants: vec![
                EnumVariant {
                    name: "Circle".into(),
                    fields: vec!["Float".into()],
                },
                EnumVariant {
                    name: "Rect".into(),
                    fields: vec!["Float".into(), "Float".into()],
                },
            ],
        };
        let mut code = String::new();
        generate_enum_def(&e, &mut code);
        assert!(code.contains("Circle(f64)"));
        assert!(code.contains("Rect(f64, f64)"));
        // Display shows (...) for fields
        assert!(code.contains("Circle(...)"));
    }

    #[test]
    fn enum_def_generic_no_display() {
        let e = EnumDef {
            name: "Option".into(),
            type_params: vec!["T".into()],
            variants: vec![
                EnumVariant {
                    name: "Some".into(),
                    fields: vec!["T".into()],
                },
                EnumVariant {
                    name: "None".into(),
                    fields: vec![],
                },
            ],
        };
        let mut code = String::new();
        generate_enum_def(&e, &mut code);
        assert!(code.contains("pub enum Option<T>"));
        // Generic enums skip Display impl
        assert!(!code.contains("impl std::fmt::Display"));
        // Generic enums skip exhaustiveness check
        assert!(!code.contains("__exhaustive_check"));
    }

    #[test]
    fn enum_def_empty_variants_no_exhaustive() {
        let e = EnumDef {
            name: "Empty".into(),
            type_params: vec![],
            variants: vec![],
        };
        let mut code = String::new();
        generate_enum_def(&e, &mut code);
        assert!(code.contains("pub enum Empty {"));
        assert!(!code.contains("__exhaustive_check"));
    }

    // ---- proptest_strategy_for_type ----

    #[test]
    fn proptest_strategy_known_types() {
        assert!(proptest_strategy_for_type("i64").contains("any::<i64>()"));
        assert!(proptest_strategy_for_type("bool").contains("any::<bool>()"));
        assert!(proptest_strategy_for_type("f64").contains("any::<f64>()"));
        assert!(proptest_strategy_for_type("u8").contains("any::<u8>()"));
    }

    #[test]
    fn proptest_strategy_unknown_type() {
        let s = proptest_strategy_for_type("MyStruct");
        assert!(s.contains("any::<MyStruct>()"));
    }

    // ---- try_refine_strategy ----

    #[test]
    fn refine_neq_zero() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Neq,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let (param, strategy) = try_refine_strategy(&expr).unwrap();
        assert_eq!(param, "x");
        assert!(strategy.contains("1i64..=i64::MAX"));
    }

    #[test]
    fn refine_gt_zero() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("n".into())),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let (param, strategy) = try_refine_strategy(&expr).unwrap();
        assert_eq!(param, "n");
        assert!(strategy.contains("1i64..=i64::MAX"));
    }

    #[test]
    fn refine_gte_zero() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Gte,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        let (_, strategy) = try_refine_strategy(&expr).unwrap();
        assert!(strategy.contains("0i64..=i64::MAX"));
    }

    #[test]
    fn refine_lt_bound() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Lt,
            rhs: Box::new(Expr::Literal(Literal::Int("100".into()))),
        };
        let (_, strategy) = try_refine_strategy(&expr).unwrap();
        assert!(strategy.contains("100i64"));
        assert!(strategy.contains("i64::MIN"));
    }

    #[test]
    fn refine_lte_bound() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Ident("x".into())),
            op: BinOp::Lte,
            rhs: Box::new(Expr::Literal(Literal::Int("50".into()))),
        };
        let (_, strategy) = try_refine_strategy(&expr).unwrap();
        assert!(strategy.contains("=50i64"));
    }

    #[test]
    fn refine_non_ident_lhs_returns_none() {
        let expr = Expr::BinOp {
            lhs: Box::new(Expr::Literal(Literal::Int("1".into()))),
            op: BinOp::Gt,
            rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
        };
        assert!(try_refine_strategy(&expr).is_none());
    }

    #[test]
    fn refine_non_binop_returns_none() {
        let expr = Expr::Ident("x".into());
        assert!(try_refine_strategy(&expr).is_none());
    }

    // ---- contract_is_testable ----

    #[test]
    fn testable_contract_has_input_and_ensures() {
        let c = mk_contract(
            "Div",
            vec![
                mk_clause(ClauseKind::Input, Expr::Ident("x".into())),
                mk_clause(ClauseKind::Ensures, Expr::Literal(Literal::Bool(true))),
            ],
        );
        assert!(contract_is_testable(&c));
    }

    #[test]
    fn not_testable_missing_ensures() {
        let c = mk_contract(
            "Div",
            vec![mk_clause(ClauseKind::Input, Expr::Ident("x".into()))],
        );
        assert!(!contract_is_testable(&c));
    }

    #[test]
    fn not_testable_missing_input() {
        let c = mk_contract(
            "Div",
            vec![mk_clause(
                ClauseKind::Ensures,
                Expr::Literal(Literal::Bool(true)),
            )],
        );
        assert!(!contract_is_testable(&c));
    }

    // ---- extract_output_type ----

    #[test]
    fn output_type_from_cast() {
        let body = Expr::Cast {
            expr: Box::new(Expr::Ident("result".into())),
            ty: "Int".into(),
        };
        assert_eq!(extract_output_type(&body), "i64");
    }

    #[test]
    fn output_type_from_ident() {
        let body = Expr::Ident("Bool".into());
        assert_eq!(extract_output_type(&body), "bool");
    }

    #[test]
    fn output_type_from_paren() {
        let body = Expr::Paren(Box::new(Expr::Ident("Float".into())));
        assert_eq!(extract_output_type(&body), "f64");
    }

    #[test]
    fn output_type_from_raw_colon() {
        let body = Expr::Raw(vec!["result".into(), ":".into(), "Int".into()]);
        assert_eq!(extract_output_type(&body), "i64");
    }

    #[test]
    fn output_type_unknown_returns_unit() {
        let body = Expr::Literal(Literal::Int("42".into()));
        assert_eq!(extract_output_type(&body), "()");
    }

    // ---- extract_error_variants ----

    #[test]
    fn error_variants_single_ident() {
        let body = Expr::Ident("DivByZero".into());
        assert_eq!(extract_error_variants(&body), vec!["DivByZero"]);
    }

    #[test]
    fn error_variants_tuple() {
        let body = Expr::Tuple(vec![
            Expr::Ident("DivByZero".into()),
            Expr::Ident("Overflow".into()),
        ]);
        let vars = extract_error_variants(&body);
        assert_eq!(vars, vec!["DivByZero", "Overflow"]);
    }

    #[test]
    fn error_variants_raw_tokens() {
        let body = Expr::Raw(vec!["DivByZero".into(), ",".into(), "Overflow".into()]);
        let vars = extract_error_variants(&body);
        assert_eq!(vars, vec!["DivByZero", "Overflow"]);
    }

    #[test]
    fn error_variants_nested_paren() {
        let body = Expr::Paren(Box::new(Expr::Ident("Err".into())));
        assert_eq!(extract_error_variants(&body), vec!["Err"]);
    }

    // ---- collect_error_variants ----

    #[test]
    fn collect_errors_from_clauses() {
        let clauses = vec![
            mk_clause(ClauseKind::Requires, Expr::Literal(Literal::Bool(true))),
            mk_clause(ClauseKind::Errors, Expr::Ident("DivByZero".into())),
            mk_clause(ClauseKind::Errors, Expr::Ident("Overflow".into())),
        ];
        let vars = collect_error_variants(&clauses);
        assert_eq!(vars, vec!["DivByZero", "Overflow"]);
    }

    #[test]
    fn collect_errors_empty() {
        let clauses = vec![mk_clause(
            ClauseKind::Requires,
            Expr::Literal(Literal::Bool(true)),
        )];
        assert!(collect_error_variants(&clauses).is_empty());
    }

    // ---- generate_error_enum ----

    #[test]
    fn error_enum_basic() {
        let mut code = String::new();
        generate_error_enum("Div", &["DivByZero".into(), "Overflow".into()], &mut code);
        assert!(code.contains("pub enum DivError"));
        assert!(code.contains("#[derive(Debug, thiserror::Error)]"));
        assert!(code.contains("#[error(\"DivByZero\")]"));
        assert!(code.contains("DivByZero,"));
        assert!(code.contains("Overflow,"));
    }

    // ---- generate_contract ----

    #[test]
    fn contract_wraps_in_pub_mod() {
        let c = mk_contract(
            "SafeDiv",
            vec![mk_clause(
                ClauseKind::Requires,
                Expr::BinOp {
                    lhs: Box::new(Expr::Ident("b".into())),
                    op: BinOp::Neq,
                    rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                },
            )],
        );
        let mut code = String::new();
        generate_contract(&c, &mut code);
        assert!(code.contains("pub mod contract_safediv"));
        assert!(code.contains("/// Contract: SafeDiv"));
    }

    #[test]
    fn contract_interface_generates_trait() {
        let c = mk_contract(
            "Hashable",
            vec![mk_clause(
                ClauseKind::Other("interface".into()),
                Expr::Literal(Literal::Bool(true)),
            )],
        );
        let mut code = String::new();
        generate_contract(&c, &mut code);
        assert!(code.contains("pub trait Hashable"));
        assert!(!code.contains("pub mod"));
    }

    // ---- generate_contract_contents ----

    #[test]
    fn contract_contents_with_requires_and_ensures() {
        let c = mk_contract(
            "SafeDiv",
            vec![
                mk_clause(
                    ClauseKind::Requires,
                    Expr::BinOp {
                        lhs: Box::new(Expr::Ident("b".into())),
                        op: BinOp::Neq,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
                mk_clause(ClauseKind::Ensures, Expr::Literal(Literal::Bool(true))),
            ],
        );
        let mut code = String::new();
        generate_contract_contents(&c, &mut code);
        assert!(code.contains("pub fn check("));
        assert!(code.contains("debug_assert!"));
        assert!(code.contains("__result"));
    }

    #[test]
    fn contract_contents_with_errors_generates_result() {
        let c = mk_contract(
            "Div",
            vec![mk_clause(
                ClauseKind::Errors,
                Expr::Ident("DivByZero".into()),
            )],
        );
        let mut code = String::new();
        generate_contract_contents(&c, &mut code);
        assert!(code.contains("pub enum DivError"));
        assert!(code.contains("Result<"));
        assert!(code.contains("DivError"));
    }

    #[test]
    fn contract_contents_no_ensures_emits_todo() {
        let c = mk_contract("Simple", vec![]);
        let mut code = String::new();
        generate_contract_contents(&c, &mut code);
        assert!(code.contains("todo!(\"implementation provided by AI agent\")"));
    }

    #[test]
    fn contract_contents_with_implements() {
        let c = mk_contract(
            "MyImpl",
            vec![mk_clause(
                ClauseKind::Other("implements".into()),
                Expr::Ident("Hashable".into()),
            )],
        );
        let mut code = String::new();
        generate_contract_contents(&c, &mut code);
        assert!(code.contains("pub struct MyImpl;"));
        assert!(code.contains("impl Hashable for MyImpl"));
    }

    // ---- generate_interface_trait_from_contract ----

    #[test]
    fn interface_trait_simple() {
        let c = mk_contract(
            "Serializable",
            vec![
                mk_clause(
                    ClauseKind::Other("interface".into()),
                    Expr::Literal(Literal::Bool(true)),
                ),
                mk_clause(
                    ClauseKind::Other("method".into()),
                    Expr::Ident("serialize".into()),
                ),
            ],
        );
        let mut code = String::new();
        generate_interface_trait_from_contract(&c, &mut code);
        assert!(code.contains("pub trait Serializable"));
        assert!(code.contains("fn serialize(&self)"));
    }

    #[test]
    fn interface_trait_with_extends() {
        let c = mk_contract(
            "AdvHash",
            vec![
                mk_clause(
                    ClauseKind::Other("interface".into()),
                    Expr::Literal(Literal::Bool(true)),
                ),
                mk_clause(
                    ClauseKind::Other("extends".into()),
                    Expr::Ident("Hashable".into()),
                ),
            ],
        );
        let mut code = String::new();
        generate_interface_trait_from_contract(&c, &mut code);
        assert!(code.contains("pub trait AdvHash: Hashable"));
    }

    #[test]
    fn interface_trait_with_invariant() {
        let c = mk_contract(
            "Bounded",
            vec![
                mk_clause(
                    ClauseKind::Other("interface".into()),
                    Expr::Literal(Literal::Bool(true)),
                ),
                mk_clause(
                    ClauseKind::Invariant,
                    Expr::BinOp {
                        lhs: Box::new(Expr::Ident("x".into())),
                        op: BinOp::Gt,
                        rhs: Box::new(Expr::Literal(Literal::Int("0".into()))),
                    },
                ),
            ],
        );
        let mut code = String::new();
        generate_interface_trait_from_contract(&c, &mut code);
        assert!(code.contains("fn check_invariant(&self)"));
        assert!(code.contains("debug_assert!"));
    }

    // ---- extract_input_params ----

    #[test]
    fn extract_input_from_cast() {
        let body = Expr::Call {
            func: Box::new(Expr::Ident("input".into())),
            args: vec![Expr::Cast {
                expr: Box::new(Expr::Ident("x".into())),
                ty: "Int".into(),
            }],
        };
        let mut params = Vec::new();
        extract_input_params(&body, &mut params);
        // extract_clause_params from the parser handles this
        // Behavior depends on extract_clause_params implementation
        // At minimum, should not panic
    }

    #[test]
    fn extract_input_from_ident() {
        let body = Expr::Ident("x".into());
        let mut params = Vec::new();
        extract_input_params(&body, &mut params);
        // Single ident extraction depends on extract_clause_params
    }
}
