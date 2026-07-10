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

pub(crate) fn run_init(project_name: &str) {
    if let Err(msg) = validate_project_name(project_name) {
        eprintln!("Error: {msg}");
        eprintln!("Usage: assura init <project-name>");
        process::exit(2);
    }

    let project_dir = Path::new(project_name);

    if project_dir.exists() {
        eprintln!("Error: directory '{project_name}' already exists");
        process::exit(1);
    }

    // Create project directory and contracts subdirectory
    let contracts_dir = project_dir.join("contracts");
    fs::create_dir_all(&contracts_dir).unwrap_or_else(|e| {
        eprintln!("Error: cannot create directory: {e}");
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
        eprintln!("Error: cannot write {}: {e}", toml_path.display());
        process::exit(1);
    });

    // Write starter contract
    let contract_content = r#"// SafeDivision: ensures division by zero is impossible
//
// The requires clause guarantees callers must pass a non-zero divisor.
// The ensures clause states the result is always defined (not an error).
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b != 0 }
    ensures  { b != 0 }
}
"#;
    let contract_path = contracts_dir.join("lib.assura");
    fs::write(&contract_path, contract_content).unwrap_or_else(|e| {
        eprintln!("Error: cannot write {}: {e}", contract_path.display());
        process::exit(1);
    });

    // Report what was created
    println!("Created new Assura project '{project_name}':");
    println!("  {}", toml_path.display());
    println!("  {}", contract_path.display());
}

pub(crate) fn run_explain(code: &str) {
    match assura_diagnostics::explain(code) {
        Some(info) => {
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
        None => {
            eprintln!("Unknown error code: {code}");
            eprintln!();
            eprintln!("Known error codes:");
            let catalog = assura_diagnostics::error_catalog();
            for info in &catalog {
                eprintln!("  {} - {}", info.code, info.name);
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
        assert!(validate_project_name("myproj").is_ok());
        assert!(validate_project_name("my-proj").is_ok());
        assert!(validate_project_name("My_Proj2").is_ok());
        assert!(validate_project_name("_private").is_ok());
    }

    #[test]
    fn rejects_empty_and_dot() {
        assert!(validate_project_name("").is_err());
        assert!(validate_project_name(".").is_err());
        assert!(validate_project_name("..").is_err());
    }

    #[test]
    fn rejects_paths_and_spaces() {
        assert!(validate_project_name("bad name").is_err());
        assert!(validate_project_name("a/b").is_err());
        assert!(validate_project_name("a\\b").is_err());
        assert!(validate_project_name("foo/../bar").is_err());
    }

    #[test]
    fn rejects_leading_digit_or_hyphen() {
        assert!(validate_project_name("1proj").is_err());
        assert!(validate_project_name("-proj").is_err());
    }
}

// ---------------------------------------------------------------------------
