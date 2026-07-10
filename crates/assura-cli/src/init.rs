// `assura init <project-name>` -- scaffold a new Assura project
// ---------------------------------------------------------------------------

use super::*;

/// Validate a project name for `assura init`.
///
/// Rejects empty names (which would write into the current directory), path
/// separators, `.` / `..`, and characters outside `[A-Za-z0-9_-]` with a
/// leading letter or underscore (package-safe identifiers).
pub(crate) fn validate_project_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("project name must not be empty".into());
    }
    if name == "." || name == ".." {
        return Err(format!("invalid project name '{name}'"));
    }
    if name.contains('/') || name.contains('\\') {
        return Err("project name must not contain path separators".into());
    }
    if name.contains("..") {
        return Err("project name must not contain '..'".into());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("project name must not be empty".into());
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return Err("project name must start with a letter or underscore (A-Z, a-z, _)".into());
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(
            "project name may only contain letters, digits, underscores, and hyphens".into(),
        );
    }
    Ok(())
}

pub(crate) fn run_init(project_name: &str, output_mode: OutputMode) {
    let json = output_mode == OutputMode::Json;
    if let Err(msg) = validate_project_name(project_name) {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": msg,
                "usage": "assura init <project-name>",
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: {msg}");
            eprintln!("Usage: assura init <project-name>");
        }
        process::exit(2);
    }

    let project_dir = Path::new(project_name);

    if project_dir.exists() {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": format!("directory '{project_name}' already exists"),
                "project": project_name,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: directory '{project_name}' already exists");
        }
        process::exit(1);
    }

    // Create project directory and contracts subdirectory
    let contracts_dir = project_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).unwrap_or_else(|e| {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": format!("cannot create directory: {e}"),
                "project": project_name,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: cannot create directory: {e}");
        }
        process::exit(1);
    });

    // Write assura.toml
    let toml_content = format!(
        r#"[package]
name = "{project_name}"
version = "0.1.0"

[build]
target = "native"       # "native" or "wasm32-wasi"
output = "generated"

[verify]
smt-solver = "z3"       # "z3", "cvc5", or "portfolio"
layer = 1               # 0 = structural only, 1 = SMT
timeout = 1000          # SMT timeout in ms

[profile]
type = "minimal"        # minimal, parser, database, etc.

# [ai]
# mode = "api"            # "api" (direct HTTP) or "cli" (shell out)
# provider = "anthropic"  # "anthropic", "openai", "ollama"
# model = "claude-sonnet-4-20250514"
#
# For CLI mode (any tool that accepts a prompt):
# mode = "cli"
# command = "claude"      # or "aider", "sgpt", etc.
# args = ["-p", "{{prompt}}"]  # {{prompt}} is replaced with the actual prompt
"#
    );
    let toml_path = project_dir.join("assura.toml");
    fs::write(&toml_path, &toml_content).unwrap_or_else(|e| {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": format!("cannot write {}: {e}", toml_path.display()),
                "project": project_name,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: cannot write {}: {e}", toml_path.display());
        }
        process::exit(1);
    });

    // Write starter contract + co-located IR so ensures on `result` verify.
    // Previously ensures copied requires (vacuous); see #920.
    let contract_content = r#"// SafeDivision: division by zero is impossible, and result == a / b.
//
// requires: callers must pass a non-zero divisor.
// ensures: the co-located SafeDivision.ir body binds `result` so Z3 can
// prove the postcondition (not a free output variable).
//
// Try: assura check contracts/lib.assura
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b != 0 }
    ensures  { result == a / b }
}
"#;
    let contract_path = contracts_dir.join("lib.assura");
    fs::write(&contract_path, contract_content).unwrap_or_else(|e| {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": format!("cannot write {}: {e}", contract_path.display()),
                "project": project_name,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: cannot write {}: {e}", contract_path.display());
        }
        process::exit(1);
    });

    let ir_content = r#"module SafeDivision {
  fn #0 : ($0: Int, $1: Int) -> Int ! pure
  {
    $result = arith div $0 $1 : Int
  }
}
"#;
    let ir_path = contracts_dir.join("SafeDivision.ir");
    fs::write(&ir_path, ir_content).unwrap_or_else(|e| {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": format!("cannot write {}: {e}", ir_path.display()),
                "project": project_name,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: cannot write {}: {e}", ir_path.display());
        }
        process::exit(1);
    });

    // Report what was created
    if json {
        let report = serde_json::json!({
            "ok": true,
            "project": project_name,
            "files": [
                toml_path.display().to_string(),
                contract_path.display().to_string(),
                ir_path.display().to_string(),
            ],
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!("Created new Assura project '{project_name}':");
        println!("  {}", toml_path.display());
        println!("  {}", contract_path.display());
        println!("  {}", ir_path.display());
    }
}

pub(crate) fn run_explain(code: &str, output_mode: OutputMode) {
    match assura_diagnostics::explain(code) {
        Some(info) => {
            if output_mode == OutputMode::Json {
                let json = serde_json::json!({
                    "code": info.code,
                    "name": info.name,
                    "description": info.description,
                    "example": info.example,
                    "fix": info.fix,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                );
            } else {
                println!("{}: {}", info.code, info.name);
                println!();
                println!("{}", info.description);
                println!();
                println!("Example:");
                println!();
                println!("{}", info.example);
                println!();
                println!("How to fix:");
                println!();
                println!("{}", info.fix);
            }
        }
        None => {
            if output_mode == OutputMode::Json {
                let catalog = assura_diagnostics::error_catalog();
                let codes: Vec<_> = catalog
                    .iter()
                    .map(|i| serde_json::json!({"code": i.code, "name": i.name}))
                    .collect();
                let json = serde_json::json!({
                    "error": format!("Unknown error code: {code}"),
                    "known_codes": codes,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json).unwrap_or_default()
                );
            } else {
                eprintln!("Unknown error code: {code}");
                eprintln!();
                eprintln!("Known error codes:");
                let catalog = assura_diagnostics::error_catalog();
                for info in &catalog {
                    eprintln!("  {} - {}", info.code, info.name);
                }
            }
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod init_name_tests {
    use super::validate_project_name;

    #[test]
    fn accepts_simple_names() {
        validate_project_name("myproj").expect("myproj");
        validate_project_name("my-proj").expect("my-proj");
        validate_project_name("My_Proj2").expect("My_Proj2");
        validate_project_name("_private").expect("_private");
    }

    #[test]
    fn rejects_empty_and_dot() {
        assert!(
            validate_project_name("").is_err(),
            "empty name should be rejected"
        );
        assert!(
            validate_project_name(".").is_err(),
            "dot alone should be rejected"
        );
        assert!(
            validate_project_name("..").is_err(),
            "dotdot should be rejected"
        );
    }

    #[test]
    fn rejects_paths_and_spaces() {
        assert!(
            validate_project_name("bad name").is_err(),
            "spaces should be rejected"
        );
        assert!(
            validate_project_name("a/b").is_err(),
            "slash path should be rejected"
        );
        assert!(
            validate_project_name("a\\b").is_err(),
            "backslash path should be rejected"
        );
        assert!(
            validate_project_name("foo/../bar").is_err(),
            "traversal path should be rejected"
        );
    }

    #[test]
    fn rejects_leading_digit_or_hyphen() {
        assert!(
            validate_project_name("1proj").is_err(),
            "leading digit should be rejected"
        );
        assert!(
            validate_project_name("-proj").is_err(),
            "leading hyphen should be rejected"
        );
    }
}

// ---------------------------------------------------------------------------
