//! Bind, extern, and function definition code generation.

use super::*;

// ---------------------------------------------------------------------------
// Bind declarations (checked wrappers for existing Rust functions)
// ---------------------------------------------------------------------------

/// Generate a checked wrapper for a `bind` declaration.
///
/// A `bind` maps an existing Rust function path to an Assura contract name.
/// The generated code calls the real function and wraps it with
/// `debug_assert!` checks for `requires` and `ensures` clauses.
pub(crate) fn generate_bind(b: &BindDecl, code: &mut String) {
    let params_s: String = b
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, map_type_tokens(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret = if b.return_ty.is_empty() {
        "()".to_string()
    } else {
        map_type_tokens(&b.return_ty)
    };

    let args_s: String = b
        .params
        .iter()
        .map(|p| p.name.clone())
        .collect::<Vec<_>>()
        .join(", ");

    let rust_path = &b.target_path;

    code.push_str(&format!(
        "/// Bind: {} -> {rust_path}\npub fn {}({params_s}) -> {ret} {{\n",
        b.name, b.name
    ));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &b.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    // Call the actual Rust function
    code.push_str(&format!(
        "    let __result: {ret} = {rust_path}({args_s});\n"
    ));

    // Generate ensures assertions on the result
    for ens in &ensures_exprs {
        generate_debug_assert(code, ens, "ensures");
    }

    code.push_str("    __result\n");
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Extern declarations
// ---------------------------------------------------------------------------

pub(crate) fn generate_extern(ex: &ExternDecl, code: &mut String) {
    let params_s: String = ex
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, map_type_tokens(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret = if ex.return_ty.is_empty() {
        "()".to_string()
    } else {
        map_type_tokens(&ex.return_ty)
    };

    // SEC.2 compile-time enforcement: determine trust boundary from clauses
    let trust_level = ex.clauses.iter().find_map(|c| {
        if matches!(&c.kind, ClauseKind::Other(k) if k == "trust" || k == "boundary") {
            match &c.body {
                Expr::Ident(v) => Some(v.as_str().to_string()),
                _ => None,
            }
        } else {
            None
        }
    });
    let is_untrusted = trust_level.as_deref() == Some("untrusted");
    let has_contract = ex
        .clauses
        .iter()
        .any(|c| c.kind == ClauseKind::Requires || c.kind == ClauseKind::Ensures);

    // SEC.2 compile-time: extern functions generate `unsafe fn` so Rust's
    // type system enforces that callers must use an unsafe block, providing
    // compile-time visibility of FFI boundary crossings.
    let unsafe_kw = if trust_level.is_some() { "unsafe " } else { "" };

    // Generate as a function with contract assertions
    code.push_str(&format!(
        "/// Extern: {} [ffi_boundary: {}]\npub {unsafe_kw}fn {}({params_s}) -> {ret} {{\n",
        ex.name,
        trust_level.as_deref().unwrap_or("none"),
        ex.name
    ));

    // SEC.2 compile-time: untrusted externs without contracts emit compile_error!
    // so the generated Rust will not compile until contracts are added.
    if is_untrusted && !has_contract {
        code.push_str(&format!(
            "    compile_error!(\"FFI boundary violation: untrusted extern `{}` \
             has no contract; add requires/ensures\");\n",
            ex.name
        ));
    }

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &ex.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    if ensures_exprs.is_empty() && (has_contract || !is_untrusted) {
        code.push_str("    todo!(\"extern function: implementation required\")\n");
    } else if !ensures_exprs.is_empty() {
        code.push_str(&format!(
            "    let __result: {ret} = todo!(\"extern function: implementation required\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        code.push_str("    __result\n");
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Function definitions
// ---------------------------------------------------------------------------

pub(crate) fn generate_fn_def(f: &FnDef, code: &mut String) {
    let params_s: String = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, map_type_tokens(&p.ty)))
        .collect::<Vec<_>>()
        .join(", ");

    let ret_ty = if f.return_ty.is_empty() {
        "()".to_string()
    } else {
        map_type_tokens(&f.return_ty)
    };

    // Generate error enum if errors clause is present
    let error_variants = collect_error_variants(&f.clauses);
    let error_enum_name = if !error_variants.is_empty() {
        let name = format!("{}Error", f.name);
        generate_error_enum(&f.name, &error_variants, code);
        Some(name)
    } else {
        None
    };

    let return_type = if let Some(ref err_name) = error_enum_name {
        format!("Result<{ret_ty}, {err_name}>")
    } else {
        ret_ty.clone()
    };

    let ret_sig = if f.return_ty.is_empty() && error_enum_name.is_none() {
        String::new()
    } else {
        format!(" -> {return_type}")
    };

    code.push_str(&format!("pub fn {}({params_s}){ret_sig} {{\n", f.name));

    // Collect old() expressions from ensures clauses and save pre-state values
    let mut ensures_exprs: Vec<String> = Vec::new();
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Ensures {
            for (var, rust_expr) in collect_old_exprs(&clause.body) {
                code.push_str(&format!("    let __old_{var} = {rust_expr}.clone();\n"));
            }
            ensures_exprs.push(expr_to_rust(&clause.body));
        }
    }

    // Generate requires assertions at function entry
    for clause in &f.clauses {
        if clause.kind == ClauseKind::Requires {
            let expr = expr_to_rust(&clause.body);
            generate_debug_assert(code, &expr, "requires");
        }
    }

    if ensures_exprs.is_empty() {
        code.push_str("    todo!(\"implementation provided by AI agent\")\n");
    } else {
        code.push_str(&format!(
            "    let __result: {ret_ty} = todo!(\"implementation provided by AI agent\");\n"
        ));
        for ens in &ensures_exprs {
            generate_debug_assert(code, ens, "ensures");
        }
        if error_enum_name.is_some() {
            code.push_str("    Ok(__result)\n");
        } else {
            code.push_str("    __result\n");
        }
    }
    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
