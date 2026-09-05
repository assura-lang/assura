use super::*;

// `assura ir <file.ir>` — parse, validate, verify, and codegen an Implementation IR file
// ---------------------------------------------------------------------------

pub(crate) fn run_ir(
    ir_file: &str,
    contract_file: Option<&str>,
    output: Option<&str>,
    verbosity: Verbosity,
    output_mode: OutputMode,
    verify: bool,
    verify_only: bool,
) {
    let ir_source = fs::read_to_string(ir_file).unwrap_or_else(|e| {
        if output_mode == OutputMode::Json {
            let report = serde_json::json!({
                "ok": false,
                "status": "error",
                "file": ir_file,
                "error": format!("{e}"),
                "message": format!("{ir_file}: {e}"),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: {ir_file}: {e}");
        }
        process::exit(2);
    });

    // Parse the IR module
    let module = match assura_smt::parse_ir_module(&ir_source) {
        Ok(m) => m,
        Err(errors) => {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "status": "error",
                    "file": ir_file,
                    "ir_errors": errors.iter().map(|e| e.to_string()).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("IR parse errors in {ir_file}:");
                for e in &errors {
                    eprintln!("  {e}");
                }
            }
            process::exit(1);
        }
    };

    if verbosity == Verbosity::Verbose && output_mode != OutputMode::Json {
        eprintln!(
            "Parsed IR module `{}`: {} function(s)",
            module.name,
            module.functions.len()
        );
    }

    // Optionally validate against a contract file
    if let Some(contract_path) = contract_file {
        let contract_source = fs::read_to_string(contract_path).unwrap_or_else(|e| {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "status": "error",
                    "file": ir_file,
                    "contract": contract_path,
                    "error": format!("{e}"),
                    "message": format!("{contract_path}: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: {contract_path}: {e}");
            }
            process::exit(2);
        });

        let parse_result = assura_parser::parse_full(&contract_source);
        let source_file = match parse_result.file {
            Some(f) => f,
            None => {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "status": "error",
                        "file": ir_file,
                        "contract": contract_path,
                        "error": "contract_parse_failed",
                        "message": format!("failed to parse contract file {contract_path}"),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error: failed to parse contract file {contract_path}");
                }
                process::exit(1);
            }
        };

        let contract_decl = contract_for_ir_preflight(&source_file.decls, &module.name);

        if let Some(contract) = contract_decl {
            let validation = assura_smt::validate_ir_against_contract(&module, contract);
            if !validation.valid {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "status": "error",
                        "file": ir_file,
                        "contract": contract.name,
                        "ir_errors": validation.errors,
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("IR validation errors:");
                    for e in &validation.errors {
                        eprintln!("  {e}");
                    }
                }
                process::exit(1);
            }
            if verbosity != Verbosity::Quiet && output_mode != OutputMode::Json {
                eprintln!(
                    "OK  IR module `{}` validates against contract `{}`",
                    module.name, contract.name
                );
            }
        } else {
            let names: Vec<&str> = source_file
                .decls
                .iter()
                .filter_map(|d| match &d.node {
                    Decl::Contract(c) => Some(c.name.as_str()),
                    _ => None,
                })
                .collect();
            if names.len() > 1 {
                let message = format!(
                    "source has {} contracts ({}); pass contract_name / --contract or name the IR module to match one of them (IR module is `{}`)",
                    names.len(),
                    names.join(", "),
                    module.name
                );
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "status": "error",
                        "file": ir_file,
                        "contract": contract_path,
                        "ir_errors": [message],
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("IR validation errors:");
                    eprintln!("  {message}");
                }
                process::exit(1);
            } else if output_mode != OutputMode::Json {
                eprintln!("Warning: no contract found in {contract_path}, skipping validation");
            }
        }

        // --- SMT Verification (12.01 AI verification loop) ---
        if verify {
            let config = assura_config::CompilerConfig::default();
            let result = assura_pipeline::verify_ir(&contract_source, &ir_source, &config);

            if output_mode == OutputMode::Json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result).unwrap_or_default()
                );
            } else {
                // Human-readable output
                eprintln!("Verification: {}", result.progress);
                for clause in &result.clauses {
                    let icon = match clause.status.as_str() {
                        "verified" => "OK ",
                        "counterexample" => "ERR",
                        "timeout" => "TMO",
                        _ => "UNK",
                    };
                    eprintln!("  {icon}  {}", clause.name);
                    if let Some(ref cex) = clause.counterexample
                        && let Some(vars) = cex.get("variables").and_then(|v| v.as_object())
                    {
                        for (k, v) in vars {
                            eprintln!("        {k} = {v}");
                        }
                    }
                    if let Some(ref reason) = clause.reason {
                        eprintln!("        reason: {reason}");
                    }
                }
            }

            if result.status != "verified" {
                process::exit(1);
            }

            // --json verify: do not also dump Rust codegen onto stdout
            // (breaks machine consumers that parse a single JSON document).
            if output_mode == OutputMode::Json {
                if let Some(out_path) = output {
                    let rust_code = assura_smt::ir_to_rust(&module);
                    let out = Path::new(out_path);
                    if let Some(parent) = out.parent() {
                        fs::create_dir_all(parent).unwrap_or_else(|e| {
                            let report = serde_json::json!({
                                "ok": false,
                                "status": "error",
                                "file": ir_file,
                                "output": out_path,
                                "error": format!(
                                    "cannot create directory {}: {e}",
                                    parent.display()
                                ),
                            });
                            println!("{}", serde_json::to_string_pretty(&report).unwrap());
                            process::exit(1);
                        });
                    }
                    fs::write(out, &rust_code).unwrap_or_else(|e| {
                        let report = serde_json::json!({
                            "ok": false,
                            "status": "error",
                            "file": ir_file,
                            "output": out_path,
                            "error": format!("cannot write {out_path}: {e}"),
                        });
                        println!("{}", serde_json::to_string_pretty(&report).unwrap());
                        process::exit(1);
                    });
                }
                return;
            }

            if verify_only {
                return;
            }
        }
    } else if verify {
        if output_mode == OutputMode::Json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "status": "error",
                    "message": "--verify requires --contract <file>",
                    "success": false,
                }))
                .unwrap()
            );
        } else {
            eprintln!("Error: --verify requires --contract <file>");
        }
        process::exit(2);
    }

    if verify_only {
        return;
    }

    // Generate Rust code
    let rust_code = assura_smt::ir_to_rust(&module);

    if let Some(out_path) = output {
        let out = Path::new(out_path);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|e| {
                if output_mode == OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "status": "error",
                        "file": ir_file,
                        "output": out_path,
                        "error": format!("cannot create directory {}: {e}", parent.display()),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error: cannot create directory {}: {e}", parent.display());
                }
                process::exit(1);
            });
        }
        fs::write(out, &rust_code).unwrap_or_else(|e| {
            if output_mode == OutputMode::Json {
                let report = serde_json::json!({
                    "ok": false,
                    "status": "error",
                    "file": ir_file,
                    "output": out_path,
                    "error": format!("cannot write {out_path}: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: cannot write {out_path}: {e}");
            }
            process::exit(1);
        });
        if output_mode == OutputMode::Json {
            println!(
                "{}",
                serde_json::json!({
                    "ok": true,
                    "status": "ok",
                    "file": ir_file,
                    "module": module.name,
                    "output": out_path,
                })
            );
        } else if verbosity != Verbosity::Quiet {
            eprintln!("OK  {ir_file} -> {out_path}");
        }
    } else if output_mode == OutputMode::Json {
        println!(
            "{}",
            serde_json::json!({
                "ok": true,
                "status": "ok",
                "module": module.name,
                "rust": rust_code,
            })
        );
    } else {
        print!("{rust_code}");
    }
}

fn contract_for_ir_preflight<'a>(
    decls: &'a [Spanned<Decl>],
    ir_module_name: &str,
) -> Option<&'a ContractDecl> {
    let contracts: Vec<&'a ContractDecl> = decls
        .iter()
        .filter_map(|d| match &d.node {
            Decl::Contract(c) => Some(c),
            _ => None,
        })
        .collect();
    if let Some(c) = contracts.iter().copied().find(|c| c.name == ir_module_name) {
        Some(c)
    } else if contracts.len() == 1 {
        contracts.into_iter().next()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::contract_for_ir_preflight;

    #[test]
    fn contract_for_ir_preflight_selects_module_name_not_first() {
        let source = "\
contract First {
  input(a: Int, b: Int)
  output(result: Int)
  ensures { result == a }
}
contract Second {
  input(x: Int)
  output(result: Int)
  ensures { result == x }
}
";
        let file = assura_parser::parse_full(source)
            .file
            .expect("two-contract source should parse");
        let selected = contract_for_ir_preflight(&file.decls, "Second")
            .expect("module Second should select Second, not First");
        assert_eq!(selected.name, "Second");
    }

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
