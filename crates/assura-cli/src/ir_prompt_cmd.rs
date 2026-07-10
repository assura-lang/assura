use std::path::Path;

use assura_config::{CompilerConfig, OutputMode};
use assura_pipeline::compile;
use assura_smt::{IrPromptPattern, render_ir_prompt};

use super::*;

/// `assura ir-prompt <file.assura>` — emit an AI prompt to generate Implementation IR.
pub(crate) fn run_ir_prompt(
    file: &str,
    decl: Option<&str>,
    list: bool,
    pattern: &str,
    verbosity: Verbosity,
    output_mode: OutputMode,
) {
    let json = output_mode == OutputMode::Json;
    let source = fs::read_to_string(file).unwrap_or_else(|e| {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "file": file,
                "error": format!("{e}"),
                "message": format!("{file}: {e}"),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!("Error: {file}: {e}");
        }
        process::exit(2);
    });

    let pattern = pattern.parse::<IrPromptPattern>().unwrap_or_else(|()| {
        if json {
            let report = serde_json::json!({
                "ok": false,
                "error": "unknown_pattern",
                "pattern": pattern,
                "message": format!(
                    "unknown pattern '{pattern}' (expected auto, identity, arithmetic, length-copy, call-chain, bounds-check, field-access)"
                ),
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!(
                "Error: unknown pattern '{pattern}' \
                 (expected auto, identity, arithmetic, length-copy, call-chain, bounds-check, field-access)"
            );
        }
        process::exit(2);
    });

    let typed = match compile_typed(&source, file, json) {
        Ok(t) => t,
        Err(diagnostics) => {
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": file,
                    "error": "compile_failed",
                    "message": format!("failed to type-check {file}"),
                    "diagnostics": diagnostics,
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            }
            process::exit(1);
        }
    };

    let contexts = assura_smt::ir_prompt_contexts_for_typed(&typed, Some(Path::new(file)));

    if list {
        let names = list_ir_prompt_decls(file);
        if names.is_empty() {
            if json {
                let report = serde_json::json!({
                    "ok": false,
                    "file": file,
                    "error": "no_declarations",
                    "message": format!("no verifiable declarations in {file}"),
                });
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                eprintln!("Error: no verifiable declarations in {file}");
            }
            process::exit(1);
        }
        if json {
            let report = serde_json::json!({
                "ok": true,
                "file": file,
                "declarations": names,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            for name in names {
                println!("{name}");
            }
        }
        return;
    }

    let jobs: Vec<_> = if let Some(name) = decl {
        contexts
            .into_iter()
            .filter(|c| c.decl_name == name)
            .collect()
    } else if contexts.len() == 1 {
        contexts
    } else if contexts.is_empty() {
        Vec::new()
    } else {
        let names: Vec<_> = contexts.iter().map(|c| c.decl_name.as_str()).collect();
        if json {
            let report = serde_json::json!({
                "ok": false,
                "file": file,
                "error": "ambiguous_declaration",
                "message": format!(
                    "{file} has {} verifiable declarations; use --decl <name> or --list",
                    names.len()
                ),
                "declarations": names,
            });
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else {
            eprintln!(
                "Error: {file} has {} verifiable declarations; use --decl <name> or --list\n  {}",
                names.len(),
                names.join(", ")
            );
        }
        process::exit(1);
    };

    if jobs.is_empty() {
        if json {
            let report = if let Some(name) = decl {
                serde_json::json!({
                    "ok": false,
                    "file": file,
                    "error": "decl_not_found",
                    "decl": name,
                    "message": format!("no verification job named '{name}' in {file}"),
                })
            } else {
                serde_json::json!({
                    "ok": false,
                    "file": file,
                    "error": "no_declarations",
                    "message": format!("no verifiable declarations in {file}"),
                })
            };
            println!("{}", serde_json::to_string_pretty(&report).unwrap());
        } else if let Some(name) = decl {
            eprintln!("Error: no verification job named '{name}' in {file}");
        } else {
            eprintln!("Error: no verifiable declarations in {file}");
        }
        process::exit(1);
    }

    if output_mode == OutputMode::Json {
        let prompts: Vec<serde_json::Value> = jobs
            .iter()
            .map(|ctx| {
                serde_json::json!({
                    "decl": ctx.decl_name,
                    "pattern": pattern.as_str(),
                    "prompt": render_ir_prompt(ctx, pattern),
                })
            })
            .collect();
        let report = serde_json::json!({
            "file": file,
            "prompts": prompts,
        });
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }

    for (i, ctx) in jobs.iter().enumerate() {
        if jobs.len() > 1 {
            if i > 0 {
                println!();
                println!("{}", "=".repeat(72));
                println!();
            }
            if verbosity != Verbosity::Quiet {
                eprintln!("--- IR prompt for `{}` ---", ctx.decl_name);
            }
        }
        print!("{}", render_ir_prompt(ctx, pattern));
    }
}

/// List declaration names eligible for IR prompts (for tooling).
pub(crate) fn list_ir_prompt_decls(file: &str) -> Vec<String> {
    let Ok(source) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(typed) = compile_typed(&source, file, /*json*/ false) else {
        return Vec::new();
    };
    assura_smt::ir_prompt_contexts_for_typed(&typed, Some(Path::new(file)))
        .into_iter()
        .map(|c| c.decl_name)
        .collect()
}

fn compile_typed(
    source: &str,
    file: &str,
    json: bool,
) -> Result<assura_types::TypedFile, Vec<assura_diagnostics::Diagnostic>> {
    let output = compile(source, file, &CompilerConfig::default());
    if output.has_errors {
        if !json {
            for d in &output.diagnostics {
                eprintln!("{d}");
            }
        }
        return Err(output.diagnostics);
    }
    match output.typed {
        Some(typed) => Ok(typed),
        None => {
            if !json {
                eprintln!("Error: type check produced no result for {file}");
            }
            Err(Vec::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_path() -> &'static str {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/test_basic.assura"
        )
    }

    #[test]
    fn ir_prompt_lists_jobs_from_fixture() {
        let names = list_ir_prompt_decls(fixture_path());
        assert!(
            !names.is_empty(),
            "expected at least one job in test_basic.assura"
        );
    }
}
