//! Encode simple Rust function bodies as Assura Implementation IR for check-rust.
//!
//! Supports int/bool arith, abs/min/max/clamp/saturating, is_positive/negative/zero, unary `-`, multi-let, if/match (incl. guards),
//! simple and nested `if`/`else` (expression branches), simple `match` with
//! int/bool/wildcard arms (no guards/bindings), and Bool comparisons for
//! `bool` return types. Body text is extracted with `syn` (co-publish-safe).
//!
//! Multi-block if IR must use **unique temp slots across sibling blocks**.
//! `eval_ir_block` clones parent slots into each block; reusing `$1`/`$2` for
//! temps collides with the condition/`if` result and makes SMT unsound
//! (false Verified). Match `Clamp.ir`: params `$0..$n-1` are shared; temps
//! monotonically increase (see shared `next` in `emit_value_blocks`).

use assura_rust_analyzer::ParamInfo;
use std::cell::Cell;

thread_local! {
    /// Saturating op bounds for the current encode (set from return type).
    static SAT_BOUNDS: Cell<Option<(i64, i64)>> = const { Cell::new(None) };
}

use quote::ToTokens;

/// Extract a simple trailing return expression for `fn_name` from Rust source.
pub(crate) fn extract_body_return(source: &str, fn_name: &str) -> Option<String> {
    let file = syn::parse_file(source).ok()?;
    for item in &file.items {
        match item {
            syn::Item::Fn(func) if func.sig.ident == fn_name => {
                return body_return_from_block(&func.block);
            }
            syn::Item::Impl(imp) => {
                for impl_item in &imp.items {
                    if let syn::ImplItem::Fn(method) = impl_item
                        && method.sig.ident == fn_name
                    {
                        return body_return_from_block(&method.block);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn body_return_from_block(block: &syn::Block) -> Option<String> {
    match block.stmts.as_slice() {
        [syn::Stmt::Expr(syn::Expr::Return(ret), _)] => ret.expr.as_ref().map(|e| expr_source(e)),
        [syn::Stmt::Expr(expr, _)] => Some(expr_source(expr)),
        stmts => fold_simple_lets(stmts).map(|e| expr_source(&e)),
    }
}

/// Fold `let a = e1; let b = a + 1; b` (or `return b`) into a single expression.
/// Only simple `Pat::Ident` bindings without type ascriptions/mut; final stmt is
/// path/return/expression that may reference prior binds.
fn fold_simple_lets(stmts: &[syn::Stmt]) -> Option<syn::Expr> {
    if stmts.len() < 2 {
        return None;
    }
    // split_last -> (last_elem, prefix)
    let (last, binds) = stmts.split_last()?;
    let mut env: Vec<(String, syn::Expr)> = Vec::new();
    for stmt in binds {
        let syn::Stmt::Local(local) = stmt else {
            return None;
        };
        let name = match &local.pat {
            syn::Pat::Ident(id) if id.by_ref.is_none() && id.mutability.is_none() => {
                id.ident.to_string()
            }
            _ => return None,
        };
        let init = local.init.as_ref()?;
        if init.diverge.is_some() {
            return None;
        }
        env.push((name, (*init.expr).clone()));
    }
    let mut final_expr: syn::Expr = match last {
        syn::Stmt::Expr(syn::Expr::Return(ret), _) => (*ret.expr.as_ref()?.as_ref()).clone(),
        syn::Stmt::Expr(e, _) => e.clone(),
        _ => return None,
    };
    // Substitute later binds first so earlier names expand fully.
    for (name, init) in env.into_iter().rev() {
        final_expr = substitute_ident_expr(final_expr, &name, &init);
    }
    Some(final_expr)
}

/// Replace free path `name` with `replacement` (structural, supported expr kinds).
fn substitute_ident_expr(expr: syn::Expr, name: &str, replacement: &syn::Expr) -> syn::Expr {
    match expr {
        syn::Expr::Path(ref p) if p.path.segments.len() == 1 => {
            if p.path.segments[0].ident == name {
                replacement.clone()
            } else {
                expr
            }
        }
        syn::Expr::Paren(mut p) => {
            *p.expr = substitute_ident_expr(*p.expr, name, replacement);
            syn::Expr::Paren(p)
        }
        syn::Expr::Group(mut g) => {
            *g.expr = substitute_ident_expr(*g.expr, name, replacement);
            syn::Expr::Group(g)
        }
        syn::Expr::Unary(mut u) => {
            *u.expr = substitute_ident_expr(*u.expr, name, replacement);
            syn::Expr::Unary(u)
        }
        syn::Expr::Binary(mut b) => {
            *b.left = substitute_ident_expr(*b.left, name, replacement);
            *b.right = substitute_ident_expr(*b.right, name, replacement);
            syn::Expr::Binary(b)
        }
        syn::Expr::MethodCall(mut m) => {
            *m.receiver = substitute_ident_expr(*m.receiver, name, replacement);
            let args: Vec<syn::Expr> = m
                .args
                .into_iter()
                .map(|a| substitute_ident_expr(a, name, replacement))
                .collect();
            m.args = args.into_iter().collect();
            syn::Expr::MethodCall(m)
        }
        syn::Expr::Call(mut c) => {
            *c.func = substitute_ident_expr(*c.func, name, replacement);
            let args: Vec<syn::Expr> = c
                .args
                .into_iter()
                .map(|a| substitute_ident_expr(a, name, replacement))
                .collect();
            c.args = args.into_iter().collect();
            syn::Expr::Call(c)
        }
        other => other,
    }
}

fn expr_source(expr: &syn::Expr) -> String {
    expr.to_token_stream()
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build IR text for a function if `body_return` is a simple supported shape.
pub(crate) fn try_ir_from_rust_body(
    item_name: &str,
    params: &[ParamInfo],
    return_ty: Option<&str>,
    body_return: &str,
) -> Option<String> {
    let ret_assura = return_ty
        .map(assura_codegen::type_map::rust_type_to_assura)
        .unwrap_or_else(|| "Int".to_string());
    if !matches!(ret_assura.as_str(), "Int" | "Nat" | "Bool") {
        return None;
    }
    let sat = match return_ty.map(str::trim) {
        Some("i8") => Some((i8::MIN as i64, i8::MAX as i64)),
        Some("i16") => Some((i16::MIN as i64, i16::MAX as i64)),
        Some("i32") => Some((i32::MIN as i64, i32::MAX as i64)),
        Some("i64") | Some("isize") => Some((i64::MIN, i64::MAX)),
        Some("u8") => Some((0, u8::MAX as i64)),
        Some("u16") => Some((0, u16::MAX as i64)),
        Some("u32") => Some((0, u32::MAX as i64)),
        // u64 max exceeds i64; clamp encode uses i64 consts — skip for now
        Some("u64") | Some("usize") => None,
        _ => None,
    };
    SAT_BOUNDS.set(sat);

    let param_names: Vec<&str> = params
        .iter()
        .filter(|p| p.name != "self")
        .map(|p| p.name.as_str())
        .collect();
    if param_names.is_empty() {
        return None;
    }

    for p in params.iter().filter(|p| p.name != "self") {
        let ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
        if !matches!(ty.as_str(), "Int" | "Nat" | "Bool") {
            return None;
        }
    }

    let expr: syn::Expr = syn::parse_str(body_return).ok()?;

    let mut sig_parts = Vec::new();
    for (i, p) in params.iter().filter(|p| p.name != "self").enumerate() {
        let ty = assura_codegen::type_map::rust_type_to_assura(&p.ty);
        sig_parts.push(format!("${i}: {ty}"));
    }
    let sig = sig_parts.join(", ");

    // If / match (including nested): multi-block IR (Clamp.ir style).
    if matches!(expr, syn::Expr::If(_) | syn::Expr::Match(_)) {
        return try_ir_from_if_tree(item_name, &sig, &ret_assura, &param_names, &expr);
    }

    let mut lines = Vec::new();
    let mut next = param_names.len();
    let result_slot = encode_syn_expr(&expr, &param_names, &mut lines, &mut next)?;
    let result_ty = if ret_assura == "Bool" { "Bool" } else { "Int" };
    lines.push(format!("$result = load ${result_slot} : {result_ty}"));

    let mut ir = String::new();
    ir.push_str(&format!("module {item_name} {{\n"));
    ir.push_str(&format!("  fn #0 : ({sig}) -> {ret_assura} ! pure\n"));
    ir.push_str("  {\n");
    for line in lines {
        ir.push_str("    ");
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  }\n");
    ir.push_str("}\n");
    Some(ir)
}

/// Flatten if/else-if tree into numbered IR functions with unique temp slots.
fn try_ir_from_if_tree(
    item_name: &str,
    sig: &str,
    ret_assura: &str,
    param_names: &[&str],
    root: &syn::Expr,
) -> Option<String> {
    let mut blocks: Vec<(usize, Vec<String>)> = Vec::new();
    let mut next_block = 0usize;
    // Shared across all blocks: params 0..n-1 stay shared; temps never reuse.
    let mut next_slot = param_names.len();
    let entry = emit_value_blocks(
        root,
        param_names,
        ret_assura,
        &mut blocks,
        &mut next_block,
        &mut next_slot,
    )?;
    if entry != 0 {
        return None;
    }

    let mut ir = String::new();
    ir.push_str(&format!("module {item_name} {{\n"));
    for (id, lines) in &blocks {
        let fn_sig = if *id == 0 {
            format!("({sig}) -> {ret_assura}")
        } else {
            format!("() -> {ret_assura}")
        };
        ir.push_str(&format!("  fn #{id} : {fn_sig} ! pure\n  {{\n"));
        for line in lines {
            ir.push_str("    ");
            ir.push_str(line);
            ir.push('\n');
        }
        ir.push_str("  }\n");
    }
    ir.push_str("}\n");
    Some(ir)
}

/// Emit IR for a value expression that may be a nested if/match. Returns block id.
fn emit_value_blocks(
    expr: &syn::Expr,
    param_names: &[&str],
    ret_assura: &str,
    blocks: &mut Vec<(usize, Vec<String>)>,
    next_block: &mut usize,
    next_slot: &mut usize,
) -> Option<usize> {
    match expr {
        syn::Expr::If(if_expr) => {
            let else_expr = if_expr.else_branch.as_ref()?.1.as_ref();
            let then_expr = block_as_expr_owned(&if_expr.then_branch)?;
            let else_expr = match else_expr {
                syn::Expr::Block(b) => block_as_expr_owned(&b.block)?,
                other => other.clone(),
            };

            let this_id = *next_block;
            *next_block += 1;
            // Reserve this block slot (filled after children so ids are sequential-ish)
            blocks.push((this_id, Vec::new()));

            let then_id = emit_value_blocks(
                &then_expr,
                param_names,
                ret_assura,
                blocks,
                next_block,
                next_slot,
            )?;
            let else_id = emit_value_blocks(
                &else_expr,
                param_names,
                ret_assura,
                blocks,
                next_block,
                next_slot,
            )?;

            let mut main_lines = Vec::new();
            let cond_slot =
                encode_syn_expr(&if_expr.cond, param_names, &mut main_lines, next_slot)?;
            let if_out = *next_slot;
            *next_slot += 1;
            main_lines.push(format!(
                "${if_out} = if ${cond_slot} then #{then_id} else #{else_id} : {ret_assura}"
            ));
            main_lines.push(format!("$result = load ${if_out} : {ret_assura}"));

            if let Some((_, lines)) = blocks.iter_mut().find(|(id, _)| *id == this_id) {
                *lines = main_lines;
            }
            Some(this_id)
        }
        syn::Expr::Match(m) => {
            // Prefer identity-guard rewrite to if-tree (#999); else lit/_ match (#993).
            if let Some(if_tree) = match_identity_guards_to_if(m) {
                return emit_value_blocks(
                    &if_tree,
                    param_names,
                    ret_assura,
                    blocks,
                    next_block,
                    next_slot,
                );
            }
            if m.arms.is_empty() {
                return None;
            }
            let this_id = *next_block;
            *next_block += 1;
            blocks.push((this_id, Vec::new()));

            let mut arm_specs: Vec<(String, usize)> = Vec::new();
            for arm in &m.arms {
                if arm.guard.is_some() {
                    return None;
                }
                let pat = match_pattern_ir(&arm.pat)?;
                let body_expr = match arm.body.as_ref() {
                    syn::Expr::Block(b) => block_as_expr_owned(&b.block)?,
                    other => other.clone(),
                };
                let arm_id = emit_value_blocks(
                    &body_expr,
                    param_names,
                    ret_assura,
                    blocks,
                    next_block,
                    next_slot,
                )?;
                arm_specs.push((pat, arm_id));
            }

            let mut main_lines = Vec::new();
            let scrut = encode_syn_expr(&m.expr, param_names, &mut main_lines, next_slot)?;
            let arms_joined = arm_specs
                .iter()
                .map(|(p, id)| format!("{p} => #{id}"))
                .collect::<Vec<_>>()
                .join(", ");
            let out = *next_slot;
            *next_slot += 1;
            main_lines.push(format!(
                "${out} = match ${scrut} {{ {arms_joined} }} : {ret_assura}"
            ));
            main_lines.push(format!("$result = load ${out} : {ret_assura}"));

            if let Some((_, lines)) = blocks.iter_mut().find(|(id, _)| *id == this_id) {
                *lines = main_lines;
            }
            Some(this_id)
        }
        other => {
            let this_id = *next_block;
            *next_block += 1;
            let mut lines = Vec::new();
            let slot = encode_syn_expr(other, param_names, &mut lines, next_slot)?;
            let ty = if ret_assura == "Bool" { "Bool" } else { "Int" };
            lines.push(format!("$result = load ${slot} : {ty}"));
            blocks.push((this_id, lines));
            Some(this_id)
        }
    }
}

/// IR match pattern text for a simple arm (int/bool/wildcard only).
fn match_pattern_ir(pat: &syn::Pat) -> Option<String> {
    match pat {
        syn::Pat::Wild(_) => Some("_".into()),
        syn::Pat::Lit(lit) => match &lit.lit {
            syn::Lit::Int(n) => {
                let _ = n.base10_digits().parse::<i64>().ok()?;
                Some(n.base10_digits().to_string())
            }
            syn::Lit::Bool(b) => Some(if b.value {
                "true".into()
            } else {
                "false".into()
            }),
            _ => None,
        },
        // `true` / `false` as path patterns
        syn::Pat::Path(p) if p.path.segments.len() == 1 => {
            let name = p.path.segments[0].ident.to_string();
            if name == "true" || name == "false" {
                Some(name)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Rewrite `match x { n if cond(n) => body, _ => e }` into nested if-tree (#999).
/// Binding + guard arms: substitute the scrutinee for the bind in guard and body.
/// Final arm must be `_` without guard. No plain bindings without guards.
fn match_identity_guards_to_if(m: &syn::ExprMatch) -> Option<syn::Expr> {
    if m.arms.len() < 2 {
        return None;
    }
    let mut nest: Option<String> = None;
    for arm in m.arms.iter().rev() {
        let body = match arm.body.as_ref() {
            syn::Expr::Block(b) => block_as_expr_owned(&b.block)?,
            other => other.clone(),
        };
        match (&arm.pat, &arm.guard) {
            (syn::Pat::Wild(_), None) => {
                if nest.is_some() {
                    return None;
                }
                nest = Some(format!("( {} )", expr_source(&body)));
            }
            (syn::Pat::Ident(id), Some((_, guard)))
                if id.by_ref.is_none() && id.mutability.is_none() =>
            {
                let bind = id.ident.to_string();
                let guard_sub = substitute_ident_expr(*guard.clone(), &bind, &m.expr);
                let body_sub = substitute_ident_expr(body, &bind, &m.expr);
                let cond_src = expr_source(&guard_sub);
                let then_src = expr_source(&body_sub);
                let else_src = nest?;
                nest = Some(format!(
                    "if {cond_src} {{ {then_src} }} else {{ {else_src} }}"
                ));
            }
            _ => return None,
        }
    }
    let tree = nest?;
    syn::parse_str(&tree).ok()
}

/// Branch/arm body as owned expression (return, single expr, multi-let fold).
fn block_as_expr_owned(block: &syn::Block) -> Option<syn::Expr> {
    match block.stmts.as_slice() {
        [syn::Stmt::Expr(syn::Expr::Return(ret), _)] => {
            Some((*ret.expr.as_ref()?.as_ref()).clone())
        }
        [syn::Stmt::Expr(e, _)] => Some(e.clone()),
        stmts => fold_simple_lets(stmts),
    }
}

/// Encode `expr` into IR lines; returns the slot holding the value.
fn encode_syn_expr(
    expr: &syn::Expr,
    param_names: &[&str],
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    match expr {
        syn::Expr::Paren(p) => encode_syn_expr(&p.expr, param_names, lines, next),
        syn::Expr::Group(g) => encode_syn_expr(&g.expr, param_names, lines, next),
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            let name = path.path.segments[0].ident.to_string();
            param_names.iter().position(|n| *n == name)
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => {
            let val = n.base10_digits();
            let _ = val.parse::<i64>().ok()?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = const {val} : Int"));
            Some(slot)
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Bool(b),
            ..
        }) => {
            let val = if b.value { 1 } else { 0 };
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = const {val} : Bool"));
            Some(slot)
        }
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            let zero = *next;
            *next += 1;
            lines.push(format!("${zero} = const 0 : Int"));
            let inner = encode_syn_expr(&u.expr, param_names, lines, next)?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = arith sub ${zero} ${inner} : Int"));
            Some(slot)
        }
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Not(_)) => {
            let zero = *next;
            *next += 1;
            lines.push(format!("${zero} = const 0 : Bool"));
            let inner = encode_syn_expr(&u.expr, param_names, lines, next)?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = cmp eq ${inner} ${zero} : Bool"));
            Some(slot)
        }
        syn::Expr::Binary(b) => {
            if let Some(cmp) = match &b.op {
                syn::BinOp::Lt(_) => Some("lt"),
                syn::BinOp::Gt(_) => Some("gt"),
                syn::BinOp::Le(_) => Some("le"),
                syn::BinOp::Ge(_) => Some("ge"),
                syn::BinOp::Eq(_) => Some("eq"),
                syn::BinOp::Ne(_) => Some("ne"),
                _ => None,
            } {
                let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
                let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = cmp {cmp} ${lhs} ${rhs} : Bool"));
                return Some(slot);
            }
            let ir_op = match &b.op {
                syn::BinOp::Add(_) => "add",
                syn::BinOp::Sub(_) => "sub",
                syn::BinOp::Mul(_) => "mul",
                syn::BinOp::Div(_) => "div",
                syn::BinOp::Rem(_) => "mod",
                _ => return None,
            };
            let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
            let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = arith {ir_op} ${lhs} ${rhs} : Int"));
            Some(slot)
        }
        syn::Expr::MethodCall(m) => {
            let method = m.method.to_string();
            match (method.as_str(), m.args.len()) {
                ("abs", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${a}) : Int"));
                    Some(slot)
                }
                ("is_positive", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let z = *next;
                    *next += 1;
                    lines.push(format!("${z} = const 0 : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp gt ${a} ${z} : Bool"));
                    Some(slot)
                }
                ("is_negative", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let z = *next;
                    *next += 1;
                    lines.push(format!("${z} = const 0 : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp lt ${a} ${z} : Bool"));
                    Some(slot)
                }
                ("is_zero", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let z = *next;
                    *next += 1;
                    lines.push(format!("${z} = const 0 : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp eq ${a} ${z} : Bool"));
                    Some(slot)
                }
                ("clone" | "to_owned", 0) => encode_syn_expr(&m.receiver, param_names, lines, next),

                ("min" | "max", 1) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call {method} (${a}, ${b}) : Int"));
                    Some(slot)
                }
                // x.clamp(lo, hi) ≡ min(max(x, lo), hi)
                ("clamp", 2) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let lo = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let hi = encode_syn_expr(&m.args[1], param_names, lines, next)?;
                    let mx = *next;
                    *next += 1;
                    lines.push(format!("${mx} = call max (${a}, ${lo}) : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call min (${mx}, ${hi}) : Int"));
                    Some(slot)
                }
                // saturating_add/sub: clamp arith to i64 range (#1007; needs param
                // range requires from check_rust for soundness on unbounded Int).
                ("saturating_add" | "saturating_sub" | "saturating_mul", 1) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let op = match method.as_str() {
                        "saturating_add" => "add",
                        "saturating_sub" => "sub",
                        "saturating_mul" => "mul",
                        _ => return None,
                    };
                    let sum = *next;
                    *next += 1;
                    lines.push(format!("${sum} = arith {op} ${a} ${b} : Int"));
                    let lo = *next;
                    *next += 1;
                    let (lo_v, hi_v) = SAT_BOUNDS.get()?;
                    lines.push(format!("${lo} = const {lo_v} : Int"));
                    let hi = *next;
                    *next += 1;
                    lines.push(format!("${hi} = const {hi_v} : Int"));
                    let mx = *next;
                    *next += 1;
                    lines.push(format!("${mx} = call max (${sum}, ${lo}) : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call min (${mx}, ${hi}) : Int"));
                    Some(slot)
                }
                _ => None,
            }
        }
        syn::Expr::Call(c) => {
            let syn::Expr::Path(path) = c.func.as_ref() else {
                return None;
            };
            if path.path.segments.len() != 1 {
                return None;
            }
            let name = path.path.segments[0].ident.to_string();
            match (name.as_str(), c.args.len()) {
                ("abs", 1) => {
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${a}) : Int"));
                    Some(slot)
                }
                ("min" | "max", 2) => {
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let b = encode_syn_expr(&c.args[1], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call {name} (${a}, ${b}) : Int"));
                    Some(slot)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Params / return type from an annotated item Function.
pub(crate) fn function_params_return(
    kind: &assura_rust_analyzer::AnnotatedItemKind,
) -> Option<(&[ParamInfo], Option<&str>)> {
    match kind {
        assura_rust_analyzer::AnnotatedItemKind::Function {
            params,
            return_type,
            ..
        } => Some((params.as_slice(), return_type.as_deref())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_rust_analyzer::ParamInfo;
    use std::cell::Cell;

    thread_local! {
        /// Saturating op bounds for the current encode (set from return type).
        static SAT_BOUNDS: Cell<Option<(i64, i64)>> = const { Cell::new(None) };
    }

    fn px() -> Vec<ParamInfo> {
        vec![ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        }]
    }

    #[test]
    fn extract_identity_and_add() {
        let src = r#"
/// @requires x > 0
/// @ensures result == x + 1
fn bad(x: i64) -> i64 { x }
fn good(x: i64) -> i64 { x + 1 }
fn with_let(x: i64) -> i64 { let y = x + 1; y }
fn multi_let(x: i64) -> i64 { let a = x + 1; let b = a + 1; b }
"#;
        assert_eq!(extract_body_return(src, "bad").as_deref(), Some("x"));
        assert_eq!(extract_body_return(src, "good").as_deref(), Some("x + 1"));
        assert_eq!(
            extract_body_return(src, "with_let").as_deref(),
            Some("x + 1")
        );
        let multi = extract_body_return(src, "multi_let").expect("multi");
        assert!(
            multi.contains('+') && !multi.contains("let"),
            "multi-let should fold: {multi}"
        );
        let ir = try_ir_from_rust_body("M", &px(), Some("i64"), &multi).expect("ir");
        assert!(ir.contains("arith add"), "{ir}");
    }

    #[test]
    fn identity_body_ir() {
        let ir = try_ir_from_rust_body("Id", &px(), Some("i64"), "x").expect("ir");
        assert!(ir.contains("$result = load $0 : Int"), "{ir}");
    }

    #[test]
    fn add_one_body_ir() {
        let ir = try_ir_from_rust_body("Inc", &px(), Some("i64"), "x + 1").expect("ir");
        assert!(ir.contains("arith add"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Inc").expect("parse");
    }

    #[test]
    fn nested_if_body_ir() {
        let ir = try_ir_from_rust_body(
            "Nest",
            &px(),
            Some("i64"),
            "if x > 10 { x } else { if x > 0 { x } else { 0 } }",
        )
        .expect("nested if");
        assert!(ir.contains("fn #0") && ir.contains("fn #3"), "{ir}");
        assert!(ir.matches("then #").count() >= 2, "{ir}");
        // Sibling temps must not reuse parent cond slots (unsound if collision).
        assert!(
            ir.contains("$4 =") || ir.contains("$5 =") || ir.contains("$6 ="),
            "expected high temp slots: {ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Nest").expect("parse nested");
    }

    #[test]
    fn bool_comparison_body_ir() {
        let ir = try_ir_from_rust_body("IsPos", &px(), Some("bool"), "x > 0").expect("bool");
        assert!(ir.contains("cmp gt") && ir.contains(": Bool"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "IsPos").expect("parse bool");
    }

    #[test]
    fn simple_if_body_ir() {
        let ir = try_ir_from_rust_body("Clamp0", &px(), Some("i64"), "if x > 0 { x } else { 0 }")
            .expect("if ir");
        assert!(
            ir.contains("cmp gt") && ir.contains("then #1 else #2"),
            "{ir}"
        );
        assert_no_slot_overlap_with_entry(&ir);
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Clamp0").expect("parse if");
    }

    #[test]
    fn if_else_negative_uses_fresh_slots() {
        let ir = try_ir_from_rust_body("Bad", &px(), Some("i64"), "if x > 0 { x } else { -1 }")
            .expect("bad if");
        assert_no_slot_overlap_with_entry(&ir);
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Bad").expect("parse");
    }

    /// Parent `fn #0` temps and sibling `fn #N` temps must be disjoint.
    /// Collision makes `eval_ir_block` (clones parent slots) unsound.
    fn assert_no_slot_overlap_with_entry(ir: &str) {
        fn assigned_temps(block: &str) -> std::collections::HashSet<usize> {
            let mut set = std::collections::HashSet::new();
            for line in block.lines() {
                let t = line.trim();
                if let Some(rest) = t.strip_prefix('$')
                    && let Some((num, _)) = rest.split_once(" =")
                    && num != "result"
                    && let Ok(n) = num.parse::<usize>()
                {
                    set.insert(n);
                }
            }
            set
        }
        let entry = ir
            .split("fn #0")
            .nth(1)
            .and_then(|s| s.split("fn #").next())
            .unwrap_or("");
        let entry_temps = assigned_temps(entry);
        // Remaining `fn #N` bodies after #0
        let after0 = ir.split("fn #0").nth(1).unwrap_or("");
        for part in after0.split("fn #").skip(1) {
            let sibling = part;
            let sib_temps = assigned_temps(sibling);
            let overlap: Vec<_> = entry_temps.intersection(&sib_temps).copied().collect();
            assert!(
                overlap.is_empty(),
                "slot collision between entry and sibling {overlap:?}:\n{ir}"
            );
        }
    }

    #[test]
    fn clamp_method_body_ir() {
        let ir =
            try_ir_from_rust_body("C", &px(), Some("i64"), "x . clamp (0 , 10)").expect("clamp");
        assert!(ir.contains("call max") && ir.contains("call min"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "C").expect("parse");
    }

    #[test]
    fn abs_min_max_method_and_call() {
        let abs = try_ir_from_rust_body("A", &px(), Some("i64"), "x . abs ()").expect("abs");
        assert!(abs.contains("call abs"), "{abs}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&abs, "A").expect("parse abs");
    }

    #[test]
    fn unsupported_returns_none() {
        assert!(try_ir_from_rust_body("F", &px(), Some("i64"), "x && y").is_none());
        assert!(try_ir_from_rust_body("F", &px(), Some("i64"), "foo(x)").is_none());
    }

    #[test]
    fn if_return_stmt_branches_extract_and_encode() {
        let src = r#"
fn f(x: i64) -> i64 {
    if x > 0 {
        return x;
    } else {
        return 0;
    }
}
"#;
        let body = extract_body_return(src, "f").expect("extract if");
        assert!(body.contains("if"), "{body}");
        let ir = try_ir_from_rust_body("F", &px(), Some("i64"), &body).expect("encode");
        assert!(ir.contains("then #1 else #2"), "{ir}");
        assert_no_slot_overlap_with_entry(&ir);
    }

    #[test]
    fn simple_match_body_ir() {
        let ir = try_ir_from_rust_body("Sign", &px(), Some("i64"), "match x { 0 => 0, _ => 1 }")
            .expect("match ir");
        assert!(ir.contains("match $0") && ir.contains("_ => #"), "{ir}");
        assert_no_slot_overlap_with_entry(&ir);
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Sign").expect("parse match");
    }

    #[test]
    fn match_identity_guard_rewrites_to_if() {
        let ir = try_ir_from_rust_body(
            "G",
            &px(),
            Some("i64"),
            "match x { n if n > 0 => n, _ => 0 }",
        )
        .expect("identity guard");
        assert!(ir.contains("cmp gt") && ir.contains("then #"), "{ir}");
        assert_no_slot_overlap_with_entry(&ir);
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "G").expect("parse");
    }

    #[test]
    fn match_guard_non_identity_body_encodes() {
        let ir = try_ir_from_rust_body(
            "G2",
            &px(),
            Some("i64"),
            "match x { n if n > 0 => -1, _ => 0 }",
        )
        .expect("guard body -1");
        assert!(ir.contains("arith sub") || ir.contains("const"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "G2").expect("parse");
    }

    #[test]
    fn match_plain_binding_still_none() {
        assert!(
            try_ir_from_rust_body("B", &px(), Some("i64"), "match x { n => n, _ => 0 }").is_none()
        );
    }
}
