# assura-driver

Custom rustc compiler driver for Assura contract verification.

This is an **exploratory prototype** (see issue #669). It hooks into the Rust
compiler via `rustc_driver::Callbacks` to extract:

- **Fully resolved types** for every parameter and local variable
- **MIR control flow graphs** (basic blocks, return paths, loops, panics)
- **Resolved call sites** including trait dispatch and generic monomorphization
- **Contract attributes** (`#[requires]`, `#[ensures]`, `#[invariant]`) on functions

This data feeds into the existing LLM + Z3 pipeline (#666, #667) to produce
higher-confidence analysis than the syn-based path can achieve.

## Requirements

```bash
# Nightly toolchain with compiler internals
rustup toolchain install nightly
rustup component add rustc-dev --toolchain nightly
```

## Build

```bash
# From the workspace root (crate is excluded from workspace)
cd crates/assura-driver
cargo +nightly build
```

## Usage

```bash
# Direct: analyze a single file
cargo +nightly run -- src/lib.rs --edition 2021

# As RUSTC_WRAPPER: analyze an entire crate
RUSTC_WRAPPER=target/debug/assura-driver cargo +nightly check

# Pipe output to the LLM pipeline
cargo +nightly run -- src/lib.rs --edition 2021 2>/dev/null | \
  assura check-rust --driver-input /dev/stdin src/
```

## Output

The driver prints a JSON `CompilerAnalysis` to stdout:

```json
{
  "annotated_functions": [
    {
      "name": "crate::security::check_path",
      "module_path": "crate::security",
      "contracts": [
        { "kind": "requires", "expression": "!path.is_empty()" },
        { "kind": "ensures", "expression": "result.is_ok() implies result.unwrap().is_absolute()" }
      ],
      "params": [
        { "name": "path", "ty": "&std::path::Path" }
      ],
      "return_type": "Result<PathBuf, PathError>",
      "mir_summary": {
        "basic_block_count": 7,
        "return_points": 3,
        "has_loops": false,
        "has_panicking_paths": true,
        "has_unsafe_blocks": false
      },
      "callees": [
        {
          "callee_name": "is_relative",
          "callee_path": "std::path::Path::is_relative",
          "is_trait_dispatch": false,
          "callee_contracts": []
        }
      ]
    }
  ],
  "total_functions": 42
}
```

## Architecture

The driver does **not** replace the existing `assura check-rust` syn-based path.
It enhances it:

```
syn parsing (stable, always works) ──┐
                                     ├──> LLM + Z3 Pipeline
rustc driver (nightly, opt-in) ──────┘
```

Both paths produce the same `AnalysisRequest` structure. The driver path fills
in richer `AnalysisContext` (exact types, resolved callees, MIR summaries)
while the syn path uses name-based heuristics.

## Status

Phase 1 prototype. The driver compiles, finds annotated functions, extracts
MIR summaries, and resolves call sites. Integration with the LLM pipeline
is planned for Phase 3.

## References

- [jyn.dev/rustc-driver](https://jyn.dev/rustc-driver/)
- [rustc dev guide: Callbacks](https://rustc-dev-guide.rust-lang.org/rustc-driver/interacting-with-the-ast.html)
- [rustc_public (Stable MIR)](https://github.com/rust-lang/rustc_public)