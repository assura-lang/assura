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

#[cfg(test)]
pub(crate) fn generate_service_method(
    code: &mut String,
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    has_invariants: bool,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    use crate::hir::*;

    let method = build_service_method_fn(name, clauses, is_mutation, has_invariants, ir_bodies);
    code.push_str(&render_item_raw(&RustItem::Fn(method)));
}

/// Build a `RustFn` for a service operation or query method.
fn build_service_method_fn(
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    has_invariants: bool,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) -> crate::hir::RustFn {
    use crate::hir::*;

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut output_name: Option<String> = None;
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
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
                if let Some(state) = extract_state_comparison(&clause.body) {
                    pre_state = Some(state);
                } else {
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
            _ => {}
        }
    }

    let kind_label = if is_mutation { "Operation" } else { "Query" };

    // Build doc comments
    let mut doc: Vec<String> = vec![format!("{kind_label}: {name}")];
    for clause in clauses {
        match clause.kind {
            ClauseKind::Requires => doc.push(format!("Requires: {}", expr_to_rust(&clause.body))),
            ClauseKind::Ensures => doc.push(format!("Ensures: {}", expr_to_rust(&clause.body))),
            ClauseKind::Effects => doc.push(format!("Effects: {}", expr_to_rust(&clause.body))),
            ClauseKind::Modifies => doc.push(format!("Modifies: {}", expr_to_rust(&clause.body))),
            ClauseKind::Ordering => {
                doc.push(format!("Ordering: {}", expr_to_rust(&clause.body)));
            }
            _ => {}
        }
    }

    // Build params (self + input params)
    let self_param = if is_mutation { "&mut self" } else { "&self" };
    let mut params = vec![RustParam {
        name: self_param.into(),
        ty: RustType::Raw("&Self".into()),
    }];
    for (n, t) in &input_params {
        params.push(RustParam {
            name: n.clone(),
            ty: RustType::Raw(t.clone()),
        });
    }

    let ret = if output_type == "()" {
        None
    } else {
        Some(RustType::Raw(output_type.clone()))
    };

    // Build function body
    let mut body: Vec<RustStmt> = Vec::new();

    if has_invariants {
        body.push(RustStmt::Raw("self.check_invariant();".into()));
    }
    if let Some(ref state) = pre_state {
        body.push(RustStmt::Raw(format!(
            "debug_assert_eq!(self.state, State::{state}, \"requires state {state}\");"
        )));
    }
    for req in &requires_exprs {
        body.push(RustStmt::Assert {
            cond: req.clone(),
            label: "requires".into(),
        });
    }

    let ir_body = ir_bodies.and_then(|m| m.get(name));

    if output_type == "()" {
        if let Some(ref state) = post_state {
            body.push(RustStmt::Raw(format!("self.state = State::{state};")));
        }
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Expr(RustExpr::Todo(format!(
                "{} implementation",
                kind_label.to_lowercase()
            ))));
        }
        for inv in &invariants {
            body.push(RustStmt::Assert {
                cond: inv.clone(),
                label: "invariant".into(),
            });
        }
        if has_invariants {
            body.push(RustStmt::Raw("self.check_invariant();".into()));
        }
    } else {
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Raw(format!(
                "let {RESULT_VAR}: {output_type} = todo!(\"{} implementation\");",
                kind_label.to_lowercase()
            )));
        }
        if let Some(ref oname) = output_name {
            body.push(RustStmt::Raw(format!(
                "let {oname} = {RESULT_VAR}.clone();"
            )));
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
        if let Some(ref state) = post_state {
            body.push(RustStmt::Raw(format!("self.state = State::{state};")));
        }
        if has_invariants {
            body.push(RustStmt::Raw("self.check_invariant();".into()));
        }
        body.push(RustStmt::Expr(RustExpr::Ident(RESULT_VAR.into())));
    }

    RustFn {
        name: name.to_string(),
        params,
        ret,
        body,
        doc,
        ..RustFn::default()
    }
}

/// Build a `RustFn` for a typestate service method.
fn build_typestate_method_fn(
    service_name: &str,
    name: &str,
    clauses: &[Clause],
    is_mutation: bool,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) -> crate::hir::RustFn {
    use crate::hir::*;

    let mut input_params: Vec<(String, String)> = Vec::new();
    let mut output_type = "()".to_string();
    let mut output_name: Option<String> = None;
    let mut requires_exprs: Vec<String> = Vec::new();
    let mut ensures_exprs: Vec<String> = Vec::new();
    let mut invariants: Vec<String> = Vec::new();
    let mut post_state: Option<String> = None;

    for clause in clauses {
        match &clause.kind {
            ClauseKind::Input => extract_input_params(&clause.body, &mut input_params),
            ClauseKind::Output => {
                output_type = extract_output_type(&clause.body);
                output_name = extract_output_name(&clause.body);
            }
            ClauseKind::Requires => {
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
            ClauseKind::Invariant => invariants.push(expr_to_rust(&clause.body)),
            _ => {}
        }
    }

    let kind_label = if is_mutation { "Operation" } else { "Query" };

    // Doc comments
    let mut doc: Vec<String> = vec![format!("{kind_label}: {name}")];
    for clause in clauses {
        match clause.kind {
            ClauseKind::Requires => doc.push(format!("Requires: {}", expr_to_rust(&clause.body))),
            ClauseKind::Ensures => doc.push(format!("Ensures: {}", expr_to_rust(&clause.body))),
            ClauseKind::Effects => doc.push(format!("Effects: {}", expr_to_rust(&clause.body))),
            ClauseKind::Modifies => doc.push(format!("Modifies: {}", expr_to_rust(&clause.body))),
            ClauseKind::Ordering => doc.push(format!("Ordering: {}", expr_to_rust(&clause.body))),
            _ => {}
        }
    }

    // Self parameter depends on whether there's a state transition
    let has_transition = post_state.is_some();
    let self_param = if has_transition {
        "self"
    } else if is_mutation {
        "&mut self"
    } else {
        "&self"
    };

    let mut params = vec![RustParam {
        name: self_param.into(),
        ty: RustType::Raw("Self".into()),
    }];
    for (n, t) in &input_params {
        params.push(RustParam {
            name: n.clone(),
            ty: RustType::Raw(t.clone()),
        });
    }

    let ret = if let Some(ref new_state) = post_state {
        Some(RustType::Raw(format!("{service_name}<{new_state}>")))
    } else if output_type == "()" {
        None
    } else {
        Some(RustType::Raw(output_type.clone()))
    };

    // Build body
    let mut body: Vec<RustStmt> = Vec::new();

    for req in &requires_exprs {
        body.push(RustStmt::Assert {
            cond: req.clone(),
            label: "requires".into(),
        });
    }
    for inv in &invariants {
        body.push(RustStmt::Assert {
            cond: inv.clone(),
            label: "invariant".into(),
        });
    }

    let ir_body = ir_bodies.and_then(|m| m.get(name));

    if post_state.is_some() || output_type == "()" {
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Expr(RustExpr::Todo(format!(
                "{} implementation",
                kind_label.to_lowercase()
            ))));
        }
    } else {
        if let Some(ir) = ir_body {
            body.push(RustStmt::Raw(ir.clone()));
        } else {
            body.push(RustStmt::Raw(format!(
                "let {RESULT_VAR}: {output_type} = todo!(\"{} implementation\");",
                kind_label.to_lowercase()
            )));
        }
        if let Some(ref oname) = output_name {
            body.push(RustStmt::Raw(format!(
                "let {oname} = {RESULT_VAR}.clone();"
            )));
        }
        for ens in &ensures_exprs {
            body.push(RustStmt::Assert {
                cond: ens.clone(),
                label: "ensures".into(),
            });
        }
        body.push(RustStmt::Expr(RustExpr::Ident(RESULT_VAR.into())));
    }

    RustFn {
        name: name.to_string(),
        params,
        ret,
        body,
        doc,
        ..RustFn::default()
    }
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
    use crate::hir::*;

    let states = collect_service_states(s);
    // Generate nested type/enum definitions
    for item in &s.items {
        match item {
            ServiceItem::TypeDef(t) => generate_type_def(t, code),
            ServiceItem::EnumDef(e) => generate_enum_def(e, code),
            _ => {}
        }
    }

    // State marker structs
    for state in &states {
        let marker = RustStruct {
            name: state.clone(),
            derives: vec![],
            doc: vec![format!("State marker: {state}")],
            ..RustStruct::default()
        };
        code.push_str(&render_item_raw(&RustItem::Struct(marker)));
    }

    // Generic service struct with PhantomData
    code.push_str(&render_item_raw(&RustItem::Struct(RustStruct {
        name: s.name.clone(),
        type_params: vec!["State".into()],
        fields: vec![RustField {
            name: "_state".into(),
            ty: RustType::Raw("std::marker::PhantomData<State>".into()),
            is_pub: false,
        }],
        derives: vec!["Debug".into()],
        ..RustStruct::default()
    })));

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
    if let Some(first) = states.first() {
        state_order.push(Some(first.clone()));
    }
    for state in states.iter().skip(1) {
        state_order.push(Some(state.clone()));
    }
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
                } else if let Some(group) = state_methods.iter_mut().find(|(k, _)| k.is_none()) {
                    group.1.push(method);
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
                } else if let Some(group) = state_methods.iter_mut().find(|(k, _)| k.is_none()) {
                    group.1.push(method);
                }
            }
            ServiceItem::Invariant(expr) => invariant_exprs.push(expr),
            ServiceItem::Other { kind, body } => other_items.push((kind, body)),
            _ => {}
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
                let mut impl_methods: Vec<RustFn> = Vec::new();
                if is_initial {
                    impl_methods.push(RustFn {
                        name: "new".into(),
                        ret: Some(RustType::Raw("Self".into())),
                        body: vec![RustStmt::Raw(
                            "Self { _state: std::marker::PhantomData }".into(),
                        )],
                        ..RustFn::default()
                    });
                }
                for method in methods {
                    impl_methods.push(build_typestate_method_fn(
                        &s.name,
                        method.name,
                        method.clauses,
                        method.is_mutation,
                        ir_bodies,
                    ));
                }
                code.push_str(&render_item_raw(&RustItem::Impl(RustImpl {
                    trait_name: None,
                    target: format!("{}<{state_name}>", s.name),
                    type_params: vec![],
                    methods: impl_methods,
                })));
            }
            None => {
                if methods.is_empty() && invariant_exprs.is_empty() && other_items.is_empty() {
                    continue;
                }
                let mut impl_methods: Vec<RustFn> = Vec::new();
                for method in methods {
                    impl_methods.push(build_typestate_method_fn(
                        &s.name,
                        method.name,
                        method.clauses,
                        method.is_mutation,
                        ir_bodies,
                    ));
                }
                for expr in &invariant_exprs {
                    let rust_expr = expr_to_rust(expr);
                    impl_methods.push(RustFn {
                        name: "check_invariant".into(),
                        params: vec![RustParam {
                            name: "&self".into(),
                            ty: RustType::Raw("&Self".into()),
                        }],
                        body: vec![RustStmt::Raw(format!("debug_assert!({rust_expr});"))],
                        doc: vec!["Service invariant".into()],
                        ..RustFn::default()
                    });
                }
                // Other items as raw comments inside the impl
                let mut raw_items: Vec<String> = Vec::new();
                for (kind, body) in &other_items {
                    raw_items.push(format!("// {kind}: {}", expr_to_rust(body)));
                }

                code.push_str(&render_item_raw(&RustItem::Impl(RustImpl {
                    trait_name: None,
                    target: s.name.clone(),
                    type_params: vec!["S".into()],
                    methods: impl_methods,
                })));
                // Append raw other items after the impl (they don't fit in methods)
                for raw in &raw_items {
                    code.push_str(raw);
                    code.push('\n');
                }
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
    use crate::hir::*;

    let has_states = s.items.iter().any(|i| matches!(i, ServiceItem::States(_)));

    if has_states {
        generate_typestate_service_body(s, code, ir_bodies);
        return;
    }

    // Stateless service: nested type/enum definitions first
    for item in &s.items {
        match item {
            ServiceItem::TypeDef(t) => generate_type_def(t, code),
            ServiceItem::EnumDef(e) => generate_enum_def(e, code),
            _ => {}
        }
    }

    // Struct definition
    code.push_str(&render_item_raw(&RustItem::Struct(RustStruct {
        name: s.name.clone(),
        derives: vec!["Debug".into()],
        ..RustStruct::default()
    })));

    // Impl block with new() + methods
    let has_invariants = s
        .items
        .iter()
        .any(|i| matches!(i, ServiceItem::Invariant(_)));

    let mut methods: Vec<RustFn> = Vec::new();

    // new() constructor
    methods.push(RustFn {
        name: "new".into(),
        ret: Some(RustType::Raw("Self".into())),
        body: vec![RustStmt::Raw("Self { }".into())],
        ..RustFn::default()
    });

    for item in &s.items {
        match item {
            ServiceItem::Operation { name, clauses } => {
                methods.push(build_service_method_fn(
                    name,
                    clauses,
                    true,
                    has_invariants,
                    ir_bodies,
                ));
            }
            ServiceItem::Query { name, clauses } => {
                methods.push(build_service_method_fn(
                    name,
                    clauses,
                    false,
                    has_invariants,
                    ir_bodies,
                ));
            }
            ServiceItem::Invariant(expr) => {
                let rust_expr = expr_to_rust(expr);
                methods.push(RustFn {
                    name: "check_invariant".into(),
                    params: vec![RustParam {
                        name: "&self".into(),
                        ty: RustType::Raw("&Self".into()),
                    }],
                    body: vec![RustStmt::Raw(format!("debug_assert!({rust_expr});"))],
                    doc: vec!["Service invariant".into()],
                    ..RustFn::default()
                });
            }
            _ => {}
        }
    }

    // Collect Other items separately (they go as raw comments after the impl)
    let mut other_comments: Vec<String> = Vec::new();
    for item in &s.items {
        if let ServiceItem::Other { kind, body } = item {
            other_comments.push(format!("// {kind}: {}", expr_to_rust(body)));
        }
    }

    code.push_str(&render_item_raw(&RustItem::Impl(RustImpl {
        trait_name: None,
        target: s.name.clone(),
        type_params: vec![],
        methods,
    })));

    for comment in &other_comments {
        code.push_str(comment);
        code.push('\n');
    }
}

pub(crate) fn generate_service(
    s: &ServiceDecl,
    code: &mut String,
    ir_bodies: Option<&std::collections::HashMap<String, String>>,
) {
    use crate::hir::*;

    // Generate the service body into a separate buffer
    let mut inner = String::new();
    generate_service_contents(s, &mut inner, ir_bodies);

    // Wrap in a module using RustMod
    let m = RustItem::Mod(RustMod {
        name: s.name.to_lowercase(),
        items: vec![RustItem::Raw(inner)],
        is_pub: true,
        doc: vec![format!("Service: {}", s.name)],
    });
    code.push_str(&render_item_raw(&m));
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
    use crate::hir::*;

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

    let mut methods: Vec<RustFn> = Vec::new();

    for clause in body {
        match &clause.kind {
            ClauseKind::Other(k) if k == "method" => {
                methods.push(build_trait_method_fn(&clause.body));
            }
            ClauseKind::Invariant | ClauseKind::Ensures => {
                let expr = expr_to_rust(&clause.body);
                methods.push(RustFn {
                    name: "check_invariant".into(),
                    params: vec![RustParam {
                        name: "&self".into(),
                        ty: RustType::Raw("&Self".into()),
                    }],
                    body: vec![RustStmt::Raw(format!("debug_assert!({expr});"))],
                    is_pub: false,
                    doc: vec!["Interface invariant".into()],
                    ..RustFn::default()
                });
            }
            _ => {}
        }
    }

    let item = RustItem::Trait(RustTrait {
        name: name.to_string(),
        type_params: vec![],
        supertraits: extends,
        methods,
        is_pub: true,
        doc: vec![format!("Interface contract: {name}")],
    });
    code.push_str(&render_item_raw(&item));
}

/// Generate a single trait method from an interface `method` clause body.
///
/// Public wrapper around `build_trait_method_fn` for callers (tests)
/// that still use the `&mut String` append pattern.
#[cfg(test)]
pub(crate) fn generate_trait_method(body: &SpExpr, code: &mut String) {
    use crate::hir::*;
    let f = build_trait_method_fn(body);
    code.push_str(&render_item_raw(&RustItem::Fn(f)));
}

/// Build a `RustFn` from an interface method clause body expression.
fn build_trait_method_fn(body: &SpExpr) -> crate::hir::RustFn {
    use crate::hir::*;

    match &body.node {
        Expr::Ident(name) => RustFn {
            name: name.clone(),
            params: vec![RustParam {
                name: "&self".into(),
                ty: RustType::Raw("&Self".into()),
            }],
            is_pub: false,
            is_abstract: true,
            ..RustFn::default()
        },
        Expr::Call { func, args } => {
            let func_name = if let Expr::Ident(n) = &func.as_ref().node {
                n.clone()
            } else {
                "unknown".to_string()
            };
            let mut params = vec![RustParam {
                name: "&self".into(),
                ty: RustType::Raw("&Self".into()),
            }];
            for (i, arg) in args.iter().enumerate() {
                let ty = if let Expr::Ident(ty) = &arg.node {
                    map_type_token(ty).to_string()
                } else {
                    "i64".to_string()
                };
                params.push(RustParam {
                    name: format!("arg{i}"),
                    ty: RustType::Raw(ty),
                });
            }
            RustFn {
                name: func_name,
                params,
                is_pub: false,
                is_abstract: true,
                ..RustFn::default()
            }
        }
        Expr::Raw(tokens) => {
            if let Some((name, rest)) = tokens.first().map(|n| (n.clone(), &tokens[1..])) {
                let mut params: Vec<RustParam> = vec![RustParam {
                    name: "&self".into(),
                    ty: RustType::Raw("&Self".into()),
                }];
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
                        if i + 2 < rest.len() && rest[i + 1] == ":" {
                            let pname = tok.clone();
                            let ptype = map_type_token(&rest[i + 2]).to_string();
                            params.push(RustParam {
                                name: pname,
                                ty: RustType::Raw(ptype),
                            });
                            i += 3;
                            continue;
                        }
                        let ptype = map_type_token(tok).to_string();
                        params.push(RustParam {
                            name: format!("arg{}", params.len() - 1),
                            ty: RustType::Raw(ptype),
                        });
                    }
                    i += 1;
                }

                let ret = if return_type.is_empty() {
                    None
                } else {
                    Some(RustType::Raw(return_type))
                };

                RustFn {
                    name,
                    params,
                    ret,
                    is_pub: false,
                    is_abstract: true,
                    ..RustFn::default()
                }
            } else {
                RustFn {
                    name: "unknown".into(),
                    is_pub: false,
                    is_abstract: true,
                    ..RustFn::default()
                }
            }
        }
        _ => {
            // Unsupported expressions: build a function with compile_error body
            RustFn {
                name: "unsupported".into(),
                body: vec![RustStmt::Raw(format!(
                    "compile_error!(\"unsupported expression in trait method: {:?}\");",
                    std::mem::discriminant(&body.node)
                ))],
                is_pub: false,
                ..RustFn::default()
            }
        }
    }
}

// ---------------------------------------------------------------------------
#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
