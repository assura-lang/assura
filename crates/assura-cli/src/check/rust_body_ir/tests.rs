//! Unit tests for check-rust body IR encode.
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

fn pu16() -> Vec<ParamInfo> {
    vec![ParamInfo {
        name: "x".into(),
        ty: "u16".into(),
    }]
}

fn pu32() -> Vec<ParamInfo> {
    vec![ParamInfo {
        name: "x".into(),
        ty: "u32".into(),
    }]
}

fn pu64() -> Vec<ParamInfo> {
    vec![ParamInfo {
        name: "x".into(),
        ty: "u64".into(),
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
    let ir = try_ir_from_rust_body("C", &px(), Some("i64"), "x . clamp (0 , 10)").expect("clamp");
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
    assert!(try_ir_from_rust_body("B", &px(), Some("i64"), "match x { n => n, _ => 0 }").is_none());
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
    let ir = try_ir_from_rust_body("N", &px(), Some("i64"), "x.next_multiple_of(4)").expect("nmo");
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
    // u8 divisor includes 0 → BNM
    let u8d = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        },
        ParamInfo {
            name: "d".into(),
            ty: "u8".into(),
        },
    ];
    assert!(
        try_ir_from_rust_body("Z", &u8d, Some("i64"), "x.rem_euclid(d)").is_none(),
        "u8 divisor includes 0"
    );
    // NonZeroU8 divisor: lo >= 1 → encodes
    let nzd = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i32".into(),
        },
        ParamInfo {
            name: "d".into(),
            ty: "NonZeroU8".into(),
        },
    ];
    let vre = try_ir_from_rust_body("V", &nzd, Some("i32"), "x.rem_euclid(d)").expect("var re");
    assert!(vre.contains("arith mod") && vre.contains("load"), "{vre}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&vre, "V").expect("parse");
    let vde = try_ir_from_rust_body("Vd", &nzd, Some("i32"), "x.div_euclid(d)").expect("var de");
    assert!(vde.contains("arith div"), "{vde}");
    let vnmo =
        try_ir_from_rust_body("Vn", &nzd, Some("i32"), "x.next_multiple_of(d)").expect("var nmo");
    assert!(
        vnmo.contains("arith mod") && vnmo.contains("cmp eq"),
        "{vnmo}"
    );
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
    // NonZeroU8 divisor
    let nzd = vec![
        ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        },
        ParamInfo {
            name: "d".into(),
            ty: "NonZeroU8".into(),
        },
    ];
    let v = try_ir_from_rust_body("V", &nzd, Some("u8"), "x.div_ceil(d)").expect("var div_ceil");
    assert!(v.contains("arith div") && v.contains("load"), "{v}");
    // signed i64 path stays BNM (may be negative)
    assert!(try_ir_from_rust_body("S", &px(), Some("i64"), "x.div_ceil(3)").is_none());
    // const non-neg lit
    let c = try_ir_from_rust_body("C", &px(), Some("u32"), "10u32.div_ceil(3)").expect("const");
    assert!(c.contains("const 4") || c.contains("arith div"), "{c}");
    let re = try_ir_from_rust_body("R", &pu8, Some("u8"), "x.rem_euclid(3)").expect("rem_euclid");
    assert!(re.contains("arith mod") && re.contains("const 3"), "{re}");
    let de = try_ir_from_rust_body("De", &pu8, Some("u8"), "x.div_euclid(3)").expect("div_euclid");
    assert!(de.contains("arith div") && de.contains("const 3"), "{de}");
    let nmo = try_ir_from_rust_body("N", &pu8, Some("u8"), "x.next_multiple_of(4)").expect("nmo");
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
fn variable_bitwise_not_encodes() {
    let pu8 = vec![ParamInfo {
        name: "x".into(),
        ty: "u8".into(),
    }];
    let ir = try_ir_from_rust_body("N", &pu8, Some("u8"), "!x").expect("!u8");
    // ones = modulus - 1 (const 256 then sub 1), not a bare const 255
    assert!(ir.contains("arith sub") && ir.contains("const 256"), "{ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "N").expect("parse");
    let pi8 = vec![ParamInfo {
        name: "x".into(),
        ty: "i8".into(),
    }];
    let s = try_ir_from_rust_body("S", &pi8, Some("i8"), "!x").expect("!i8");
    assert!(s.contains("cmp gt") && s.contains("arith sub"), "{s}");
    // bool stays logical not (eq 0)
    let pb = vec![ParamInfo {
        name: "b".into(),
        ty: "bool".into(),
    }];
    let b = try_ir_from_rust_body("B", &pb, Some("bool"), "!b").expect("!bool");
    assert!(b.contains("cmp eq"), "{b}");
    // i64/u64 !x encodes via synthetic 2^64 ones-complement (#1186)
    let i64n = try_ir_from_rust_body("I", &px(), Some("i64"), "!x").expect("!i64");
    assert!(
        i64n.contains("arith sub") && i64n.contains("4294967296"),
        "{i64n}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&i64n, "I").expect("parse !i64");
    let pu64 = vec![ParamInfo {
        name: "x".into(),
        ty: "u64".into(),
    }];
    let u64n = try_ir_from_rust_body("U", &pu64, Some("u64"), "!x").expect("!u64");
    assert!(u64n.contains("arith sub"), "{u64n}");
}

#[test]
fn const_bitops_fold() {
    let ir = try_ir_from_rust_body("A", &px(), Some("u32"), "12u32 & 10u32").expect("and");
    assert!(ir.contains("const 8 : Int"), "{ir}"); // 0b1100 & 0b1010 = 0b1000
    let or = try_ir_from_rust_body("O", &px(), Some("u32"), "12u32 | 3u32").expect("or");
    assert!(or.contains("const 15 : Int"), "{or}");
    let sh = try_ir_from_rust_body("S", &px(), Some("u32"), "3u32 << 2").expect("shl");
    assert!(sh.contains("const 12 : Int"), "{sh}");
    // both-variable unsigned bitops encode (≤32)
    let pxy = vec![
        ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        },
        ParamInfo {
            name: "y".into(),
            ty: "u8".into(),
        },
    ];
    let vv = try_ir_from_rust_body("V", &pxy, Some("u8"), "x & y").expect("x&y");
    assert!(vv.contains("arith mul") && vv.contains("arith mod"), "{vv}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&vv, "V").expect("parse x&y");
    // both-variable signed encodes via bit-pattern map (#1171)
    let pxyi = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i8".into(),
        },
        ParamInfo {
            name: "y".into(),
            ty: "i8".into(),
        },
    ];
    let si = try_ir_from_rust_body("Si", &pxyi, Some("i8"), "x & y").expect("i8 x&y");
    assert!(si.contains("cmp gt") && si.contains("arith mul"), "{si}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&si, "Si").expect("parse i8");
    // i64 both-var still BNM (bits>32)
    let p64 = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        },
        ParamInfo {
            name: "y".into(),
            ty: "i64".into(),
        },
    ];
    assert!(try_ir_from_rust_body("I", &p64, Some("i64"), "x & y").is_none());
}

#[test]
fn both_variable_bitops_encodes() {
    let pxy = vec![
        ParamInfo {
            name: "x".into(),
            ty: "u8".into(),
        },
        ParamInfo {
            name: "y".into(),
            ty: "u8".into(),
        },
    ];
    let or = try_ir_from_rust_body("O", &pxy, Some("u8"), "x | y").expect("or");
    assert!(or.contains("arith sub"), "{or}"); // or uses a+b-ab
    let xor = try_ir_from_rust_body("X", &pxy, Some("u8"), "x ^ y").expect("xor");
    assert!(xor.contains("arith mul"), "{xor}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&xor, "X").expect("parse");
    // signed both-var or/xor
    let pxyi = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i8".into(),
        },
        ParamInfo {
            name: "y".into(),
            ty: "i8".into(),
        },
    ];
    let sor = try_ir_from_rust_body("So", &pxyi, Some("i8"), "x | y").expect("i8 or");
    assert!(sor.contains("cmp gt"), "{sor}");
    let sxor = try_ir_from_rust_body("Sx", &pxyi, Some("i8"), "x ^ y").expect("i8 xor");
    assert!(sxor.contains("arith mul"), "{sxor}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&sxor, "Sx").expect("parse");
    // i16 / i32 both-var signed (#1171 acceptance surface)
    for (ty, name) in [("i16", "Si16"), ("i32", "Si32")] {
        let p = vec![
            ParamInfo {
                name: "x".into(),
                ty: ty.into(),
            },
            ParamInfo {
                name: "y".into(),
                ty: ty.into(),
            },
        ];
        let ir = try_ir_from_rust_body(name, &p, Some(ty), "x & y").expect(ty);
        assert!(
            ir.contains("cmp gt") && ir.contains("arith mul"),
            "{ty}: {ir}"
        );
        assura_smt::LoadedVerifyExtras::from_ir_text(&ir, name).expect(ty);
    }
}

#[test]
fn variable_bitop_const_mask_encodes() {
    let pu8 = vec![ParamInfo {
        name: "x".into(),
        ty: "u8".into(),
    }];
    let and = try_ir_from_rust_body("A", &pu8, Some("u8"), "x & 1").expect("and");
    assert!(
        and.contains("arith mod") && and.contains("arith mul"),
        "{and}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&and, "A").expect("parse and");
    let or = try_ir_from_rust_body("O", &pu8, Some("u8"), "x | 0xF0").expect("or");
    assert!(or.contains("arith add"), "{or}");
    let xor = try_ir_from_rust_body("X", &pu8, Some("u8"), "x ^ 0xFF").expect("xor");
    assert!(xor.contains("arith sub"), "{xor}"); // 1-bit
    // mask 0 peeps
    let z = try_ir_from_rust_body("Z", &pu8, Some("u8"), "x & 0").expect("and0");
    assert!(z.contains("const 0 : Int"), "{z}");
    let id = try_ir_from_rust_body("I", &pu8, Some("u8"), "x | 0").expect("or0");
    assert!(id.contains("load $0"), "{id}");
    // i64/u64 const-mask encodes via bit products through 64 (#1186)
    let i64m = try_ir_from_rust_body("S", &px(), Some("i64"), "x & 1").expect("i64&1");
    assert!(i64m.contains("arith mod"), "{i64m}");
    let pu64 = vec![ParamInfo {
        name: "x".into(),
        ty: "u64".into(),
    }];
    let u64m = try_ir_from_rust_body("U", &pu64, Some("u64"), "x & 1").expect("u64&1");
    assert!(u64m.contains("arith mod"), "{u64m}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&u64m, "U").expect("parse u64&1");
    // Nested expr uses SAT_BOUNDS width
    let nest = try_ir_from_rust_body("N", &pu8, Some("u8"), "(x + 1) & 1").expect("nested and");
    assert!(nest.contains("arith mod"), "{nest}");
    // Signed i8: map through unsigned bit pattern
    let pi8 = vec![ParamInfo {
        name: "x".into(),
        ty: "i8".into(),
    }];
    let sand = try_ir_from_rust_body("Sa", &pi8, Some("i8"), "x & 1").expect("i8 and");
    assert!(
        sand.contains("arith mod") && sand.contains("cmp gt"),
        "{sand}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&sand, "Sa").expect("parse i8");
    let sxor = try_ir_from_rust_body("Sx", &pi8, Some("i8"), "x ^ -1").expect("i8 xor -1");
    assert!(sxor.contains("arith sub"), "{sxor}");
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
    // Unbounded divisor may be 0 → BNM (was unsound if encoded).
    assert!(
        try_ir_from_rust_body("M", &pxy, Some("bool"), "x.is_multiple_of(y)").is_none(),
        "i64 divisor includes 0"
    );
    // NonZeroU8 divisor is safe.
    let nzd = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        },
        ParamInfo {
            name: "d".into(),
            ty: "NonZeroU8".into(),
        },
    ];
    let ir = try_ir_from_rust_body("Nz", &nzd, Some("bool"), "x.is_multiple_of(d)").expect("imo");
    assert!(ir.contains("arith mod") && ir.contains("cmp eq"), "{ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "Nz").expect("parse");
    // Literal 0 panics in Rust; must not encode as mod-by-zero.
    assert!(try_ir_from_rust_body("Z", &px(), Some("bool"), "x.is_multiple_of(0)").is_none());
    let ok = try_ir_from_rust_body("T", &px(), Some("bool"), "x.is_multiple_of(2)").expect("by2");
    assert!(ok.contains("const 2") || ok.contains("arith mod"), "{ok}");
    let by1 = try_ir_from_rust_body("O", &px(), Some("bool"), "x.is_multiple_of(1)").expect("by1");
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
    assert!(try_ir_from_rust_body("Mp", &px(), Some("bool"), "x.is_multiple_of((0))").is_none());
    let ok = try_ir_from_rust_body("D2", &px(), Some("i64"), "x / 2").expect("div2");
    assert!(ok.contains("arith div"), "{ok}");
    // Zero-including path divisors stay BNM (soundness).
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
    assert!(try_ir_from_rust_body("Vd", &pxy, Some("i64"), "x / y").is_none());
    assert!(try_ir_from_rust_body("Vm", &pxy, Some("i64"), "x % y").is_none());
    // NonZero path divisor encodes.
    let nzd = vec![
        ParamInfo {
            name: "x".into(),
            ty: "u32".into(),
        },
        ParamInfo {
            name: "d".into(),
            ty: "NonZeroU32".into(),
        },
    ];
    let nz = try_ir_from_rust_body("Nz", &nzd, Some("u32"), "x / d").expect("nz div");
    assert!(nz.contains("arith div"), "{nz}");
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
    let ir = try_ir_from_rust_body("A", &px(), Some("i64"), "x.saturating_abs()").expect("sat_abs");
    assert!(ir.contains("call abs") && ir.contains("call min"), "{ir}");
    assert!(ir.contains(&format!("const {}", i64::MAX)), "{ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "A").expect("parse");
    // Needs return-type bounds (same as other saturating_*).
    assert!(try_ir_from_rust_body("B", &px(), None, "x.saturating_abs()").is_none());
    let assoc =
        try_ir_from_rust_body("C", &px(), Some("i64"), "i64::saturating_abs(x)").expect("assoc");
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
    let sat = try_ir_from_rust_body("S", &px(), Some("bool"), "x.saturating_abs().is_negative()")
        .expect("satneg");
    assert!(sat.contains("const 0 : Bool"), "{sat}");
    let z =
        try_ir_from_rust_body("Z", &px(), Some("bool"), "x.abs_diff(x).is_zero()").expect("ad0");
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
    let ir =
        try_ir_from_rust_body("C", &px(), Some("bool"), "x.abs().is_positive()").expect("chain");
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
    let add = try_ir_from_rust_body("W", &px(), Some("i64"), "x.wrapping_add(1)").expect("i64 add");
    assert!(
        add.contains("const 4294967296") && add.contains("arith mul") && add.contains("arith mod"),
        "{add}"
    );
    assert!(add.contains("cmp gt"), "signed reinterpret: {add}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&add, "W").expect("parse add");
    let mul = try_ir_from_rust_body("M", &px(), Some("i64"), "x.wrapping_mul(2)").expect("i64 mul");
    assert!(
        mul.contains("arith mul") && mul.contains("arith mod") && mul.contains("const 4294967296"),
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
    let ir = try_ir_from_rust_body("W", &pi8, Some("i8"), "x.wrapping_add(1)").expect("i8 wrap");
    assert!(ir.contains("arith mod") && ir.contains("const 256"), "{ir}");
    assert!(ir.contains("cmp gt"), "signed reinterpret: {ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "W").expect("parse");
    let mul = try_ir_from_rust_body("M", &pi8, Some("i8"), "x.wrapping_mul(2)").expect("i8 mul");
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
    let ir = try_ir_from_rust_body("W", &pu8, Some("u8"), "x.wrapping_add(1)").expect("u8 wrap");
    assert!(ir.contains("arith add") && ir.contains("arith mod"), "{ir}");
    assert!(ir.contains("const 256"), "{ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "W").expect("parse");
    let mul = try_ir_from_rust_body("M", &pu8, Some("u8"), "x.wrapping_mul(3)").expect("u8 mul");
    assert!(
        mul.contains("arith mul") && mul.contains("arith mod"),
        "{mul}"
    );
    let neg = try_ir_from_rust_body("Ng", &pu8, Some("u8"), "x.wrapping_neg()").expect("u8 neg");
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
    let t =
        try_ir_from_rust_body("T", &px(), Some("bool"), "8i64.is_power_of_two()").expect("8 pot");
    assert!(t.contains("const 1 : Bool"), "{t}");
    let f =
        try_ir_from_rust_body("F", &px(), Some("bool"), "3i64.is_power_of_two()").expect("3 not");
    assert!(f.contains("const 0 : Bool"), "{f}");
    let z =
        try_ir_from_rust_body("Z", &px(), Some("bool"), "0i64.is_power_of_two()").expect("0 not");
    assert!(z.contains("const 0 : Bool"), "{z}");
    // i64 path param: 63-pot enum
    let ir = try_ir_from_rust_body("P", &px(), Some("bool"), "x.is_power_of_two()").expect("i64");
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
    let ir =
        try_ir_from_rust_body("P", &pu8(), Some("bool"), "x.is_power_of_two()").expect("u8 pot");
    assert!(ir.contains("cmp eq"), "{ir}");
    assert!(
        ir.contains("const 1 : Int") && ir.contains("const 128 : Int"),
        "{ir}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "P").expect("parse");
    let u32ir =
        try_ir_from_rust_body("Q", &pu32(), Some("bool"), "x.is_power_of_two()").expect("u32 pot");
    assert!(
        u32ir.contains("const 2147483648") || u32ir.contains("const 1 : Int"),
        "{u32ir}"
    );
    // u64 path: 64-pot enum with synthetic 2^63 (#1173)
    let pu64 = vec![ParamInfo {
        name: "x".into(),
        ty: "u64".into(),
    }];
    let u64ir =
        try_ir_from_rust_body("U", &pu64, Some("bool"), "x.is_power_of_two()").expect("u64 pot");
    assert!(
        u64ir.contains("cmp eq") && u64ir.contains("arith mul"),
        "{u64ir}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&u64ir, "U").expect("parse u64 pot");
}

#[test]
fn nested_is_power_of_two_encodes() {
    // #1034: nested arith inherits path-param pot width
    let ir = try_ir_from_rust_body("N", &pu8(), Some("bool"), "(x + 1).is_power_of_two()")
        .expect("nested pot");
    assert!(ir.contains("arith add") && ir.contains("cmp eq"), "{ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "N").expect("parse");
    let mul = try_ir_from_rust_body("M", &pu8(), Some("bool"), "(x * 2).is_power_of_two()")
        .expect("mul pot");
    assert!(mul.contains("arith mul"), "{mul}");
    // wrapping_* / rotate fall back to receiver width when return is bool
    let w = try_ir_from_rust_body(
        "W",
        &pu8(),
        Some("bool"),
        "x.wrapping_add(1).is_power_of_two()",
    )
    .expect("wrap pot");
    assert!(w.contains("arith") && w.contains("cmp eq"), "{w}");
    let shl = try_ir_from_rust_body(
        "Sh",
        &pu8(),
        Some("bool"),
        "x.wrapping_shl(1).is_power_of_two()",
    )
    .expect("shl pot");
    assert!(shl.contains("arith") && shl.contains("cmp eq"), "{shl}");
    let rot = try_ir_from_rust_body(
        "Ro",
        &pu8(),
        Some("bool"),
        "x.rotate_left(1).is_power_of_two()",
    )
    .expect("rot pot");
    assert!(rot.contains("arith") && rot.contains("cmp eq"), "{rot}");
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
    // signed ≤32 via bit-pattern map then popcount
    let pi8 = vec![ParamInfo {
        name: "x".into(),
        ty: "i8".into(),
    }];
    let s = try_ir_from_rust_body("S", &pi8, Some("u32"), "x.count_ones()").expect("i8 ones");
    assert!(
        s.contains("arith mod") && s.contains("cmp gt") || s.contains("arith add"),
        "{s}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&s, "S").expect("parse i8");
    // i64 width >32 stays BNM
    assert!(try_ir_from_rust_body("I", &px(), Some("u32"), "x.count_ones()").is_none());
    let c16 =
        try_ir_from_rust_body("C16", &pu16(), Some("u32"), "x.count_ones()").expect("u16 ones");
    assert!(
        c16.contains("arith add") || c16.contains("arith mul"),
        "{c16}"
    );
    let c32 =
        try_ir_from_rust_body("C32", &pu32(), Some("u32"), "x.count_ones()").expect("u32 ones");
    assert!(
        c32.contains("arith add") || c32.contains("arith mul"),
        "{c32}"
    );
    // signed count_zeros = bits - ones
    let z = try_ir_from_rust_body("Z", &pi8, Some("u32"), "x.count_zeros()").expect("i8 zeros");
    assert!(z.contains("arith sub"), "{z}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&z, "Z").expect("parse zeros");
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
    // unbounded i64 path stays BNM
    assert!(try_ir_from_rust_body("S", &px(), Some("u32"), "x.trailing_zeros()").is_none());
    // signed ≤32 via bit-pattern map
    let pi8 = vec![ParamInfo {
        name: "x".into(),
        ty: "i8".into(),
    }];
    let st = try_ir_from_rust_body("St", &pi8, Some("u32"), "x.trailing_zeros()").expect("i8 tz");
    assert!(st.contains("arith mul") && st.contains("const 8"), "{st}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&st, "St").expect("parse i8 tz");
    let sl = try_ir_from_rust_body("Sl", &pi8, Some("u32"), "x.leading_zeros()").expect("i8 lz");
    assert!(sl.contains("arith mul"), "{sl}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&sl, "Sl").expect("parse i8 lz");
    let t16 =
        try_ir_from_rust_body("T16", &pu16(), Some("u32"), "x.trailing_zeros()").expect("u16 tz");
    assert!(
        t16.contains("arith mul") && t16.contains("const 16"),
        "{t16}"
    );
    let l32 =
        try_ir_from_rust_body("L32", &pu32(), Some("u32"), "x.leading_zeros()").expect("u32 lz");
    assert!(
        l32.contains("arith mul") && l32.contains("const 32"),
        "{l32}"
    );
    // trailing_ones / leading_ones via NOT + zeros
    let to = try_ir_from_rust_body("To", &pu8, Some("u32"), "x.trailing_ones()").expect("to");
    assert!(to.contains("arith sub") && to.contains("arith mul"), "{to}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&to, "To").expect("parse to");
    let lo = try_ir_from_rust_body("Lo", &pu8, Some("u32"), "x.leading_ones()").expect("lo");
    assert!(lo.contains("arith sub"), "{lo}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&lo, "Lo").expect("parse lo");
    let sto = try_ir_from_rust_body("Sto", &pi8, Some("u32"), "x.trailing_ones()").expect("i8 to");
    assert!(sto.contains("arith sub"), "{sto}");
    let to16 =
        try_ir_from_rust_body("To16", &pu16(), Some("u32"), "x.trailing_ones()").expect("u16 to");
    assert!(to16.contains("arith sub"), "{to16}");
    let lo32 =
        try_ir_from_rust_body("Lo32", &pu32(), Some("u32"), "x.leading_ones()").expect("u32 lo");
    assert!(lo32.contains("arith sub"), "{lo32}");
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
    let cz = try_ir_from_rust_body("Cz", &px(), Some("u32"), "12u32.count_zeros()").expect("cz");
    assert!(cz.contains("const 30 : Int"), "{cz}");
    let tz = try_ir_from_rust_body("T", &px(), Some("u32"), "12u32.trailing_zeros()").expect("tz");
    assert!(tz.contains("const 2 : Int"), "{tz}");
    // Variable receivers stay BNM
    assert!(try_ir_from_rust_body("V", &px(), Some("u32"), "x.count_ones()").is_none());
    // Typed 0.trailing_zeros() == bit width
    let z0 = try_ir_from_rust_body("Z", &px(), Some("u32"), "0u32.trailing_zeros()").expect("0tz");
    assert!(z0.contains("const 32 : Int"), "{z0}");
    // bare 0 without suffix still BNM
    assert!(try_ir_from_rust_body("B", &px(), Some("u32"), "0.trailing_zeros()").is_none());
}

#[test]
fn typed_leading_zeros_peep() {
    let lz = try_ir_from_rust_body("L", &px(), Some("u32"), "8u32.leading_zeros()").expect("lz");
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
    // signed ≤32 via bit-pattern map + reinterpret
    let pi8 = vec![ParamInfo {
        name: "x".into(),
        ty: "i8".into(),
    }];
    let s = try_ir_from_rust_body("S", &pi8, Some("i8"), "x.reverse_bits()").expect("i8 rev");
    assert!(
        s.contains("arith mul") && s.contains("cmp gt") || s.contains("arith mod"),
        "{s}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&s, "S").expect("parse i8 rev");
    // const negative peep: (-128i8).reverse_bits() == 1
    let c = try_ir_from_rust_body("C", &px(), Some("i8"), "(-128i8).reverse_bits()").expect("c");
    assert!(c.contains("const 1 : Int"), "{c}");
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
    // signed i16 via bit-pattern map + reinterpret
    let pi16 = vec![ParamInfo {
        name: "x".into(),
        ty: "i16".into(),
    }];
    let s = try_ir_from_rust_body("Si", &pi16, Some("i16"), "x.swap_bytes()").expect("i16 sw");
    assert!(s.contains("const 256") && s.contains("arith mul"), "{s}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&s, "Si").expect("parse i16 sw");
    let expected = ((-2i16).swap_bytes()) as i64;
    let c = try_ir_from_rust_body("C", &px(), Some("i16"), "(-2i16).swap_bytes()").expect("c");
    assert!(
        c.contains(&format!("const {expected} : Int")),
        "want {expected}: {c}"
    );
}

#[test]
fn typed_reverse_bits_and_swap_bytes_peep() {
    // 0b0000_0001 u8 reversed → 0b1000_0000 = 128 (unsigned keeps 128)
    let rev = try_ir_from_rust_body("R", &px(), Some("u8"), "1u8.reverse_bits()").expect("rev");
    assert!(rev.contains("const 128 : Int"), "{rev}");
    // same pattern on i8 reinterprets high bit: 1i8 → -128
    let revs =
        try_ir_from_rust_body("Rs", &px(), Some("i8"), "1i8.reverse_bits()").expect("i8 rev");
    assert!(revs.contains("const -128 : Int"), "{revs}");
    // 0x1234u16.swap_bytes() → 0x3412 = 13330
    let sw = try_ir_from_rust_body("S", &px(), Some("u16"), "0x1234u16.swap_bytes()").expect("sw");
    assert!(sw.contains("const 13330 : Int"), "{sw}");
    // unbounded path (no fixed width) stays BNM
    assert!(try_ir_from_rust_body("V", &px(), Some("u8"), "x.reverse_bits()").is_none());
    let ig = try_ir_from_rust_body("I", &px(), Some("u32"), "8u32.ilog2()").expect("ilog");
    assert!(ig.contains("const 3 : Int"), "{ig}");
    assert!(try_ir_from_rust_body("Z", &px(), Some("u32"), "0u32.ilog2()").is_none());
    // Variable unsigned path-param ilog2 (#1174)
    let vilog = try_ir_from_rust_body("V", &pu8(), Some("u32"), "x.ilog2()").expect("var ilog2");
    assert!(
        vilog.contains("arith mod") && vilog.contains("arith mul"),
        "{vilog}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&vilog, "V").expect("parse");
    let vilog10 =
        try_ir_from_rust_body("L", &pu8(), Some("u32"), "x.ilog10()").expect("var ilog10");
    assert!(vilog10.contains("cmp ge"), "{vilog10}");
    let vilog16 =
        try_ir_from_rust_body("V16", &pu16(), Some("u32"), "x.ilog2()").expect("u16 ilog2");
    assert!(vilog16.contains("arith mod"), "{vilog16}");
    let vilog32 =
        try_ir_from_rust_body("V32", &pu32(), Some("u32"), "x.ilog2()").expect("u32 ilog2");
    assert!(vilog32.contains("arith mod"), "{vilog32}");
    let vilog10_32 =
        try_ir_from_rust_body("L32", &pu32(), Some("u32"), "x.ilog10()").expect("u32 ilog10");
    assert!(vilog10_32.contains("cmp ge"), "{vilog10_32}");
    // unbounded i64 stays BNM; fixed-width signed encodes with positivity gate
    assert!(try_ir_from_rust_body("S", &px(), Some("u32"), "x.ilog2()").is_none());
    let pi8 = vec![ParamInfo {
        name: "x".into(),
        ty: "i8".into(),
    }];
    let silog = try_ir_from_rust_body("Si", &pi8, Some("u32"), "x.ilog2()").expect("i8 ilog2");
    assert!(
        silog.contains("call max") && silog.contains("cmp gt") && silog.contains("arith mul"),
        "{silog}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&silog, "Si").expect("parse signed ilog");
    let silog10 =
        try_ir_from_rust_body("Si10", &pi8, Some("u32"), "x.ilog10()").expect("i8 ilog10");
    assert!(
        silog10.contains("call max") && silog10.contains("cmp ge"),
        "{silog10}"
    );
    let np =
        try_ir_from_rust_body("Np", &px(), Some("u32"), "3u32.next_power_of_two()").expect("np");
    assert!(np.contains("const 4 : Int"), "{np}");
    let z1 =
        try_ir_from_rust_body("Z1", &px(), Some("u32"), "0u32.next_power_of_two()").expect("0np");
    assert!(z1.contains("const 1 : Int"), "{z1}");
    // Variable path-param next_power_of_two (#1185)
    let vnp =
        try_ir_from_rust_body("Vnp", &pu8(), Some("u8"), "x.next_power_of_two()").expect("vnp");
    assert!(vnp.contains("cmp le") && vnp.contains("const 128"), "{vnp}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&vnp, "Vnp").expect("parse");
    let vnp64 = try_ir_from_rust_body("V64", &pu64(), Some("u64"), "x.next_power_of_two()")
        .expect("u64 npot");
    assert!(vnp64.contains("cmp le"), "{vnp64}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&vnp64, "V64").expect("parse u64 npot");
    let wvar = try_ir_from_rust_body("Wv", &pu8(), Some("u8"), "x.wrapping_next_power_of_two()")
        .expect("wvar");
    assert!(wvar.contains("cmp le"), "{wvar}");
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
    // Variable unsigned path ≤16 (#1187 follow-on / MPI encode)
    let vsq = try_ir_from_rust_body("Vsq", &pu8(), Some("u8"), "x.isqrt()").expect("var isqrt");
    assert!(vsq.contains("cmp ge") && vsq.contains("const 15"), "{vsq}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&vsq, "Vsq").expect("parse");
    // u32 path stays BNM (ladder too large)
    assert!(try_ir_from_rust_body("U32", &pu32(), Some("u32"), "x.isqrt()").is_none());
    let l10 = try_ir_from_rust_body("L10", &px(), Some("u32"), "100u32.ilog10()").expect("ilog10");
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
    let shr = try_ir_from_rust_body("N", &px(), Some("i64"), "x.wrapping_shr(1)").expect("i64 shr");
    assert!(shr.contains("arith div"), "{shr}");
}

#[test]
fn variable_u8_rotate_left_encodes() {
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
    let ir = try_ir_from_rust_body("R", &p, Some("u8"), "x.rotate_left(n)").expect("rot");
    assert!(ir.contains("cmp eq") && ir.contains("arith mul"), "{ir}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "R").expect("parse");
    let rr = try_ir_from_rust_body("Rr", &p, Some("u8"), "x.rotate_right(n)").expect("rotr");
    assert!(rr.contains("cmp eq"), "{rr}");
    let c = try_ir_from_rust_body("C", &p[..1], Some("u8"), "x.rotate_left(1)").expect("c1");
    assert!(c.contains("arith mul") && c.contains("arith div"), "{c}");
    // i64 variable rotate encodes via 64-case-sum (#1172)
    let pi = vec![
        ParamInfo {
            name: "x".into(),
            ty: "i64".into(),
        },
        ParamInfo {
            name: "n".into(),
            ty: "u32".into(),
        },
    ];
    let i64r = try_ir_from_rust_body("I", &pi, Some("i64"), "x.rotate_left(n)").expect("i64 rot");
    assert!(
        i64r.contains("cmp eq") && i64r.contains("arith mul"),
        "{i64r}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&i64r, "I").expect("parse i64 rot");
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
    let srot = try_ir_from_rust_body("Sr", &pi8, Some("i8"), "x.rotate_left(1)").expect("i8 rotl");
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
        ir.contains("cmp eq") && ir.contains("const 4294967296") && ir.contains("const 2147483648"),
        "{ir}"
    );
    assura_smt::LoadedVerifyExtras::from_ir_text(&ir, "S").expect("parse");
    let shr = try_ir_from_rust_body("R", &p, Some("i64"), "x.wrapping_shr(n)").expect("shr");
    assert!(shr.contains("arith div") && shr.contains("cmp eq"), "{shr}");
    assura_smt::LoadedVerifyExtras::from_ir_text(&shr, "R").expect("parse");
    // const shift by 63 now encodes (2^63 = 2^32*2^31)
    let c63 = try_ir_from_rust_body("C", &px(), Some("i64"), "x.wrapping_shl(63)").expect("shl63");
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
    let c1 = try_ir_from_rust_body("C", &p[..1], Some("u64"), "x.wrapping_shl(1)").expect("const1");
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
    let us = try_ir_from_rust_body("U", &pu, Some("usize"), "x.wrapping_shl(n)").expect("usize");
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
    let i8shr = try_ir_from_rust_body("R", &pi8, Some("i8"), "x.wrapping_shr(1)").expect("i8 shr");
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
    let notz = try_ir_from_rust_body("Z", &px(), Some("bool"), "!x.is_zero()").expect("not zero");
    assert!(notz.contains("cmp eq"), "{notz}");
}

#[test]
fn rem_euclid_nonzero_get_peel() {
    let nzd = vec![
        ParamInfo {
            name: "x".into(),
            ty: "u32".into(),
        },
        ParamInfo {
            name: "d".into(),
            ty: "NonZeroU32".into(),
        },
    ];
    let ir =
        try_ir_from_rust_body("G", &nzd, Some("u32"), "x.rem_euclid(d.get())").expect("get peel");
    assert!(ir.contains("arith mod"), "{ir}");
}
