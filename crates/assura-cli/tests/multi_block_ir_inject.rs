//! Multi-block co-located IR inject (#882).
use std::process::Command;

fn assura_bin() -> String {
    std::env::var("CARGO_BIN_EXE_assura").unwrap_or_else(|_| {
        let mut p = std::env::current_exe().unwrap();
        p.pop();
        if p.ends_with("deps") {
            p.pop();
        }
        p.push("assura");
        p.to_string_lossy().into_owned()
    })
}

fn unique_temp(prefix: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "{prefix}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn build_write_ir_abs_multi_block_compiles_and_tests() {
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
        ir_text.contains("fn #1") && ir_text.contains("fn #2"),
        "expected multi-block IR, got:\n{ir_text}"
    );
    let lib = std::fs::read_to_string(out_dir.join("src/lib.rs")).unwrap();
    assert!(
        lib.contains("block_1") && lib.contains("block_2"),
        "expected block closures in lib.rs, got:\n{lib}"
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
