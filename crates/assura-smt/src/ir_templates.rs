//! AI prompt assembly for Implementation IR generation.
//!
//! Loads a shared base template plus thin pattern overlays from `templates/ir/`.
//! Pattern detection reuses `ir_generate::classify_ensures_shape` — no duplicated
//! clause analysis logic.

use std::path::Path;

use assura_parser::ast::{Clause, ClauseKind, Param};
use assura_parser::display::expr_to_string;

use crate::ir_generate::{EnsuresShape, classify_ensures_shape};

/// Pattern overlay for IR prompt generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrPromptPattern {
    Auto,
    Identity,
    Arithmetic,
    LengthCopy,
    CallChain,
    BoundsCheck,
    FieldAccess,
}

impl std::str::FromStr for IrPromptPattern {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "identity" => Ok(Self::Identity),
            "arithmetic" | "arith" => Ok(Self::Arithmetic),
            "length-copy" | "length_copy" | "lengthcopy" => Ok(Self::LengthCopy),
            "call-chain" | "call_chain" | "callchain" => Ok(Self::CallChain),
            "bounds" | "bounds-check" | "bounds_check" => Ok(Self::BoundsCheck),
            "field" | "field-access" | "field_access" => Ok(Self::FieldAccess),
            _ => Err(()),
        }
    }
}

impl IrPromptPattern {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Identity => "identity",
            Self::Arithmetic => "arithmetic",
            Self::LengthCopy => "length-copy",
            Self::CallChain => "call-chain",
            Self::BoundsCheck => "bounds-check",
            Self::FieldAccess => "field-access",
        }
    }

    fn from_shape(shape: EnsuresShape) -> Self {
        match shape {
            EnsuresShape::Identity => Self::Identity,
            EnsuresShape::Arithmetic => Self::Arithmetic,
            EnsuresShape::LengthCopy => Self::LengthCopy,
            EnsuresShape::BoundsCheck => Self::BoundsCheck,
            EnsuresShape::FieldAccess => Self::FieldAccess,
            EnsuresShape::CallChain => Self::CallChain,
            EnsuresShape::Unknown => Self::Identity,
        }
    }
}

/// Inputs for rendering an IR generation prompt.
#[derive(Debug, Clone)]
pub struct IrPromptContext {
    pub decl_name: String,
    pub params: Vec<Param>,
    pub return_ty: Vec<String>,
    pub clauses: Vec<Clause>,
    pub source_file: Option<String>,
}

impl IrPromptContext {
    pub fn param_names(&self) -> Vec<String> {
        self.params.iter().map(|p| p.name.clone()).collect()
    }

    pub fn return_type_str(&self) -> String {
        if self.return_ty.is_empty() {
            "Unit".into()
        } else {
            self.return_ty.join(" ")
        }
    }

    pub fn module_name(&self) -> String {
        sanitize_module_name(&self.decl_name)
    }

    pub fn param_slots(&self) -> String {
        self.params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let ty = if p.ty.is_empty() {
                    "Int".to_string()
                } else {
                    p.ty.join(" ")
                };
                format!("${i}: {ty}")
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Suggest the best pattern overlay from contract clauses.
pub fn suggest_ir_pattern(ctx: &IrPromptContext) -> IrPromptPattern {
    IrPromptPattern::from_shape(classify_ensures_shape(&ctx.clauses, &ctx.param_names()))
}

/// Resolve pattern: `Auto` runs clause analysis; explicit patterns pass through.
pub fn resolve_ir_pattern(ctx: &IrPromptContext, pattern: IrPromptPattern) -> IrPromptPattern {
    match pattern {
        IrPromptPattern::Auto => suggest_ir_pattern(ctx),
        other => other,
    }
}

/// Render a complete AI prompt (base + pattern overlay + contract block).
pub fn render_ir_prompt(ctx: &IrPromptContext, pattern: IrPromptPattern) -> String {
    let pattern = resolve_ir_pattern(ctx, pattern);
    let base = BASE_TEMPLATE;
    let overlay = load_pattern_overlay(pattern);
    let contract_block = format_contract_block(ctx);
    let heuristic_ir = crate::ir_generate::generate_ir_sidecar_text(
        &ctx.decl_name,
        &param_slot_types(ctx),
        &ctx.param_names(),
        &ctx.return_type_str(),
        &ctx.clauses,
    );

    let pattern_section = format!(
        "---\n\n{overlay}\n\n## Heuristic starting point (optional)\n\n\
         The compiler generated this stub from ensures analysis. Refine or replace:\n\n\
         {heuristic_ir}"
    );

    base.replace("{contract_block}", &contract_block)
        .replace("{module_name}", &ctx.module_name())
        .replace("{decl_name}", &ctx.decl_name)
        .replace("{param_slots}", &ctx.param_slots())
        .replace("{return_type}", &ctx.return_type_str())
        .replace(
            "{source_file}",
            ctx.source_file.as_deref().unwrap_or("<contract>"),
        )
        .replace("{pattern_section}", &pattern_section)
}

fn format_contract_block(ctx: &IrPromptContext) -> String {
    let mut lines = vec![format!("// Declaration: {}", ctx.decl_name)];
    if !ctx.params.is_empty() {
        let params: Vec<String> = ctx
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, p.ty.join(" ")))
            .collect();
        lines.push(format!("input({})", params.join(", ")));
    }
    if !ctx.return_ty.is_empty() {
        lines.push(format!("output(result: {})", ctx.return_type_str()));
    }
    for clause in &ctx.clauses {
        let kind = match clause.kind {
            ClauseKind::Requires => "requires",
            ClauseKind::Ensures => "ensures",
            ClauseKind::Invariant => "invariant",
            _ => continue,
        };
        lines.push(format!("{kind} {{ {} }}", expr_to_string(&clause.body)));
    }
    lines.join("\n")
}

fn param_slot_types(ctx: &IrPromptContext) -> Vec<(usize, String)> {
    ctx.params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                i,
                if p.ty.is_empty() {
                    "Int".into()
                } else {
                    p.ty.join(" ")
                },
            )
        })
        .collect()
}

const BASE_TEMPLATE: &str = include_str!("../../../templates/ir/base.md");

fn load_pattern_overlay(pattern: IrPromptPattern) -> &'static str {
    match pattern {
        IrPromptPattern::Identity | IrPromptPattern::Auto => {
            include_str!("../../../templates/ir/patterns/identity.md")
        }
        IrPromptPattern::Arithmetic => {
            include_str!("../../../templates/ir/patterns/arithmetic.md")
        }
        IrPromptPattern::LengthCopy => {
            include_str!("../../../templates/ir/patterns/length-copy.md")
        }
        IrPromptPattern::CallChain => {
            include_str!("../../../templates/ir/patterns/call-chain.md")
        }
        IrPromptPattern::BoundsCheck => {
            include_str!("../../../templates/ir/patterns/bounds-check.md")
        }
        IrPromptPattern::FieldAccess => {
            include_str!("../../../templates/ir/patterns/field-access.md")
        }
    }
}

fn sanitize_module_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build prompt contexts for every verification job in a typed file.
pub fn ir_prompt_contexts_for_typed(
    typed: &assura_types::TypedFile,
    source_path: Option<&Path>,
) -> Vec<IrPromptContext> {
    crate::entry::collect_verification_jobs(typed)
        .into_iter()
        .map(|(name, clauses, params, return_ty)| IrPromptContext {
            decl_name: name,
            params,
            return_ty,
            clauses,
            source_file: source_path.map(|p| p.display().to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{BinOp, ClauseKind, Expr};

    fn copy_bytes_ctx() -> IrPromptContext {
        IrPromptContext {
            decl_name: "CopyBytes".into(),
            params: vec![Param {
                name: "raw".into(),
                ty: vec!["Bytes".into()],
                parsed_type: None,
            }],
            return_ty: vec!["Bytes".into()],
            clauses: vec![
                Clause {
                    kind: ClauseKind::Requires,
                    body: Expr::BinOp {
                        op: BinOp::Gt,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("raw".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::Literal(assura_parser::ast::Literal::Int("0".into()))),
                    },
                    effect_variables: vec![],
                },
                Clause {
                    kind: ClauseKind::Ensures,
                    body: Expr::BinOp {
                        op: BinOp::Lte,
                        lhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("result".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                        rhs: Box::new(Expr::MethodCall {
                            receiver: Box::new(Expr::Ident("raw".into())),
                            method: "length".into(),
                            args: vec![],
                        }),
                    },
                    effect_variables: vec![],
                },
            ],
            source_file: Some("copy.assura".into()),
        }
    }

    #[test]
    fn suggest_length_copy_for_bytes_postcondition() {
        let ctx = copy_bytes_ctx();
        assert_eq!(suggest_ir_pattern(&ctx), IrPromptPattern::LengthCopy);
    }

    #[test]
    fn render_includes_base_and_pattern_without_duplicate_syntax_table() {
        let ctx = copy_bytes_ctx();
        let prompt = render_ir_prompt(&ctx, IrPromptPattern::Auto);
        assert!(prompt.contains("Instruction reference"));
        assert!(prompt.contains("Pattern: length-preserving copy"));
        assert!(prompt.contains("CopyBytes"));
        assert!(prompt.contains("result.length()"));
        assert!(!prompt.contains("{contract_block}"));
    }

    #[test]
    fn suggest_call_chain_when_result_eq_helper_call() {
        let ctx = IrPromptContext {
            decl_name: "Main".into(),
            params: vec![Param {
                name: "x".into(),
                ty: vec!["Int".into()],
                parsed_type: None,
            }],
            return_ty: vec!["Int".into()],
            clauses: vec![Clause {
                kind: ClauseKind::Ensures,
                body: Expr::BinOp {
                    op: BinOp::Eq,
                    lhs: Box::new(Expr::Ident("result".into())),
                    rhs: Box::new(Expr::Call {
                        func: Box::new(Expr::Ident("double".into())),
                        args: vec![Expr::Ident("x".into())],
                    }),
                },
                effect_variables: vec![],
            }],
            source_file: None,
        };
        assert_eq!(suggest_ir_pattern(&ctx), IrPromptPattern::CallChain);
    }

    #[test]
    fn render_explicit_call_chain_overlay() {
        let ctx = copy_bytes_ctx();
        let prompt = render_ir_prompt(&ctx, IrPromptPattern::CallChain);
        assert!(prompt.contains("Pattern: call chain"));
        assert!(prompt.contains("double.ir"));
    }

    #[test]
    fn pattern_from_str_roundtrip() {
        assert_eq!(
            "length-copy".parse::<IrPromptPattern>(),
            Ok(IrPromptPattern::LengthCopy)
        );
        assert_eq!("auto".parse::<IrPromptPattern>(), Ok(IrPromptPattern::Auto));
    }
}
