#!/usr/bin/env bash
# Grep sites that usually need a new arm when adding a `Decl` variant.
# Not exhaustive; run `cargo build` after for non-exhaustive match errors.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== Decl match / visitor sites (review each when adding a Decl variant) ==="
echo
echo "--- assura-ast (trait + summary_label) ---"
rg -n 'trait DeclVisitor|trait DeclFolder|fn summary_label|enum Decl' crates/assura-ast/src/ast/mod.rs || true
echo
echo "--- assura-parser (grammar / lower / syntax_kind) ---"
rg -n 'Decl::|VARIANT_DECL|_DECL' crates/assura-parser/src/syntax_kind.rs crates/assura-parser/src/grammar/items.rs crates/assura-parser/src/lower/mod.rs 2>/dev/null | head -40 || true
echo
echo "--- assura-types (env / checks / pipeline) ---"
rg -n 'Decl::' crates/assura-types/src/env.rs crates/assura-types/src/checks/mod.rs crates/assura-types/src/pipeline.rs 2>/dev/null | head -30 || true
echo
echo "--- assura-codegen ---"
rg -n 'Decl::|DeclVisitor' crates/assura-codegen/src/lib.rs 2>/dev/null | head -30 || true
echo
echo "--- assura-smt ---"
rg -n 'Decl::' crates/assura-smt/src/entry/mod.rs crates/assura-smt/src/display.rs 2>/dev/null | head -20 || true
echo
echo "--- assura-lsp / assura-cli / assura-mcp ---"
rg -n 'Decl::' crates/assura-lsp/src/lib.rs crates/assura-cli/src/check.rs crates/assura-mcp/src/lib.rs 2>/dev/null | head -25 || true
echo
echo "Done. Fix non-exhaustive matches via: cargo build 2>&1 | rg non-exhaustive"
