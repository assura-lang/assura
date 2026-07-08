use super::*;
// This test file is included via #[path] from both ir.rs and ir_codegen.rs.
// Each inclusion makes one of these imports redundant (super::* covers it),
// but the other is needed. Suppress the warning rather than split the file.
#[allow(unused_imports)]
use crate::ir::*;
#[allow(unused_imports)]
use crate::ir_codegen::*;

// -------------------------------------------------------------------
// IrParser (text format) tests
// -------------------------------------------------------------------

#[test]
fn stub_ir_sidecar_text_includes_identity_load() {
    let text = stub_ir_sidecar_text("CopyBytes", &[(0, "Bytes".into())], "Bytes", 1, 1);
    assert!(text.contains("CopyBytes"));
    assert!(text.contains("$result = load $0"));
    assert!(text.contains("pre: true"));
    assert!(text.contains("1 requires, 1 ensures"));
    parse_ir_module(&text).unwrap();
}

#[test]
fn test_ir_parser_empty() {
    let mut p = IrParser::new();
    p.parse_text("").unwrap();
    assert_eq!(p.node_count(), 0);
}

#[test]
fn test_ir_parser_fn_decl() {
    let mut p = IrParser::new();
    p.parse_text("fn foo(x: Int)").unwrap();
    assert_eq!(p.node_count(), 1);
}

#[test]
fn test_ir_parser_var_decl() {
    let mut p = IrParser::new();
    p.parse_text("let x: Int").unwrap();
    assert_eq!(p.node_count(), 1);
}

#[test]
fn test_ir_parser_return_literal() {
    let mut p = IrParser::new();
    p.parse_text("return 42").unwrap();
    assert_eq!(p.node_count(), 1);
}

#[test]
fn test_ir_parser_comments_skipped() {
    let mut p = IrParser::new();
    p.parse_text("// comment\nfn bar()").unwrap();
    assert_eq!(p.node_count(), 1);
}

#[test]
fn test_ir_parser_serialize_binary() {
    let mut p = IrParser::new();
    p.parse_text("fn foo()\nlet x: Int\nreturn 0").unwrap();
    let bin = p.serialize_binary();
    // 4 bytes for count (3) + 3 nodes
    assert!(bin.len() > 4);
    // First 4 bytes = 3 (little-endian u32)
    let count = u32::from_le_bytes([bin[0], bin[1], bin[2], bin[3]]);
    assert_eq!(count, 3);
}

#[test]
fn test_ir_parser_default() {
    let p = IrParser::default();
    assert_eq!(p.node_count(), 0);
}

// -------------------------------------------------------------------
// IR module parser tests
// -------------------------------------------------------------------

#[test]
fn test_parse_ir_module_minimal() {
    let src = "module test {\n}";
    let m = parse_ir_module(src).unwrap();
    assert_eq!(m.name, "test");
    assert!(m.functions.is_empty());
}

#[test]
fn test_parse_ir_module_with_function() {
    let src = "\
module math {
  fn #0 : ($0: Int, $1: Int) -> Int ! pure
  {
$2 = arith add $0 $1 : Int
$result = load $2 : Int
  }
}";
    let m = parse_ir_module(src).unwrap();
    assert_eq!(m.name, "math");
    assert_eq!(m.functions.len(), 1);
    assert_eq!(m.functions[0].id, "#0");
    assert_eq!(m.functions[0].params.len(), 2);
    assert_eq!(m.functions[0].return_type, "Int");
    assert_eq!(m.functions[0].effects, "pure");
    assert_eq!(m.functions[0].body.len(), 2);
}

#[test]
fn test_parse_ir_module_with_pre_post() {
    let src = "\
module check {
  fn #0 : ($0: Int) -> Int ! pure
  pre: cmp ne $0 (const 0)
  post: cmp gt $result (const 0)
  {
$result = load $0 : Int
  }
}";
    let m = parse_ir_module(src).unwrap();
    m.functions[0].pre.as_ref().unwrap();
    m.functions[0].post.as_ref().unwrap();
}

#[test]
fn test_parse_ir_module_error_no_header() {
    let result = parse_ir_module("not a module");
    assert!(result.is_err());
}

#[test]
fn test_parse_ir_module_error_empty() {
    let result = parse_ir_module("");
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// IR instruction parsing tests
// -------------------------------------------------------------------

#[test]
fn test_parse_instr_const_int() {
    let instr = parse_ir_instr("$0 = const 42 : Int").unwrap();
    assert_eq!(instr.target, 0);
    assert_eq!(instr.ty, "Int");
    assert!(matches!(instr.expr, IrExprKind::Const(IrLiteral::Int(42))));
}

#[test]
fn test_parse_instr_load() {
    let instr = parse_ir_instr("$2 = load $1 : Int").unwrap();
    assert_eq!(instr.target, 2);
    assert!(matches!(instr.expr, IrExprKind::Load(1)));
}

#[test]
fn test_parse_instr_arith() {
    let instr = parse_ir_instr("$3 = arith mul $1 $2 : Int").unwrap();
    assert!(matches!(
        instr.expr,
        IrExprKind::Arith {
            op: IrArithOp::Mul,
            lhs: 1,
            rhs: 2
        }
    ));
}

#[test]
fn test_parse_instr_cmp() {
    let instr = parse_ir_instr("$3 = cmp lt $0 $1 : Bool").unwrap();
    assert!(matches!(
        instr.expr,
        IrExprKind::Cmp {
            op: IrCmpOp::Lt,
            lhs: 0,
            rhs: 1
        }
    ));
}

#[test]
fn test_parse_instr_call() {
    let instr = parse_ir_instr("$2 = call foo ($0, $1) : Int").unwrap();
    match instr.expr {
        IrExprKind::Call { func, args } => {
            assert_eq!(func, "foo");
            assert_eq!(args, vec![0, 1]);
        }
        other => panic!("expected Call, got {other:?}"),
    }
}

#[test]
fn test_parse_instr_field() {
    let instr = parse_ir_instr("$2 = field $0 .1 : Int").unwrap();
    assert!(matches!(
        instr.expr,
        IrExprKind::Field {
            slot: 0,
            index: 1,
            name: None,
        }
    ));
}

#[test]
fn test_parse_instr_cast() {
    let instr = parse_ir_instr("$1 = cast $0 as Float : Float").unwrap();
    assert!(matches!(instr.expr, IrExprKind::Cast { slot: 0, .. }));
}

#[test]
fn test_parse_instr_result_slot() {
    let instr = parse_ir_instr("$result = load $0 : Int").unwrap();
    assert_eq!(instr.target, usize::MAX);
}

#[test]
fn test_parse_instr_if() {
    let instr = parse_ir_instr("$3 = if $0 then #1 else #2 : Int").unwrap();
    assert!(matches!(
        instr.expr,
        IrExprKind::If {
            cond: 0,
            then_block: 1,
            else_block: 2
        }
    ));
}

#[test]
fn test_parse_instr_transition() {
    let instr = parse_ir_instr("$1 = transition $0 to Active : Unit").unwrap();
    match instr.expr {
        IrExprKind::Transition { slot: 0, ref state } => assert_eq!(state, "Active"),
        other => panic!("expected Transition, got {other:?}"),
    }
}

// -------------------------------------------------------------------
// IR literal parsing tests
// -------------------------------------------------------------------

#[test]
fn test_parse_literal_int() {
    assert_eq!(parse_ir_literal("42").unwrap(), IrLiteral::Int(42));
}

#[test]
fn test_parse_literal_float() {
    assert_eq!(parse_ir_literal("3.14").unwrap(), IrLiteral::Float(3.14));
}

#[test]
fn test_parse_literal_bool() {
    assert_eq!(parse_ir_literal("true").unwrap(), IrLiteral::Bool(true));
    assert_eq!(parse_ir_literal("false").unwrap(), IrLiteral::Bool(false));
}

#[test]
fn test_parse_literal_string() {
    assert_eq!(
        parse_ir_literal("\"hello\"").unwrap(),
        IrLiteral::Str("hello".into())
    );
}

// -------------------------------------------------------------------
// IR type mapping tests
// -------------------------------------------------------------------

#[test]
fn test_ir_type_to_rust_mapping() {
    assert_eq!(ir_type_to_rust("Int"), "i64");
    assert_eq!(ir_type_to_rust("Nat"), "u64");
    assert_eq!(ir_type_to_rust("Float"), "f64");
    assert_eq!(ir_type_to_rust("Bool"), "bool");
    assert_eq!(ir_type_to_rust("String"), "String");
    assert_eq!(ir_type_to_rust("Bytes"), "Vec<u8>");
    assert_eq!(ir_type_to_rust("Unit"), "()");
    assert_eq!(ir_type_to_rust(""), "_");
    assert_eq!(ir_type_to_rust("CustomType"), "CustomType");
}

// -------------------------------------------------------------------
// ir_type_default tests
// -------------------------------------------------------------------

#[test]
fn ir_type_default_covers_all_base_types() {
    assert_eq!(ir_type_default("Int"), "0_i64");
    assert_eq!(ir_type_default("Nat"), "0_u64");
    assert_eq!(ir_type_default("Float"), "0.0_f64");
    assert_eq!(ir_type_default("Bool"), "false");
    assert_eq!(ir_type_default("String"), "String::new()");
    assert_eq!(ir_type_default("Bytes"), "Vec::new()");
    assert_eq!(ir_type_default("Unit"), "()");
    assert_eq!(ir_type_default(""), "()");
}

#[test]
fn ir_type_default_unknown_uses_default_trait() {
    assert_eq!(ir_type_default("CustomType"), "Default::default()");
    assert_eq!(ir_type_default("List<Int>"), "Default::default()");
}

// -------------------------------------------------------------------
// IR to Rust codegen tests
// -------------------------------------------------------------------

#[test]
fn test_ir_to_rust_generates_function() {
    let module = IrModule {
        name: "test".into(),
        functions: vec![IrFunction {
            id: "#0".into(),
            params: vec![
                IrSlotDecl {
                    slot: 0,
                    ty: "Int".into(),
                },
                IrSlotDecl {
                    slot: 1,
                    ty: "Int".into(),
                },
            ],
            return_type: "Int".into(),
            effects: "pure".into(),
            pre: None,
            post: None,
            body: vec![
                IrInstr {
                    target: 2,
                    expr: IrExprKind::Arith {
                        op: IrArithOp::Add,
                        lhs: 0,
                        rhs: 1,
                    },
                    ty: "Int".into(),
                },
                IrInstr {
                    target: usize::MAX,
                    expr: IrExprKind::Load(2),
                    ty: "Int".into(),
                },
            ],
        }],
    };
    let code = ir_to_rust(&module);
    assert!(code.contains("fn ir_0("));
    assert!(code.contains("slot_0: i64"));
    assert!(code.contains("slot_1: i64"));
    assert!(code.contains("-> i64"));
    assert!(code.contains("(slot_0 + slot_1)"));
    assert!(code.contains("__result"));
}

#[test]
fn test_ir_to_rust_with_pre_post() {
    let module = IrModule {
        name: "guarded".into(),
        functions: vec![IrFunction {
            id: "#0".into(),
            params: vec![IrSlotDecl {
                slot: 0,
                ty: "Int".into(),
            }],
            return_type: "Int".into(),
            effects: "pure".into(),
            pre: Some(IrPred::Cmp {
                op: IrCmpOp::Gt,
                lhs: IrPredArg::Slot(0),
                rhs: IrPredArg::Lit(IrLiteral::Int(0)),
            }),
            post: Some(IrPred::True),
            body: vec![IrInstr {
                target: usize::MAX,
                expr: IrExprKind::Load(0),
                ty: "Int".into(),
            }],
        }],
    };
    let code = ir_to_rust(&module);
    assert!(code.contains("debug_assert!"));
    assert!(code.contains("slot_0"));
}

// -------------------------------------------------------------------
// Predicate parsing tests
// -------------------------------------------------------------------

#[test]
fn test_parse_pred_true() {
    assert_eq!(parse_ir_pred_str("true"), Some(IrPred::True));
}

#[test]
fn test_parse_pred_false() {
    assert_eq!(parse_ir_pred_str("false"), Some(IrPred::False));
}

#[test]
fn test_parse_pred_empty() {
    assert_eq!(parse_ir_pred_str(""), None);
}

#[test]
fn test_parse_pred_cmp() {
    let pred = parse_ir_pred_str("cmp eq $0 $1").unwrap();
    assert!(matches!(
        pred,
        IrPred::Cmp {
            op: IrCmpOp::Eq,
            ..
        }
    ));
}

#[test]
fn test_parse_pred_not() {
    let pred = parse_ir_pred_str("not true").unwrap();
    assert!(matches!(pred, IrPred::Not(_)));
}

// -------------------------------------------------------------------
// Arith/Cmp op parsing tests
// -------------------------------------------------------------------

#[test]
fn test_parse_arith_ops() {
    assert_eq!(parse_arith_op("add").unwrap(), IrArithOp::Add);
    assert_eq!(parse_arith_op("sub").unwrap(), IrArithOp::Sub);
    assert_eq!(parse_arith_op("mul").unwrap(), IrArithOp::Mul);
    assert_eq!(parse_arith_op("div").unwrap(), IrArithOp::Div);
    assert_eq!(parse_arith_op("mod").unwrap(), IrArithOp::Mod);
    assert!(parse_arith_op("bad").is_err());
}

#[test]
fn test_parse_cmp_ops() {
    assert_eq!(parse_cmp_op("eq").unwrap(), IrCmpOp::Eq);
    assert_eq!(parse_cmp_op("ne").unwrap(), IrCmpOp::Ne);
    assert_eq!(parse_cmp_op("lt").unwrap(), IrCmpOp::Lt);
    assert_eq!(parse_cmp_op("le").unwrap(), IrCmpOp::Le);
    assert_eq!(parse_cmp_op("gt").unwrap(), IrCmpOp::Gt);
    assert_eq!(parse_cmp_op("ge").unwrap(), IrCmpOp::Ge);
    assert!(parse_cmp_op("bad").is_err());
}

// -------------------------------------------------------------------
// ir_function_body_to_rust tests
// -------------------------------------------------------------------

#[test]
fn test_ir_function_body_generates_instructions() {
    let func = IrFunction {
        id: "#0".into(),
        params: vec![
            IrSlotDecl {
                slot: 0,
                ty: "Int".into(),
            },
            IrSlotDecl {
                slot: 1,
                ty: "Int".into(),
            },
        ],
        return_type: "Int".into(),
        effects: "pure".into(),
        pre: None,
        post: None,
        body: vec![
            IrInstr {
                target: 2,
                expr: IrExprKind::Arith {
                    op: IrArithOp::Add,
                    lhs: 0,
                    rhs: 1,
                },
                ty: "Int".into(),
            },
            IrInstr {
                target: usize::MAX,
                expr: IrExprKind::Load(2),
                ty: "Int".into(),
            },
        ],
    };
    let body = ir_function_body_to_rust(&func);
    assert!(body.contains("(slot_0 + slot_1)"), "body: {body}");
    assert!(body.contains("__result"), "body: {body}");
    // No function signature
    assert!(
        !body.contains("fn "),
        "body should not contain fn signature"
    );
}

#[test]
fn test_ir_function_body_with_pre_post() {
    let func = IrFunction {
        id: "#0".into(),
        params: vec![IrSlotDecl {
            slot: 0,
            ty: "Int".into(),
        }],
        return_type: "Int".into(),
        effects: "pure".into(),
        pre: Some(IrPred::Cmp {
            op: IrCmpOp::Ge,
            lhs: IrPredArg::Slot(0),
            rhs: IrPredArg::Lit(IrLiteral::Int(0)),
        }),
        post: Some(IrPred::Cmp {
            op: IrCmpOp::Ge,
            lhs: IrPredArg::SlotResult,
            rhs: IrPredArg::Lit(IrLiteral::Int(0)),
        }),
        body: vec![IrInstr {
            target: usize::MAX,
            expr: IrExprKind::Load(0),
            ty: "Int".into(),
        }],
    };
    let body = ir_function_body_to_rust(&func);
    assert!(body.contains("debug_assert!"), "body: {body}");
    assert!(body.contains("IR pre-condition"), "body: {body}");
    assert!(body.contains("IR post-condition"), "body: {body}");
}

#[test]
fn test_ir_module_to_body_map() {
    let module = IrModule {
        name: "AddOne".into(),
        functions: vec![IrFunction {
            id: "#0".into(),
            params: vec![IrSlotDecl {
                slot: 0,
                ty: "Int".into(),
            }],
            return_type: "Int".into(),
            effects: "pure".into(),
            pre: None,
            post: None,
            body: vec![IrInstr {
                target: 1,
                expr: IrExprKind::Arith {
                    op: IrArithOp::Add,
                    lhs: 0,
                    rhs: 0,
                },
                ty: "Int".into(),
            }],
        }],
    };
    let map = ir_module_to_body_map(&module);
    assert!(
        map.contains_key("AddOne"),
        "map keys: {:?}",
        map.keys().collect::<Vec<_>>()
    );
    let body = &map["AddOne"];
    assert!(body.contains("slot_0 + slot_0"), "body: {body}");
}

// -------------------------------------------------------------------
// Match and Loop IR instruction tests
// -------------------------------------------------------------------

#[test]
fn test_parse_ir_match_instruction() {
    let src = "\
module matcher {
  fn #0 : ($0: Int) -> Int ! pure
  {
$1 = match $0 { 0 => #0, 1 => #1, _ => #2 } : Int
$result = load $1 : Int
  }
}";
    let m = parse_ir_module(src).unwrap();
    assert_eq!(m.functions.len(), 1);
    assert_eq!(m.functions[0].body.len(), 2);
    match &m.functions[0].body[0].expr {
        IrExprKind::Match { scrutinee, arms } => {
            assert_eq!(*scrutinee, 0);
            assert_eq!(arms.len(), 3);
            assert_eq!(arms[0], (IrMatchPattern::Int(0), 0));
            assert_eq!(arms[1], (IrMatchPattern::Int(1), 1));
            assert_eq!(arms[2], (IrMatchPattern::Wildcard, 2));
        }
        other => panic!("expected Match, got: {other:?}"),
    }
}

#[test]
fn test_parse_ir_loop_instruction() {
    let src = "\
module looper {
  fn #0 : ($0: Int) -> Int ! pure
  {
$1 = const 0 : Int
$2 = loop #0 $0 : Int
$result = load $1 : Int
  }
}";
    let m = parse_ir_module(src).unwrap();
    assert_eq!(m.functions.len(), 1);
    match &m.functions[0].body[1].expr {
        IrExprKind::Loop { body_block, cond } => {
            assert_eq!(*body_block, 0);
            assert_eq!(*cond, 0);
        }
        other => panic!("expected Loop, got: {other:?}"),
    }
}

#[test]
fn test_ir_match_to_rust() {
    let expr = IrExprKind::Match {
        scrutinee: 0,
        arms: vec![(IrMatchPattern::Int(1), 0), (IrMatchPattern::Wildcard, 1)],
    };
    let rust = ir_expr_to_rust(&expr);
    assert!(rust.contains("match slot_0"), "got: {rust}");
    assert!(rust.contains("1 => block_0()"), "got: {rust}");
    assert!(rust.contains("_ => block_1()"), "got: {rust}");
}

#[test]
fn test_ir_builtin_calls_to_rust_methods() {
    // abs/min/max must lower to methods, not free functions (generated crates
    // have no abs/min/max in scope).
    let abs = ir_expr_to_rust(&IrExprKind::Call {
        func: "abs".into(),
        args: vec![0],
    });
    assert_eq!(abs, "slot_0.abs()");

    let min = ir_expr_to_rust(&IrExprKind::Call {
        func: "min".into(),
        args: vec![0, 1],
    });
    assert_eq!(min, "slot_0.min(slot_1)");

    let max = ir_expr_to_rust(&IrExprKind::Call {
        func: "max".into(),
        args: vec![2, 3],
    });
    assert_eq!(max, "slot_2.max(slot_3)");

    let length = ir_expr_to_rust(&IrExprKind::Call {
        func: "length".into(),
        args: vec![0],
    });
    assert_eq!(length, "slot_0.len() as u64");

    // Unknown callees stay free-function form (sibling helpers / user fns).
    let other = ir_expr_to_rust(&IrExprKind::Call {
        func: "double".into(),
        args: vec![0],
    });
    assert_eq!(other, "double(slot_0)");
}

#[test]
fn test_ir_loop_to_rust() {
    let expr = IrExprKind::Loop {
        body_block: 0,
        cond: 1,
    };
    let rust = ir_expr_to_rust(&expr);
    assert!(rust.contains("loop"), "got: {rust}");
    assert!(rust.contains("block_0()"), "got: {rust}");
    assert!(rust.contains("slot_1"), "got: {rust}");
}

#[test]
fn test_match_referenced_slots() {
    let expr = IrExprKind::Match {
        scrutinee: 3,
        arms: vec![(IrMatchPattern::Wildcard, 0)],
    };
    assert_eq!(referenced_slots(&expr), vec![3]);
}

#[test]
fn test_loop_referenced_slots() {
    let expr = IrExprKind::Loop {
        body_block: 0,
        cond: 5,
    };
    assert_eq!(referenced_slots(&expr), vec![5]);
}

// --- count_input_params / validate_ir_against_contract tests (#706) ---

#[test]
fn count_input_params_multi_param_cast_block() {
    // Simulate the AST shape for input(val: Int, lo: Int, hi: Int):
    // Block of Cast expressions (name as Type)
    use assura_ast::{Expr, Spanned};
    let body = Spanned::no_span(Expr::Block(vec![
        Spanned::no_span(Expr::Cast {
            expr: Box::new(Spanned::no_span(Expr::Ident("val".into()))),
            ty: "Int".into(),
        }),
        Spanned::no_span(Expr::Cast {
            expr: Box::new(Spanned::no_span(Expr::Ident("lo".into()))),
            ty: "Int".into(),
        }),
        Spanned::no_span(Expr::Cast {
            expr: Box::new(Spanned::no_span(Expr::Ident("hi".into()))),
            ty: "Int".into(),
        }),
    ]));
    assert_eq!(
        count_input_params(&body),
        3,
        "Block of 3 Cast exprs should count as 3 params"
    );
}

#[test]
fn count_input_params_single_param_cast() {
    use assura_ast::{Expr, Spanned};
    let body = Spanned::no_span(Expr::Cast {
        expr: Box::new(Spanned::no_span(Expr::Ident("x".into()))),
        ty: "Int".into(),
    });
    assert_eq!(
        count_input_params(&body),
        1,
        "Single Cast expr should count as 1 param"
    );
}

#[test]
fn count_input_params_tuple() {
    use assura_ast::{Expr, Spanned};
    let body = Spanned::no_span(Expr::Tuple(vec![
        Spanned::no_span(Expr::Ident("a".into())),
        Spanned::no_span(Expr::Ident("b".into())),
    ]));
    assert_eq!(
        count_input_params(&body),
        2,
        "Tuple of 2 idents should count as 2 params"
    );
}

// --- Clamp conditional IR end-to-end (#704) ---

#[test]
fn clamp_conditional_ir_parses_and_codegen() {
    // A Clamp contract: result = if val < lo then lo, else if val > hi then hi, else val
    // Expressed as nested if with block references.
    let source = r#"
module Clamp {
  fn #0 : ($0: Int, $1: Int, $2: Int) -> Int ! pure
  {
    $3 = cmp lt $0 $1 : Bool
    $4 = if $3 then #1 else #2 : Int
    $result = load $4 : Int
  }
  fn #1 : () -> Int ! pure
  {
    $result = load $1 : Int
  }
  fn #2 : () -> Int ! pure
  {
    $5 = cmp gt $0 $2 : Bool
    $6 = if $5 then #3 else #4 : Int
    $result = load $6 : Int
  }
  fn #3 : () -> Int ! pure
  {
    $result = load $2 : Int
  }
  fn #4 : () -> Int ! pure
  {
    $result = load $0 : Int
  }
}
"#;
    let module = parse_ir_module(source).expect("Clamp IR should parse");
    assert_eq!(module.name, "Clamp");
    assert_eq!(module.functions.len(), 5, "5 functions: #0 + 4 blocks");

    // The main function's body should have cmp, if, load
    let main_fn = &module.functions[0];
    assert!(matches!(main_fn.body[0].expr, IrExprKind::Cmp { .. }));
    assert!(matches!(main_fn.body[1].expr, IrExprKind::If { .. }));

    // Rust codegen should produce if-else blocks
    let rust = ir_to_rust(&module);
    assert!(
        rust.contains("if slot_3"),
        "Rust codegen should produce if condition: {rust}"
    );
    assert!(
        rust.contains("block_1()"),
        "Rust codegen should reference then block: {rust}"
    );
    assert!(
        rust.contains("block_2()"),
        "Rust codegen should reference else block: {rust}"
    );
}

#[test]
fn embedded_body_defines_block_closures() {
    use crate::ir::parse_ir_module;
    use crate::ir_codegen::ir_module_to_embedded_body;
    let src = r#"
module AbsX {
fn #0 : ($0: Int) -> Int ! pure
{
    $1 = const 0 : Int
    $2 = cmp ge $0 $1 : Bool
    $3 = if $2 then #1 else #2 : Int
    $result = load $3 : Int
}
  fn #1 : ($0: Int) -> Int ! pure
{
    $result = load $0 : Int
}
  fn #2 : ($0: Int) -> Int ! pure
{
    $1 = const 0 : Int
    $2 = arith sub $1 $0 : Int
    $result = load $2 : Int
}
}
"#;
    let module = parse_ir_module(src).expect("parse");
    let rust = ir_module_to_embedded_body(&module);
    assert!(rust.contains("let block_1 = ||"), "got:\n{rust}");
    assert!(rust.contains("let block_2 = ||"), "got:\n{rust}");
    assert!(rust.contains("block_1()"), "got:\n{rust}");
    assert!(rust.contains("block_2()"), "got:\n{rust}");
}
