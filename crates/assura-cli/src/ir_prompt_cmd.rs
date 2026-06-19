use std::path::Path;

use assura_config::CompilerConfig;
use assura_pipeline::compile;
use assura_smt::{IrPromptPattern, render_ir_prompt};

use super::*;

/// `assura ir-prompt <file.assura>` — emit an AI prompt to generate Implementation IR.
pub(crate) fn run_ir_prompt(file: &str, decl: Option<&str>, pattern: &str, verbosity: Verbosity) {
    let source = fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("Error: {file}: {e}");
        process::exit(2);
    });

    let pattern = pattern.parse::<IrPromptPattern>().unwrap_or_else(|()| {
        eprintln!(
            "Error: unknown pattern '{pattern}' \
             (expected auto, identity, arithmetic, length-copy, call-chain, bounds-check, field-access)"
        );
        process::exit(2);
    });

    let typed = match compile_typed(&source, file) {
        Ok(t) => t,
        Err(()) => process::exit(1),
    };

    let contexts = assura_smt::ir_prompt_contexts_for_typed(&typed, Some(Path::new(file)));
    let jobs: Vec<_> = if let Some(name) = decl {
        contexts
            .into_iter()
            .filter(|c| c.decl_name == name)
            .collect()
    } else {
        contexts
    };

    if jobs.is_empty() {
        if let Some(name) = decl {
            eprintln!("Error: no verification job named '{name}' in {file}");
        } else {
            eprintln!("Error: no verifiable declarations in {file}");
        }
        process::exit(1);
    }

    if jobs.len() > 1 && decl.is_none() && verbosity != Verbosity::Quiet {
        let names: Vec<_> = jobs.iter().map(|j| j.decl_name.as_str()).collect();
        eprintln!(
            "Note: emitting prompts for {} declarations: {}",
            names.len(),
            names.join(", ")
        );
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
#[cfg_attr(not(test), expect(dead_code, reason = "used by integration tests"))]
pub(crate) fn list_ir_prompt_decls(file: &str) -> Vec<String> {
    let Ok(source) = fs::read_to_string(file) else {
        return Vec::new();
    };
    let Ok(typed) = compile_typed(&source, file) else {
        return Vec::new();
    };
    assura_smt::ir_prompt_contexts_for_typed(&typed, Some(Path::new(file)))
        .into_iter()
        .map(|c| c.decl_name)
        .collect()
}

fn compile_typed(source: &str, file: &str) -> Result<assura_types::TypedFile, ()> {
    let output = compile(source, file, &CompilerConfig::default());
    if output.has_errors {
        for d in &output.diagnostics {
            eprintln!("{d}");
        }
        return Err(());
    }
    match output.typed {
        Some(typed) => Ok(typed),
        None => {
            eprintln!("Error: type check produced no result for {file}");
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ir_prompt_lists_jobs_from_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/test_basic.assura"
        );
        let names = list_ir_prompt_decls(path);
        assert!(
            !names.is_empty(),
            "expected at least one job in test_basic.assura"
        );
    }
}
