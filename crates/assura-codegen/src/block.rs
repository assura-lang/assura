//! Codec registry, generic block, and Rust formatting code generation.

use super::*;

// Codec registry (FMT.4)
// ---------------------------------------------------------------------------

pub(crate) fn generate_codec_registry(cr: &CodecRegistryDecl, code: &mut String) {
    use crate::hir::*;

    let output_ty = if cr.output_type.is_empty() {
        "()".to_string()
    } else {
        cr.output_type.join(" ")
    };

    let mut body: Vec<RustStmt> = Vec::new();

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
                body.push(RustStmt::Raw(format!(
                    "if data.len() >= {len} && {cond} {{\n    return Some({}(data));\n}}",
                    codec.decoder
                )));
            }
            MagicPattern::Extension(exts) => {
                body.push(RustStmt::Comment(format!(
                    "Extension-based detection: {exts:?}"
                )));
            }
            MagicPattern::Probe(fn_name) => {
                body.push(RustStmt::Raw(format!(
                    "if {fn_name}(data) {{\n    return Some({}(data));\n}}",
                    codec.decoder
                )));
            }
        }
    }

    body.push(RustStmt::Expr(RustExpr::Ident("None".into())));

    let item = RustItem::Fn(RustFn {
        name: format!("dispatch_{}", cr.name.to_lowercase()),
        params: vec![RustParam {
            name: "data".into(),
            ty: RustType::Raw("&[u8]".into()),
        }],
        ret: Some(RustType::Raw(format!("Option<{output_ty}>"))),
        body,
        doc: vec![format!("Codec registry `{}` dispatch function.", cr.name)],
        ..RustFn::default()
    });
    code.push_str(&render_item_raw(&item));
}

// ---------------------------------------------------------------------------
// Generic blocks (feature, incremental, etc.)
// ---------------------------------------------------------------------------

pub(crate) fn generate_block(kind: &BlockKind, name: &str, body: &[Clause], code: &mut String) {
    use crate::hir::*;

    // Interface blocks generate Rust traits
    if *kind == BlockKind::Interface {
        generate_interface_trait(name, body, code);
        return;
    }

    // Feature blocks with only Other clauses: compile-time only, no Rust codegen needed.
    if *kind == BlockKind::Feature
        && body
            .iter()
            .all(|c| matches!(c.kind, ClauseKind::Other(_) | ClauseKind::Requires))
    {
        let item = RustItem::Comment(format!("{kind} {name}: compile-time feature flag"));
        code.push_str(&render_item_raw(&item));
        code.push('\n');
        return;
    }

    // Table blocks: compile-time verified by SMT, no Rust codegen.
    if *kind == BlockKind::Table {
        let item = RustItem::Comment(format!("{kind} {name}: compile-time verified by SMT"));
        code.push_str(&render_item_raw(&item));
        code.push('\n');
        return;
    }

    // MISC.1 incremental state machines: verified by SMT. Emitting a mod with
    // typestate `self.state` asserts and trailing `///` metadata for `on` /
    // `transition` clauses produced invalid Rust (doc comments with no
    // following item; free-fn `self`). Keep a single compile-time marker.
    if *kind == BlockKind::Incremental {
        let item = RustItem::Comment(format!(
            "incremental {name}: state machine verified by SMT (MISC.1); no runtime scaffolding"
        ));
        code.push_str(&render_item_raw(&item));
        code.push('\n');
        return;
    }

    // Other blocks: generate as a module with documented constants/assertions
    let lower_name = name.to_lowercase();
    let mut items: Vec<RustItem> = Vec::new();

    for clause in body {
        let expr = expr_to_rust(&clause.body);
        match clause.kind {
            ClauseKind::Ensures | ClauseKind::Invariant => {
                items.push(RustItem::Fn(RustFn {
                    name: format!("check_{lower_name}"),
                    body: vec![RustStmt::Raw(format!("debug_assert!({expr});"))],
                    is_pub: true,
                    doc: vec![format!("Invariant: {expr}")],
                    ..RustFn::default()
                }));
            }
            ClauseKind::Requires => {
                items.push(RustItem::Const(RustConst {
                    name: "PRECONDITION".into(),
                    ty: RustType::Raw("&str".into()),
                    value: format!("\"{}\"", expr.replace('"', "\\\"")),
                    is_pub: true,
                    doc: vec![format!("Precondition: {expr}")],
                }));
            }
            ClauseKind::Rule => {
                items.push(RustItem::Fn(RustFn {
                    name: format!("check_rule_{lower_name}"),
                    body: vec![RustStmt::Raw(format!("debug_assert!({expr});"))],
                    is_pub: true,
                    doc: vec![format!("Rule: {expr}")],
                    ..RustFn::default()
                }));
            }
            ClauseKind::MustNot => {
                items.push(RustItem::Fn(RustFn {
                    name: format!("check_must_not_{lower_name}"),
                    body: vec![RustStmt::Raw(format!("debug_assert!(!({expr}));"))],
                    is_pub: true,
                    doc: vec![format!("MustNot: {expr}")],
                    ..RustFn::default()
                }));
            }
            ClauseKind::Ordering => {
                // Use // not /// so trailing metadata does not leave a doc
                // comment with no following item (invalid Rust).
                items.push(RustItem::Raw(format!("// Ordering: {expr}\n")));
                if let Some(ord) = resolve_ordering_variant(&clause.body) {
                    items.push(RustItem::Raw(format!(
                        "const ORDERING: std::sync::atomic::Ordering = std::sync::atomic::Ordering::{ord};\n"
                    )));
                }
            }
            ClauseKind::Effects => {
                items.push(RustItem::Raw(format!("// Effects: {expr}\n")));
            }
            ClauseKind::Modifies => {
                items.push(RustItem::Raw(format!("// Modifies: {expr}\n")));
            }
            ClauseKind::Input => {
                items.push(RustItem::Raw(format!("// Input: {expr}\n")));
            }
            ClauseKind::Output => {
                items.push(RustItem::Raw(format!("// Output: {expr}\n")));
            }
            ClauseKind::Errors => {
                items.push(RustItem::Raw(format!("// Errors: {expr}\n")));
            }
            ClauseKind::DataFlow => {
                items.push(RustItem::Raw(format!("// DataFlow: {expr}\n")));
            }
            ClauseKind::Decreases => {
                items.push(RustItem::Raw(format!("// Decreases: {expr}\n")));
            }
            ClauseKind::Other(ref kind_name) => {
                items.push(RustItem::Raw(format!("// {kind_name}: {expr}\n")));
            }
        }
    }

    let m = RustItem::Mod(RustMod {
        name: format!("block_{lower_name}"),
        items,
        is_pub: true,
        doc: vec![format!("{kind}: {name}")],
    });
    code.push_str(&render_item_raw(&m));
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
    use assura_ast::Spanned;

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
    fn block_incremental_is_smt_marker_only() {
        // #833 / PR #834: must not emit mod with trailing /// or free-fn `self`.
        let clauses = vec![
            mk_clause(
                ClauseKind::Invariant,
                Spanned::no_span(Expr::Field(
                    Box::new(Spanned::no_span(Expr::Ident("self".into()))),
                    "state".into(),
                )),
            ),
            mk_clause(
                ClauseKind::Other("on".into()),
                Spanned::no_span(Expr::Raw(vec![
                    "step".into(),
                    "requires".into(),
                    "true".into(),
                ])),
            ),
        ];
        let mut code = String::new();
        generate_block(
            &BlockKind::Incremental,
            "InflateDecoder",
            &clauses,
            &mut code,
        );
        assert!(
            code.contains("MISC.1") || code.contains("incremental InflateDecoder"),
            "expected SMT marker, got: {code}"
        );
        assert!(
            !code.contains("pub mod block_"),
            "incremental must not emit a mod: {code}"
        );
        assert!(
            !code.contains("debug_assert!(self"),
            "must not emit free-fn self assert: {code}"
        );
        // Generated snippet must parse as valid Rust when embedded in a file.
        let wrapped = format!("#![allow(dead_code)]\n{code}\n");
        syn::parse_file(&wrapped).unwrap_or_else(|e| panic!("invalid Rust: {e}\n{wrapped}"));
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
            codecs: vec![assura_ast::CodecEntry {
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
            codecs: vec![assura_ast::CodecEntry {
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
