#!/usr/bin/env bash
# Scaffold / checklist for adding a new Decl enum variant.
#
# Usage:
#   bash scripts/agent-new-decl.sh Widget
#   bash scripts/agent-new-decl.sh Widget WIDGET_DECL
#
# Prints steps and runs check-decl-variant.sh at the end.
set -euo pipefail
cd "$(dirname "$0")/.."

variant="${1:-}"
syntax_kind="${2:-}"

if [[ -z "$variant" || "$variant" == "-h" || "$variant" == "--help" ]]; then
  cat <<'USAGE'
Usage: bash scripts/agent-new-decl.sh <PascalCaseVariant> [SYNTAX_KIND_NAME]

Examples:
  bash scripts/agent-new-decl.sh Widget
  bash scripts/agent-new-decl.sh Widget WIDGET_DECL

High blast radius (17+ match sites). Prefer extending DeclVisitor/DeclFolder
defaults over new open-coded match arms where possible.
USAGE
  exit 0
fi

if [[ -z "$syntax_kind" ]]; then
  syntax_kind=$(echo "$variant" | sed 's/\([A-Z]\)/_\1/g' | sed 's/^_//' | tr '[:lower:]' '[:upper:]')
  syntax_kind="${syntax_kind}_DECL"
fi

# rough snake_case for helper names (Widget -> widget, FooBar -> foo_bar)
snake=$(echo "$variant" | sed 's/\([A-Z]\)/_\1/g' | sed 's/^_//' | tr '[:upper:]' '[:lower:]')

cat <<EOF
=== agent-new-decl: Decl::${variant} / ${syntax_kind} ===

1) assura-ast (canonical types + visitors)
   crates/assura-ast/src/ast/mod.rs
   - Add struct ${variant}Decl (or appropriate name) with Span-carrying fields
   - Add Decl::${variant}(...) arm
   - Update DeclVisitor::visit_${snake} default + walk_decl arm
   - Update DeclFolder::fold_${snake} default
   - Update summary_label() arm
   - Update clauses() / name() / params() accessors if applicable

2) assura-parser
   crates/assura-parser/src/syntax_kind.rs   — ${syntax_kind}
   crates/assura-parser/src/grammar/items.rs — grammar + wire into decl() recovery sets
   crates/assura-parser/src/lower/mod.rs     — lower_${snake} + lower_decl match

3) Display / fmt
   crates/assura-parser/src/display.rs (if present)
   crates/assura-fmt/src/lib.rs — format_${snake} if declarations are formatted

4) assura-resolve
   SymbolKind + register in symbol table + all match sites (grep SymbolKind / Decl::)

5) assura-types
   env.rs build_type_env
   checks/mod.rs helpers if new clause shapes
   Any checker that open-matches Decl (migrate to accessors/visitor when touching)

6) assura-codegen
   Type collection, generic arity, codegen dispatch (many match sites)

7) assura-smt
   entry/mod.rs verification loop, display stats if needed

8) Frontends
   assura-lsp: hover, completion, document symbols
   assura-cli: stats, REPL, extract_decl_summary
   assura-mcp: declaration listing

9) Mechanical sweep
   bash scripts/check-decl-variant.sh
   cargo build 2>&1 | rg non-exhaustive
   # fix every non-exhaustive match; do not allow dead_code on new arms

10) Verify
   cargo test -p assura-parser --locked --lib
   cargo test -p assura-resolve --locked --lib
   cargo test -p assura-types --locked --lib
   bash scripts/agent-guards.sh
   cargo run --bin assura -- check demos/libwebp-huffman.assura

Tip: if you only need a new *walk* over existing decls, implement DeclVisitor
in the pass instead of adding a Decl variant.

EOF

echo "=== Running check-decl-variant.sh (current codebase sites) ==="
bash scripts/check-decl-variant.sh
