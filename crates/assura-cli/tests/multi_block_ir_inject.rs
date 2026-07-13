//! Multi-block co-located IR inject (#882).
mod common;

use common::{assura_bin, unique_temp};
use std::process::Command;

#[test]
fn build_write_ir_abs_call_compiles_and_tests() {
    // abs/min/max synthesize as IR `call` (not multi-block if/else) so nested
    // forms like abs(min(x,y)) compose (#891). IR→Rust maps abs → .abs().
    let tmp = unique_temp("assura_write_ir_abs");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("AbsX.assura");
    std::fs::write(
        &assura_path,
        "contract AbsX {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == abs(x) }\n}\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir abs should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_text = std::fs::read_to_string(tmp.join("AbsX.ir")).unwrap();
    assert!(
        ir_text.contains("call abs") && !ir_text.contains("Stub IR"),
        "expected call abs IR, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains(".abs()"),
        "expected i64::abs() in lib.rs, got:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_tuple_field_compiles_and_tests() {
    // result == t.0 → field $0 .0 → Rust slot.0
    let tmp = unique_temp("assura_write_ir_tuple");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("Fst.assura");
    std::fs::write(
        &assura_path,
        "contract Fst {\n  input(t: (Int, Bool))\n  output(result: Int)\n  ensures { result == t.0 }\n}\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir tuple field should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_text = std::fs::read_to_string(tmp.join("Fst.ir")).unwrap();
    assert!(
        ir_text.contains("field $0 .0") && !ir_text.contains("Stub IR"),
        "expected field $0 .0 IR, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("slot_0.0") || lib.contains(".0"),
        "expected tuple .0 access in lib.rs, got:\n{lib}"
    );
    assert!(
        !lib.contains("todo!"),
        "injected body should not be todo!:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_tuple_field_second_compiles_and_tests() {
    // #905: result == t.1 (Bool) → field $0 .1 → Rust .1
    let tmp = unique_temp("assura_write_ir_tuple_snd");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("Snd.assura");
    std::fs::write(
        &assura_path,
        "contract Snd {\n  input(t: (Int, Bool))\n  output(result: Bool)\n  ensures { result == t.1 }\n}\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir t.1 should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_text = std::fs::read_to_string(tmp.join("Snd.ir")).unwrap();
    assert!(
        ir_text.contains("field $0 .1") && !ir_text.contains("Stub IR"),
        "expected field $0 .1 IR, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains(".1"),
        "expected tuple .1 access in lib.rs, got:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_nested_tuple_field_compiles_and_tests() {
    // #905: result == t.1.0 → chained field loads → Rust .1.0
    let tmp = unique_temp("assura_write_ir_tuple_nested");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("NestedFst.assura");
    std::fs::write(
        &assura_path,
        "contract NestedFst {\n  input(t: (Int, (Bool, Int)))\n  output(result: Bool)\n  ensures { result == t.1.0 }\n}\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir t.1.0 should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_text = std::fs::read_to_string(tmp.join("NestedFst.ir")).unwrap();
    assert!(
        ir_text.contains("field")
            && (ir_text.contains(".1") || ir_text.contains(" .1"))
            && (ir_text.contains(".0") || ir_text.contains(" .0"))
            && !ir_text.contains("Stub IR"),
        "expected nested field IR for t.1.0, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains(".1") && lib.contains(".0"),
        "expected chained .1/.0 access in lib.rs, got:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_length_value_compiles_and_tests() {
    // result == xs.length() → call length + Rust .len() as u64
    let tmp = unique_temp("assura_write_ir_len");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("LenOf.assura");
    std::fs::write(
        &assura_path,
        "contract LenOf {\n  input(xs: List<Int>)\n  output(result: Nat)\n  ensures { result == xs.length() }\n}\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir length should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_text = std::fs::read_to_string(tmp.join("LenOf.ir")).unwrap();
    assert!(
        ir_text.contains("call length") && !ir_text.contains("Stub IR"),
        "expected call length IR, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains(".len()") && lib.contains("as u64"),
        "expected .len() as u64 in lib.rs, got:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_nested_field_compiles_and_tests() {
    // Nested struct field IR + Arbitrary for Outer{inner: Inner} must cargo-test.
    let tmp = unique_temp("assura_write_ir_nested_field");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("GetDeep.assura");
    std::fs::write(
        &assura_path,
        "type Inner { v: Int }\n\
         type Outer { inner: Inner }\n\
         contract GetDeep {\n\
           input(o: Outer)\n\
           output(result: Int)\n\
           ensures { result == o.inner.v }\n\
         }\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir nested field should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ir_text = std::fs::read_to_string(tmp.join("GetDeep.ir")).unwrap();
    assert!(
        ir_text.contains("field") && ir_text.contains(".inner") && ir_text.contains(".v"),
        "expected nested field IR, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("Arbitrary for Outer") && lib.contains(".inner"),
        "expected Outer Arbitrary + field access, got:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn build_write_ir_if_then_multi_block_compiles_and_tests() {
    let tmp = unique_temp("assura_write_ir_if");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let assura_path = tmp.join("AbsLike.assura");
    std::fs::write(
        &assura_path,
        "contract AbsLike {\n  input(x: Int)\n  output(result: Int)\n  ensures { result == if x > 0 then x else 0 - x }\n}\n",
    )
    .unwrap();

    let out_dir = tmp.join("out");
    let out = Command::new(assura_bin())
        .args([
            "build",
            assura_path.to_str().unwrap(),
            "--write-ir",
            "--output",
            out_dir.to_str().unwrap(),
        ])
        .current_dir(&tmp)
        .output()
        .expect("run assura build");
    assert!(
        out.status.success(),
        "build --write-ir if-then should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("block_1") && lib.contains("bool"),
        "expected bool cond + blocks in lib.rs, got:\n{lib}"
    );

    let test = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(&out_dir)
        .output()
        .expect("cargo test");
    assert!(
        test.status.success(),
        "cargo test should pass: {}\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
