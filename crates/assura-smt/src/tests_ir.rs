use super::*;

#[test]
fn ir_parse_safe_division() {
    let source = r#"
module safe_division {
  fn #0 : ($0: Int @omega, $1: Int @omega) -> Int ! pure
    pre: cmp ne $1 (const 0)
    post: cmp eq (arith add (arith mul $result $1) (arith mod $0 $1)) $0
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
    let module = parse_ir_module(source).expect("parse should succeed");
    assert_eq!(module.name, "safe_division");
    assert_eq!(module.functions.len(), 1);
    let func = &module.functions[0];
    assert_eq!(func.id, "#0");
    assert_eq!(func.params.len(), 2);
    assert_eq!(func.params[0].slot, 0);
    assert_eq!(func.params[0].ty, "Int");
    assert_eq!(func.params[1].slot, 1);
    assert_eq!(func.return_type, "Int");
    assert_eq!(func.effects, "pure");
    func.pre.as_ref().unwrap();
    func.post.as_ref().unwrap();
    assert_eq!(func.body.len(), 2);
    // First instruction: $2 = arith div $0 $1 : Int
    assert_eq!(func.body[0].target, 2);
    assert_eq!(func.body[0].ty, "Int");
    assert!(matches!(
        func.body[0].expr,
        IrExprKind::Arith {
            op: IrArithOp::Div,
            lhs: 0,
            rhs: 1,
        }
    ));
    // Second instruction: $result = load $2 : Int
    assert_eq!(func.body[1].target, usize::MAX);
    assert!(matches!(func.body[1].expr, IrExprKind::Load(2)));
}

#[test]
fn ir_parse_const_and_call() {
    let source = r#"
module test {
  fn #0 : ($0: Int) -> Bool ! pure
  {
    $1 = const 42 : Int
    $2 = call is_valid ($0, $1) : Bool
    $result = load $2 : Bool
  }
}
"#;
    let module = parse_ir_module(source).expect("parse should succeed");
    assert_eq!(module.functions.len(), 1);
    let body = &module.functions[0].body;
    assert_eq!(body.len(), 3);
    assert!(matches!(
        &body[0].expr,
        IrExprKind::Const(IrLiteral::Int(42))
    ));
    assert!(matches!(
        &body[1].expr,
        IrExprKind::Call { func, args } if func == "is_valid" && args == &[0, 1]
    ));
}

#[test]
fn ir_parse_field_and_construct() {
    let source = r#"
module test {
  fn #0 : ($0: Point) -> Point ! pure
  {
    $1 = field $0 .0 : Int
    $2 = field $0 .1 : Int
    $3 = construct Point { .0 = $2, .1 = $1 } : Point
    $result = load $3 : Point
  }
}
"#;
    let module = parse_ir_module(source).expect("parse should succeed");
    let body = &module.functions[0].body;
    assert!(matches!(
        &body[0].expr,
        IrExprKind::Field { slot: 0, index: 0 }
    ));
    assert!(matches!(
        &body[2].expr,
        IrExprKind::Construct { type_id, fields }
        if type_id == "Point" && fields == &[(0, 2), (1, 1)]
    ));
}

#[test]
fn ir_parse_cmp_and_cast() {
    let source = r#"
module test {
  fn #0 : ($0: Int, $1: Int) -> Bool ! pure
  {
    $2 = cmp lt $0 $1 : Bool
    $3 = cast $0 as Float : Float
    $result = load $2 : Bool
  }
}
"#;
    let module = parse_ir_module(source).expect("parse should succeed");
    let body = &module.functions[0].body;
    assert!(matches!(
        &body[0].expr,
        IrExprKind::Cmp {
            op: IrCmpOp::Lt,
            lhs: 0,
            rhs: 1,
        }
    ));
    assert!(matches!(&body[1].expr, IrExprKind::Cast { slot: 0, .. }));
}

#[test]
fn ir_parse_if_and_transition() {
    let source = r#"
module test {
  fn #0 : ($0: Bool, $1: Connection) -> Unit ! io
  {
    $2 = if $0 then #0 else #1 : Unit
    $3 = transition $1 to Connected : Connection
    $result = load $3 : Connection
  }
}
"#;
    let module = parse_ir_module(source).expect("parse should succeed");
    let body = &module.functions[0].body;
    assert!(matches!(
        &body[0].expr,
        IrExprKind::If {
            cond: 0,
            then_block: 0,
            else_block: 1,
        }
    ));
    assert!(matches!(
        &body[1].expr,
        IrExprKind::Transition { slot: 1, .. }
    ));
}

#[test]
fn ir_parse_empty_module() {
    let source = "module empty {\n}\n";
    let module = parse_ir_module(source).expect("parse should succeed");
    assert_eq!(module.name, "empty");
    assert!(module.functions.is_empty());
}

#[test]
fn ir_parse_error_no_module() {
    let source = "fn #0 : () -> Unit ! pure {}";
    let result = parse_ir_module(source);
    assert!(result.is_err());
}

#[test]
fn ir_to_rust_safe_division() {
    let source = r#"
module safe_division {
  fn #0 : ($0: Int, $1: Int) -> Int ! pure
    pre: cmp ne $1 (const 0)
  {
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
  }
}
"#;
    let module = parse_ir_module(source).unwrap();
    let rust = ir_to_rust(&module);
    assert!(rust.contains("fn ir_0("));
    assert!(rust.contains("slot_0: i64"));
    assert!(rust.contains("slot_1: i64"));
    assert!(rust.contains("-> i64"));
    assert!(rust.contains("debug_assert!"));
    assert!(rust.contains("(slot_0 / slot_1)"));
    assert!(rust.contains("__result"));
}

#[test]
fn ir_validate_slot_gap() {
    let module = IrModule {
        name: "test".into(),
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
                target: 5, // gap: skips $1-$4
                expr: IrExprKind::Load(0),
                ty: "Int".into(),
            }],
        }],
    };
    let contract = assura_ast::ContractDecl {
        name: "Test".into(),
        type_params: vec![],
        clauses: vec![],
        fn_params: vec![],
    };
    let validation = validate_ir_against_contract(&module, &contract);
    assert!(!validation.valid);
    assert!(validation.errors[0].contains("skips slot"));
}

#[test]
fn ir_arith_ops() {
    for (s, expected) in [
        ("add", IrArithOp::Add),
        ("sub", IrArithOp::Sub),
        ("mul", IrArithOp::Mul),
        ("div", IrArithOp::Div),
        ("mod", IrArithOp::Mod),
    ] {
        assert_eq!(parse_arith_op(s).unwrap(), expected);
    }
    assert!(parse_arith_op("xor").is_err());
}

#[test]
fn ir_cmp_ops() {
    for (s, expected) in [
        ("eq", IrCmpOp::Eq),
        ("ne", IrCmpOp::Ne),
        ("lt", IrCmpOp::Lt),
        ("le", IrCmpOp::Le),
        ("gt", IrCmpOp::Gt),
        ("ge", IrCmpOp::Ge),
    ] {
        assert_eq!(parse_cmp_op(s).unwrap(), expected);
    }
    assert!(parse_cmp_op("in").is_err());
}

#[test]
fn ir_pred_true_false() {
    assert_eq!(parse_ir_pred_str("true"), Some(IrPred::True));
    assert_eq!(parse_ir_pred_str("false"), Some(IrPred::False));
    assert_eq!(parse_ir_pred_str(""), None);
}

#[test]
fn ir_pred_not() {
    let pred = parse_ir_pred_str("not true");
    assert!(matches!(pred, Some(IrPred::Not(_))));
}

#[test]
fn ir_type_to_rust_mapping() {
    assert_eq!(ir_type_to_rust("Int"), "i64");
    assert_eq!(ir_type_to_rust("Nat"), "u64");
    assert_eq!(ir_type_to_rust("Float"), "f64");
    assert_eq!(ir_type_to_rust("Bool"), "bool");
    assert_eq!(ir_type_to_rust("String"), "String");
    assert_eq!(ir_type_to_rust("Unit"), "()");
    assert_eq!(ir_type_to_rust("CustomType"), "CustomType");
}
