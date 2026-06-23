#!/usr/bin/env bash
# Scaffold / checklist for adding a new Layer-0 type checker (run_*_checks).
#
# Usage:
#   bash scripts/agent-new-checker.sh my_feature
#   bash scripts/agent-new-checker.sh my_feature --category memory
#
# Does not edit files automatically (avoids wrong-layer mistakes). Prints the
# exact steps and grep targets an agent should follow.
set -euo pipefail
cd "$(dirname "$0")/.."

name="${1:-}"
category="${3:-meta}"
if [[ "${2:-}" == "--category" && -n "${3:-}" ]]; then
  category="$3"
fi

if [[ -z "$name" || "$name" == "-h" || "$name" == "--help" ]]; then
  cat <<'USAGE'
Usage: bash scripts/agent-new-checker.sh <snake_name> [--category <checks_file_stem>]

Examples:
  bash scripts/agent-new-checker.sh widget_safety
  bash scripts/agent-new-checker.sh lock_order --category concurrency

Categories (existing checks/*.rs stems): concurrency core effects ffi_error format
  frame_totality info_flow linear_typestate memory meta numeric platform safety storage

Read crates/assura-types/src/CHECKER-LAYERS.md before implementing.
USAGE
  exit 0
fi

# normalize: accept run_foo_checks or foo
name="${name#run_}"
name="${name%_checks}"
fn="run_${name}_checks"
dispatch_line="    CheckerDispatch::Source(${fn}),"

cat <<EOF
=== agent-new-checker: ${fn} ===

Layer map (see crates/assura-types/src/CHECKER-LAYERS.md):
  domain/     feature / CVE / invariant logic (the *what*)
  checkers/   structural AST/symbol analysis (the *how* on syntax)
  checks/     thin run_*_checks wiring only (instantiate + collect errors)
  pipeline.rs CHECKER_PIPELINE registry (mandatory or dead code)

1) Put logic in the right layer
   - New domain feature?     crates/assura-types/src/domain/${category}.rs (or new module)
   - Structural checker?     crates/assura-types/src/checkers/
   - Wiring only:            crates/assura-types/src/checks/${category}.rs

2) Add wiring function signature (checks/${category}.rs)
   pub(crate) fn ${fn}(source: &assura_parser::ast::SourceFile) -> Vec<TypeError> {
       // instantiate domain/checker, walk decls (prefer Decl accessors / DeclVisitor),
       // collect TypeError with real spans + Axxxxx codes from SPEC Appendix D
       Vec::new()
   }

   Signature variants if you need symbols/env (match existing peers):
   - Source:      fn(source) -> Vec<TypeError>
   - Symbols:     fn(source, symbols: &SymbolTable) -> Vec<TypeError>
   - Env:         fn(source, type_env: &TypeEnv) -> Vec<TypeError>
   - EnvSymbols:  fn(source, type_env, symbols) -> Vec<TypeError>
   - Effects/Totality: special cases in pipeline.rs (do not invent a third special)

3) Register in CHECKER_PIPELINE (same PR — non-negotiable)
   File: crates/assura-types/src/pipeline.rs
   Insert near related checkers (order roughly matches historical run_all_checks):
${dispatch_line}

   If you used Symbols/Env/EnvSymbols, use that CheckerDispatch variant instead of Source.

4) Export from checks/mod.rs if you added a new checks submodule
   (existing category files are already pub(crate) use category::*;)

5) Tests
   - Unit tests on the domain/checker struct (same file or tests/)
   - At least one pipeline/wiring test that expects an Axxxxx code when input is bad
   - assura-types tests: resolve_ok + type_check (NOT assura_test_support::typecheck_ok
     returning TypedFile into this crate — type instance footgun)
   - Other crates: assura_test_support::compile_result / expect_type_errors OK

6) Verify before commit
   bash scripts/agent-guards.sh
   cargo test -p assura-types ${name} --lib --locked   # or your test name filter
   cargo clippy -p assura-types --lib --locked -- -D warnings

7) Grep for accidental orphans / duplicates
   rg -n '${fn}' crates/assura-types/
   # must appear in checks/*.rs (def) AND pipeline.rs (registry)

Done. Implement logic; do not mark done until agent-guards passes and a negative test exists.
EOF
