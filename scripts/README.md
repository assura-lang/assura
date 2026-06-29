# Assura Scripts

Scripts for verification gates, developer workflow, and setup.

## Developer Scripts

Catch common mistakes and scaffold new components.

| Script | Purpose | When to Use |
|--------|---------|-------------|
| `guards.sh` | 9-section static analysis for anti-patterns (orphan checkers, unwired SMT methods, `Verifier::new` outside allowed crates) | After touching types pipeline, SMT managers, or Verifier |
| `preflight.sh [crates...]` | fmt + guards + clippy on key crates + one demo check | Before every commit (fast, accepts crate subset) |
| `new-checker.sh <name> [--category <stem>]` | Print steps to scaffold a new Layer 0 type checker | When adding a new checker to `CHECKER_PIPELINE` |
| `new-decl.sh <VariantName>` | Print steps to scaffold a new `Decl` variant | When adding a new declaration type to the AST |

## Verification Scripts

Machine-enforced gates for correctness. Use these before marking tasks
done or closing issues.

| Script | Purpose | When to Use |
|--------|---------|-------------|
| `verify-task.sh <FEATURE>` | Build, clippy, test, demo, and coverage gate for verification features | After completing any of the 50 verification features |
| `check-smt-feature-matrix.sh [--lint\|--require-cvc5]` | Lint + compile under default, `--no-default-features`, and `cvc5-verify` | After touching `assura-smt` CVC5 or IR encode paths |
| `check-decl-variant.sh` | Grep all `match` sites on `Decl` for completeness | After adding a new `Decl` variant (then `cargo build` to fix) |

## Setup Scripts

Environment setup for local development.

| Script | Purpose | When to Use |
|--------|---------|-------------|
| `setup-cvc5.sh` | Download prebuilt CVC5 static libraries for macOS/Linux | Before running CVC5 native tests locally |
| `check-cvc5-env.sh` | Verify `CVC5_LIB_DIR` and `CVC5_INCLUDE_DIR` are set correctly | When CVC5 native tests fail to compile |

## Audit Scripts

Used for issue triage and parity verification.

| Script | Purpose | When to Use |
|--------|---------|-------------|
| `audit-cvc5-parity-closures.sh` | Check CVC5 parity issue closure coverage | When auditing CVC5 feature completeness |
| `wait-for-ci-cvc5.sh <sha>` | Wait for CI CVC5 job to complete on a commit | Before closing `cvc5-parity` issues (#304 rule) |
