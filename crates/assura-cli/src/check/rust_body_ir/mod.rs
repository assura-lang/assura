//! Encode simple Rust function bodies as Assura Implementation IR for check-rust.
//!
//! Supports int/bool arith, abs/min/max/clamp/saturating(+neg/abs)/abs_diff/signum,
//! is_positive/negative/zero, `is_multiple_of`, PartialOrd methods, logical `&&`/`||`,
//! unary `-`/`!`/`*`/`&`, identity ops (`into`/`as` lossless/`clone`/`copied`/`as_ref`/
//! `not`), `default` and integer MIN/MAX, small `pow`, multi-let (incl. ref/cast folds),
//! if/match (incl. guards), and Bool comparisons. Body text via `syn` (co-publish-safe).
//!
//! Peeps: wrapping `+0`/`-0`/`*1`/`*0`/`sub(x,x)`; shift/rotate by 0;
//! `is_multiple_of(±1)`; same-path `abs_diff`/`min`/`max`/`clamp(_,y,y)`;
//! `abs`/`saturating_abs` `.is_negative()` → false; const `is_power_of_two` /
//! `count_ones` / `count_zeros` / `trailing_zeros` / `leading_zeros` /
//! `reverse_bits` / `swap_bytes` for unsigned path params (bit products; ≤32).
//! Unsigned wrapping_* / shl/shr/rotate via mod 2^w (#1010). Signed
//! wrapping_add/sub/mul and wrapping_shl via double-mod+reinterpret for i8..i64
//! (i64 modulus is synthetic `(2^32)*(2^32)`). Signed rotate via bit-pattern map.
//! Signed wrapping_shr via floor div by 2^k. Top-level signed `wrapping_neg`
//! (multi-block if); nested signed via modular (0-x) mod 2^w + reinterpret.
//! Variable wrapping_shl/shr case-sum for bits≤64 (i64 and u64/usize use
//! synthetic 2^64 modulus; 2^63 factor is 2^32*2^31). Variable rotate_left/right
//! case-sum for bits≤64 (same budget as wrapping_shl/shr). Variable BitAnd/Or/Xor:
//! const mask (unsigned/signed ≤64; signed via bit-pattern map) or both-variable
//! signed/unsigned ≤32. Variable bitwise `!x` for fixed-width ints ≤64
//! (`(2^w-1)-u`, synthetic 2^64 for i64/u64). Variable is_power_of_two via pot
//! enum (≤64 exponents incl. u64/usize). Variable `ilog2`/`ilog10` and
//! `next_power_of_two` for unsigned path params ≤32. Literal `/0`, `%0`,
//! `is_multiple_of(0)` BNM. `signum` nestable clamp (#1032).
//! rem_euclid/div_euclid with positive const (signed Euclidean).
//!
//! Multi-block if IR must use **unique temp slots across sibling blocks**.
//! `eval_ir_block` clones parent slots into each block; reusing `$1`/`$2` for
//! temps collides with the condition/`if` result and makes SMT unsound
//! (false Verified). Match `Clamp.ir`: params `$0..$n-1` are shared; temps
//! monotonically increase (see shared `next` in `emit_value_blocks`).

use assura_rust_analyzer::ParamInfo;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;

thread_local! {
    /// Saturating op bounds for the current encode (set from return type).
    static SAT_BOUNDS: Cell<Option<(i64, i64)>> = const { Cell::new(None) };
    /// Param name → integer bounds (for methods that need width, e.g. is_power_of_two).
    static PARAM_BOUNDS: RefCell<HashMap<String, (i64, i64)>> = RefCell::new(HashMap::new());
}

use quote::ToTokens;

// Layout (issue #1187):
// - bitops: BitAnd/Or/Xor, bit counts, reverse, ilog, next_power_of_two
// - width: wrap_width, pot exponents, path/receiver bounds, rotl, emit_pow2
// - tests: unit coverage for try_ir_from_rust_body
// - this file: extract/body fold, multi-block if/match, encode_syn_expr dispatch
mod bitops;
mod width;
use bitops::*;
use width::*;

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
        syn::Expr::Reference(mut r) => {
            *r.expr = substitute_ident_expr(*r.expr, name, replacement);
            syn::Expr::Reference(r)
        }
        syn::Expr::Cast(mut c) => {
            *c.expr = substitute_ident_expr(*c.expr, name, replacement);
            syn::Expr::Cast(c)
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
    let sat = rust_int_bounds(return_ty.map(str::trim).unwrap_or(""));
    SAT_BOUNDS.set(sat);

    let mut pbounds: HashMap<String, (i64, i64)> = HashMap::new();
    for p in params.iter().filter(|p| p.name != "self") {
        if let Some(b) = rust_int_bounds(p.ty.trim()) {
            pbounds.insert(p.name.clone(), b);
        }
    }
    PARAM_BOUNDS.with(|c| *c.borrow_mut() = pbounds);

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

    let mut expr: syn::Expr = syn::parse_str(body_return).ok()?;
    // Top-level wrapping_neg → multi-block if (MIN stays MIN). Nested signed uses
    // modular encode in the method arm (same bit pattern).
    if let Some(e) = expand_wrapping_neg_method(&expr) {
        expr = e;
    }

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

/// `x.wrapping_neg()` → if x == MIN { MIN } else { -x } (needs SAT_BOUNDS).
/// Signed only (multi-block if). Unsigned uses encode MethodCall mod path.
fn expand_wrapping_neg_method(expr: &syn::Expr) -> Option<syn::Expr> {
    let syn::Expr::MethodCall(m) = expr else {
        return None;
    };
    if m.method != "wrapping_neg" || !m.args.is_empty() {
        return None;
    }
    let (lo, _) = SAT_BOUNDS.get()?;
    // Unsigned: handled in encode_syn_expr via mod 2^w.
    if lo == 0 {
        return None;
    }
    let recv = expr_source(&m.receiver);
    // Avoid raw MIN literals that do not parse as i64 tokens (use -MAX-1).
    let lo_src = if lo == i64::MIN {
        format!("-{} - 1", i64::MAX)
    } else {
        lo.to_string()
    };
    let tree = format!("if {recv} == ({lo_src}) {{ ({lo_src}) }} else {{ -({recv}) }}");
    syn::parse_str(&tree).ok()
}

/// True when `expr` is integer literal 0 (after paren/group peels).
fn is_lit_int_zero(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Paren(p) => is_lit_int_zero(&p.expr),
        syn::Expr::Group(g) => is_lit_int_zero(&g.expr),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => n.base10_digits() == "0",
        _ => false,
    }
}

/// Integer value of a literal (optionally negated), after paren/group peels.
fn lit_int_i64(expr: &syn::Expr) -> Option<i64> {
    match expr {
        syn::Expr::Paren(p) => lit_int_i64(&p.expr),
        syn::Expr::Group(g) => lit_int_i64(&g.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            let v = lit_int_i64(&u.expr)?;
            v.checked_neg()
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => n.base10_parse().ok(),
        _ => None,
    }
}

fn is_lit_int_abs_one(expr: &syn::Expr) -> bool {
    match expr {
        syn::Expr::Paren(p) => is_lit_int_abs_one(&p.expr),
        syn::Expr::Group(g) => is_lit_int_abs_one(&g.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            matches!(
                u.expr.as_ref(),
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Int(n),
                    ..
                }) if n.base10_digits() == "1"
            )
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => n.base10_digits() == "1",
        _ => false,
    }
}

/// Same single-segment path after paren/group peels (`x` vs `x`, not `x` vs `y`).
fn expr_same_simple_path(a: &syn::Expr, b: &syn::Expr) -> bool {
    fn path_name(expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Paren(p) => path_name(&p.expr),
            syn::Expr::Group(g) => path_name(&g.expr),
            syn::Expr::Path(p) if p.path.segments.len() == 1 => {
                Some(p.path.segments[0].ident.to_string())
            }
            _ => None,
        }
    }
    match (path_name(a), path_name(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Clamp `val` slot into SAT_BOUNDS using max/min; returns result slot.
fn emit_sat_clamp(val: usize, lines: &mut Vec<String>, next: &mut usize) -> Option<usize> {
    let (lo_v, hi_v) = SAT_BOUNDS.get()?;
    if is_u64_width_bounds(lo_v, hi_v) {
        // Cannot emit u64::MAX as a single i64 const.
        return None;
    }
    let lo = *next;
    *next += 1;
    lines.push(format!("${lo} = const {lo_v} : Int"));
    let hi = *next;
    *next += 1;
    lines.push(format!("${hi} = const {hi_v} : Int"));
    let mx = *next;
    *next += 1;
    lines.push(format!("${mx} = call max (${val}, ${lo}) : Int"));
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = call min (${mx}, ${hi}) : Int"));
    Some(slot)
}

pub(crate) fn is_identity_peel_method(name: &str) -> bool {
    matches!(
        name,
        "clone"
            | "to_owned"
            | "into"
            | "copied"
            | "cloned"
            | "as_ref"
            | "as_mut"
            | "borrow"
            | "borrow_mut"
            | "deref"
            | "deref_mut"
    )
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
        syn::Expr::Reference(r) => encode_syn_expr(&r.expr, param_names, lines, next),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => {
            encode_syn_expr(&u.expr, param_names, lines, next)
        }
        syn::Expr::Path(path) if path.path.segments.len() == 1 => {
            let name = path.path.segments[0].ident.to_string();
            param_names.iter().position(|n| *n == name)
        }
        // i64::MIN / i64::MAX / u8::MAX (fits i64) as Int consts
        syn::Expr::Path(path) if path.path.segments.len() == 2 => {
            let ty = path.path.segments[0].ident.to_string();
            let name = path.path.segments[1].ident.to_string();
            let val: Option<i64> = match (ty.as_str(), name.as_str()) {
                ("i8", "MIN") => Some(i8::MIN as i64),
                ("i8", "MAX") => Some(i8::MAX as i64),
                ("i16", "MIN") => Some(i16::MIN as i64),
                ("i16", "MAX") => Some(i16::MAX as i64),
                ("i32", "MIN") => Some(i32::MIN as i64),
                ("i32", "MAX") => Some(i32::MAX as i64),
                ("i64", "MIN") | ("isize", "MIN") => Some(i64::MIN),
                ("i64", "MAX") | ("isize", "MAX") => Some(i64::MAX),
                ("u8", "MAX") => Some(u8::MAX as i64),
                ("u16", "MAX") => Some(u16::MAX as i64),
                ("u32", "MAX") => Some(u32::MAX as i64),
                ("u8" | "u16" | "u32", "MIN") => Some(0),
                _ => None,
            };
            let v = val?;
            let slot = *next;
            *next += 1;
            lines.push(format!("${slot} = const {v} : Int"));
            Some(slot)
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
            // Typed integer lit: bitwise NOT (ones' complement within width).
            if let Some((v, bits)) = lit_int_i64_bits(&u.expr)
                && v >= 0
            {
                let mask = if bits == 64 {
                    u64::MAX
                } else {
                    (1u64 << bits) - 1
                };
                let notv = (!(v as u64)) & mask;
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = const {notv} : Int"));
                return Some(slot);
            }
            // Variable integer bitwise NOT (bits ≤64): !x ≡ (2^w-1) - u(x).
            // Integer-typed receivers must not fall through to bool logical not.
            if let Some((lo, hi)) = path_param_bounds(&u.expr).or_else(|| SAT_BOUNDS.get()) {
                let (bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                if bits == 0 || bits > 64 {
                    return None;
                }
                let a = encode_syn_expr(&u.expr, param_names, lines, next)?;
                let mslot = if modulus_i64.is_none() {
                    let half = *next;
                    *next += 1;
                    lines.push(format!("${half} = const 4294967296 : Int"));
                    let m = *next;
                    *next += 1;
                    lines.push(format!("${m} = arith mul ${half} ${half} : Int"));
                    m
                } else {
                    let modulus = modulus_i64?;
                    let m = *next;
                    *next += 1;
                    lines.push(format!("${m} = const {modulus} : Int"));
                    m
                };
                let one = *next;
                *next += 1;
                lines.push(format!("${one} = const 1 : Int"));
                let ones = *next;
                *next += 1;
                lines.push(format!("${ones} = arith sub ${mslot} ${one} : Int"));
                let u_in = emit_to_unsigned_bits(a, mslot, lines, next);
                let not_u = *next;
                *next += 1;
                lines.push(format!("${not_u} = arith sub ${ones} ${u_in} : Int"));
                if signed {
                    return Some(emit_from_unsigned_bits(not_u, mslot, hi, lines, next));
                }
                return Some(not_u);
            }
            // Bool / general: logical not as eq 0.
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
            // Const-fold bitops when both sides are integer lits (partial #1034 bitops).
            if let (Some(l), Some(r)) = (lit_int_i64(&b.left), lit_int_i64(&b.right)) {
                let folded = match &b.op {
                    syn::BinOp::BitAnd(_) if l >= 0 && r >= 0 => Some((l as u64 & r as u64) as i64),
                    syn::BinOp::BitOr(_) if l >= 0 && r >= 0 => Some((l as u64 | r as u64) as i64),
                    syn::BinOp::BitXor(_) if l >= 0 && r >= 0 => Some((l as u64 ^ r as u64) as i64),
                    syn::BinOp::Shl(_) if l >= 0 && (0..63).contains(&r) => {
                        Some(((l as u64) << (r as u32)) as i64)
                    }
                    syn::BinOp::Shr(_) if l >= 0 && (0..63).contains(&r) => {
                        Some(((l as u64) >> (r as u32)) as i64)
                    }
                    _ => None,
                };
                if let Some(val) = folded {
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {val} : Int"));
                    return Some(slot);
                }
            }
            // Variable BitAnd/Or/Xor: const mask (signed/unsigned) or both-var ≤32.
            if let Some(kind) = match &b.op {
                syn::BinOp::BitAnd(_) => Some(BitOpKind::And),
                syn::BinOp::BitOr(_) => Some(BitOpKind::Or),
                syn::BinOp::BitXor(_) => Some(BitOpKind::Xor),
                _ => None,
            } {
                let sides: Option<(&syn::Expr, i64)> =
                    match (lit_int_i64(&b.left), lit_int_i64(&b.right)) {
                        (Some(m), None) => Some((&b.right, m)),
                        (None, Some(m)) => Some((&b.left, m)),
                        _ => None,
                    };
                if let Some((var_e, mask_i)) = sides {
                    let bounds = path_param_bounds(var_e).or_else(|| SAT_BOUNDS.get());
                    if let Some((lo, hi)) = bounds
                        && let Some((bits, modulus_i64, signed)) = wrap_width(lo, hi)
                        && bits > 0
                        && bits <= 64
                        && (signed || mask_i >= 0)
                    {
                        // Signed path needs concrete modulus for reinterpret (no i64 free-range).
                        if signed && modulus_i64.is_none() {
                            // i64 both-var signed bitops still too heavy; const mask OK via u map
                        }
                        let mask = mask_bits_u64(mask_i, bits);
                        let a = encode_syn_expr(var_e, param_names, lines, next)?;
                        if !signed {
                            return encode_unsigned_bitop_var_const(
                                a, mask, kind, bits, lines, next,
                            );
                        }
                        // Signed: map to unsigned bit pattern, bitop, reinterpret.
                        let Some(modulus) = modulus_i64 else {
                            // i64: synthetic 2^64 modulus
                            let half = *next;
                            *next += 1;
                            lines.push(format!("${half} = const 4294967296 : Int"));
                            let mslot = *next;
                            *next += 1;
                            lines.push(format!("${mslot} = arith mul ${half} ${half} : Int"));
                            let u_in = emit_to_unsigned_bits(a, mslot, lines, next);
                            let u_out = encode_unsigned_bitop_var_const(
                                u_in, mask, kind, bits, lines, next,
                            )?;
                            return Some(emit_from_unsigned_bits(u_out, mslot, hi, lines, next));
                        };
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = const {modulus} : Int"));
                        let u_in = emit_to_unsigned_bits(a, mslot, lines, next);
                        let u_out =
                            encode_unsigned_bitop_var_const(u_in, mask, kind, bits, lines, next)?;
                        return Some(emit_from_unsigned_bits(u_out, mslot, hi, lines, next));
                    }
                } else if lit_int_i64(&b.left).is_none() && lit_int_i64(&b.right).is_none() {
                    // Both variable: unsigned or signed (bit-pattern map) for bits ≤32.
                    let info = path_param_bounds(&b.left)
                        .or_else(|| path_param_bounds(&b.right))
                        .or_else(|| SAT_BOUNDS.get())
                        .and_then(|(lo, hi)| {
                            let (bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                            if bits == 0 || bits > 32 {
                                return None;
                            }
                            let modulus = modulus_i64?;
                            Some((bits, modulus, signed, hi))
                        });
                    if let Some((bits, modulus, signed, hi)) = info {
                        let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
                        let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
                        if !signed {
                            return encode_unsigned_bitop_var_var(
                                lhs, rhs, kind, bits, lines, next,
                            );
                        }
                        // Signed: map both to unsigned bit patterns, bitop, reinterpret.
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = const {modulus} : Int"));
                        let u_l = emit_to_unsigned_bits(lhs, mslot, lines, next);
                        let u_r = emit_to_unsigned_bits(rhs, mslot, lines, next);
                        let u_out =
                            encode_unsigned_bitop_var_var(u_l, u_r, kind, bits, lines, next)?;
                        return Some(emit_from_unsigned_bits(u_out, mslot, hi, lines, next));
                    }
                }
            }
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
            // Bool 0/1: a && b → mul; a || b → (a+b) != 0 (matches ir_generate)
            if matches!(b.op, syn::BinOp::And(_)) {
                let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
                let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = arith mul ${lhs} ${rhs} : Bool"));
                return Some(slot);
            }
            if matches!(b.op, syn::BinOp::Or(_)) {
                let lhs = encode_syn_expr(&b.left, param_names, lines, next)?;
                let rhs = encode_syn_expr(&b.right, param_names, lines, next)?;
                let sum = *next;
                *next += 1;
                lines.push(format!("${sum} = arith add ${lhs} ${rhs} : Bool"));
                let zero = *next;
                *next += 1;
                lines.push(format!("${zero} = const 0 : Bool"));
                let slot = *next;
                *next += 1;
                lines.push(format!("${slot} = cmp ne ${sum} ${zero} : Bool"));
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
            // Refuse literal /0 and %0 (Rust panic / UB); do not give SMT a free
            // div-by-zero term that can pass ensures spuriously.
            if matches!(ir_op, "div" | "mod") && is_lit_int_zero(&b.right) {
                return None;
            }
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
                ("abs" | "unsigned_abs", 0) => {
                    // unsigned_abs is |x| as unsigned magnitude; SMT Int uses abs.
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${a}) : Int"));
                    Some(slot)
                }
                // Integer signum ≡ clamp to [-1, 1] via min/max (nestable; #1032).
                // max(min(x, 1), -1) matches x.signum() for all Int values.
                ("signum", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let lo = *next;
                    *next += 1;
                    lines.push(format!("${lo} = const -1 : Int"));
                    let hi = *next;
                    *next += 1;
                    lines.push(format!("${hi} = const 1 : Int"));
                    let mx = *next;
                    *next += 1;
                    lines.push(format!("${mx} = call max (${a}, ${lo}) : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call min (${mx}, ${hi}) : Int"));
                    Some(slot)
                }
                ("is_positive", 0) => {
                    // abs_diff(x, x).is_positive() is always false
                    if let syn::Expr::MethodCall(inner) = m.receiver.as_ref()
                        && inner.method == "abs_diff"
                        && inner.args.len() == 1
                        && expr_same_simple_path(&inner.receiver, &inner.args[0])
                    {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const 0 : Bool"));
                        return Some(slot);
                    }
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
                    // abs/saturating_abs(...).is_negative() is always false
                    if let syn::Expr::MethodCall(inner) = m.receiver.as_ref()
                        && matches!(inner.method.to_string().as_str(), "abs" | "saturating_abs")
                        && inner.args.is_empty()
                    {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const 0 : Bool"));
                        return Some(slot);
                    }
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
                    // abs_diff(x, x).is_zero() is always true
                    if let syn::Expr::MethodCall(inner) = m.receiver.as_ref()
                        && inner.method == "abs_diff"
                        && inner.args.len() == 1
                        && expr_same_simple_path(&inner.receiver, &inner.args[0])
                    {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const 1 : Bool"));
                        return Some(slot);
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let z = *next;
                    *next += 1;
                    lines.push(format!("${z} = const 0 : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp eq ${a} ${z} : Bool"));
                    Some(slot)
                }
                // Const lit peep, or pot enum over path/nested expr bounds (#1034).
                // Nested: `(x+1).is_power_of_two()` uses expr_int_bounds(x).
                ("is_power_of_two", 0) => {
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        let pot = v > 0 && (v as u64).is_power_of_two();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!(
                            "${slot} = const {} : Bool",
                            if pot { 1 } else { 0 }
                        ));
                        return Some(slot);
                    }
                    let (lo, hi) = expr_int_bounds(&m.receiver)?;
                    let n_exp = pot_exponents(lo, hi)?;
                    // Cap IR size: u64 uses 64 OR-chain steps (2^63 via emit_pow2_factor).
                    if n_exp > 64 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let mut acc: Option<usize> = None;
                    for e in 0..n_exp {
                        // e==63 needs synthetic 2^63 (1i64<<63 is negative)
                        let c = emit_pow2_factor(e, lines, next)?;
                        let eq = *next;
                        *next += 1;
                        lines.push(format!("${eq} = cmp eq ${a} ${c} : Bool"));
                        acc = Some(match acc {
                            None => eq,
                            Some(prev) => {
                                let sum = *next;
                                *next += 1;
                                lines.push(format!("${sum} = arith add ${prev} ${eq} : Bool"));
                                let zero = *next;
                                *next += 1;
                                lines.push(format!("${zero} = const 0 : Bool"));
                                let or_s = *next;
                                *next += 1;
                                lines.push(format!("${or_s} = cmp ne ${sum} ${zero} : Bool"));
                                or_s
                            }
                        });
                    }
                    acc
                }
                // count_ones: const peep, or unsigned path-param bit-sum (≤32 bits).
                ("count_ones", 0) => {
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let ones = (v as u64).count_ones();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {ones} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None;
                    }
                    let modulus = hi.checked_add(1)?;
                    let modulus_u = modulus as u64;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_bit_sum_count_ones(a, bits, lines, next)
                }
                // trailing_ones: non-neg lit (width-independent for magnitude).
                ("trailing_ones", 0) => {
                    let v = lit_int_i64(&m.receiver)?;
                    if v < 0 {
                        return None;
                    }
                    let t = (v as u64).trailing_ones();
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {t} : Int"));
                    Some(slot)
                }
                // leading_ones needs typed width (like leading_zeros).
                ("leading_ones", 0) => {
                    let (v, bits) = lit_int_i64_bits(&m.receiver)?;
                    if v < 0 {
                        return None;
                    }
                    let lo = match bits {
                        8 => (v as u8).leading_ones(),
                        16 => (v as u16).leading_ones(),
                        32 => (v as u32).leading_ones(),
                        64 => (v as u64).leading_ones(),
                        _ => return None,
                    };
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {lo} : Int"));
                    Some(slot)
                }
                // Typed width: count_zeros = bits - count_ones (non-neg lit).
                // Unsigned path params: bits - count_ones (shared bit-sum helper).
                ("count_zeros", 0) => {
                    if let Some((v, bits)) = lit_int_i64_bits(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let zeros = bits - (v as u64).count_ones();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {zeros} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None;
                    }
                    let modulus = hi.checked_add(1)?;
                    let modulus_u = modulus as u64;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let ones = encode_bit_sum_count_ones(a, bits, lines, next)?;
                    let bits_c = *next;
                    *next += 1;
                    lines.push(format!("${bits_c} = const {bits} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith sub ${bits_c} ${ones} : Int"));
                    Some(slot)
                }
                // Non-neg lit; 0.trailing_zeros needs typed width (= bits).
                // Unsigned path params: first-set-bit product encode (≤32 bits).
                ("trailing_zeros", 0) => {
                    if let Some((v, bits)) = lit_int_i64_bits(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let tz = if v == 0 {
                            bits
                        } else {
                            (v as u64).trailing_zeros()
                        };
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {tz} : Int"));
                        return Some(slot);
                    }
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        if v <= 0 {
                            return None;
                        }
                        let tz = (v as u64).trailing_zeros();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {tz} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None;
                    }
                    let modulus = hi.checked_add(1)?;
                    let modulus_u = modulus as u64;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_trailing_zeros(a, bits, lines, next)
                }
                // Needs typed suffix for bit width (8u32.leading_zeros() → 28).
                // Unsigned path params: high→low zero-prefix product (≤32 bits).
                ("leading_zeros", 0) => {
                    if let Some((v, bits)) = lit_int_i64_bits(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let lz = if v == 0 {
                            bits
                        } else {
                            (v as u64).leading_zeros() - (64 - bits)
                        };
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {lz} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None;
                    }
                    let modulus = hi.checked_add(1)?;
                    let modulus_u = modulus as u64;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_leading_zeros(a, bits, lines, next)
                }
                // Typed non-neg lit or unsigned path-param bit reverse.
                ("reverse_bits", 0) => {
                    if let Some((v, bits)) = lit_int_i64_bits(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let rev = match bits {
                            8 => (v as u8).reverse_bits() as i64,
                            16 => (v as u16).reverse_bits() as i64,
                            32 => (v as u32).reverse_bits() as i64,
                            64 => (v as u64).reverse_bits() as i64,
                            _ => return None,
                        };
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {rev} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None;
                    }
                    let modulus = hi.checked_add(1)?;
                    let modulus_u = modulus as u64;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_reverse_bits(a, bits, lines, next)
                }
                ("swap_bytes", 0) => {
                    if let Some((v, bits)) = lit_int_i64_bits(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let sw = match bits {
                            8 => v, // no-op
                            16 => (v as u16).swap_bytes() as i64,
                            32 => (v as u32).swap_bytes() as i64,
                            64 => (v as u64).swap_bytes() as i64,
                            _ => return None,
                        };
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {sw} : Int"));
                        return Some(slot);
                    }
                    // Unsigned path params: reverse byte order (≤32 bits).
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None;
                    }
                    let modulus = hi.checked_add(1)?;
                    let modulus_u = modulus as u64;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    if bits == 0 || bits > 32 || !bits.is_multiple_of(8) {
                        return None;
                    }
                    let nbytes = bits / 8;
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    if nbytes == 1 {
                        return Some(a); // u8 identity
                    }
                    let b256 = *next;
                    *next += 1;
                    lines.push(format!("${b256} = const 256 : Int"));
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Int"));
                    let mut acc = zero;
                    for i in 0..nbytes {
                        // byte_i = (a / 256^i) % 256
                        let mut div = a;
                        for _ in 0..i {
                            let d = *next;
                            *next += 1;
                            lines.push(format!("${d} = arith div ${div} ${b256} : Int"));
                            div = d;
                        }
                        let byte = *next;
                        *next += 1;
                        lines.push(format!("${byte} = arith mod ${div} ${b256} : Int"));
                        // place at 256^(nbytes-1-i)
                        let mut placed = byte;
                        for _ in 0..(nbytes - 1 - i) {
                            let m = *next;
                            *next += 1;
                            lines.push(format!("${m} = arith mul ${placed} ${b256} : Int"));
                            placed = m;
                        }
                        let sum = *next;
                        *next += 1;
                        lines.push(format!("${sum} = arith add ${acc} ${placed} : Int"));
                        acc = sum;
                    }
                    Some(acc)
                }
                // Const peep (ilog2(0) panics → BNM). Variable: unsigned path ≤32.
                ("ilog2", 0) => {
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        if v <= 0 {
                            return None;
                        }
                        let log = (v as u64).ilog2();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {log} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 {
                        return None; // signed / non-unsigned
                    }
                    if is_u64_width_bounds(lo, hi) {
                        return None; // u64: 64-bit product too large for now
                    }
                    let modulus_u = (hi as u64).checked_add(1)?;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    if bits == 0 || bits > 32 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_ilog2(a, bits, lines, next)
                }
                // Const peep; variable unsigned path ≤32 via threshold sum.
                ("ilog10", 0) => {
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        if v <= 0 {
                            return None;
                        }
                        let log = (v as u64).ilog10();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {log} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 || is_u64_width_bounds(lo, hi) || hi <= 0 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_ilog10(a, hi, lines, next)
                }
                // Const peep; variable unsigned path ≤32 (overflow → 0, like wrap).
                ("next_power_of_two", 0) => {
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let pot = (v as u64).next_power_of_two();
                        // Keep result in i64 (next_power_of_two of values near 2^63 overflows)
                        if pot > i64::MAX as u64 {
                            return None;
                        }
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {pot} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 || is_u64_width_bounds(lo, hi) {
                        return None;
                    }
                    let modulus_u = (hi as u64).checked_add(1)?;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_next_power_of_two(a, bits, lines, next)
                }
                // Const peep or variable unsigned path ≤32 (overflow → 0).
                // Manual: stable API; unstable wrapping_next_power_of_two not used.
                ("wrapping_next_power_of_two", 0) => {
                    if let Some((v, bits)) = lit_int_i64_bits(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let vu = v as u64;
                        let pot = if vu == 0 {
                            1u64
                        } else {
                            let n = vu.next_power_of_two();
                            // If next power does not fit in `bits`, wrap to 0.
                            if bits < 64 && n >= (1u64 << bits) {
                                0
                            } else if bits == 64 && n < vu {
                                // u64 overflow wraps to 0
                                0
                            } else {
                                n
                            }
                        };
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {pot} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 || is_u64_width_bounds(lo, hi) {
                        return None;
                    }
                    let modulus_u = (hi as u64).checked_add(1)?;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_next_power_of_two(a, bits, lines, next)
                }
                // Const peep; variable unsigned path ≤16 (IR ladder size).
                ("isqrt", 0) => {
                    if let Some(v) = lit_int_i64(&m.receiver) {
                        if v < 0 {
                            return None;
                        }
                        let root = (v as u64).isqrt();
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const {root} : Int"));
                        return Some(slot);
                    }
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    if lo != 0 || is_u64_width_bounds(lo, hi) {
                        return None;
                    }
                    let modulus_u = (hi as u64).checked_add(1)?;
                    if !modulus_u.is_power_of_two() {
                        return None;
                    }
                    let bits = modulus_u.trailing_zeros();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    encode_unsigned_isqrt(a, bits, lines, next)
                }
                ("default", 0) => {
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const 0 : Int"));
                    Some(slot)
                }
                ("lt" | "le" | "gt" | "ge" | "eq" | "ne", 1) => {
                    let cmp = method.as_str();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp {cmp} ${a} ${b} : Bool"));
                    Some(slot)
                }
                (name, 0) if is_identity_peel_method(name) => {
                    encode_syn_expr(&m.receiver, param_names, lines, next)
                }
                ("not", 0) => {
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Bool"));
                    let inner = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp eq ${inner} ${zero} : Bool"));
                    Some(slot)
                }

                ("is_multiple_of", 1) => {
                    // Rust panics on divisor 0; refuse literal 0 so we do not
                    // model mod-by-zero as a free SMT fact (false Verified risk).
                    if is_lit_int_zero(&m.args[0]) {
                        return None;
                    }
                    // is_multiple_of(±1) is always true for integers.
                    if is_lit_int_abs_one(&m.args[0]) {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const 1 : Bool"));
                        return Some(slot);
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let rem = *next;
                    *next += 1;
                    lines.push(format!("${rem} = arith mod ${a} ${b} : Int"));
                    let z = *next;
                    *next += 1;
                    lines.push(format!("${z} = const 0 : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = cmp eq ${rem} ${z} : Bool"));
                    Some(slot)
                }
                // div_ceil for non-neg receiver + positive const divisor: (a+b-1)/b.
                // Requires unsigned/Nat param bounds (or const non-neg lit receiver).
                ("div_ceil", 1) => {
                    let b_val = lit_int_i64(&m.args[0])?;
                    if b_val <= 0 {
                        return None;
                    }
                    // Receiver must be non-negative: path param with lo>=0, or non-neg lit.
                    let nonneg = if let Some(v) = lit_int_i64(&m.receiver) {
                        v >= 0
                    } else if let Some((lo, _)) = path_param_bounds(&m.receiver) {
                        lo >= 0
                    } else {
                        false
                    };
                    if !nonneg {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = *next;
                    *next += 1;
                    lines.push(format!("${b} = const {b_val} : Int"));
                    let one = *next;
                    *next += 1;
                    lines.push(format!("${one} = const 1 : Int"));
                    let bm1 = *next;
                    *next += 1;
                    lines.push(format!("${bm1} = arith sub ${b} ${one} : Int"));
                    let sum = *next;
                    *next += 1;
                    lines.push(format!("${sum} = arith add ${a} ${bm1} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith div ${sum} ${b} : Int"));
                    Some(slot)
                }
                // rem_euclid with positive const divisor: ((a mod b) + b) mod b
                // (works for signed; non-neg reduces to a mod b).
                ("rem_euclid", 1) => {
                    let b_val = lit_int_i64(&m.args[0])?;
                    if b_val <= 0 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = *next;
                    *next += 1;
                    lines.push(format!("${b} = const {b_val} : Int"));
                    let t1 = *next;
                    *next += 1;
                    lines.push(format!("${t1} = arith mod ${a} ${b} : Int"));
                    let t2 = *next;
                    *next += 1;
                    lines.push(format!("${t2} = arith add ${t1} ${b} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith mod ${t2} ${b} : Int"));
                    Some(slot)
                }
                // div_euclid with positive const divisor ≡ floor div (SMT Int).
                ("div_euclid", 1) => {
                    let b_val = lit_int_i64(&m.args[0])?;
                    if b_val <= 0 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = *next;
                    *next += 1;
                    lines.push(format!("${b} = const {b_val} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith div ${a} ${b} : Int"));
                    Some(slot)
                }
                // next_multiple_of with positive const m: a - rem + m*[rem!=0]
                // where rem = rem_euclid(a,m). Works for signed a.
                ("next_multiple_of", 1) => {
                    let m_val = lit_int_i64(&m.args[0])?;
                    if m_val <= 0 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let mv = *next;
                    *next += 1;
                    lines.push(format!("${mv} = const {m_val} : Int"));
                    // rem = ((a mod m) + m) mod m
                    let t1 = *next;
                    *next += 1;
                    lines.push(format!("${t1} = arith mod ${a} ${mv} : Int"));
                    let t2 = *next;
                    *next += 1;
                    lines.push(format!("${t2} = arith add ${t1} ${mv} : Int"));
                    let rem = *next;
                    *next += 1;
                    lines.push(format!("${rem} = arith mod ${t2} ${mv} : Int"));
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Int"));
                    let is_zero = *next;
                    *next += 1;
                    lines.push(format!("${is_zero} = cmp eq ${rem} ${zero} : Bool"));
                    let one = *next;
                    *next += 1;
                    lines.push(format!("${one} = const 1 : Int"));
                    let not_zero = *next;
                    *next += 1;
                    lines.push(format!("${not_zero} = arith sub ${one} ${is_zero} : Int"));
                    let m_if = *next;
                    *next += 1;
                    lines.push(format!("${m_if} = arith mul ${mv} ${not_zero} : Int"));
                    let a_m_rem = *next;
                    *next += 1;
                    lines.push(format!("${a_m_rem} = arith sub ${a} ${rem} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith add ${a_m_rem} ${m_if} : Int"));
                    Some(slot)
                }
                ("pow", 1) => {
                    let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Int(n),
                        ..
                    }) = &m.args[0]
                    else {
                        return None;
                    };
                    let exp: u32 = n.base10_parse().ok()?;
                    if exp > 4 {
                        return None;
                    }
                    let base = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    if exp == 0 {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const 1 : Int"));
                        return Some(slot);
                    }
                    let mut acc = base;
                    for _ in 1..exp {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = arith mul ${acc} ${base} : Int"));
                        acc = slot;
                    }
                    Some(acc)
                }

                ("min" | "max", 1) => {
                    if expr_same_simple_path(&m.receiver, &m.args[0]) {
                        return encode_syn_expr(&m.receiver, param_names, lines, next);
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call {method} (${a}, ${b}) : Int"));
                    Some(slot)
                }
                // midpoint: floor((a+b)/2); Int SMT is unbounded so no overflow.
                ("midpoint", 1) => {
                    if expr_same_simple_path(&m.receiver, &m.args[0]) {
                        return encode_syn_expr(&m.receiver, param_names, lines, next);
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let sum = *next;
                    *next += 1;
                    lines.push(format!("${sum} = arith add ${a} ${b} : Int"));
                    let two = *next;
                    *next += 1;
                    lines.push(format!("${two} = const 2 : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith div ${sum} ${two} : Int"));
                    Some(slot)
                }
                // x.clamp(lo, hi) ≡ min(max(x, lo), hi)
                ("clamp", 2) => {
                    // clamp(x, b, b) ≡ b for any x
                    if expr_same_simple_path(&m.args[0], &m.args[1]) {
                        return encode_syn_expr(&m.args[0], param_names, lines, next);
                    }
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
                ("abs_diff", 1) => {
                    if expr_same_simple_path(&m.receiver, &m.args[0]) {
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = const 0 : Int"));
                        return Some(slot);
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let d = *next;
                    *next += 1;
                    lines.push(format!("${d} = arith sub ${a} ${b} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${d}) : Int"));
                    Some(slot)
                }
                ("saturating_neg", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Int"));
                    let neg = *next;
                    *next += 1;
                    lines.push(format!("${neg} = arith sub ${zero} ${a} : Int"));
                    emit_sat_clamp(neg, lines, next)
                }
                // saturating_abs: abs then clamp to MAX (i64::MIN → MAX, not |MIN|).
                ("saturating_abs", 0) => {
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let ab = *next;
                    *next += 1;
                    lines.push(format!("${ab} = call abs (${a}) : Int"));
                    let (_, hi_v) = SAT_BOUNDS.get()?;
                    let hi = *next;
                    *next += 1;
                    lines.push(format!("${hi} = const {hi_v} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call min (${ab}, ${hi}) : Int"));
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
                    emit_sat_clamp(sum, lines, next)
                }
                // wrapping_* peeps for identity constants (before general mod 2^w path):
                // wrapping_add(x, 0) / wrapping_sub(x, 0) ≡ x; wrapping_mul(x, 1) ≡ x;
                // wrapping_mul(x, 0) ≡ 0; wrapping_sub(x, x) ≡ 0.
                ("wrapping_add" | "wrapping_sub", 1) if is_lit_int_zero(&m.args[0]) => {
                    encode_syn_expr(&m.receiver, param_names, lines, next)
                }
                ("wrapping_sub", 1) if expr_same_simple_path(&m.receiver, &m.args[0]) => {
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const 0 : Int"));
                    Some(slot)
                }
                ("wrapping_mul", 1) if is_lit_int_zero(&m.args[0]) => {
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const 0 : Int"));
                    Some(slot)
                }
                ("wrapping_mul", 1)
                    if matches!(
                        &m.args[0],
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Int(n),
                            ..
                        }) if n.base10_digits() == "1"
                    ) =>
                {
                    encode_syn_expr(&m.receiver, param_names, lines, next)
                }
                // Shift/rotate by 0 is identity (signed or unsigned).
                ("wrapping_shl" | "wrapping_shr" | "rotate_left" | "rotate_right", 1)
                    if is_lit_int_zero(&m.args[0]) =>
                {
                    encode_syn_expr(&m.receiver, param_names, lines, next)
                }
                // wrapping_shl/shr by const or variable amount (#1010/#1145/#1151/#1160).
                // Unsigned: shl (x * 2^k) mod 2^w; shr floor x / 2^k.
                // Signed: wrapping_shl via mul+double-mod+reinterpret; wrapping_shr
                // via floor div by 2^k. Variable k: case-sum over k%bits for bits<=64.
                // u64/usize use synthetic 2^64 (bounds sentinel (0,-1)).
                ("wrapping_shl" | "wrapping_shr", 1) => {
                    let (lo, hi) = wrap_bounds_for(&m.receiver)?;
                    let (bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                    let use_synthetic_2_64 = modulus_i64.is_none();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    // Const shift amount (fast path)
                    if let Some(k) = lit_int_i64(&m.args[0]) {
                        if k < 0 {
                            return None;
                        }
                        let k_eff = (k as u64) % (bits as u64);
                        if k_eff == 0 {
                            return Some(a);
                        }
                        let f = emit_pow2_factor(k_eff as u32, lines, next)?;
                        if method == "wrapping_shr" {
                            let slot = *next;
                            *next += 1;
                            lines.push(format!("${slot} = arith div ${a} ${f} : Int"));
                            return Some(slot);
                        }
                        let raw = *next;
                        *next += 1;
                        lines.push(format!("${raw} = arith mul ${a} ${f} : Int"));
                        let mslot = if use_synthetic_2_64 {
                            let half = *next;
                            *next += 1;
                            lines.push(format!("${half} = const 4294967296 : Int"));
                            let mslot = *next;
                            *next += 1;
                            lines.push(format!("${mslot} = arith mul ${half} ${half} : Int"));
                            mslot
                        } else {
                            let modulus = modulus_i64?;
                            let mslot = *next;
                            *next += 1;
                            lines.push(format!("${mslot} = const {modulus} : Int"));
                            mslot
                        };
                        let t1 = *next;
                        *next += 1;
                        lines.push(format!("${t1} = arith mod ${raw} ${mslot} : Int"));
                        let t2 = *next;
                        *next += 1;
                        lines.push(format!("${t2} = arith add ${t1} ${mslot} : Int"));
                        let u = *next;
                        *next += 1;
                        lines.push(format!("${u} = arith mod ${t2} ${mslot} : Int"));
                        if !signed {
                            return Some(u);
                        }
                        let his = *next;
                        *next += 1;
                        lines.push(format!("${his} = const {hi} : Int"));
                        let gt = *next;
                        *next += 1;
                        lines.push(format!("${gt} = cmp gt ${u} ${his} : Bool"));
                        let adj = *next;
                        *next += 1;
                        lines.push(format!("${adj} = arith mul ${gt} ${mslot} : Int"));
                        let slot = *next;
                        *next += 1;
                        lines.push(format!("${slot} = arith sub ${u} ${adj} : Int"));
                        return Some(slot);
                    }
                    // Variable shift: case-sum over k%bits (bits<=64, incl. signed i64).
                    if bits > 64 {
                        return None;
                    }
                    let k_slot = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    // Require non-neg path param for shift amount when available.
                    if let Some((klo, _)) = path_param_bounds(&m.args[0])
                        && klo < 0
                    {
                        return None;
                    }
                    let bits_c = *next;
                    *next += 1;
                    lines.push(format!("${bits_c} = const {bits} : Int"));
                    // k_eff = k mod bits (k non-neg)
                    let t1 = *next;
                    *next += 1;
                    lines.push(format!("${t1} = arith mod ${k_slot} ${bits_c} : Int"));
                    let t2 = *next;
                    *next += 1;
                    lines.push(format!("${t2} = arith add ${t1} ${bits_c} : Int"));
                    let k_eff = *next;
                    *next += 1;
                    lines.push(format!("${k_eff} = arith mod ${t2} ${bits_c} : Int"));
                    let mslot = if use_synthetic_2_64 {
                        let half = *next;
                        *next += 1;
                        lines.push(format!("${half} = const 4294967296 : Int"));
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = arith mul ${half} ${half} : Int"));
                        mslot
                    } else {
                        let modulus = modulus_i64?;
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = const {modulus} : Int"));
                        mslot
                    };
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Int"));
                    let mut acc = zero;
                    for e in 0..bits {
                        let e_c = *next;
                        *next += 1;
                        lines.push(format!("${e_c} = const {e} : Int"));
                        let eq = *next;
                        *next += 1;
                        lines.push(format!("${eq} = cmp eq ${k_eff} ${e_c} : Bool"));
                        let f = emit_pow2_factor(e, lines, next)?;
                        let case_val = if method == "wrapping_shr" {
                            let d = *next;
                            *next += 1;
                            lines.push(format!("${d} = arith div ${a} ${f} : Int"));
                            d
                        } else {
                            let raw = *next;
                            *next += 1;
                            lines.push(format!("${raw} = arith mul ${a} ${f} : Int"));
                            let s1 = *next;
                            *next += 1;
                            lines.push(format!("${s1} = arith mod ${raw} ${mslot} : Int"));
                            let s2 = *next;
                            *next += 1;
                            lines.push(format!("${s2} = arith add ${s1} ${mslot} : Int"));
                            let u = *next;
                            *next += 1;
                            lines.push(format!("${u} = arith mod ${s2} ${mslot} : Int"));
                            if !signed {
                                u
                            } else {
                                let his = *next;
                                *next += 1;
                                lines.push(format!("${his} = const {hi} : Int"));
                                let gt = *next;
                                *next += 1;
                                lines.push(format!("${gt} = cmp gt ${u} ${his} : Bool"));
                                let adj = *next;
                                *next += 1;
                                lines.push(format!("${adj} = arith mul ${gt} ${mslot} : Int"));
                                let slot = *next;
                                *next += 1;
                                lines.push(format!("${slot} = arith sub ${u} ${adj} : Int"));
                                slot
                            }
                        };
                        let term = *next;
                        *next += 1;
                        lines.push(format!("${term} = arith mul ${eq} ${case_val} : Int"));
                        let sum = *next;
                        *next += 1;
                        lines.push(format!("${sum} = arith add ${acc} ${term} : Int"));
                        acc = sum;
                    }
                    Some(acc)
                }
                // rotate_left/right by const or variable k.
                // Unsigned: rotl ≡ (x*2^k + x/2^(bits-k)) mod 2^w.
                // Signed: map to unsigned bit pattern, rotate, reinterpret.
                // Variable: case-sum over k%bits for bits<=64 (same budget as shifts).
                ("rotate_left" | "rotate_right", 1) => {
                    let (lo, hi) = wrap_bounds_for(&m.receiver)?;
                    let (bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                    let use_synthetic_2_64 = modulus_i64.is_none();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let mslot = if use_synthetic_2_64 {
                        let half = *next;
                        *next += 1;
                        lines.push(format!("${half} = const 4294967296 : Int"));
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = arith mul ${half} ${half} : Int"));
                        mslot
                    } else {
                        let modulus = modulus_i64?;
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = const {modulus} : Int"));
                        mslot
                    };
                    let wrap = RotWrap {
                        bits,
                        mslot,
                        signed,
                        hi,
                    };
                    // Unsigned bit pattern: ((a mod m) + m) mod m
                    let t1 = *next;
                    *next += 1;
                    lines.push(format!("${t1} = arith mod ${a} ${mslot} : Int"));
                    let t2 = *next;
                    *next += 1;
                    lines.push(format!("${t2} = arith add ${t1} ${mslot} : Int"));
                    let u_in = *next;
                    *next += 1;
                    lines.push(format!("${u_in} = arith mod ${t2} ${mslot} : Int"));

                    // Const shift amount (fast path)
                    if let Some(k) = lit_int_i64(&m.args[0]) {
                        if k < 0 {
                            return None;
                        }
                        let k_eff = (k as u32) % bits;
                        if k_eff == 0 {
                            return Some(a);
                        }
                        // rotate_right(k) ≡ rotate_left(bits-k)
                        let k_left = if method == "rotate_left" {
                            k_eff
                        } else {
                            bits - k_eff
                        };
                        return emit_rotl_bits(u_in, k_left, &wrap, lines, next);
                    }

                    // Variable: case-sum over k%bits (bits<=64, matching wrapping_shl/shr).
                    if bits == 0 || bits > 64 {
                        return None;
                    }
                    let k_slot = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    if let Some((klo, _)) = path_param_bounds(&m.args[0])
                        && klo < 0
                    {
                        return None;
                    }
                    let bits_c = *next;
                    *next += 1;
                    lines.push(format!("${bits_c} = const {bits} : Int"));
                    let tm1 = *next;
                    *next += 1;
                    lines.push(format!("${tm1} = arith mod ${k_slot} ${bits_c} : Int"));
                    let tm2 = *next;
                    *next += 1;
                    lines.push(format!("${tm2} = arith add ${tm1} ${bits_c} : Int"));
                    let k_eff = *next;
                    *next += 1;
                    lines.push(format!("${k_eff} = arith mod ${tm2} ${bits_c} : Int"));
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Int"));
                    let mut acc = zero;
                    for e in 0..bits {
                        let e_c = *next;
                        *next += 1;
                        lines.push(format!("${e_c} = const {e} : Int"));
                        let eq = *next;
                        *next += 1;
                        lines.push(format!("${eq} = cmp eq ${k_eff} ${e_c} : Bool"));
                        // For rotate_right, case e means rotl by (bits-e)%bits
                        let k_left = if method == "rotate_left" {
                            e
                        } else if e == 0 {
                            0
                        } else {
                            bits - e
                        };
                        let case_val = if k_left == 0 {
                            if signed {
                                emit_rotl_bits(u_in, 0, &wrap, lines, next)?
                            } else {
                                u_in
                            }
                        } else {
                            emit_rotl_bits(u_in, k_left, &wrap, lines, next)?
                        };
                        let term = *next;
                        *next += 1;
                        lines.push(format!("${term} = arith mul ${eq} ${case_val} : Int"));
                        let sum = *next;
                        *next += 1;
                        lines.push(format!("${sum} = arith add ${acc} ${term} : Int"));
                        acc = sum;
                    }
                    Some(acc)
                }
                // wrapping_* via mod 2^w (#1010).
                // Unsigned (lo==0): result in [0, 2^w).
                // Signed: double-mod to [0, 2^w) then reinterpret as two's complement.
                // Double-mod ((raw mod m) + m) mod m works for large |raw| (signed mul)
                // without a 2^(2w-1) offset that may not fit in i64.
                // i64 modulus is 2^64: emit as (2^32)*(2^32) (const 2^64 is not i64).
                ("wrapping_add" | "wrapping_sub" | "wrapping_mul", 1) => {
                    // Prefer return-type bounds; fall back to receiver width for nested
                    // uses like `x.wrapping_add(1).is_power_of_two()` (bool return).
                    let (lo, hi) = wrap_bounds_for(&m.receiver)?;
                    let (_bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                    let use_synthetic_2_64 = modulus_i64.is_none();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let b = encode_syn_expr(&m.args[0], param_names, lines, next)?;
                    let raw = *next;
                    *next += 1;
                    let op = match method.as_str() {
                        "wrapping_add" => "add",
                        "wrapping_sub" => "sub",
                        "wrapping_mul" => "mul",
                        _ => return None,
                    };
                    lines.push(format!("${raw} = arith {op} ${a} ${b} : Int"));
                    let mslot = if use_synthetic_2_64 {
                        // 2^32 fits in i64; product is 2^64 in SMT Int
                        let half = *next;
                        *next += 1;
                        lines.push(format!("${half} = const 4294967296 : Int"));
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = arith mul ${half} ${half} : Int"));
                        mslot
                    } else {
                        let modulus = modulus_i64?;
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = const {modulus} : Int"));
                        mslot
                    };
                    // ((raw mod m) + m) mod m → [0, m) for any raw (truncating or Euclidean)
                    let t1 = *next;
                    *next += 1;
                    lines.push(format!("${t1} = arith mod ${raw} ${mslot} : Int"));
                    let t2 = *next;
                    *next += 1;
                    lines.push(format!("${t2} = arith add ${t1} ${mslot} : Int"));
                    let u = *next;
                    *next += 1;
                    lines.push(format!("${u} = arith mod ${t2} ${mslot} : Int"));
                    if !signed {
                        return Some(u);
                    }
                    // Reinterpret u ∈ [0, 2^w) as signed: u - 2^w * (u > hi)
                    let his = *next;
                    *next += 1;
                    lines.push(format!("${his} = const {hi} : Int"));
                    let gt = *next;
                    *next += 1;
                    lines.push(format!("${gt} = cmp gt ${u} ${his} : Bool"));
                    let adj = *next;
                    *next += 1;
                    lines.push(format!("${adj} = arith mul ${gt} ${mslot} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith sub ${u} ${adj} : Int"));
                    Some(slot)
                }
                // wrapping_neg: (0 - x) mod 2^w; signed reinterprets into [lo, hi].
                // Nested signed works in single-block IR (no top-level expand required).
                ("wrapping_neg", 0) => {
                    let (lo, hi) = wrap_bounds_for(&m.receiver)?;
                    let (_bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                    let use_synthetic_2_64 = modulus_i64.is_none();
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let zero = *next;
                    *next += 1;
                    lines.push(format!("${zero} = const 0 : Int"));
                    let raw = *next;
                    *next += 1;
                    lines.push(format!("${raw} = arith sub ${zero} ${a} : Int"));
                    let mslot = if use_synthetic_2_64 {
                        let half = *next;
                        *next += 1;
                        lines.push(format!("${half} = const 4294967296 : Int"));
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = arith mul ${half} ${half} : Int"));
                        mslot
                    } else {
                        let modulus = modulus_i64?;
                        let mslot = *next;
                        *next += 1;
                        lines.push(format!("${mslot} = const {modulus} : Int"));
                        mslot
                    };
                    // rem_euclid into [0, m)
                    let t1 = *next;
                    *next += 1;
                    lines.push(format!("${t1} = arith mod ${raw} ${mslot} : Int"));
                    let t2 = *next;
                    *next += 1;
                    lines.push(format!("${t2} = arith add ${t1} ${mslot} : Int"));
                    let u = *next;
                    *next += 1;
                    lines.push(format!("${u} = arith mod ${t2} ${mslot} : Int"));
                    if !signed {
                        return Some(u);
                    }
                    let his = *next;
                    *next += 1;
                    lines.push(format!("${his} = const {hi} : Int"));
                    let gt = *next;
                    *next += 1;
                    lines.push(format!("${gt} = cmp gt ${u} ${his} : Bool"));
                    let adj = *next;
                    *next += 1;
                    lines.push(format!("${adj} = arith mul ${gt} ${mslot} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = arith sub ${u} ${adj} : Int"));
                    Some(slot)
                }
                _ => None,
            }
        }
        // `x as T` identity only for lossless targets (i64/isize/bool).
        // Narrowing (`i64 as i32`) stays unencoded — both map to Int in Assura
        // and would silently drop high bits.
        syn::Expr::Cast(c) => {
            let ty_tokens = c.ty.to_token_stream().to_string().replace(' ', "");
            if !matches!(ty_tokens.as_str(), "i64" | "isize" | "bool") {
                return None;
            }
            encode_syn_expr(&c.expr, param_names, lines, next)
        }
        syn::Expr::Call(c) => {
            let syn::Expr::Path(path) = c.func.as_ref() else {
                return None;
            };
            // Free `abs`/`min`/`max` or associated `i64::max` / `i32::min`.
            let name = path.path.segments.last()?.ident.to_string();
            if path.path.segments.len() > 2 {
                return None;
            }
            match (name.as_str(), c.args.len()) {
                ("abs", 1) => {
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call abs (${a}) : Int"));
                    Some(slot)
                }
                // i64::saturating_abs(x) associated form
                ("saturating_abs", 1) if path.path.segments.len() == 2 => {
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let ab = *next;
                    *next += 1;
                    lines.push(format!("${ab} = call abs (${a}) : Int"));
                    let (_, hi_v) = SAT_BOUNDS.get()?;
                    let hi = *next;
                    *next += 1;
                    lines.push(format!("${hi} = const {hi_v} : Int"));
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call min (${ab}, ${hi}) : Int"));
                    Some(slot)
                }
                ("min" | "max", 2) => {
                    if expr_same_simple_path(&c.args[0], &c.args[1]) {
                        return encode_syn_expr(&c.args[0], param_names, lines, next);
                    }
                    let a = encode_syn_expr(&c.args[0], param_names, lines, next)?;
                    let b = encode_syn_expr(&c.args[1], param_names, lines, next)?;
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = call {name} (${a}, ${b}) : Int"));
                    Some(slot)
                }
                ("from", 1) if path.path.segments.len() == 2 => {
                    // i64::from(x) identity for integer-like
                    encode_syn_expr(&c.args[0], param_names, lines, next)
                }
                ("default", 0) => {
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const 0 : Int"));
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
mod tests;
