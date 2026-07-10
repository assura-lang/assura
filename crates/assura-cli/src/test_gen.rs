use super::*;

// `assura test-gen <file.assura>` -- generate tests from contracts
// ---------------------------------------------------------------------------

/// Map a structured AST type expression to a type-checker `Type` for proptest.
fn type_from_type_expr(te: &assura_parser::ast::TypeExpr) -> assura_types::Type {
    use assura_parser::ast::TypeExpr;
    match te {
        TypeExpr::Unit => assura_types::Type::Unit,
        TypeExpr::Named(name) => match name.as_str() {
            "Int" | "I64" => assura_types::Type::Int,
            "Nat" | "U64" => assura_types::Type::Nat,
            "Bool" => assura_types::Type::Bool,
            "String" => assura_types::Type::String,
            "Float" | "F64" => assura_types::Type::Float,
            "F32" => assura_types::Type::F32,
            "U8" => assura_types::Type::U8,
            "U16" => assura_types::Type::U16,
            "U32" => assura_types::Type::U32,
            "I8" => assura_types::Type::I8,
            "I16" => assura_types::Type::I16,
            "I32" => assura_types::Type::I32,
            other => assura_types::Type::Named(other.to_string()),
        },
        TypeExpr::Tuple(elems) if elems.is_empty() => assura_types::Type::Unit,
        TypeExpr::Tuple(elems) => {
            assura_types::Type::Tuple(elems.iter().map(type_from_type_expr).collect())
        }
        TypeExpr::Generic(name, args) => {
            let mapped: Vec<_> = args.iter().map(type_from_type_expr).collect();
            match name.as_str() {
                "List" | "Vec" => assura_types::Type::List(Box::new(
                    mapped
                        .into_iter()
                        .next()
                        .unwrap_or(assura_types::Type::Unknown),
                )),
                _ => assura_types::Type::Named(name.clone()),
            }
        }
        TypeExpr::Fn { .. } | TypeExpr::Refined { .. } => assura_types::Type::Unknown,
    }
}

fn is_concrete_type(ty: &assura_types::Type) -> bool {
    !matches!(ty, assura_types::Type::Unknown | assura_types::Type::Error)
}

fn resolve_param_type(
    name: &str,
    parsed_ty: Option<&assura_parser::ast::TypeExpr>,
    type_env: Option<&assura_types::TypeEnv>,
) -> assura_types::Type {
    let from_env = type_env.and_then(|env| env.lookup(name)).cloned();
    let from_ast = parsed_ty.map(type_from_type_expr);
    // Prefer a concrete AST annotation over indeterminate env entries
    // (inline `fn` params are often Unknown in the type env for contracts).
    match (from_env, from_ast) {
        (Some(ref t), _) if is_concrete_type(t) => t.clone(),
        (_, Some(ref t)) if is_concrete_type(t) => t.clone(),
        (Some(t), _) => t,
        (_, Some(t)) => t,
        _ => assura_types::Type::Unknown,
    }
}

/// Extract a `TestableContract` from any declaration that has clauses.
/// Works for `Decl::Contract` (incl. inline `fn` params), `FnDef`, `Extern`, `Bind`.
fn extract_testable(
    name: Option<&str>,
    fn_params: &[assura_parser::ast::Param],
    clauses: &[assura_parser::ast::Clause],
    type_env: Option<&assura_types::TypeEnv>,
) -> Option<assura_types::TestableContract> {
    let name = name?;

    let mut params = Vec::new();
    let mut requires = Vec::new();
    let mut ensures = Vec::new();

    // Decl::params() covers FnDef/Extern/Bind and Contract.fn_params (inline fn).
    for p in fn_params {
        let ty = resolve_param_type(&p.name, p.ty.as_ref(), type_env);
        params.push((p.name.clone(), ty));
    }

    // Then, walk clauses for input/requires/ensures
    for clause in clauses {
        match &clause.kind {
            ClauseKind::Input => {
                let parsed = assura_parser::ast::extract_clause_params(&clause.body);
                for p in parsed {
                    // Avoid duplicating params already added from fn_params
                    if !params.iter().any(|(n, _)| n == &p.name) {
                        let ty = resolve_param_type(&p.name, p.ty.as_ref(), type_env);
                        params.push((p.name, ty));
                    }
                }
            }
            ClauseKind::Requires => {
                requires.push(assura_codegen::expr_to_rust_static(&clause.body));
            }
            ClauseKind::Ensures => {
                ensures.push(assura_codegen::expr_to_rust_static(&clause.body));
            }
            _ => {}
        }
    }

    if params.is_empty() && ensures.is_empty() {
        return None;
    }

    Some(assura_types::TestableContract {
        name: name.to_string(),
        params,
        requires,
        ensures,
    })
}

pub(crate) fn run_test_gen(
    filename: &str,
    output: Option<&str>,
    verbosity: Verbosity,
    output_mode: assura_config::OutputMode,
) {
    let json = output_mode == assura_config::OutputMode::Json;
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "status": "error",
                    "source": filename,
                    "error": format!("{e}"),
                    "message": format!("{filename}: {e}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: {filename}: {e}");
            }
            process::exit(1);
        }
    };

    let CompilationResult {
        file,
        typed,
        has_errors,
        ..
    } = compile(&source, filename);

    if has_errors || file.is_none() {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "status": "error",
                "source": filename,
                "error": "compilation_errors",
                "message": format!("{filename} has compilation errors; fix them before generating tests."),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!(
                "Error: {filename} has compilation errors; fix them before generating tests."
            );
        }
        process::exit(1);
    }

    let file = file.unwrap();
    let type_env = typed.as_ref().map(|t| &t.type_env);

    let mut test_gen = assura_types::TestGenerator::new();

    // Extract testable contracts from ALL declaration types, not just Decl::Contract.
    // Decl::params() includes Contract.fn_params from inline `fn` definitions.
    for spanned in &file.decls {
        let decl = &spanned.node;
        if let Some(contract) =
            extract_testable(decl.name(), decl.params(), decl.clauses(), type_env)
        {
            test_gen.add_contract(contract);
        }
    }

    let tests = test_gen.generate_all();

    if tests.is_empty() {
        if json {
            let report = serde_json::json!({
                "ok": true,
                "status": "ok",
                "source": filename,
                "test_count": 0,
                "message": format!("No testable contracts found in {filename}."),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("No testable contracts found in {filename}.");
        }
        process::exit(0);
    }

    let mut out = String::new();
    out.push_str("// Generated by `assura test-gen`\n");
    out.push_str("// Source: ");
    out.push_str(filename);
    out.push('\n');
    out.push_str("use proptest::prelude::*;\n\n");

    for test in &tests {
        out.push_str(&test.body);
        out.push_str("\n\n");
    }

    if output_mode == assura_config::OutputMode::Json && output.is_none() {
        let report = serde_json::json!({
            "source": filename,
            "test_count": tests.len(),
            "rust_source": out,
            "tests": tests.iter().map(|t| serde_json::json!({
                "body": t.body,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }

    if let Some(path) = output {
        match fs::write(path, &out) {
            Ok(()) => {
                if verbosity != Verbosity::Quiet {
                    if output_mode == assura_config::OutputMode::Json {
                        println!(
                            "{}",
                            serde_json::json!({
                                "source": filename,
                                "output": path,
                                "test_count": tests.len(),
                                "status": "ok",
                            })
                        );
                    } else {
                        eprintln!(
                            "Generated {} test(s) from {filename} -> {path}",
                            tests.len()
                        );
                    }
                }
            }
            Err(e) => {
                if output_mode == assura_config::OutputMode::Json {
                    let report = serde_json::json!({
                        "ok": false,
                        "status": "error",
                        "source": filename,
                        "output": path,
                        "error": "write_failed",
                        "message": format!("Error writing {path}: {e}"),
                    });
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                } else {
                    eprintln!("Error writing {path}: {e}");
                }
                process::exit(1);
            }
        }
    } else {
        print!("{out}");
        if verbosity != Verbosity::Quiet {
            eprintln!("Generated {} test(s) from {filename}", tests.len());
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #[test]
    fn test_generator_produces_tests_from_contract() {
        let mut tg = assura_types::TestGenerator::new();
        tg.add_contract(assura_types::TestableContract {
            name: "SafeDiv".to_string(),
            params: vec![
                ("a".to_string(), assura_types::Type::Int),
                ("b".to_string(), assura_types::Type::Int),
            ],
            requires: vec!["b != 0".to_string()],
            ensures: vec!["result * b + (a % b) == a".to_string()],
        });

        let tests = tg.generate_all();
        assert!(
            !tests.is_empty(),
            "should generate at least one test from a contract"
        );
        // Each test should have a non-empty body.
        for t in &tests {
            assert!(!t.body.is_empty(), "test body should not be empty");
            assert!(!t.name.is_empty(), "test name should not be empty");
        }
    }

    #[test]
    fn test_generator_empty_contracts_produces_no_tests() {
        let tg = assura_types::TestGenerator::new();
        let tests = tg.generate_all();
        assert!(tests.is_empty(), "no contracts should yield no tests");
    }

    #[test]
    fn test_generator_contract_name_appears_in_test_name() {
        let mut tg = assura_types::TestGenerator::new();
        tg.add_contract(assura_types::TestableContract {
            name: "BoundsCheck".to_string(),
            params: vec![("idx".to_string(), assura_types::Type::Nat)],
            requires: vec![],
            ensures: vec![],
        });

        let tests = tg.generate_all();
        // Names are snake_cased for rustc (BoundsCheck → bounds_check).
        let has_name = tests
            .iter()
            .any(|t| t.name.contains("bounds_check") || t.name.contains("BoundsCheck"));
        assert!(
            has_name,
            "at least one test name should reference the contract name, got: {:?}",
            tests.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn extract_testable_from_fn_def_params() {
        let params = vec![assura_parser::ast::Param {
            name: "x".to_string(),
            ty: None,
        }];
        let clauses = vec![assura_parser::ast::Clause {
            kind: assura_parser::ast::ClauseKind::Requires,
            body: assura_parser::ast::Spanned::no_span(assura_parser::ast::Expr::Raw(vec![
                "x".to_string(),
                ">".to_string(),
                "0".to_string(),
            ])),
            effect_variables: vec![],
        }];
        let result = super::extract_testable(Some("my_fn"), &params, &clauses, None);
        let tc = result.expect("fn with params should be testable");
        assert_eq!(tc.name, "my_fn");
        assert_eq!(tc.params.len(), 1);
        assert_eq!(tc.requires.len(), 1);
    }

    #[test]
    fn extract_testable_skips_empty_decl() {
        let result = super::extract_testable(Some("empty"), &[], &[], None);
        assert!(
            result.is_none(),
            "decl with no params or ensures should not be testable"
        );
    }

    #[test]
    fn test_gen_produces_tests_for_fn_decls() {
        // Simulate a file with a standalone fn that has requires
        let source = r#"
fn check_bounds(size: Nat)
    requires { size >= 0 }
    ensures { size >= 0 }
"#;
        let (file, _) = assura_parser::parse(source);
        let file = file.expect("should parse");

        let mut tg = assura_types::TestGenerator::new();
        for spanned in &file.decls {
            let decl = &spanned.node;
            if let Some(contract) =
                super::extract_testable(decl.name(), decl.params(), decl.clauses(), None)
            {
                tg.add_contract(contract);
            }
        }

        let tests = tg.generate_all();
        assert!(
            !tests.is_empty(),
            "standalone fn with requires/ensures should produce tests"
        );
    }

    #[test]
    fn extract_testable_uses_contract_inline_fn_params() {
        let source = r#"
contract TG {
  requires { a >= 0 }
  ensures { result >= a }
  fn bump(a: Nat) -> Nat
}
"#;
        let (file, errs) = assura_parser::parse(source);
        assert!(errs.is_empty(), "{errs:?}");
        let file = file.expect("parse");
        let decl = &file.decls[0].node;
        let tc = super::extract_testable(decl.name(), decl.params(), decl.clauses(), None)
            .expect("contract with inline fn should be testable");
        assert_eq!(tc.name, "TG");
        assert_eq!(tc.params.len(), 1, "params: {:?}", tc.params);
        assert_eq!(tc.params[0].0, "a");
        assert!(
            matches!(tc.params[0].1, assura_types::Type::Nat),
            "expected Nat from inline fn annotation, got {:?}",
            tc.params[0].1
        );
        assert!(!tc.requires.is_empty());
        assert!(!tc.ensures.is_empty());
    }
}
