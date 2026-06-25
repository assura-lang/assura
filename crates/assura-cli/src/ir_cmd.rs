use super::*;

// `assura ir <file.ir>` — parse, validate, and codegen an Implementation IR file
// ---------------------------------------------------------------------------

pub(crate) fn run_ir(
    ir_file: &str,
    contract_file: Option<&str>,
    output: Option<&str>,
    verbosity: Verbosity,
) {
    let ir_source = fs::read_to_string(ir_file).unwrap_or_else(|e| {
        eprintln!("Error: {ir_file}: {e}");
        process::exit(2);
    });

    // Parse the IR module
    let module = match assura_smt::parse_ir_module(&ir_source) {
        Ok(m) => m,
        Err(errors) => {
            eprintln!("IR parse errors in {ir_file}:");
            for e in &errors {
                eprintln!("  {e}");
            }
            process::exit(1);
        }
    };

    if verbosity == Verbosity::Verbose {
        eprintln!(
            "Parsed IR module `{}`: {} function(s)",
            module.name,
            module.functions.len()
        );
    }

    // Optionally validate against a contract file
    if let Some(contract_path) = contract_file {
        let contract_source = fs::read_to_string(contract_path).unwrap_or_else(|e| {
            eprintln!("Error: {contract_path}: {e}");
            process::exit(2);
        });

        let parse_result = assura_parser::parse_full(&contract_source);
        let source_file = match parse_result.file {
            Some(f) => f,
            None => {
                eprintln!("Error: failed to parse contract file {contract_path}");
                process::exit(1);
            }
        };

        // Find the first contract declaration for validation
        let contract_decl = source_file.decls.iter().find_map(|d| {
            if let Decl::Contract(c) = &d.node {
                Some(c)
            } else {
                None
            }
        });

        if let Some(contract) = contract_decl {
            let validation = assura_smt::validate_ir_against_contract(&module, contract);
            if !validation.valid {
                eprintln!("IR validation errors:");
                for e in &validation.errors {
                    eprintln!("  {e}");
                }
                process::exit(1);
            }
            if verbosity != Verbosity::Quiet {
                eprintln!(
                    "OK  IR module `{}` validates against contract `{}`",
                    module.name, contract.name
                );
            }
        } else {
            eprintln!("Warning: no contract found in {contract_path}, skipping validation");
        }
    }

    // Generate Rust code
    let rust_code = assura_smt::ir_to_rust(&module);

    if let Some(out_path) = output {
        let out = Path::new(out_path);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                eprintln!("Error: cannot create directory {}: {e}", parent.display());
                process::exit(1);
            });
        }
        fs::write(out, &rust_code).unwrap_or_else(|e| {
            eprintln!("Error: cannot write {out_path}: {e}");
            process::exit(1);
        });
        if verbosity != Verbosity::Quiet {
            eprintln!("OK  {ir_file} -> {out_path}");
        }
    } else {
        print!("{rust_code}");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parse_ir_module_valid() {
        let source = "\
module TestMod {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
}";
        let module = assura_smt::parse_ir_module(source).expect("should parse valid IR");
        assert_eq!(module.name, "TestMod");
        assert_eq!(module.functions.len(), 1);
    }

    #[test]
    fn parse_ir_module_rejects_invalid_input() {
        let source = "not a valid module";
        let result = assura_smt::parse_ir_module(source);
        assert!(result.is_err(), "malformed IR should produce errors");
    }

    #[test]
    fn ir_to_rust_produces_module_comment() {
        let source = "\
module MyMod {
  fn #0 : ($0: Int) -> Int ! pure
  {
    $result = load $0 : Int
  }
}";
        let module = assura_smt::parse_ir_module(source).expect("should parse");
        let rust_code = assura_smt::ir_to_rust(&module);
        assert!(
            rust_code.contains("MyMod"),
            "generated Rust should reference the module name"
        );
    }
}
