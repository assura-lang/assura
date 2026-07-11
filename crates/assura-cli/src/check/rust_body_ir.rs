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
//! synthetic 2^64 modulus; 2^63 factor is 2^32*2^31). Variable is_power_of_two
//! for fixed-width ints via pot enum (≤63 exponents; identity peels keep bounds).
//! Literal `/0`, `%0`, `is_multiple_of(0)` BNM. `signum` nestable clamp (#1032).
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

/// Full-width unsigned 64-bit (`u64`/`usize`): max does not fit in i64.
/// Sentinel: `(0, -1)` → modulus `2^64` via synthetic `(2^32)*(2^32)`.
fn is_u64_width_bounds(lo: i64, hi: i64) -> bool {
    lo == 0 && hi == -1
}

/// Integer range for a Rust primitive type name, or `None` if unsupported.
fn rust_int_bounds(ty: &str) -> Option<(i64, i64)> {
    match ty {
        "i8" => Some((i8::MIN as i64, i8::MAX as i64)),
        "i16" => Some((i16::MIN as i64, i16::MAX as i64)),
        "i32" => Some((i32::MIN as i64, i32::MAX as i64)),
        "i64" | "isize" => Some((i64::MIN, i64::MAX)),
        "u8" => Some((0, u8::MAX as i64)),
        "u16" => Some((0, u16::MAX as i64)),
        "u32" => Some((0, u32::MAX as i64)),
        // u64 max exceeds i64; use sentinel for synthetic 2^64 (#1160)
        "u64" | "usize" => Some((0, -1)),
        _ => None,
    }
}

/// Resolve wrap/shift width: `(bits, Some(modulus)|None for synthetic 2^64, signed)`.
fn wrap_width(lo: i64, hi: i64) -> Option<(u32, Option<i64>, bool)> {
    if is_u64_width_bounds(lo, hi) {
        return Some((64, None, false));
    }
    let signed = lo != 0;
    if signed && lo == i64::MIN && hi == i64::MAX {
        return Some((64, None, true));
    }
    let modulus = if signed {
        hi.checked_sub(lo).and_then(|d| d.checked_add(1))?
    } else {
        hi.checked_add(1)?
    };
    if modulus <= 0 || !(modulus as u64).is_power_of_two() {
        return None;
    }
    Some(((modulus as u64).trailing_zeros(), Some(modulus), signed))
}

/// Emit IR for `2^e` as an Int slot (`e` in 0..=63).
/// `2^63` does not fit in a positive i64 const; build it as `2^32 * 2^31`.
fn emit_pow2_factor(e: u32, lines: &mut Vec<String>, next: &mut usize) -> Option<usize> {
    if e > 63 {
        return None;
    }
    if e < 63 {
        let factor = 1i64 << e;
        let f = *next;
        *next += 1;
        lines.push(format!("${f} = const {factor} : Int"));
        return Some(f);
    }
    let half = *next;
    *next += 1;
    lines.push(format!("${half} = const 4294967296 : Int"));
    let q = *next;
    *next += 1;
    lines.push(format!("${q} = const 2147483648 : Int"));
    let f = *next;
    *next += 1;
    lines.push(format!("${f} = arith mul ${half} ${q} : Int"));
    Some(f)
}

/// Positive power-of-two exponents for a param with known integer bounds.
/// Unsigned: 0..bits (1..2^(bits-1)). Signed: 0..(bits-1) (1..2^(bits-2)).
fn pot_exponents(lo: i64, hi: i64) -> Option<u32> {
    if is_u64_width_bounds(lo, hi) {
        // pot case-sum over 64 exponents is huge; leave is_power_of_two BNM for u64
        return None;
    }
    if lo == 0 {
        // unsigned: hi+1 must be power of two
        let m = (hi as u64).checked_add(1)?;
        if !m.is_power_of_two() {
            return None;
        }
        Some(m.trailing_zeros())
    } else if lo < 0 && hi > 0 {
        // signed two's complement: hi == 2^(n-1)-1
        let half = (hi as u64).checked_add(1)?;
        if !half.is_power_of_two() {
            return None;
        }
        // positive pots: 1 << e for e in 0..(bits-1)
        Some(half.trailing_zeros())
    } else {
        None
    }
}

/// Bounds for a simple path param (`x`), if registered.
/// Also peels paren/group/ref/deref and identity methods (`clone`, `into`, …)
/// so `x.clone().is_power_of_two()` shares the path-param pot enum path.
fn path_param_bounds(expr: &syn::Expr) -> Option<(i64, i64)> {
    let name = match expr {
        syn::Expr::Paren(p) => return path_param_bounds(&p.expr),
        syn::Expr::Group(g) => return path_param_bounds(&g.expr),
        syn::Expr::Reference(r) => return path_param_bounds(&r.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Deref(_)) => {
            return path_param_bounds(&u.expr);
        }
        syn::Expr::MethodCall(m)
            if m.args.is_empty() && is_identity_peel_method(&m.method.to_string()) =>
        {
            return path_param_bounds(&m.receiver);
        }
        syn::Expr::Path(p) if p.path.segments.len() == 1 => p.path.segments[0].ident.to_string(),
        _ => return None,
    };
    PARAM_BOUNDS.with(|c| c.borrow().get(&name).copied())
}

/// Literal integer with known bit width from a typed suffix (`8u32` → (8, 32)).
/// Bare unsuffixed lits return `None` (leading_zeros needs a width).
fn lit_int_i64_bits(expr: &syn::Expr) -> Option<(i64, u32)> {
    match expr {
        syn::Expr::Paren(p) => lit_int_i64_bits(&p.expr),
        syn::Expr::Group(g) => lit_int_i64_bits(&g.expr),
        syn::Expr::Unary(u) if matches!(u.op, syn::UnOp::Neg(_)) => {
            let (v, bits) = lit_int_i64_bits(&u.expr)?;
            Some((v.checked_neg()?, bits))
        }
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(n),
            ..
        }) => {
            let bits = match n.suffix() {
                "u8" | "i8" => 8,
                "u16" | "i16" => 16,
                "u32" | "i32" => 32,
                "u64" | "i64" | "usize" | "isize" => 64,
                _ => return None,
            };
            let v: i64 = n.base10_parse().ok()?;
            Some((v, bits))
        }
        _ => None,
    }
}

/// Popcount bit-sum for an unsigned value already in slot `a` with width `bits`.
/// Emits `sum_i (a / 2^i) mod 2` into IR; returns the accumulator slot.
fn encode_bit_sum_count_ones(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let mut acc: Option<usize> = None;
    for i in 0..bits {
        let factor = 1i64 << i;
        let f = *next;
        *next += 1;
        lines.push(format!("${f} = const {factor} : Int"));
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        acc = Some(match acc {
            None => bit,
            Some(prev) => {
                let sum = *next;
                *next += 1;
                lines.push(format!("${sum} = arith add ${prev} ${bit} : Int"));
                sum
            }
        });
    }
    acc
}

/// trailing_zeros for unsigned `a` with width `bits`.
/// `sum_i i * bit_i * prod_{j<i}(1-bit_j) + bits * prod_all(1-bit)`.
fn encode_unsigned_trailing_zeros(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    // prod starts at 1 (all lower bits zero so far)
    let mut prod = one;
    let mut acc = zero;
    for i in 0..bits {
        let factor = 1i64 << i;
        let f = *next;
        *next += 1;
        lines.push(format!("${f} = const {factor} : Int"));
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        // term = i * bit * prod
        let i_c = *next;
        *next += 1;
        lines.push(format!("${i_c} = const {i} : Int"));
        let ib = *next;
        *next += 1;
        lines.push(format!("${ib} = arith mul ${i_c} ${bit} : Int"));
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${ib} ${prod} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
        // prod *= (1 - bit)
        let one_m_bit = *next;
        *next += 1;
        lines.push(format!("${one_m_bit} = arith sub ${one} ${bit} : Int"));
        let new_prod = *next;
        *next += 1;
        lines.push(format!(
            "${new_prod} = arith mul ${prod} ${one_m_bit} : Int"
        ));
        prod = new_prod;
    }
    // + bits when all zero (prod still 1)
    let bits_c = *next;
    *next += 1;
    lines.push(format!("${bits_c} = const {bits} : Int"));
    let all_zero = *next;
    *next += 1;
    lines.push(format!("${all_zero} = arith mul ${bits_c} ${prod} : Int"));
    let slot = *next;
    *next += 1;
    lines.push(format!("${slot} = arith add ${acc} ${all_zero} : Int"));
    Some(slot)
}

/// leading_zeros for unsigned `a` with width `bits`.
/// Scan high→low: count consecutive zero bits while still in prefix.
fn encode_unsigned_leading_zeros(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let one = *next;
    *next += 1;
    lines.push(format!("${one} = const 1 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut still = one;
    let mut acc = zero;
    for i in (0..bits).rev() {
        let factor = 1i64 << i;
        let f = *next;
        *next += 1;
        lines.push(format!("${f} = const {factor} : Int"));
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        let zbit = *next;
        *next += 1;
        lines.push(format!("${zbit} = arith sub ${one} ${bit} : Int"));
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${still} ${zbit} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
        let new_still = *next;
        *next += 1;
        lines.push(format!("${new_still} = arith mul ${still} ${zbit} : Int"));
        still = new_still;
    }
    Some(acc)
}

/// reverse_bits for unsigned `a` with width `bits`: sum_i bit_i * 2^(bits-1-i).
fn encode_unsigned_reverse_bits(
    a: usize,
    bits: u32,
    lines: &mut Vec<String>,
    next: &mut usize,
) -> Option<usize> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let two = *next;
    *next += 1;
    lines.push(format!("${two} = const 2 : Int"));
    let zero = *next;
    *next += 1;
    lines.push(format!("${zero} = const 0 : Int"));
    let mut acc = zero;
    for i in 0..bits {
        let factor = 1i64 << i;
        let rev_factor = 1i64 << (bits - 1 - i);
        let f = *next;
        *next += 1;
        lines.push(format!("${f} = const {factor} : Int"));
        let shifted = *next;
        *next += 1;
        lines.push(format!("${shifted} = arith div ${a} ${f} : Int"));
        let bit = *next;
        *next += 1;
        lines.push(format!("${bit} = arith mod ${shifted} ${two} : Int"));
        let rf = *next;
        *next += 1;
        lines.push(format!("${rf} = const {rev_factor} : Int"));
        let term = *next;
        *next += 1;
        lines.push(format!("${term} = arith mul ${bit} ${rf} : Int"));
        let new_acc = *next;
        *next += 1;
        lines.push(format!("${new_acc} = arith add ${acc} ${term} : Int"));
        acc = new_acc;
    }
    Some(acc)
}

/// True for literal `1` or `-1` (after paren/group peels).
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
                // Const lit peep, or variable path with param bounds via pot enum
                // (partial #1034; avoids needing bitwise AND in IR).
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
                    let (lo, hi) = path_param_bounds(&m.receiver)?;
                    let n_exp = pot_exponents(lo, hi)?;
                    // Cap IR size: i64 uses 63 OR-chain steps (~300 lines); fine for SMT.
                    if n_exp > 63 {
                        return None;
                    }
                    let a = encode_syn_expr(&m.receiver, param_names, lines, next)?;
                    let mut acc: Option<usize> = None;
                    for e in 0..n_exp {
                        let pot = 1i64 << e;
                        let c = *next;
                        *next += 1;
                        lines.push(format!("${c} = const {pot} : Int"));
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
                // Positive lit only (ilog2(0) panics in Rust).
                ("ilog2", 0) => {
                    let v = lit_int_i64(&m.receiver)?;
                    if v <= 0 {
                        return None;
                    }
                    let log = (v as u64).ilog2();
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {log} : Int"));
                    Some(slot)
                }
                // Positive lit only.
                ("ilog10", 0) => {
                    let v = lit_int_i64(&m.receiver)?;
                    if v <= 0 {
                        return None;
                    }
                    let log = (v as u64).ilog10();
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {log} : Int"));
                    Some(slot)
                }
                // Non-neg lit; 0.next_power_of_two() == 1. Oversized stays BNM.
                ("next_power_of_two", 0) => {
                    let v = lit_int_i64(&m.receiver)?;
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
                    Some(slot)
                }
                // Typed wrapping_next_power_of_two (overflow wraps to 0).
                // Manual: stable API; unstable wrapping_next_power_of_two not used.
                ("wrapping_next_power_of_two", 0) => {
                    let (v, bits) = lit_int_i64_bits(&m.receiver)?;
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
                    Some(slot)
                }
                // Non-neg lit integer square root.
                ("isqrt", 0) => {
                    let v = lit_int_i64(&m.receiver)?;
                    if v < 0 {
                        return None;
                    }
                    let root = (v as u64).isqrt();
                    let slot = *next;
                    *next += 1;
                    lines.push(format!("${slot} = const {root} : Int"));
                    Some(slot)
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
                // wrapping_* peeps only for identity constants (full wrap needs #1010 BV).
                // wrapping_add(x, 0) / wrapping_sub(x, 0) ≡ x; wrapping_mul(x, 1) ≡ x;
                // wrapping_mul(x, 0) ≡ 0; wrapping_sub(x, x) ≡ 0. Non-constant stay BNM.
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
                    let (lo, hi) = SAT_BOUNDS.get()?;
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
                // rotate_left/right by const k (#1010 partial).
                // Unsigned bits: rotl ≡ (x*2^k + x/2^(bits-k)) mod 2^w.
                // Signed: map to unsigned bit pattern, rotate, reinterpret.
                ("rotate_left" | "rotate_right", 1) => {
                    let (lo, hi) = SAT_BOUNDS.get()?;
                    let (bits, modulus_i64, signed) = wrap_width(lo, hi)?;
                    let use_synthetic_2_64 = modulus_i64.is_none();
                    let k = lit_int_i64(&m.args[0])?;
                    if k < 0 {
                        return None;
                    }
                    let k_eff = (k as u64) % (bits as u64);
                    if k_eff == 0 {
                        return encode_syn_expr(&m.receiver, param_names, lines, next);
                    }
                    // rotate_right(k) ≡ rotate_left(bits-k)
                    let k_left = if method == "rotate_left" {
                        k_eff
                    } else {
                        bits as u64 - k_eff
                    };
                    if k_left == 0 || k_left >= 63 || (bits as u64 - k_left) >= 63 {
                        return None;
                    }
                    let hi_f = 1i64 << k_left;
                    let lo_f = 1i64 << (bits as u64 - k_left);
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
                    let hf = *next;
                    *next += 1;
                    lines.push(format!("${hf} = const {hi_f} : Int"));
                    let lf = *next;
                    *next += 1;
                    lines.push(format!("${lf} = const {lo_f} : Int"));
                    let hi_part = *next;
                    *next += 1;
                    lines.push(format!("${hi_part} = arith mul ${u_in} ${hf} : Int"));
                    let lo_part = *next;
                    *next += 1;
                    lines.push(format!("${lo_part} = arith div ${u_in} ${lf} : Int"));
                    let raw = *next;
                    *next += 1;
                    lines.push(format!("${raw} = arith add ${hi_part} ${lo_part} : Int"));
                    let t3 = *next;
                    *next += 1;
                    lines.push(format!("${t3} = arith mod ${raw} ${mslot} : Int"));
                    let t4 = *next;
                    *next += 1;
                    lines.push(format!("${t4} = arith add ${t3} ${mslot} : Int"));
                    let u = *next;
                    *next += 1;
                    lines.push(format!("${u} = arith mod ${t4} ${mslot} : Int"));
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
                // wrapping_* via mod 2^w (#1010).
                // Unsigned (lo==0): result in [0, 2^w).
                // Signed: double-mod to [0, 2^w) then reinterpret as two's complement.
                // Double-mod ((raw mod m) + m) mod m works for large |raw| (signed mul)
                // without a 2^(2w-1) offset that may not fit in i64.
                // i64 modulus is 2^64: emit as (2^32)*(2^32) (const 2^64 is not i64).
                ("wrapping_add" | "wrapping_sub" | "wrapping_mul", 1) => {
                    let (lo, hi) = SAT_BOUNDS.get()?;
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
                    let (lo, hi) = SAT_BOUNDS.get()?;
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
mod tests {
    use super::*;
    use assura_rust_analyzer::ParamInfo;

    fn px() -> Vec<ParamInfo> {
        vec![ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        }]
    }

    fn pu8() -> Vec<ParamInfo> {
        vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }]
    }

    fn pu32() -> Vec<ParamInfo> {
        vec![ParamInfo {
            name: "x".into(),
            ty: "u32".into(),
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
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let same = try_ir_from_rust_body("B", &pxy, Some("i64"), "x.clamp(y, y)").expect("same");
        assert!(same.contains("$result = load $1"), "{same}");
    }

    #[test]
    fn abs_min_max_method_and_call() {
        let abs = try_ir_from_rust_body("A", &px(), Some("i64"), "x . abs ()").expect("abs");
        assert!(abs.contains("call abs"), "{abs}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&abs, "A").expect("parse abs");
        let mn = try_ir_from_rust_body("M", &px(), Some("i64"), "x.min(x)").expect("min self");
        assert!(mn.contains("$result = load $0"), "{mn}");
        let mx = try_ir_from_rust_body("X", &px(), Some("i64"), "x.max(x)").expect("max self");
        assert!(mx.contains("$result = load $0"), "{mx}");
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

    fn pab() -> Vec<ParamInfo> {
        vec![
            ParamInfo {
                name: "a".into(),
                ty: "bool".into(),
            },
            ParamInfo {
                name: "b".into(),
                ty: "bool".into(),
            },
        ]
    }

    #[test]
    fn logical_and_or_body_ir() {
        let and = try_ir_from_rust_body("And", &pab(), Some("bool"), "a && b").expect("and");
        assert!(and.contains("arith mul"), "{and}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&and, "And").expect("parse and");
        let or = try_ir_from_rust_body("Or", &pab(), Some("bool"), "a || b").expect("or");
        assert!(or.contains("arith add") && or.contains("cmp ne"), "{or}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&or, "Or").expect("parse or");
    }

    #[test]
    fn into_and_as_identity_body_ir() {
        let into = try_ir_from_rust_body("I", &px(), Some("i64"), "x.into()").expect("into");
        assert!(into.contains("$result = load $0"), "{into}");
        let cast = try_ir_from_rust_body("C", &px(), Some("i64"), "x as i64").expect("as");
        assert!(cast.contains("$result = load $0"), "{cast}");
        // Narrowing must not pretend to be identity on unbounded Int.
        assert!(try_ir_from_rust_body("N", &px(), Some("i32"), "x as i32").is_none());
        assura_smt::LoadedVerifyExtras::from_ir_text(&into, "I").expect("parse into");
        assura_smt::LoadedVerifyExtras::from_ir_text(&cast, "C").expect("parse cast");
    }

    #[test]
    fn midpoint_encodes() {
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let ir = try_ir_from_rust_body("M", &pxy, Some("i64"), "x.midpoint(y)").expect("mid");
        assert!(ir.contains("arith add") && ir.contains("arith div"), "{ir}");
        assert!(ir.contains("const 2"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "M").expect("parse");
        let same = try_ir_from_rust_body("S", &px(), Some("i64"), "x.midpoint(x)").expect("same");
        assert!(same.contains("load $0"), "{same}");
    }

    #[test]
    fn signed_next_multiple_of_encodes() {
        let ir =
            try_ir_from_rust_body("N", &px(), Some("i64"), "x.next_multiple_of(4)").expect("nmo");
        assert!(ir.contains("arith mod") && ir.contains("cmp eq"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "N").expect("parse");
    }

    #[test]
    fn signed_rem_euclid_encodes() {
        let ir = try_ir_from_rust_body("R", &px(), Some("i64"), "x.rem_euclid(3)").expect("re");
        assert!(ir.contains("arith mod") && ir.contains("arith add"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "R").expect("parse");
        let de = try_ir_from_rust_body("D", &px(), Some("i64"), "x.div_euclid(3)").expect("de");
        assert!(de.contains("arith div"), "{de}");
    }

    #[test]
    fn div_ceil_const_divisor_encodes() {
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let ir = try_ir_from_rust_body("D", &pu8, Some("u8"), "x.div_ceil(3)").expect("div_ceil");
        assert!(ir.contains("arith div") && ir.contains("const 3"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "D").expect("parse");
        // signed i64 path stays BNM (may be negative)
        assert!(try_ir_from_rust_body("S", &px(), Some("i64"), "x.div_ceil(3)").is_none());
        // const non-neg lit
        let c = try_ir_from_rust_body("C", &px(), Some("u32"), "10u32.div_ceil(3)").expect("const");
        assert!(c.contains("const 4") || c.contains("arith div"), "{c}");
        let re =
            try_ir_from_rust_body("R", &pu8, Some("u8"), "x.rem_euclid(3)").expect("rem_euclid");
        assert!(re.contains("arith mod") && re.contains("const 3"), "{re}");
        let de =
            try_ir_from_rust_body("De", &pu8, Some("u8"), "x.div_euclid(3)").expect("div_euclid");
        assert!(de.contains("arith div") && de.contains("const 3"), "{de}");
        let nmo =
            try_ir_from_rust_body("N", &pu8, Some("u8"), "x.next_multiple_of(4)").expect("nmo");
        // rem_euclid formula: rem = ((a mod m)+m) mod m; a - rem + m*[rem!=0]
        assert!(
            nmo.contains("arith mod") && nmo.contains("cmp eq") && nmo.contains("arith mul"),
            "{nmo}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&nmo, "N").expect("parse");
    }

    #[test]
    fn const_bitwise_not_typed() {
        let ir = try_ir_from_rust_body("N", &px(), Some("u8"), "!5u8").expect("not");
        assert!(ir.contains("const 250 : Int"), "{ir}");
    }

    #[test]
    fn const_bitops_fold() {
        let ir = try_ir_from_rust_body("A", &px(), Some("u32"), "12u32 & 10u32").expect("and");
        assert!(ir.contains("const 8 : Int"), "{ir}"); // 0b1100 & 0b1010 = 0b1000
        let or = try_ir_from_rust_body("O", &px(), Some("u32"), "12u32 | 3u32").expect("or");
        assert!(or.contains("const 15 : Int"), "{or}");
        let sh = try_ir_from_rust_body("S", &px(), Some("u32"), "3u32 << 2").expect("shl");
        assert!(sh.contains("const 12 : Int"), "{sh}");
        // variable bitops stay BNM
        assert!(try_ir_from_rust_body("V", &px(), Some("u32"), "x & 1").is_none());
    }

    #[test]
    fn is_multiple_of_body_ir() {
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let ir =
            try_ir_from_rust_body("M", &pxy, Some("bool"), "x.is_multiple_of(y)").expect("imo");
        assert!(ir.contains("arith mod") && ir.contains("cmp eq"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "M").expect("parse");
        // Literal 0 panics in Rust; must not encode as mod-by-zero.
        assert!(try_ir_from_rust_body("Z", &px(), Some("bool"), "x.is_multiple_of(0)").is_none());
        let ok =
            try_ir_from_rust_body("T", &px(), Some("bool"), "x.is_multiple_of(2)").expect("by2");
        assert!(ok.contains("const 2") || ok.contains("arith mod"), "{ok}");
        let by1 =
            try_ir_from_rust_body("O", &px(), Some("bool"), "x.is_multiple_of(1)").expect("by1");
        assert!(by1.contains("const 1 : Bool"), "{by1}");
        let by_neg1 =
            try_ir_from_rust_body("N", &px(), Some("bool"), "x.is_multiple_of(-1)").expect("byn1");
        assert!(by_neg1.contains("const 1 : Bool"), "{by_neg1}");
    }

    #[test]
    fn div_rem_by_literal_zero_stays_unencoded() {
        assert!(try_ir_from_rust_body("D", &px(), Some("i64"), "x / 0").is_none());
        assert!(try_ir_from_rust_body("R", &px(), Some("i64"), "x % 0").is_none());
        assert!(try_ir_from_rust_body("Dp", &px(), Some("i64"), "x / (0)").is_none());
        assert!(
            try_ir_from_rust_body("Mp", &px(), Some("bool"), "x.is_multiple_of((0))").is_none()
        );
        let ok = try_ir_from_rust_body("D2", &px(), Some("i64"), "x / 2").expect("div2");
        assert!(ok.contains("arith div"), "{ok}");
    }

    #[test]
    fn abs_diff_and_ref_deref_body_ir() {
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let ir = try_ir_from_rust_body("D", &pxy, Some("i64"), "x.abs_diff(y)").expect("diff");
        assert!(ir.contains("arith sub") && ir.contains("call abs"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "D").expect("parse");
        let same = try_ir_from_rust_body("S", &px(), Some("i64"), "x.abs_diff(x)").expect("same");
        assert!(same.contains("const 0"), "{same}");
        let r = try_ir_from_rust_body("R", &px(), Some("i64"), "&x").expect("ref");
        assert!(r.contains("$result = load $0"), "{r}");
        let d = try_ir_from_rust_body("De", &px(), Some("i64"), "*&x").expect("deref");
        assert!(d.contains("$result = load $0"), "{d}");
    }

    #[test]
    fn saturating_neg_body_ir() {
        let ir = try_ir_from_rust_body("N", &px(), Some("i64"), "x.saturating_neg()").expect("neg");
        assert!(ir.contains("arith sub") && ir.contains("call max"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "N").expect("parse");
    }

    #[test]
    fn saturating_abs_body_ir() {
        let ir =
            try_ir_from_rust_body("A", &px(), Some("i64"), "x.saturating_abs()").expect("sat_abs");
        assert!(ir.contains("call abs") && ir.contains("call min"), "{ir}");
        assert!(ir.contains(&format!("const {}", i64::MAX)), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "A").expect("parse");
        // Needs return-type bounds (same as other saturating_*).
        assert!(try_ir_from_rust_body("B", &px(), None, "x.saturating_abs()").is_none());
        let assoc = try_ir_from_rust_body("C", &px(), Some("i64"), "i64::saturating_abs(x)")
            .expect("assoc");
        assert!(
            assoc.contains("call abs") && assoc.contains("call min"),
            "{assoc}"
        );
    }

    #[test]
    fn saturating_add_body_ir() {
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let ir = try_ir_from_rust_body("S", &pxy, Some("i64"), "x.saturating_add(y)").expect("sat");
        assert!(ir.contains("arith add") && ir.contains("call max"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
    }

    #[test]
    fn abs_diff_then_is_positive_body_ir() {
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let ir = try_ir_from_rust_body("A", &pxy, Some("bool"), "x.abs_diff(y).is_positive()")
            .expect("chain");
        assert!(ir.contains("call abs") && ir.contains("cmp gt"), "{ir}");
        let never =
            try_ir_from_rust_body("N", &px(), Some("bool"), "x.abs().is_negative()").expect("neg");
        assert!(never.contains("const 0 : Bool"), "{never}");
        let sat =
            try_ir_from_rust_body("S", &px(), Some("bool"), "x.saturating_abs().is_negative()")
                .expect("satneg");
        assert!(sat.contains("const 0 : Bool"), "{sat}");
        let z = try_ir_from_rust_body("Z", &px(), Some("bool"), "x.abs_diff(x).is_zero()")
            .expect("ad0");
        assert!(z.contains("const 1 : Bool"), "{z}");
        let p = try_ir_from_rust_body("P", &px(), Some("bool"), "x.abs_diff(x).is_positive()")
            .expect("adp");
        assert!(p.contains("const 0 : Bool"), "{p}");
    }

    #[test]
    fn copied_cloned_identity_body_ir() {
        let ir = try_ir_from_rust_body("C", &px(), Some("i64"), "x.copied()").expect("copied");
        assert!(ir.contains("$result = load $0"), "{ir}");
        let ir2 = try_ir_from_rust_body("Cl", &px(), Some("i64"), "x.cloned()").expect("cloned");
        assert!(ir2.contains("$result = load $0"), "{ir2}");
    }

    #[test]
    fn partial_ord_methods_body_ir() {
        let ir = try_ir_from_rust_body("G", &px(), Some("bool"), "x.gt(&0)").expect("gt");
        assert!(ir.contains("cmp gt"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "G").expect("parse");
        let ir2 = try_ir_from_rust_body("E", &px(), Some("bool"), "x.eq(&0)").expect("eq");
        assert!(ir2.contains("cmp eq"), "{ir2}");
    }

    #[test]
    fn default_const_body_ir() {
        let ir = try_ir_from_rust_body("D", &px(), Some("i64"), "i64::default()").expect("default");
        assert!(ir.contains("const 0"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "D").expect("parse");
    }

    #[test]
    fn associated_min_max_body_ir() {
        let ir = try_ir_from_rust_body("M", &px(), Some("i64"), "i64::MAX").expect("max");
        assert!(ir.contains(&i64::MAX.to_string()), "{ir}");
        let ir2 = try_ir_from_rust_body("N", &px(), Some("i64"), "i64::MIN").expect("min");
        assert!(ir2.contains(&i64::MIN.to_string()), "{ir2}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "M").expect("parse");
        let free = try_ir_from_rust_body("F", &px(), Some("i64"), "min(x, x)").expect("free min");
        assert!(free.contains("$result = load $0"), "{free}");
    }

    #[test]
    fn pow_const_body_ir() {
        let ir = try_ir_from_rust_body("P", &px(), Some("i64"), "x.pow(2)").expect("pow2");
        assert!(ir.contains("arith mul"), "{ir}");
        let ir0 = try_ir_from_rust_body("P0", &px(), Some("i64"), "x.pow(0)").expect("pow0");
        assert!(ir0.contains("const 1"), "{ir0}");
        assert!(try_ir_from_rust_body("Pb", &px(), Some("i64"), "x.pow(5)").is_none());
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "P").expect("parse");
    }

    #[test]
    fn as_ref_not_body_ir() {
        let ir = try_ir_from_rust_body("R", &px(), Some("i64"), "x.as_ref()").expect("as_ref");
        assert!(ir.contains("$result = load $0"), "{ir}");
        let pab = vec![ParamInfo {
            name: "a".into(),
            ty: "bool".into(),
        }];
        let n = try_ir_from_rust_body("N", &pab, Some("bool"), "a.not()").expect("not");
        assert!(n.contains("cmp eq"), "{n}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&n, "N").expect("parse");
    }

    #[test]
    fn multi_let_ref_and_cast_fold() {
        let src = r#"
fn f(x: i64) -> i64 { let y = &x; *y }
"#;
        let body = extract_body_return(src, "f").expect("extract");
        let ir = try_ir_from_rust_body("F", &px(), Some("i64"), &body).expect("ir");
        assert!(ir.contains("$result = load $0"), "{ir}");
    }

    #[test]
    fn true_false_path_body_ir() {
        let pab = vec![ParamInfo {
            name: "a".into(),
            ty: "bool".into(),
        }];
        let ir = try_ir_from_rust_body("T", &pab, Some("bool"), "true").expect("true");
        assert!(ir.contains("const 1"), "{ir}");
        let ir2 = try_ir_from_rust_body("F", &pab, Some("bool"), "a && false").expect("andf");
        assert!(ir2.contains("const 0"), "{ir2}");
    }

    #[test]
    fn narrowing_cast_returns_none() {
        assert!(try_ir_from_rust_body("N", &px(), Some("i32"), "x as i32").is_none());
    }

    #[test]
    fn nested_method_chain_body_ir() {
        let ir = try_ir_from_rust_body("C", &px(), Some("bool"), "x.abs().is_positive()")
            .expect("chain");
        assert!(ir.contains("call abs") && ir.contains("cmp gt"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "C").expect("parse");
    }

    #[test]
    fn borrow_identity_body_ir() {
        let ir = try_ir_from_rust_body("B", &px(), Some("i64"), "x.borrow()").expect("borrow");
        assert!(ir.contains("$result = load $0"), "{ir}");
    }

    #[test]
    fn deref_identity_body_ir() {
        let ir = try_ir_from_rust_body("D", &px(), Some("i64"), "x.deref()").expect("deref");
        assert!(ir.contains("$result = load $0"), "{ir}");
    }

    #[test]
    fn is_identity_peel_method_list() {
        for m in [
            "clone",
            "to_owned",
            "into",
            "copied",
            "cloned",
            "as_ref",
            "as_mut",
            "borrow",
            "borrow_mut",
            "deref",
            "deref_mut",
        ] {
            assert!(super::is_identity_peel_method(m), "{m}");
        }
        assert!(!super::is_identity_peel_method("abs"));
        assert!(!super::is_identity_peel_method("signum"));
    }

    #[test]
    fn i64_wrapping_encodes_via_synthetic_modulus() {
        // i64 modulus 2^64 = (2^32)*(2^32) in IR (const 2^64 not representable as i64)
        let add =
            try_ir_from_rust_body("W", &px(), Some("i64"), "x.wrapping_add(1)").expect("i64 add");
        assert!(
            add.contains("const 4294967296")
                && add.contains("arith mul")
                && add.contains("arith mod"),
            "{add}"
        );
        assert!(add.contains("cmp gt"), "signed reinterpret: {add}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&add, "W").expect("parse add");
        let mul =
            try_ir_from_rust_body("M", &px(), Some("i64"), "x.wrapping_mul(2)").expect("i64 mul");
        assert!(
            mul.contains("arith mul")
                && mul.contains("arith mod")
                && mul.contains("const 4294967296"),
            "{mul}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&mul, "M").expect("parse mul");
        // Nested wrapping_neg encodes via modular (0-x) mod 2^w + reinterpret.
        let nest =
            try_ir_from_rust_body("N", &px(), Some("i64"), "x.wrapping_neg() + 1").expect("nest");
        assert!(
            nest.contains("arith sub") && nest.contains("arith mod") && nest.contains("cmp gt"),
            "{nest}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&nest, "N").expect("parse nest");
    }

    #[test]
    fn signed_i8_wrapping_add_encodes() {
        let pi8 = vec![ParamInfo {
            name: "x".into(),
            ty: "i8".into(),
        }];
        let ir =
            try_ir_from_rust_body("W", &pi8, Some("i8"), "x.wrapping_add(1)").expect("i8 wrap");
        assert!(ir.contains("arith mod") && ir.contains("const 256"), "{ir}");
        assert!(ir.contains("cmp gt"), "signed reinterpret: {ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "W").expect("parse");
        let mul =
            try_ir_from_rust_body("M", &pi8, Some("i8"), "x.wrapping_mul(2)").expect("i8 mul");
        assert!(
            mul.contains("arith mul") && mul.contains("arith mod"),
            "{mul}"
        );
        // i32 signed mul via double-mod (no huge offset)
        let pi32 = vec![ParamInfo {
            name: "x".into(),
            ty: "i32".into(),
        }];
        let m32 =
            try_ir_from_rust_body("M32", &pi32, Some("i32"), "x.wrapping_mul(2)").expect("i32 mul");
        assert!(
            m32.contains("arith mul") && m32.contains("arith mod"),
            "{m32}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&m32, "M32").expect("parse");
    }

    #[test]
    fn unsigned_wrapping_add_encodes_via_mod() {
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let ir =
            try_ir_from_rust_body("W", &pu8, Some("u8"), "x.wrapping_add(1)").expect("u8 wrap");
        assert!(ir.contains("arith add") && ir.contains("arith mod"), "{ir}");
        assert!(ir.contains("const 256"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "W").expect("parse");
        let mul =
            try_ir_from_rust_body("M", &pu8, Some("u8"), "x.wrapping_mul(3)").expect("u8 mul");
        assert!(
            mul.contains("arith mul") && mul.contains("arith mod"),
            "{mul}"
        );
        let neg =
            try_ir_from_rust_body("Ng", &pu8, Some("u8"), "x.wrapping_neg()").expect("u8 neg");
        assert!(
            neg.contains("arith sub") && neg.contains("arith mod"),
            "{neg}"
        );
    }

    #[test]
    fn wrapping_identity_peeps_encode() {
        let a0 = try_ir_from_rust_body("A", &px(), Some("i64"), "x.wrapping_add(0)").expect("+0");
        assert!(a0.contains("$result = load $0"), "{a0}");
        let s0 = try_ir_from_rust_body("S", &px(), Some("i64"), "x.wrapping_sub(0)").expect("-0");
        assert!(s0.contains("$result = load $0"), "{s0}");
        let m1 = try_ir_from_rust_body("M", &px(), Some("i64"), "x.wrapping_mul(1)").expect("*1");
        assert!(m1.contains("$result = load $0"), "{m1}");
        let m0 = try_ir_from_rust_body("Z", &px(), Some("i64"), "x.wrapping_mul(0)").expect("*0");
        assert!(m0.contains("const 0"), "{m0}");
        let sx = try_ir_from_rust_body("Sx", &px(), Some("i64"), "x.wrapping_sub(x)").expect("x-x");
        assert!(sx.contains("const 0"), "{sx}");
    }

    #[test]
    fn top_level_wrapping_neg_encodes() {
        let ir = try_ir_from_rust_body("W", &px(), Some("i64"), "x.wrapping_neg()").expect("wneg");
        assert!(ir.contains("then #") || ir.contains("if $"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "W").expect("parse");
    }

    #[test]
    fn is_power_of_two_const_and_i64_var() {
        // Const lit peeps
        let t = try_ir_from_rust_body("T", &px(), Some("bool"), "8i64.is_power_of_two()")
            .expect("8 pot");
        assert!(t.contains("const 1 : Bool"), "{t}");
        let f = try_ir_from_rust_body("F", &px(), Some("bool"), "3i64.is_power_of_two()")
            .expect("3 not");
        assert!(f.contains("const 0 : Bool"), "{f}");
        let z = try_ir_from_rust_body("Z", &px(), Some("bool"), "0i64.is_power_of_two()")
            .expect("0 not");
        assert!(z.contains("const 0 : Bool"), "{z}");
        // i64 path param: 63-pot enum
        let ir =
            try_ir_from_rust_body("P", &px(), Some("bool"), "x.is_power_of_two()").expect("i64");
        assert!(
            ir.contains("cmp eq") && ir.contains("const 1 : Int"),
            "{ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "P").expect("parse");
        // Identity peels keep path-param bounds (#1034 nested receivers)
        let clone = try_ir_from_rust_body("C", &px(), Some("bool"), "x.clone().is_power_of_two()")
            .expect("clone pot");
        assert!(clone.contains("cmp eq"), "{clone}");
        let into = try_ir_from_rust_body("I", &px(), Some("bool"), "x.into().is_power_of_two()")
            .expect("into pot");
        assert!(into.contains("cmp eq"), "{into}");
    }

    #[test]
    fn variable_u8_is_power_of_two_encodes() {
        // #1034: u8/u32 path params enumerate 1,2,4,... via OR chain
        let ir = try_ir_from_rust_body("P", &pu8(), Some("bool"), "x.is_power_of_two()")
            .expect("u8 pot");
        assert!(ir.contains("cmp eq"), "{ir}");
        assert!(
            ir.contains("const 1 : Int") && ir.contains("const 128 : Int"),
            "{ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "P").expect("parse");
        let u32ir = try_ir_from_rust_body("Q", &pu32(), Some("bool"), "x.is_power_of_two()")
            .expect("u32 pot");
        assert!(
            u32ir.contains("const 2147483648") || u32ir.contains("const 1 : Int"),
            "{u32ir}"
        );
    }

    #[test]
    fn variable_u8_count_ones_encodes() {
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let ir = try_ir_from_rust_body("C", &pu8, Some("u32"), "x.count_ones()").expect("u8 ones");
        assert!(
            ir.contains("arith div") && ir.contains("arith mod") && ir.contains("const 2"),
            "{ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "C").expect("parse");
        // signed stays const-only / BNM for variables
        assert!(try_ir_from_rust_body("S", &px(), Some("u32"), "x.count_ones()").is_none());
    }

    #[test]
    fn variable_u8_trailing_zeros_encodes() {
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let ir = try_ir_from_rust_body("T", &pu8, Some("u32"), "x.trailing_zeros()").expect("tz");
        assert!(
            ir.contains("arith mul") && ir.contains("const 8") && ir.contains("arith mod"),
            "{ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "T").expect("parse");
        assert!(try_ir_from_rust_body("S", &px(), Some("u32"), "x.trailing_zeros()").is_none());
    }

    #[test]
    fn const_count_ones_and_trailing_zeros_peep() {
        let c = try_ir_from_rust_body("C", &px(), Some("u32"), "12u32.count_ones()").expect("co");
        assert!(c.contains("const 2 : Int"), "{c}"); // 12 = 0b1100
        // 0b0111 = 7 has 3 trailing ones
        let to = try_ir_from_rust_body("To", &px(), Some("u8"), "7u8.trailing_ones()").expect("to");
        assert!(to.contains("const 3 : Int"), "{to}");
        // 0xF000_0000u32 leading ones = 4
        let lo = try_ir_from_rust_body("Lo", &px(), Some("u32"), "0xF000_0000u32.leading_ones()")
            .expect("lo");
        assert!(lo.contains("const 4 : Int"), "{lo}");
        // 12u32 has 2 ones → 30 zeros
        let cz =
            try_ir_from_rust_body("Cz", &px(), Some("u32"), "12u32.count_zeros()").expect("cz");
        assert!(cz.contains("const 30 : Int"), "{cz}");
        let tz =
            try_ir_from_rust_body("T", &px(), Some("u32"), "12u32.trailing_zeros()").expect("tz");
        assert!(tz.contains("const 2 : Int"), "{tz}");
        // Variable receivers stay BNM
        assert!(try_ir_from_rust_body("V", &px(), Some("u32"), "x.count_ones()").is_none());
        // Typed 0.trailing_zeros() == bit width
        let z0 =
            try_ir_from_rust_body("Z", &px(), Some("u32"), "0u32.trailing_zeros()").expect("0tz");
        assert!(z0.contains("const 32 : Int"), "{z0}");
        // bare 0 without suffix still BNM
        assert!(try_ir_from_rust_body("B", &px(), Some("u32"), "0.trailing_zeros()").is_none());
    }

    #[test]
    fn typed_leading_zeros_peep() {
        let lz =
            try_ir_from_rust_body("L", &px(), Some("u32"), "8u32.leading_zeros()").expect("lz");
        // 8u32 = 0b1000 → 28 leading zeros in 32 bits
        assert!(lz.contains("const 28 : Int"), "{lz}");
        // bare unsuffixed lit has no width
        assert!(try_ir_from_rust_body("B", &px(), Some("u32"), "8.leading_zeros()").is_none());
    }

    #[test]
    fn variable_u8_reverse_bits_encodes() {
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let ir = try_ir_from_rust_body("R", &pu8, Some("u8"), "x.reverse_bits()").expect("rev");
        assert!(ir.contains("arith mul") && ir.contains("arith mod"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "R").expect("parse");
    }

    #[test]
    fn variable_u16_swap_bytes_encodes() {
        let pu16 = vec![ParamInfo {
            name: "x".into(),
            ty: "u16".into(),
        }];
        let ir = try_ir_from_rust_body("S", &pu16, Some("u16"), "x.swap_bytes()").expect("sw");
        assert!(ir.contains("const 256") && ir.contains("arith mod"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let id = try_ir_from_rust_body("I", &pu8, Some("u8"), "x.swap_bytes()").expect("u8 id");
        assert!(id.contains("load $0"), "{id}");
    }

    #[test]
    fn typed_reverse_bits_and_swap_bytes_peep() {
        // 0b0000_0001 u8 reversed → 0b1000_0000 = 128
        let rev = try_ir_from_rust_body("R", &px(), Some("u8"), "1u8.reverse_bits()").expect("rev");
        assert!(rev.contains("const 128 : Int"), "{rev}");
        // 0x1234u16.swap_bytes() → 0x3412 = 13330
        let sw =
            try_ir_from_rust_body("S", &px(), Some("u16"), "0x1234u16.swap_bytes()").expect("sw");
        assert!(sw.contains("const 13330 : Int"), "{sw}");
        // i64 path param has no unsigned bit reverse (signed)
        assert!(try_ir_from_rust_body("V", &px(), Some("u8"), "x.reverse_bits()").is_none());
        let ig = try_ir_from_rust_body("I", &px(), Some("u32"), "8u32.ilog2()").expect("ilog");
        assert!(ig.contains("const 3 : Int"), "{ig}");
        assert!(try_ir_from_rust_body("Z", &px(), Some("u32"), "0u32.ilog2()").is_none());
        let np = try_ir_from_rust_body("Np", &px(), Some("u32"), "3u32.next_power_of_two()")
            .expect("np");
        assert!(np.contains("const 4 : Int"), "{np}");
        let z1 = try_ir_from_rust_body("Z1", &px(), Some("u32"), "0u32.next_power_of_two()")
            .expect("0np");
        assert!(z1.contains("const 1 : Int"), "{z1}");
        // 200u8 wraps (256 would overflow u8)
        let wnp = try_ir_from_rust_body(
            "Wnp",
            &px(),
            Some("u8"),
            "200u8.wrapping_next_power_of_two()",
        )
        .expect("wnp");
        assert!(wnp.contains("const 0 : Int"), "{wnp}");
        let sq = try_ir_from_rust_body("Sq", &px(), Some("u32"), "10u32.isqrt()").expect("isqrt");
        assert!(sq.contains("const 3 : Int"), "{sq}");
        assert!(try_ir_from_rust_body("Neg", &px(), Some("i64"), "(-1i64).isqrt()").is_none());
        let l10 =
            try_ir_from_rust_body("L10", &px(), Some("u32"), "100u32.ilog10()").expect("ilog10");
        assert!(l10.contains("const 2 : Int"), "{l10}");
        let ua = try_ir_from_rust_body("Ua", &px(), Some("i64"), "x.unsigned_abs()").expect("uabs");
        assert!(ua.contains("call abs"), "{ua}");
    }

    #[test]
    fn shift_rotate_zero_identity_peep() {
        let shl = try_ir_from_rust_body("S", &px(), Some("i64"), "x.wrapping_shl(0)").expect("shl");
        assert!(shl.contains("load $0"), "{shl}");
        assert!(!shl.contains("arith"), "{shl}");
        let rot = try_ir_from_rust_body("R", &px(), Some("i64"), "x.rotate_left(0)").expect("rot");
        assert!(rot.contains("load $0"), "{rot}");
        // signed wrapping_shr via floor div
        let shr =
            try_ir_from_rust_body("N", &px(), Some("i64"), "x.wrapping_shr(1)").expect("i64 shr");
        assert!(shr.contains("arith div"), "{shr}");
    }

    #[test]
    fn unsigned_wrapping_shl_const_encodes() {
        let pu8 = vec![ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        }];
        let ir = try_ir_from_rust_body("S", &pu8, Some("u8"), "x.wrapping_shl(1)").expect("shl1");
        assert!(ir.contains("arith mul") && ir.contains("const 2"), "{ir}");
        assert!(ir.contains("arith mod") && ir.contains("const 256"), "{ir}");
        // shift 8 on u8 ≡ shift 0 (mask)
        let id = try_ir_from_rust_body("I", &pu8, Some("u8"), "x.wrapping_shl(8)").expect("shl8");
        assert!(id.contains("load $0"), "{id}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let shr = try_ir_from_rust_body("R", &pu8, Some("u8"), "x.wrapping_shr(1)").expect("shr1");
        assert!(
            shr.contains("arith div") && shr.contains("const 2"),
            "{shr}"
        );
        let rot = try_ir_from_rust_body("Ro", &pu8, Some("u8"), "x.rotate_left(1)").expect("rotl");
        assert!(
            rot.contains("arith mul") && rot.contains("arith div"),
            "{rot}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&rot, "Ro").expect("parse rot");
        // signed rotate via bit-pattern map + reinterpret
        let pi8 = vec![ParamInfo {
            name: "x".into(),
            ty: "i8".into(),
        }];
        let srot =
            try_ir_from_rust_body("Sr", &pi8, Some("i8"), "x.rotate_left(1)").expect("i8 rotl");
        assert!(
            srot.contains("cmp gt") && srot.contains("arith mod"),
            "{srot}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&srot, "Sr").expect("parse srot");
    }

    #[test]
    fn variable_u8_wrapping_shl_encodes() {
        let p = vec![
            ParamInfo {
                name: "x".into(),
                ty: "u8".into(),
            },
            ParamInfo {
                name: "n".into(),
                ty: "u32".into(),
            },
        ];
        let ir = try_ir_from_rust_body("S", &p, Some("u8"), "x.wrapping_shl(n)").expect("var shl");
        assert!(ir.contains("cmp eq") && ir.contains("arith mul"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let shr = try_ir_from_rust_body("R", &p, Some("u8"), "x.wrapping_shr(n)").expect("var shr");
        assert!(shr.contains("arith div"), "{shr}");
    }

    #[test]
    fn variable_i8_wrapping_shl_encodes() {
        let p = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i8".into(),
            },
            ParamInfo {
                name: "n".into(),
                ty: "u32".into(),
            },
        ];
        let ir = try_ir_from_rust_body("S", &p, Some("i8"), "x.wrapping_shl(n)").expect("i8");
        assert!(ir.contains("cmp eq") && ir.contains("cmp gt"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let shr = try_ir_from_rust_body("R", &p, Some("i8"), "x.wrapping_shr(n)").expect("shr");
        assert!(shr.contains("arith div"), "{shr}");
    }

    #[test]
    fn variable_u32_wrapping_shl_encodes() {
        let p = vec![
            ParamInfo {
                name: "x".into(),
                ty: "u32".into(),
            },
            ParamInfo {
                name: "n".into(),
                ty: "u32".into(),
            },
        ];
        let ir = try_ir_from_rust_body("S", &p, Some("u32"), "x.wrapping_shl(n)").expect("u32");
        assert!(ir.contains("cmp eq"), "{ir}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
    }

    #[test]
    fn variable_i64_wrapping_shl_encodes() {
        let p = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "n".into(),
                ty: "u32".into(),
            },
        ];
        let ir = try_ir_from_rust_body("S", &p, Some("i64"), "x.wrapping_shl(n)").expect("i64");
        assert!(
            ir.contains("cmp eq")
                && ir.contains("const 4294967296")
                && ir.contains("const 2147483648"),
            "{ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let shr = try_ir_from_rust_body("R", &p, Some("i64"), "x.wrapping_shr(n)").expect("shr");
        assert!(shr.contains("arith div") && shr.contains("cmp eq"), "{shr}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&shr, "R").expect("parse");
        // const shift by 63 now encodes (2^63 = 2^32*2^31)
        let c63 =
            try_ir_from_rust_body("C", &px(), Some("i64"), "x.wrapping_shl(63)").expect("shl63");
        assert!(c63.contains("const 2147483648"), "{c63}");
    }

    #[test]
    fn variable_u64_wrapping_shl_encodes() {
        let p = vec![
            ParamInfo {
                name: "x".into(),
                ty: "u64".into(),
            },
            ParamInfo {
                name: "n".into(),
                ty: "u32".into(),
            },
        ];
        let ir = try_ir_from_rust_body("S", &p, Some("u64"), "x.wrapping_shl(n)").expect("u64");
        assert!(
            ir.contains("cmp eq") && ir.contains("const 4294967296") && !ir.contains("cmp gt"),
            "unsigned synthetic, no signed reinterpret: {ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let shr = try_ir_from_rust_body("R", &p, Some("u64"), "x.wrapping_shr(n)").expect("shr");
        assert!(shr.contains("arith div") && shr.contains("cmp eq"), "{shr}");
        assura_smt::LoadedVerifyExtras::from_ir_text(&shr, "R").expect("parse");
        let c1 =
            try_ir_from_rust_body("C", &p[..1], Some("u64"), "x.wrapping_shl(1)").expect("const1");
        assert!(
            c1.contains("arith mul") && c1.contains("const 4294967296"),
            "{c1}"
        );
        // usize same bounds path
        let pu = vec![
            ParamInfo {
                name: "x".into(),
                ty: "usize".into(),
            },
            ParamInfo {
                name: "n".into(),
                ty: "u32".into(),
            },
        ];
        let us =
            try_ir_from_rust_body("U", &pu, Some("usize"), "x.wrapping_shl(n)").expect("usize");
        assert!(us.contains("const 4294967296"), "{us}");
    }

    #[test]
    fn signed_wrapping_shl_const_encodes() {
        let pi8 = vec![ParamInfo {
            name: "x".into(),
            ty: "i8".into(),
        }];
        let ir = try_ir_from_rust_body("S", &pi8, Some("i8"), "x.wrapping_shl(1)").expect("i8 shl");
        assert!(
            ir.contains("arith mul") && ir.contains("arith mod") && ir.contains("cmp gt"),
            "{ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
        let i8shr =
            try_ir_from_rust_body("R", &pi8, Some("i8"), "x.wrapping_shr(1)").expect("i8 shr");
        assert!(i8shr.contains("arith div"), "{i8shr}");
        let i64ir =
            try_ir_from_rust_body("L", &px(), Some("i64"), "x.wrapping_shl(1)").expect("i64 shl");
        assert!(
            i64ir.contains("const 4294967296") && i64ir.contains("arith mul"),
            "{i64ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&i64ir, "L").expect("parse i64");
    }

    #[test]
    fn nested_signum_encodes_as_clamp() {
        // #1032: signum ≡ min(max(x, -1), 1); works inside arith without multi-block if.
        let ir = try_ir_from_rust_body("S", &px(), Some("i64"), "x.signum() + 1").expect("nested");
        assert!(ir.contains("call max"), "{ir}");
        assert!(ir.contains("call min"), "{ir}");
        assert!(ir.contains("arith add"), "{ir}");
        assert!(!ir.contains("then #"), "must stay single-block: {ir}");
    }

    #[test]
    fn top_level_signum_encodes() {
        let ir = try_ir_from_rust_body("S", &px(), Some("i64"), "x.signum()").expect("signum");
        assert!(ir.contains("const -1"), "{ir}");
        assert!(ir.contains("const 1"), "{ir}");
        assert!(ir.contains("call max"), "{ir}");
        assert!(ir.contains("call min"), "{ir}");
    }

    #[test]
    fn signum_method_chains_and_neg_encode() {
        let abs = try_ir_from_rust_body("A", &px(), Some("i64"), "x.signum().abs()").expect("abs");
        assert!(abs.contains("call abs"), "{abs}");
        let pxy = vec![
            ParamInfo {
                name: "x".into(),
                ty: "i64".into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: "i64".into(),
            },
        ];
        let sum = try_ir_from_rust_body("T", &pxy, Some("i64"), "(x + y).signum()").expect("sum");
        assert!(sum.contains("arith add"), "{sum}");
        let neg = try_ir_from_rust_body("N", &px(), Some("i64"), "-x.signum()").expect("neg");
        assert!(neg.contains("arith sub"), "{neg}");
        let mul = try_ir_from_rust_body("M", &px(), Some("i64"), "x.signum() * x").expect("mul");
        assert!(mul.contains("arith mul"), "{mul}");
        let notz =
            try_ir_from_rust_body("Z", &px(), Some("bool"), "!x.is_zero()").expect("not zero");
        assert!(notz.contains("cmp eq"), "{notz}");
    }
}
