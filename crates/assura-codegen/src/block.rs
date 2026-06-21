//! Codec registry, generic block, and Rust formatting code generation.

use super::*;

// Codec registry (FMT.4)
// ---------------------------------------------------------------------------

pub(crate) fn generate_codec_registry(cr: &CodecRegistryDecl, code: &mut String) {
    // Generate a dispatch function that matches magic bytes
    let output_ty = if cr.output_type.is_empty() {
        "()".to_string()
    } else {
        cr.output_type.join(" ")
    };
    code.push_str(&format!(
        "/// Codec registry `{}` dispatch function.\n",
        cr.name
    ));
    code.push_str(&format!(
        "pub fn dispatch_{}(data: &[u8]) -> Option<{}> {{\n",
        cr.name.to_lowercase(),
        output_ty
    ));
    for codec in &cr.codecs {
        match &codec.magic {
            MagicPattern::Bytes { bytes, prefix: _ } => {
                let len = bytes.len();
                let byte_checks: Vec<String> = bytes
                    .iter()
                    .enumerate()
                    .map(|(i, b)| format!("data[{i}] == 0x{b:02X}"))
                    .collect();
                let cond = byte_checks.join(" && ");
                code.push_str(&format!("    if data.len() >= {len} && {cond} {{\n"));
                code.push_str(&format!("        return Some({}(data));\n", codec.decoder));
                code.push_str("    }\n");
            }
            MagicPattern::Extension(exts) => {
                code.push_str(&format!("    // Extension-based detection: {:?}\n", exts));
            }
            MagicPattern::Probe(fn_name) => {
                code.push_str(&format!(
                    "    if {fn_name}(data) {{\n        return Some({}(data));\n    }}\n",
                    codec.decoder
                ));
            }
        }
    }
    code.push_str("    None\n}\n\n");
}

// ---------------------------------------------------------------------------
// Generic blocks (feature, incremental, etc.)
// ---------------------------------------------------------------------------

pub(crate) fn generate_block(kind: &BlockKind, name: &str, body: &[Clause], code: &mut String) {
    // Interface blocks generate Rust traits
    if *kind == BlockKind::Interface {
        generate_interface_trait(name, body, code);
        return;
    }

    // Feature blocks with only Other clauses: compile-time only, no Rust codegen needed.
    // Feature blocks that contain structural clauses (Ordering, Effects, Ensures, etc.)
    // are handled by the generic block codegen below.
    if *kind == BlockKind::Feature
        && body
            .iter()
            .all(|c| matches!(c.kind, ClauseKind::Other(_) | ClauseKind::Requires))
    {
        code.push_str(&format!("// {kind} {name}: compile-time feature flag\n\n"));
        return;
    }

    // Table blocks: generate a doc comment describing the table.
    // The actual compile-time verification happens in the SMT layer,
    // not in generated Rust code.
    if *kind == BlockKind::Table {
        code.push_str(&format!(
            "// {kind} {name}: compile-time verified by SMT\n\n"
        ));
        return;
    }

    // Other blocks: generate as documented constants/assertions
    code.push_str(&format!("/// {kind}: {name}\n"));
    code.push_str(&format!("pub mod block_{} {{\n", name.to_lowercase()));

    for clause in body {
        let expr = expr_to_rust(&clause.body);
        match clause.kind {
            ClauseKind::Ensures | ClauseKind::Invariant => {
                code.push_str(&format!(
                    "    /// Invariant: {expr}\n    pub fn check_{name}() {{ debug_assert!({expr}); }}\n",
                    name = name.to_lowercase()
                ));
            }
            ClauseKind::Requires => {
                code.push_str(&format!(
                    "    /// Precondition: {expr}\n    pub const PRECONDITION: &str = \"{}\";\n",
                    expr.replace('"', "\\\"")
                ));
            }
            ClauseKind::Effects => {
                code.push_str(&format!("    /// Effects: {expr}\n"));
            }
            ClauseKind::Modifies => {
                code.push_str(&format!("    /// Modifies: {expr}\n"));
            }
            ClauseKind::Input => {
                code.push_str(&format!("    /// Input: {expr}\n"));
            }
            ClauseKind::Output => {
                code.push_str(&format!("    /// Output: {expr}\n"));
            }
            ClauseKind::Errors => {
                code.push_str(&format!("    /// Errors: {expr}\n"));
            }
            ClauseKind::Rule => {
                code.push_str(&format!(
                    "    /// Rule: {expr}\n    pub fn check_rule_{name}() {{ debug_assert!({expr}); }}\n",
                    name = name.to_lowercase()
                ));
            }
            ClauseKind::DataFlow => {
                code.push_str(&format!("    /// DataFlow: {expr}\n"));
            }
            ClauseKind::MustNot => {
                code.push_str(&format!(
                    "    /// MustNot: {expr}\n    pub fn check_must_not_{name}() {{ debug_assert!(!({expr})); }}\n",
                    name = name.to_lowercase()
                ));
            }
            ClauseKind::Decreases => {
                code.push_str(&format!("    /// Decreases: {expr}\n"));
            }
            ClauseKind::Ordering => {
                code.push_str(&format!("    /// Ordering: {expr}\n"));
                if let Some(ord) = resolve_ordering_variant(&clause.body) {
                    code.push_str(&format!(
                        "    const ORDERING: std::sync::atomic::Ordering = std::sync::atomic::Ordering::{ord};\n"
                    ));
                }
            }
            ClauseKind::Other(ref kind_name) => {
                code.push_str(&format!("    /// {kind_name}: {expr}\n"));
            }
        }
    }

    code.push_str("}\n\n");
}

// ---------------------------------------------------------------------------
// Rust formatting via prettyplease
// ---------------------------------------------------------------------------

/// Format a Rust source string via prettyplease.
///
/// If parsing fails (the generated code is not valid Rust syntax),
/// returns the input unchanged with a comment noting the failure.
pub(crate) fn format_rust(code: &str) -> String {
    match syn::parse_file(code) {
        Ok(syntax_tree) => prettyplease::unparse(&syntax_tree),
        Err(e) => {
            eprintln!("warning: generated Rust has syntax errors, skipping formatting: {e}");
            format!("// WARNING: prettyplease formatting skipped (parse error: {e})\n\n{code}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::Spanned;

    fn mk_clause(kind: ClauseKind, body: SpExpr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    // ---- generate_block ----

    #[test]
    fn block_interface_delegates_to_trait() {
        let clauses = vec![mk_clause(
            ClauseKind::Other("method".into()),
            Spanned::no_span(Expr::Ident("compute".into())),
        )];
        let mut code = String::new();
        generate_block(&BlockKind::Interface, "Computable", &clauses, &mut code);
        assert!(code.contains("pub trait Computable"));
    }

    #[test]
    fn block_feature_compile_time_only() {
        let clauses = vec![mk_clause(
            ClauseKind::Other("flag".into()),
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        )];
        let mut code = String::new();
        generate_block(&BlockKind::Feature, "my_flag", &clauses, &mut code);
        assert!(code.contains("compile-time feature flag"));
        assert!(!code.contains("pub mod"));
    }

    #[test]
    fn block_table_compile_time_smt() {
        let mut code = String::new();
        generate_block(&BlockKind::Table, "lookup", &[], &mut code);
        assert!(code.contains("compile-time verified by SMT"));
    }

    #[test]
    fn block_generic_with_ensures() {
        let clauses = vec![mk_clause(
            ClauseKind::Ensures,
            Spanned::no_span(Expr::BinOp {
                lhs: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
                op: BinOp::Gt,
                rhs: Box::new(Spanned::no_span(Expr::Literal(Literal::Int("0".into())))),
            }),
        )];
        let mut code = String::new();
        generate_block(
            &BlockKind::Other("verification".into()),
            "positive",
            &clauses,
            &mut code,
        );
        assert!(code.contains("pub mod block_positive"));
        assert!(code.contains("debug_assert!"));
    }

    #[test]
    fn block_with_requires() {
        let clauses = vec![mk_clause(
            ClauseKind::Requires,
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        )];
        let mut code = String::new();
        generate_block(
            &BlockKind::Other("verification".into()),
            "precond",
            &clauses,
            &mut code,
        );
        assert!(code.contains("PRECONDITION"));
    }

    #[test]
    fn block_with_must_not() {
        let clauses = vec![mk_clause(
            ClauseKind::MustNot,
            Spanned::no_span(Expr::Ident("overflow".into())),
        )];
        let mut code = String::new();
        generate_block(
            &BlockKind::Other("verification".into()),
            "safe",
            &clauses,
            &mut code,
        );
        assert!(code.contains("check_must_not_safe"));
        assert!(code.contains("!(overflow)"));
    }

    #[test]
    fn block_with_rule() {
        let clauses = vec![mk_clause(
            ClauseKind::Rule,
            Spanned::no_span(Expr::Literal(Literal::Bool(true))),
        )];
        let mut code = String::new();
        generate_block(
            &BlockKind::Other("verification".into()),
            "r1",
            &clauses,
            &mut code,
        );
        assert!(code.contains("check_rule_r1"));
    }

    // ---- generate_codec_registry ----

    #[test]
    fn codec_registry_bytes() {
        let cr = CodecRegistryDecl {
            name: "images".into(),
            output_type: vec!["Image".into()],
            codecs: vec![assura_parser::ast::CodecEntry {
                name: "png".into(),
                decoder: "decode_png".into(),
                magic: MagicPattern::Bytes {
                    bytes: vec![0x89, 0x50, 0x4E, 0x47],
                    prefix: true,
                },
                contracts: vec![],
            }],
        };
        let mut code = String::new();
        generate_codec_registry(&cr, &mut code);
        assert!(code.contains("dispatch_images"));
        assert!(code.contains("0x89"));
        assert!(code.contains("decode_png"));
    }

    #[test]
    fn codec_registry_probe() {
        let cr = CodecRegistryDecl {
            name: "formats".into(),
            output_type: vec![],
            codecs: vec![assura_parser::ast::CodecEntry {
                name: "custom".into(),
                decoder: "decode_custom".into(),
                magic: MagicPattern::Probe("is_custom".into()),
                contracts: vec![],
            }],
        };
        let mut code = String::new();
        generate_codec_registry(&cr, &mut code);
        assert!(code.contains("is_custom(data)"));
        assert!(code.contains("decode_custom"));
    }

    // ---- format_rust ----

    #[test]
    fn format_valid_rust() {
        let code = "fn main() { let x = 1; }";
        let formatted = format_rust(code);
        assert!(formatted.contains("fn main()"));
    }

    #[test]
    fn format_invalid_rust_returns_original() {
        let code = "this is not valid rust {{{";
        let result = format_rust(code);
        assert!(result.contains("WARNING"));
        assert!(result.contains(code));
    }
}
