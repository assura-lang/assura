//! Service, typestate, and interface trait code generation.

use super::*;

// Service declarations
// ---------------------------------------------------------------------------

/// Generate a service operation or query method with proper parameter extraction.
///
/// Operations take `&mut self`, queries take `&self`. Both extract input params
/// and output types from their clauses for proper function signatures.
/// Extract a state name from a `self.state == StateName` pattern.
pub(crate) fn extract_state_comparison(body: &SpExpr) -> Option<String> {
    if let Expr::BinOp { lhs, op, rhs } = &body.node
        && matches!(op, BinOp::Eq)
    {
        // Check lhs is self.state
        let is_self_state = matches!(
            &lhs.as_ref().node,
            Expr::Field(recv, field) if matches!(&recv.as_ref().node, Expr::Ident(s) if s == "self") && field == "state"
        );
        if is_self_state && let Expr::Ident(state_name) = &rhs.as_ref().node {
            return Some(state_name.clone());
        }
    }
    None
}

pub(crate) fn generate_service_method(
    code: &mut String,
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    has_invariants: bool,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    // Extract input/output from clauses
    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut output_name: Option<String> = None;
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut modifies: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();
    let mut pre_state: Option<String> = None;
    let mut post_state: Option<String> = None;

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Input => {
                extract_input_params(&clause.body, &mut input_params);
            }
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
                output_name = extract_output_name(&clause.body);
            }
            ClauseKind::Requires => {
                // Check for state guard pattern: requires { self.state == X }
                if let Some(state) = extract_state_comparison(&clause.body) {
                    pre_state = Some(state);
                } else {
                    requires_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Ensures => {
                // Check for state transition pattern: ensures { self.state == X }
                if let Some(state) = extract_state_comparison(&clause.body) {
                    post_state = Some(state);
                } else {
                    ensures_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Modifies => {
                modifies.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Invariant => {
                invariants.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Effects
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Ordering
            | ClauseKind::Other(_) => {}
        }
    }

    let kind_label = if is_mutation { "Operation" } else { "Query" };
    code.push_str(&format!("        /// {kind_label}: {name}\n"));

    // Doc comments for requires/ensures/effects/modifies
    for clause in clauses {
        match clause.kind {
            ClauseKind::Requires => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Requires: {expr}\n"));
            }
            ClauseKind::Ensures => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Ensures: {expr}\n"));
            }
            ClauseKind::Effects => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Effects: {expr}\n"));
            }
            ClauseKind::Modifies => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Modifies: {expr}\n"));
            }
            ClauseKind::Ordering => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("        /// Ordering: {expr}\n"));
                if let Some(ord) = resolve_ordering_variant(&clause.body) {
                    code.push_str(&format!(
                        "        const ORDERING: std::sync::atomic::Ordering = std::sync::atomic::Ordering::{ord};\n"
                    ));
                }
            }
            // Input/Output are handled in the signature generation.
            // Other clause kinds don't produce doc comments.
            ClauseKind::Input
            | ClauseKind::Output
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    // Build function signature
    let self_param = if is_mutation { "&mut self" } else { "&self" };
    let extra_params = if input_params.is_empty() {
        String::new()
    } else {
        let ps: Vec<String> = input_params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect();
        format!(", {}", ps.join(", "))
    };
    let ret_sig = if output_type == "()" {
        String::new()
    } else {
        format!(" -> {output_type}")
    };

    code.push_str(&format!(
        "        pub fn {name}({self_param}{extra_params}){ret_sig} {{\n"
    ));

    // Invariant check at entry
    if has_invariants {
        code.push_str("            self.check_invariant();\n");
    }

    // State pre-condition guard
    if let Some(ref state) = pre_state {
        code.push_str(&format!(
            "            debug_assert_eq!(self.state, State::{state}, \"requires state {state}\");\n"
        ));
    }

    // Requires assertions
    for req in &requires_exprs {
        generate_debug_assert_indented(code, req, "requires", 3);
    }

    let ir_body = ir_bodies.and_then(|m| m.get(name));

    if output_type == "()" {
        // State transition
        if let Some(ref state) = post_state {
            code.push_str(&format!("            self.state = State::{state};\n"));
        }
        if let Some(body) = ir_body {
            code.push_str(body);
        } else {
            code.push_str(&format!(
                "            todo!(\"{} implementation\")\n",
                kind_label.to_lowercase()
            ));
        }
        // Operation-level invariant assertions
        for inv in &invariants {
            generate_debug_assert_indented(code, inv, "invariant", 3);
        }
        // Invariant check at exit (for void operations)
        if has_invariants {
            code.push_str("            self.check_invariant();\n");
        }
    } else {
        if let Some(body) = ir_body {
            code.push_str(body);
        } else {
            code.push_str(&format!(
                "            let {result_var}: {output_type} = todo!(\"{} implementation\");\n",
                kind_label.to_lowercase(),
                result_var = RESULT_VAR
            ));
        }
        // Bind the output variable name so ensures clauses can reference it
        if let Some(ref name) = output_name {
            code.push_str(&format!("            let {name} = {RESULT_VAR}.clone();\n"));
        }
        // Ensures assertions
        for ens in &ensures_exprs {
            generate_debug_assert_indented(code, ens, "ensures", 3);
        }
        // Operation-level invariant assertions
        for inv in &invariants {
            generate_debug_assert_indented(code, inv, "invariant", 3);
        }
        // State transition
        if let Some(ref state) = post_state {
            code.push_str(&format!("            self.state = State::{state};\n"));
        }
        // Invariant check at exit
        if has_invariants {
            code.push_str("            self.check_invariant();\n");
        }
        code.push_str(&format!("            {RESULT_VAR}\n"));
    }

    code.push_str("        }\n\n");
}

/// Generate a service method for typestate-encoded services.
///
/// State transitions consume `self` and return `ServiceName<NewState>`.
/// Pre-state guards are enforced by the type system (the method only
/// exists on `impl ServiceName<PreState>`), so no runtime assertions.
pub(crate) fn generate_typestate_method(
    code: &mut String,
    service_name: &str,
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    _has_invariants: bool,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut output_name: Option<String> = None;
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();
    let mut post_state: Option<String> = None;

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Input => {
                extract_input_params(&clause.body, &mut input_params);
            }
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
                output_name = extract_output_name(&clause.body);
            }
            ClauseKind::Requires => {
                // State guards are encoded in the type, skip them
                if extract_state_comparison(&clause.body).is_none() {
                    requires_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Ensures => {
                if let Some(state) = extract_state_comparison(&clause.body) {
                    post_state = Some(state);
                } else {
                    ensures_exprs.push(expr_to_rust(&clause.body));
                }
            }
            ClauseKind::Invariant => {
                invariants.push(expr_to_rust(&clause.body));
            }
            ClauseKind::Modifies
            | ClauseKind::Effects
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Ordering
            | ClauseKind::Other(_) => {}
        }
    }

    let kind_label = if is_mutation { "Operation" } else { "Query" };
    code.push_str(&format!("/// {kind_label}: {name}\n"));

    // Doc comments
    for clause in clauses {
        match clause.kind {
            ClauseKind::Requires => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Requires: {expr}\n"));
            }
            ClauseKind::Ensures => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Ensures: {expr}\n"));
            }
            ClauseKind::Effects => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Effects: {expr}\n"));
            }
            ClauseKind::Modifies => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Modifies: {expr}\n"));
            }
            ClauseKind::Ordering => {
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!("/// Ordering: {expr}\n"));
                if let Some(ord) = resolve_ordering_variant(&clause.body) {
                    code.push_str(&format!(
                        "const ORDERING: std::sync::atomic::Ordering = std::sync::atomic::Ordering::{ord};\n"
                    ));
                }
            }
            ClauseKind::Input
            | ClauseKind::Output
            | ClauseKind::Invariant
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Other(_) => {}
        }
    }

    // Determine self parameter and return type based on state transition
    let has_transition = post_state.is_some();
    let self_param = if has_transition {
        "self" // consume self for state transitions
    } else if is_mutation {
        "&mut self"
    } else {
        "&self"
    };

    let extra_params = if input_params.is_empty() {
        String::new()
    } else {
        let ps: Vec<String> = input_params
            .iter()
            .map(|(n, t)| format!("{n}: {t}"))
            .collect();
        format!(", {}", ps.join(", "))
    };

    let ret_sig = if let Some(ref new_state) = post_state {
        format!(" -> {service_name}<{new_state}>")
    } else if output_type == "()" {
        String::new()
    } else {
        format!(" -> {output_type}")
    };

    code.push_str(&format!(
        "pub fn {name}({self_param}{extra_params}){ret_sig} {{\n"
    ));

    // Requires assertions (non-state-guard ones)
    for req in &requires_exprs {
        generate_debug_assert_indented(code, req, "requires", 1);
    }

    // For state transitions, todo!() coerces to the return type
    // For non-transitions, standard pattern
    // Invariant assertions (emitted before the body in all cases)
    for inv in &invariants {
        generate_debug_assert_indented(code, inv, "invariant", 1);
    }

    let ir_body = ir_bodies.and_then(|m| m.get(name));

    if post_state.is_some() || output_type == "()" {
        if let Some(body) = ir_body {
            code.push_str(body);
        } else {
            code.push_str(&format!(
                "    todo!(\"{} implementation\")\n",
                kind_label.to_lowercase()
            ));
        }
    } else {
        if let Some(body) = ir_body {
            code.push_str(body);
        } else {
            code.push_str(&format!(
                "    let {result_var}: {output_type} = todo!(\"{} implementation\");\n",
                kind_label.to_lowercase(),
                result_var = RESULT_VAR
            ));
        }
        // Bind the output variable name so ensures clauses can reference it
        if let Some(ref name) = output_name {
            code.push_str(&format!("    let {name} = {RESULT_VAR}.clone();\n"));
        }
        for ens in &ensures_exprs {
            generate_debug_assert_indented(code, ens, "ensures", 1);
        }
        code.push_str(&format!("    {RESULT_VAR}\n"));
    }

    code.push_str("}\n");
}

/// Collect state names from a ServiceDecl.
pub(crate) fn collect_service_states(s: &ServiceDecl) -> Vec<String> {
    s.items
        .iter()
        .find_map(|i| match i {
            ServiceItem::States(states) => Some(states.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Extract pre_state from a method's clauses (the state guard in requires).
pub(crate) fn method_pre_state(clauses: &[Clause]) -> Option<String> {
    clauses.iter().find_map(|c| {
        if matches!(c.kind, ClauseKind::Requires) {
            extract_state_comparison(&c.body)
        } else {
            None
        }
    })
}

/// Generate typestate-encoded service body (marker structs, generic struct,
/// state-specific impl blocks). Used when the service declares states.
pub(crate) fn generate_typestate_service_body(
    s: &ServiceDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    let states = collect_service_states(s);
    let has_invariants = s
        .items
        .iter()
        .any(|i| matches!(i, ServiceItem::Invariant(_)));

    // Generate nested type/enum definitions
    for item in &s.items {
        match item {
            ServiceItem::TypeDef(t) => generate_type_def(t, code),
            ServiceItem::EnumDef(e) => generate_enum_def(e, code),
            ServiceItem::States(_)
            | ServiceItem::Operation { .. }
            | ServiceItem::Query { .. }
            | ServiceItem::Invariant(_)
            | ServiceItem::Other { .. } => {}
        }
    }

    // State marker structs
    for state in &states {
        code.push_str(&format!("/// State marker: {state}\npub struct {state};\n"));
    }
    code.push('\n');

    // Generic service struct with PhantomData
    code.push_str(&format!(
        "#[derive(Debug)]\npub struct {}<State> {{\n    _state: std::marker::PhantomData<State>,\n}}\n\n",
        s.name
    ));

    // Group methods by pre_state
    struct MethodRef<'a> {
        name: &'a str,
        clauses: &'a [Clause],
        is_mutation: bool,
    }

    let mut state_methods: Vec<(Option<String>, Vec<MethodRef<'_>>)> = Vec::new();
    let mut invariant_exprs: Vec<&SpExpr> = Vec::new();
    let mut other_items: Vec<(&str, &SpExpr)> = Vec::new();

    // Build ordered grouping: preserve state order from declaration
    let mut state_order: Vec<Option<String>> = Vec::new();
    // First entry: initial state (for new())
    if let Some(first) = states.first() {
        state_order.push(Some(first.clone()));
    }
    // Remaining states
    for state in states.iter().skip(1) {
        state_order.push(Some(state.clone()));
    }
    // Generic (None) for state-independent methods
    state_order.push(None);

    for key in &state_order {
        state_methods.push((key.clone(), Vec::new()));
    }

    for item in &s.items {
        match item {
            ServiceItem::Operation { name, clauses } => {
                let pre = method_pre_state(clauses);
                let method = MethodRef {
                    name,
                    clauses,
                    is_mutation: true,
                };
                if let Some(group) = state_methods.iter_mut().find(|(k, _)| *k == pre) {
                    group.1.push(method);
                } else {
                    // State not in declared list; add to generic
                    if let Some(group) = state_methods.iter_mut().find(|(k, _)| k.is_none()) {
                        group.1.push(method);
                    }
                }
            }
            ServiceItem::Query { name, clauses } => {
                let pre = method_pre_state(clauses);
                let method = MethodRef {
                    name,
                    clauses,
                    is_mutation: false,
                };
                if let Some(group) = state_methods.iter_mut().find(|(k, _)| *k == pre) {
                    group.1.push(method);
                } else {
                    if let Some(group) = state_methods.iter_mut().find(|(k, _)| k.is_none()) {
                        group.1.push(method);
                    }
                }
            }
            ServiceItem::Invariant(expr) => invariant_exprs.push(expr),
            ServiceItem::Other { kind, body } => other_items.push((kind, body)),
            ServiceItem::TypeDef(_) | ServiceItem::EnumDef(_) | ServiceItem::States(_) => {}
        }
    }

    let initial_state = states
        .first()
        .cloned()
        .unwrap_or_else(|| "Default".to_string());

    // Generate impl blocks per state
    for (state_key, methods) in &state_methods {
        match state_key {
            Some(state_name) => {
                let is_initial = *state_name == initial_state;
                if methods.is_empty() && !is_initial {
                    continue;
                }
                code.push_str(&format!("impl {}<{state_name}> {{\n", s.name));
                if is_initial {
                    code.push_str(
                        "pub fn new() -> Self { Self { _state: std::marker::PhantomData } }\n",
                    );
                }
                for method in methods {
                    generate_typestate_method(
                        code,
                        &s.name,
                        method.name,
                        method.clauses,
                        method.is_mutation,
                        has_invariants,
                        ir_bodies,
                    );
                }
                code.push_str("}\n\n");
            }
            None => {
                // Generic impl block for state-independent methods + invariants
                if methods.is_empty() && invariant_exprs.is_empty() && other_items.is_empty() {
                    continue;
                }
                code.push_str(&format!("impl<S> {}<S> {{\n", s.name));
                for method in methods {
                    generate_typestate_method(
                        code,
                        &s.name,
                        method.name,
                        method.clauses,
                        method.is_mutation,
                        has_invariants,
                        ir_bodies,
                    );
                }
                for expr in &invariant_exprs {
                    let rust_expr = expr_to_rust(expr);
                    code.push_str(&format!(
                        "/// Service invariant\npub fn check_invariant(&self) {{ debug_assert!({rust_expr}); }}\n"
                    ));
                }
                for (kind, body) in &other_items {
                    let rust_expr = expr_to_rust(body);
                    code.push_str(&format!("// {kind}: {rust_expr}\n"));
                }
                code.push_str("}\n\n");
            }
        }
    }
}

/// Generate service body as standalone module contents (no `pub mod` wrapper).
/// Used in multi-file mode where each service gets its own `.rs` file.
pub(crate) fn generate_service_contents(
    s: &ServiceDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    let has_states = s.items.iter().any(|i| matches!(i, ServiceItem::States(_)));

    if has_states {
        generate_typestate_service_body(s, code, ir_bodies);
        return;
    }

    // Stateless service: simple struct + impl block
    for item in &s.items {
        match item {
            ServiceItem::TypeDef(t) => generate_type_def(t, code),
            ServiceItem::EnumDef(e) => generate_enum_def(e, code),
            ServiceItem::States(_)
            | ServiceItem::Operation { .. }
            | ServiceItem::Query { .. }
            | ServiceItem::Invariant(_)
            | ServiceItem::Other { .. } => {}
        }
    }

    code.push_str(&format!("#[derive(Debug)]\npub struct {} {{\n", s.name));
    code.push_str("}\n\n");

    code.push_str(&format!("impl {} {{\n", s.name));
    code.push_str("    pub fn new() -> Self {\n        Self { }\n    }\n\n");

    let has_invariants = s
        .items
        .iter()
        .any(|i| matches!(i, ServiceItem::Invariant(_)));

    for item in &s.items {
        match item {
            ServiceItem::Operation { name, clauses } => {
                generate_service_method(code, name, clauses, true, has_invariants, ir_bodies);
            }
            ServiceItem::Query { name, clauses } => {
                generate_service_method(code, name, clauses, false, has_invariants, ir_bodies);
            }
            ServiceItem::Invariant(expr) => {
                let rust_expr = expr_to_rust(expr);
                code.push_str(&format!(
                    "    /// Service invariant\n    pub fn check_invariant(&self) {{ debug_assert!({rust_expr}); }}\n\n"
                ));
            }
            ServiceItem::Other { kind, body } => {
                let rust_expr = expr_to_rust(body);
                code.push_str(&format!("    // {kind}: {rust_expr}\n\n"));
            }
            ServiceItem::TypeDef(_) | ServiceItem::EnumDef(_) | ServiceItem::States(_) => {}
        }
    }

    code.push_str("}\n"); // close impl
}

pub(crate) fn generate_service(
    s: &ServiceDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    code.push_str(&format!(
        "/// Service: {}\npub mod {} {{\n",
        s.name,
        s.name.to_lowercase()
    ));

    // Generate the service body (typestate or classic), then indent it
    let mut inner = String::new();
    generate_service_contents(s, &mut inner, ir_bodies);
    for line in inner.lines() {
        if line.is_empty() {
            code.push('\n');
        } else {
            code.push_str(&format!("    {line}\n"));
        }
    }

    code.push_str("}\n\n"); // close mod
}

// ---------------------------------------------------------------------------
// Interface contracts -> Rust traits (T062)
// ---------------------------------------------------------------------------

/// Generate a Rust trait from an Assura interface block.
///
/// Interface blocks contain `method` clauses that declare required
/// methods, and `extends` clauses that declare supertrait bounds.
/// Generates a Rust trait with the declared methods.
pub(crate) fn generate_interface_trait(name: &str, body: &[Clause], code: &mut String) {
    // Collect extends (supertraits)
    let extends: Vec<String> = body
        .iter()
        .filter(|c| matches!(&c.kind, ClauseKind::Other(k) if k == "extends"))
        .filter_map(|c| {
            if let Expr::Ident(n) = &c.body.node {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();

    // Build trait header with supertraits
    if extends.is_empty() {
        code.push_str(&format!(
            "/// Interface contract: {name}\npub trait {name} {{\n"
        ));
    } else {
        let bounds = extends.join(" + ");
        code.push_str(&format!(
            "/// Interface contract: {name}\npub trait {name}: {bounds} {{\n"
        ));
    }

    // Collect method declarations
    for clause in body {
        match &clause.kind {
            ClauseKind::Other(k) if k == "method" => {
                generate_trait_method(&clause.body, code);
            }
            ClauseKind::Invariant | ClauseKind::Ensures => {
                // Interface invariants become provided methods with assertions
                let expr = expr_to_rust(&clause.body);
                code.push_str(&format!(
                    "    /// Interface invariant\n    fn check_invariant(&self) {{ debug_assert!({expr}); }}\n\n"
                ));
            }
            // Interface blocks only use method and invariant clauses.
            // Other clause kinds are ignored in trait generation.
            ClauseKind::Requires
            | ClauseKind::Effects
            | ClauseKind::Modifies
            | ClauseKind::Input
            | ClauseKind::Output
            | ClauseKind::Errors
            | ClauseKind::Rule
            | ClauseKind::DataFlow
            | ClauseKind::MustNot
            | ClauseKind::Decreases
            | ClauseKind::Ordering
            | ClauseKind::Other(_) => {}
        }
    }

    code.push_str("}\n\n");
}

/// Generate a single trait method declaration from an interface method clause.
pub(crate) fn generate_trait_method(body: &SpExpr, code: &mut String) {
    match &body.node {
        Expr::Ident(name) => {
            // Simple method with no params: fn name(&self);
            code.push_str(&format!("    fn {name}(&self);\n\n"));
        }
        Expr::Call { func, args } => {
            // Method with params: fn name(&self, param: Type, ...) -> RetType
            let func_name = if let Expr::Ident(n) = &func.as_ref().node {
                n.clone()
            } else {
                "unknown".to_string()
            };
            let params: String = args
                .iter()
                .enumerate()
                .map(|(i, arg)| {
                    if let Expr::Ident(ty) = &arg.node {
                        format!("arg{i}: {}", map_type_token(ty))
                    } else {
                        format!("arg{i}: i64")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            if params.is_empty() {
                code.push_str(&format!("    fn {func_name}(&self);\n\n"));
            } else {
                code.push_str(&format!("    fn {func_name}(&self, {params});\n\n"));
            }
        }
        Expr::Raw(tokens) => {
            // Parse method from raw tokens: "name(Type, Type) -> RetType"
            if let Some((name, rest)) = tokens.first().map(|n| (n.clone(), &tokens[1..])) {
                let mut params = Vec::new();
                let mut return_type = String::new();
                let mut i = 0;
                let mut in_params = false;

                while i < rest.len() {
                    let tok = &rest[i];
                    if tok == "(" {
                        in_params = true;
                        i += 1;
                        continue;
                    }
                    if tok == ")" {
                        in_params = false;
                        i += 1;
                        continue;
                    }
                    if tok == "->" {
                        i += 1;
                        if i < rest.len() {
                            return_type = map_type_token(&rest[i]).to_string();
                        }
                        break;
                    }
                    if tok == "," {
                        i += 1;
                        continue;
                    }
                    if in_params {
                        // Check for "name: Type" pattern
                        if i + 2 < rest.len() && rest[i + 1] == ":" {
                            let pname = tok.clone();
                            let ptype = map_type_token(&rest[i + 2]).to_string();
                            params.push(format!("{pname}: {ptype}"));
                            i += 3;
                            continue;
                        }
                        // Just a type name
                        let ptype = map_type_token(tok).to_string();
                        params.push(format!("arg{}: {ptype}", params.len()));
                    }
                    i += 1;
                }

                let params_s = if params.is_empty() {
                    String::new()
                } else {
                    format!(", {}", params.join(", "))
                };

                if return_type.is_empty() {
                    code.push_str(&format!("    fn {name}(&self{params_s});\n\n"));
                } else {
                    code.push_str(&format!(
                        "    fn {name}(&self{params_s}) -> {return_type};\n\n"
                    ));
                }
            }
        }
        // These expression forms are not valid trait method declarations;
        // emit a compile_error! so the generated Rust surfaces the issue.
        Expr::Literal(_)
        | Expr::Field(_, _)
        | Expr::MethodCall { .. }
        | Expr::Index { .. }
        | Expr::BinOp { .. }
        | Expr::UnaryOp { .. }
        | Expr::Old(_)
        | Expr::Forall { .. }
        | Expr::Exists { .. }
        | Expr::If { .. }
        | Expr::List(_)
        | Expr::Cast { .. }
        | Expr::Block(_)
        | Expr::Ghost(_)
        | Expr::Apply { .. }
        | Expr::Let { .. }
        | Expr::Match { .. }
        | Expr::Tuple(_) => {
            code.push_str(&format!(
                "    compile_error!(\"unsupported expression in trait method: {:?}\");\n\n",
                std::mem::discriminant(&body.node)
            ));
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use assura_ast::Spanned;

    fn mk_clause(kind: ClauseKind, body: SpExpr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    // ---- extract_state_comparison ----

    #[test]
    fn state_comparison_match() {
        // self.state == Open
        let body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                "state".into(),
            ))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
        });
        assert_eq!(extract_state_comparison(&body), Some("Open".into()));
    }

    #[test]
    fn state_comparison_not_self() {
        let body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("other".into()))),
                "state".into(),
            ))),
            op: BinOp::Eq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
        });
        assert_eq!(extract_state_comparison(&body), None);
    }

    #[test]
    fn state_comparison_not_eq() {
        let body = Spanned::no_span(Expr::BinOp {
            lhs: Box::new(Spanned::no_span(Expr::Field(
                Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                "state".into(),
            ))),
            op: BinOp::Neq,
            rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
        });
        assert_eq!(extract_state_comparison(&body), None);
    }

    // ---- collect_service_states ----

    #[test]
    fn collect_states_present() {
        let s = ServiceDecl {
            name: "MyService".into(),
            items: vec![ServiceItem::States(vec![
                "Init".into(),
                "Running".into(),
                "Done".into(),
            ])],
        };
        assert_eq!(collect_service_states(&s), vec!["Init", "Running", "Done"]);
    }

    #[test]
    fn collect_states_none() {
        let s = ServiceDecl {
            name: "Simple".into(),
            items: vec![],
        };
        assert!(collect_service_states(&s).is_empty());
    }

    // ---- method_pre_state ----

    #[test]
    fn pre_state_found() {
        let clauses = vec![mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                    "state".into(),
                ))),
                op: BinOp::Eq,
                rhs: Box::new(Spanned::no_span(Expr::Ident("Init".into()))),
            }),
        )];
        assert_eq!(method_pre_state(&clauses), Some("Init".into()));
    }

    #[test]
    fn pre_state_not_found() {
        let clauses = vec![mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
        )];
        assert_eq!(method_pre_state(&clauses), None);
    }

    // ---- generate_service_method ----

    #[test]
    fn service_method_operation_mut_self() {
        let mut code = String::new();
        generate_service_method(&mut code, "process", &[], true, false, None);
        assert!(code.contains("&mut self"), "operation uses &mut self");
        assert!(code.contains("pub fn process"));
    }

    #[test]
    fn service_method_query_ref_self() {
        let mut code = String::new();
        generate_service_method(&mut code, "get_value", &[], false, false, None);
        assert!(code.contains("&self"), "query uses &self");
        assert!(code.contains("pub fn get_value"));
    }

    #[test]
    fn service_method_with_invariant_check() {
        let mut code = String::new();
        generate_service_method(&mut code, "do_it", &[], true, true, None);
        assert!(
            code.contains("self.check_invariant()"),
            "invariant check on entry/exit"
        );
    }

    #[test]
    fn service_method_with_state_guard() {
        let clauses = vec![mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                    "state".into(),
                ))),
                op: BinOp::Eq,
                rhs: Box::new(Spanned::no_span(Expr::Ident("Ready".into()))),
            }),
        )];
        let mut code = String::new();
        generate_service_method(&mut code, "start", &clauses, true, false, None);
        assert!(
            code.contains("State::Ready"),
            "state guard should be in code"
        );
    }

    #[test]
    fn service_method_with_state_transition() {
        let clauses = vec![mk_clause(
            ClauseKind::Ensures,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                    "state".into(),
                ))),
                op: BinOp::Eq,
                rhs: Box::new(Spanned::no_span(Expr::Ident("Running".into()))),
            }),
        )];
        let mut code = String::new();
        generate_service_method(&mut code, "start", &clauses, true, false, None);
        assert!(
            code.contains("State::Running"),
            "state transition in output"
        );
    }

    // ---- generate_service (stateless) ----

    #[test]
    fn service_stateless_struct_and_impl() {
        let s = ServiceDecl {
            name: "Counter".into(),
            items: vec![ServiceItem::Operation {
                name: "increment".into(),
                clauses: vec![],
            }],
        };
        let mut code = String::new();
        generate_service(&s, &mut code, None);
        assert!(code.contains("pub mod counter"));
        assert!(code.contains("pub struct Counter"));
        assert!(code.contains("pub fn new()"));
        assert!(code.contains("pub fn increment"));
    }

    // ---- generate_service (typestate) ----

    #[test]
    fn service_typestate_has_marker_structs() {
        let s = ServiceDecl {
            name: "Conn".into(),
            items: vec![
                ServiceItem::States(vec!["Closed".into(), "Open".into()]),
                ServiceItem::Operation {
                    name: "open".into(),
                    clauses: vec![
                        mk_clause(
                            ClauseKind::Requires,
                            Spanned::no_span(Expr::BinOp {
                                lhs: Box::new(Spanned::no_span(Expr::Field(
                                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                                    "state".into(),
                                ))),
                                op: BinOp::Eq,
                                rhs: Box::new(Spanned::no_span(Expr::Ident("Closed".into()))),
                            }),
                        ),
                        mk_clause(
                            ClauseKind::Ensures,
                            Spanned::no_span(Expr::BinOp {
                                lhs: Box::new(Spanned::no_span(Expr::Field(
                                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                                    "state".into(),
                                ))),
                                op: BinOp::Eq,
                                rhs: Box::new(Spanned::no_span(Expr::Ident("Open".into()))),
                            }),
                        ),
                    ],
                },
            ],
        };
        let mut code = String::new();
        generate_service(&s, &mut code, None);
        assert!(code.contains("pub struct Closed;"), "Closed marker");
        assert!(code.contains("pub struct Open;"), "Open marker");
        assert!(code.contains("PhantomData"), "generic state param");
        assert!(code.contains("impl Conn<Closed>"), "initial state impl");
        assert!(code.contains("fn new()"), "new() on initial state");
        assert!(code.contains("-> Conn<Open>"), "state transition return");
    }

    // ---- generate_interface_trait ----

    #[test]
    fn interface_simple_method() {
        let clauses = vec![mk_clause(
            ClauseKind::Other("method".into()),
            Spanned::no_span(Expr::Ident("do_something".into())),
        )];
        let mut code = String::new();
        generate_interface_trait("Doable", &clauses, &mut code);
        assert!(code.contains("pub trait Doable"));
        assert!(code.contains("fn do_something(&self);"));
    }

    #[test]
    fn interface_with_extends() {
        let clauses = vec![
            mk_clause(
                ClauseKind::Other("extends".into()),
                Spanned::no_span(Expr::Ident("Base".into())),
            ),
            mk_clause(
                ClauseKind::Other("method".into()),
                Spanned::no_span(Expr::Ident("extra".into())),
            ),
        ];
        let mut code = String::new();
        generate_interface_trait("Extended", &clauses, &mut code);
        assert!(
            code.contains("pub trait Extended: Base"),
            "supertrait bound"
        );
    }

    #[test]
    fn interface_invariant_becomes_provided_method() {
        let clauses = vec![mk_clause(
            ClauseKind::Invariant,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
        )];
        let mut code = String::new();
        generate_interface_trait("Positive", &clauses, &mut code);
        assert!(code.contains("fn check_invariant(&self)"));
        assert!(code.contains("debug_assert!"));
    }

    // ---- generate_trait_method ----

    #[test]
    fn trait_method_ident() {
        let mut code = String::new();
        let body = Spanned::no_span(Expr::Ident("compute".into()));
        generate_trait_method(&body, &mut code);
        assert!(code.contains("fn compute(&self);"));
    }

    #[test]
    fn trait_method_call_with_args() {
        let body = Spanned::no_span(Expr::Call {
            func: Box::new(Spanned::no_span(Expr::Ident("process".into()))),
            args: vec![
                Spanned::no_span(Expr::Ident("Int".into())),
                Spanned::no_span(Expr::Ident("Bool".into())),
            ],
        });
        let mut code = String::new();
        generate_trait_method(&body, &mut code);
        assert!(code.contains("fn process(&self, arg0: i64, arg1: bool)"));
    }

    #[test]
    fn trait_method_unsupported_expr() {
        let body = Spanned::no_span(Expr::Literal(Literal::Int("42".into())));
        let mut code = String::new();
        generate_trait_method(&body, &mut code);
        assert!(code.contains("compile_error!"));
    }
}
