//! IR-to-Rust code generation helpers.
//!
//! Converts parsed `IrModule` / `IrFunction` structures into Rust source text.

use crate::ir::{
    IrArithOp, IrCmpOp, IrExprKind, IrFunction, IrLiteral, IrMatchPattern, IrModule, IrPred,
    IrPredArg,
};

/// Placeholder `.ir` sidecar text for a declaration (AI replaces with real IR).
///
/// Uses identity `load` from the first parameter when present so SMT havoc+assume
/// has a minimal implementation constraint to refine.
pub fn stub_ir_sidecar_text(
    name: &str,
    params: &[(usize, String)],
    return_ty: &str,
    requires_count: usize,
    ensures_count: usize,
) -> String {
    let module = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let param_list = params
        .iter()
        .map(|(slot, ty)| format!("${slot}: {ty}"))
        .collect::<Vec<_>>()
        .join(", ");
    let body = if let Some((slot, _)) = params.first() {
        format!("    $result = load ${slot} : {return_ty}\n")
    } else {
        format!("    $result = const 0 : {return_ty}\n")
    };
    format!(
        "// Stub IR for {name} — AI replaces body to satisfy contract ensures\n\
         // Contract: {requires_count} requires, {ensures_count} ensures\n\
         module {module} {{\n\
           fn #0 : ({param_list}) -> {return_ty} ! pure\n\
           pre: true\n\
           {{\n\
         {body}\
           }}\n\
         }}\n"
    )
}

/// Generate Rust source code from a validated IR module.
///
/// Each IR function becomes a Rust function with debug_assert!
/// for pre/post conditions.
pub fn ir_to_rust(module: &IrModule) -> String {
    let mut code = String::new();
    code.push_str(&format!("// Generated from IR module: {}\n\n", module.name));

    for func in &module.functions {
        // Function signature
        let params: Vec<String> = func
            .params
            .iter()
            .map(|p| format!("slot_{}: {}", p.slot, ir_type_to_rust(&p.ty)))
            .collect();

        let ret_type = ir_type_to_rust(&func.return_type);
        code.push_str(&format!(
            "fn ir_{}({}) -> {} {{\n",
            func.id.trim_start_matches('#'),
            params.join(", "),
            ret_type
        ));

        // Pre-condition
        if let Some(ref pre) = func.pre {
            let pre_rust = pred_to_rust(pre);
            code.push_str(&format!("    debug_assert!({pre_rust});\n"));
        }

        // Body instructions
        for instr in &func.body {
            let target = if instr.target == usize::MAX {
                crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
            } else {
                format!("slot_{}", instr.target)
            };
            let ty = ir_type_to_rust(&instr.ty);
            let expr_code = ir_expr_to_rust(&instr.expr);
            code.push_str(&format!("    let {target}: {ty} = {expr_code};\n"));
        }

        // Post-condition
        if let Some(ref post) = func.post {
            let post_rust = pred_to_rust(post);
            code.push_str(&format!("    debug_assert!({post_rust});\n"));
        }

        // Return $result if it was assigned, otherwise use a default value
        if func.body.iter().any(|i| i.target == usize::MAX) {
            code.push_str("    __result\n");
        } else {
            // Generate a type-appropriate default return value
            let default_val = ir_type_default(&func.return_type);
            code.push_str(&format!("    {default_val}\n"));
        }

        code.push_str("}\n\n");
    }

    code
}

/// Generate only the function body (instructions + postcondition) from an IR function.
///
/// Unlike `ir_to_rust` which generates complete Rust functions, this returns
/// the body code suitable for embedding into codegen-produced contract/fn/service
/// bodies in place of `todo!()` placeholders. The code uses slot variables
/// (`slot_0`, `slot_1`, etc.) and assumes the caller maps contract input params
/// to the corresponding slot bindings.
pub fn ir_function_body_to_rust(func: &IrFunction) -> String {
    let mut code = String::new();

    // Pre-condition
    if let Some(ref pre) = func.pre {
        let pre_rust = pred_to_rust(pre);
        if pre_rust != "true" {
            code.push_str(&format!(
                "    debug_assert!({pre_rust}, \"IR pre-condition\");\n"
            ));
        }
    }

    // Body instructions
    for instr in &func.body {
        let target = if instr.target == usize::MAX {
            crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
        } else {
            format!("slot_{}", instr.target)
        };
        let ty = ir_type_to_rust(&instr.ty);
        let expr_code = ir_expr_to_rust(&instr.expr);
        code.push_str(&format!("    let {target}: {ty} = {expr_code};\n"));
    }

    // Post-condition
    if let Some(ref post) = func.post {
        let post_rust = pred_to_rust(post);
        if post_rust != "true" {
            code.push_str(&format!(
                "    debug_assert!({post_rust}, \"IR post-condition\");\n"
            ));
        }
    }

    // Return __result if it was assigned
    if func.body.iter().any(|i| i.target == usize::MAX) {
        code.push_str("    __result\n");
    } else {
        let default_val = ir_type_default(&func.return_type);
        code.push_str(&format!("    {default_val}\n"));
    }

    code
}

/// Embed a full IR module (main `fn #0` + sibling blocks) as Rust statements
/// suitable for injection into a contract `check` body.
///
/// Sibling `fn #N` become `let block_N = || -> Ret { ... };` closures that
/// capture outer `slot_*` bindings (branch arms close over main slots).
/// Multi-block modules that only exported `ir_function_body_to_rust(fn#0)`
/// previously emitted `block_N()` calls with no definitions (#882).
pub fn ir_module_to_embedded_body(module: &IrModule) -> String {
    let mut code = String::new();
    let ret_ty = module
        .functions
        .first()
        .map(|f| ir_type_to_rust(&f.return_type))
        .unwrap_or_else(|| "i64".to_string());

    // Sibling blocks first (referenced from main).
    for func in module.functions.iter().skip(1) {
        let id = func.id.trim_start_matches('#').to_string();
        let body = ir_function_body_to_rust(func);
        // Indent body one level inside the closure.
        let indented: String = body
            .lines()
            .map(|l| {
                if l.is_empty() {
                    String::new()
                } else {
                    format!("    {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        code.push_str(&format!(
            "    let block_{id} = || -> {ret_ty} {{\n{indented}\n    }};\n"
        ));
    }

    // Main function body (may call block_N()).
    if let Some(main) = module.functions.first() {
        code.push_str(&ir_function_body_to_rust(main));
    }
    code
}

/// Build a map from contract/function names to their IR-generated Rust body code.
///
/// For each function in the module, the first function is mapped to the module name,
/// and subsequent functions are mapped to their function ID.
pub fn ir_module_to_body_map(module: &IrModule) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for (i, func) in module.functions.iter().enumerate() {
        let key = if i == 0 {
            module.name.clone()
        } else {
            func.id.trim_start_matches('#').to_string()
        };
        map.insert(key, ir_function_body_to_rust(func));
    }
    map
}

pub(crate) fn ir_type_to_rust(ty: &str) -> String {
    match ty {
        "Int" => "i64".to_string(),
        "Nat" => "u64".to_string(),
        "Float" => "f64".to_string(),
        "Bool" => "bool".to_string(),
        "String" => "String".to_string(),
        "Bytes" => "Vec<u8>".to_string(),
        "Unit" => "()".to_string(),
        "" => "_".to_string(),
        other => other.to_string(),
    }
}

/// Generate a default value for an IR return type.
pub(crate) fn ir_type_default(ty: &str) -> String {
    match ty {
        "Int" => "0_i64".to_string(),
        "Nat" => "0_u64".to_string(),
        "Float" => "0.0_f64".to_string(),
        "Bool" => "false".to_string(),
        "String" => "String::new()".to_string(),
        "Bytes" => "Vec::new()".to_string(),
        "Unit" | "" => "()".to_string(),
        _ => "Default::default()".to_string(),
    }
}

pub(crate) fn ir_expr_to_rust(expr: &IrExprKind) -> String {
    match expr {
        IrExprKind::Const(lit) => match lit {
            IrLiteral::Int(n) => format!("{n}_i64"),
            IrLiteral::Float(f) => format!("{f}_f64"),
            IrLiteral::Str(s) => format!("\"{s}\".to_string()"),
            IrLiteral::Bool(b) => format!("{b}"),
        },
        IrExprKind::Load(s) => {
            if *s == usize::MAX {
                crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
            } else {
                format!("slot_{s}")
            }
        }
        IrExprKind::Call { func, args } => {
            let arg_strs: Vec<String> = args
                .iter()
                .map(|a| {
                    if *a == usize::MAX {
                        crate::encode_atom_policy::RESULT_VAR_NAME.to_string()
                    } else {
                        format!("slot_{a}")
                    }
                })
                .collect();
            format!("{func}({})", arg_strs.join(", "))
        }
        IrExprKind::Field { slot, index } => format!("slot_{slot}.{index}"),
        IrExprKind::Arith { op, lhs, rhs } => {
            let op_str = match op {
                IrArithOp::Add => "+",
                IrArithOp::Sub => "-",
                IrArithOp::Mul => "*",
                IrArithOp::Div => "/",
                IrArithOp::Mod => "%",
            };
            format!("(slot_{lhs} {op_str} slot_{rhs})")
        }
        IrExprKind::Cmp { op, lhs, rhs } => {
            let op_str = match op {
                IrCmpOp::Eq => "==",
                IrCmpOp::Ne => "!=",
                IrCmpOp::Lt => "<",
                IrCmpOp::Le => "<=",
                IrCmpOp::Gt => ">",
                IrCmpOp::Ge => ">=",
            };
            format!("(slot_{lhs} {op_str} slot_{rhs})")
        }
        IrExprKind::Cast { slot, target_type } => {
            format!("slot_{slot} as {}", ir_type_to_rust(target_type))
        }
        IrExprKind::Construct {
            type_id, fields, ..
        } => {
            let field_strs: Vec<String> = fields.iter().map(|(_, s)| format!("slot_{s}")).collect();
            format!("{type_id}::new({})", field_strs.join(", "))
        }
        IrExprKind::If {
            cond,
            then_block,
            else_block,
        } => {
            format!("if slot_{cond} {{ block_{then_block}() }} else {{ block_{else_block}() }}")
        }
        IrExprKind::Transition { slot, state } => {
            format!("slot_{slot}.transition_to_{state}()")
        }
        IrExprKind::Match { scrutinee, arms } => {
            let arm_strs: Vec<String> = arms
                .iter()
                .map(|(pat, block)| {
                    let pat_str = match pat {
                        IrMatchPattern::Int(n) => format!("{n}"),
                        IrMatchPattern::Bool(b) => format!("{b}"),
                        IrMatchPattern::Str(s) => format!("\"{s}\""),
                        IrMatchPattern::Wildcard => "_".to_string(),
                    };
                    format!("{pat_str} => block_{block}()")
                })
                .collect();
            format!("match slot_{scrutinee} {{ {} }}", arm_strs.join(", "))
        }
        IrExprKind::Loop { body_block, cond } => {
            format!("loop {{ block_{body_block}(); if !slot_{cond} {{ break; }} }}")
        }
    }
}

fn pred_to_rust(pred: &IrPred) -> String {
    match pred {
        IrPred::True => "true".to_string(),
        IrPred::False => "false".to_string(),
        IrPred::Cmp { op, lhs, rhs } => {
            let op_str = match op {
                IrCmpOp::Eq => "==",
                IrCmpOp::Ne => "!=",
                IrCmpOp::Lt => "<",
                IrCmpOp::Le => "<=",
                IrCmpOp::Gt => ">",
                IrCmpOp::Ge => ">=",
            };
            format!(
                "({} {} {})",
                pred_arg_to_rust(lhs),
                op_str,
                pred_arg_to_rust(rhs)
            )
        }
        IrPred::And(a, b) => format!("({} && {})", pred_to_rust(a), pred_to_rust(b)),
        IrPred::Or(a, b) => format!("({} || {})", pred_to_rust(a), pred_to_rust(b)),
        IrPred::Not(p) => format!("!({})", pred_to_rust(p)),
    }
}

fn pred_arg_to_rust(arg: &IrPredArg) -> String {
    match arg {
        IrPredArg::Slot(n) => format!("slot_{n}"),
        IrPredArg::SlotResult => crate::encode_atom_policy::RESULT_VAR_NAME.to_string(),
        IrPredArg::Lit(lit) => match lit {
            IrLiteral::Int(n) => format!("{n}_i64"),
            IrLiteral::Float(f) => format!("{f}_f64"),
            IrLiteral::Str(s) => format!("\"{s}\""),
            IrLiteral::Bool(b) => format!("{b}"),
        },
        IrPredArg::Arith { op, lhs, rhs } => {
            let op_str = match op {
                IrArithOp::Add => "+",
                IrArithOp::Sub => "-",
                IrArithOp::Mul => "*",
                IrArithOp::Div => "/",
                IrArithOp::Mod => "%",
            };
            format!(
                "({} {} {})",
                pred_arg_to_rust(lhs),
                op_str,
                pred_arg_to_rust(rhs)
            )
        }
    }
}
#[cfg(test)]
#[path = "ir_tests.rs"]
mod tests;
