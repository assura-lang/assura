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
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut effects: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Output => output_type = extract_output_type(&clause.body),
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

    if ensures_exprs.is_empty() && invariants.is_empty() {
        code.push_str("    todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {output_type} = todo!(\"implementation provided by AI agent\");\n"
        ));
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
    // Check if this contract is an interface declaration
    let is_interface = c
        .clauses
        .iter()
        .any(|cl| matches!(&cl.kind, ClauseKind::Other(k) if k == "interface"));
    if is_interface {
        generate_interface_trait_from_contract(c, code);
        return;
    }

    // Check if this contract implements an interface
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

    code.push_str(&format!(
        "/// Contract: {}\npub mod contract_{} {{\n",
        c.name,
        c.name.to_lowercase()
    ));

    // Extract input params and output type from clauses
    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();

    let mut effects: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();

    for clause in &c.clauses {
        match &clause.kind {
            ClauseKind::Input => {
                extract_input_params(&clause.body, &mut input_params);
            }
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
            }
            ClauseKind::Requires => {
                requires_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Ensures => {
                ensures_exprs.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Effects => {
                effects.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Modifies => {
                modifies.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Invariant => {
                invariants.push(expr_to_rust(&clause.body));
            }
            // Other clause kinds don't produce direct codegen output.
            ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Ordering
            | ClauseKind::Other(_) => {}
        }
    }

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&c.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", c.name);
        code.push_str("    ");
        // Generate the enum inside the module (indented)
        let mut enum_code = String::new();
        generate_error_enum(&c.name, &error_variants, &mut enum_code);
        // Indent each line for the module context
        for line in enum_code.lines() {
            code.push_str(&format!("    {line}\n"));
        }
        code.push('\n');
        Some(name)
    } else {
        None
    };

    // Determine return type
    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{output_type}, {err_name}>")
    } else {
        output_type.clone()
    };

    // Generate doc comments for requires, effects, and modifies
    for req in &requires_exprs {
        code.push_str(&format!("    /// Requires: {req}\n"));
    }
    for eff in &effects {
        code.push_str(&format!("    /// Effects: {eff}\n"));
    }
    for m in &modifies {
        code.push_str(&format!("    /// Modifies: {m}\n"));
    }

    // Generate the contract function signature
    let params_s: String = input_params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");

    code.push_str(&format!(
        "    pub fn check{tps}({params_s}) -> {return_type} {{\n"
    ));

    // Collect old() expressions from ensures clauses and save pre-state values
    for clause in &c.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("        let __old_{var} = {rust_expr}.clone();\n"));
            }
        }
    }

    // Generate requires assertions
    for req in &requires_exprs {
        generate_debug_assert_indented(code, req, "requires", 2);
    }

    if ensures_exprs.is_empty() && invariants.is_empty() {
        code.push_str("        todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "        let __result: {output_type} = todo!(\"implementation provided by AI agent\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert_indented(code, ens, "ensures", 2);
        }
        for inv in &invariants {
            generate_debug_assert_indented(code, inv, "invariant", 2);
        }
        if error_enum_name.is_some() {
            code.push_str("        Ok(__result)\n");
        } else {
            code.push_str("        __result\n");
        }
    }
    code.push_str("    }\n");

    // Generate struct + impl Trait if the contract implements an interface
    if !implements.is_empty() {
        // Generate a struct for this contract
        code.push_str(&format!("\n    pub struct {}{tps};\n\n", c.name));
        // Generate impl blocks for each implemented trait
        for iface in &implements {
            code.push_str(&format!("    impl{tps} {iface} for {}{tps} {{\n", c.name));
            // Extract method clauses and generate stubs
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
                        code.push_str(&format!("        fn {method_name}(&self) {{ todo!() }}\n"));
                    }
                }
            }
            code.push_str("    }\n");
        }
    }

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
    if !contract_is_testable(c) {
        return;
    }

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut requires_ast: Vec<&Expr> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();

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
            ClauseKind::Output
            | ClauseKind::Effects
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

    // Build refined strategies from requires constraints
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
    let contract_mod = format!("contract_{fn_name}");

    code.push_str("#[cfg(test)]\n");
    code.push_str(&format!("mod proptest_{fn_name} {{\n"));
    code.push_str("    use proptest::prelude::*;\n\n");
    code.push_str("    proptest! {\n");
    code.push_str("        #[test]\n");

    // Build parameter list with strategies
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

    // Emit prop_assume! for requires that could not be encoded as strategies
    for req in &unrefined_requires {
        code.push_str(&format!("            prop_assume!({req});\n"));
    }

    // Call the contract check function
    let call_args: Vec<&str> = input_params.iter().map(|(n, _)| n.as_str()).collect();
    code.push_str(&format!(
        "            let result = super::{contract_mod}::check({});\n",
        call_args.join(", ")
    ));

    // Emit prop_assert! for each ensures clause
    for ens in &ensures_exprs {
        code.push_str(&format!("            prop_assert!({ens});\n"));
    }

    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n\n");
}

/// Generate proptest for a contract in multi-file mode (the test module
/// is inside the contract's own .rs file, so the call is `super::check()`).
pub(crate) fn generate_proptest_for_contract_contents(c: &ContractDecl, code: &mut String) {
    if !contract_is_testable(c) {
        return;
    }

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut requires_ast: Vec<&Expr> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();

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
            ClauseKind::Output
            | ClauseKind::Effects
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
        "            let result = super::check({});\n",
        call_args.join(", ")
    ));

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

