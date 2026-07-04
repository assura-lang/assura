# Assura

**Write what it should do. AI proves it does.**

Assura is a contract-first AI-native language that transpiles to verified Rust.
Humans write behavioral contracts. AI writes implementations. The compiler
proves correctness mathematically via SMT solvers. Ships as native or WASM
binaries through `rustc`.

```assura
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)

  requires { b != 0 }
  ensures  { result * b + (a mod b) == a }
  ensures  { abs(result) <= abs(a) }
  effects  { pure }
}
```

## Quick Start

```bash
# Preferred: prebuilt binary from GitHub Releases (cargo-dist)
# https://github.com/assura-lang/assura/releases
#
# From a clone (the CLI is not published to crates.io yet):
cargo install --path crates/assura-cli

# Check a contract
assura check my_contract.assura

# Build (generates verified Rust)
assura build my_contract.assura
```

Do **not** run `cargo install assura`: that name is only a crates.io
placeholder and will not install this toolchain. See
[CRATES-IO.md](../CRATES-IO.md) and the root README.

## Documentation

- **[Tutorial](TUTORIAL.md)**: Get started writing your first contracts
- **[Cheatsheet](CHEATSHEET.md)**: Quick reference for syntax and features
- **[Cookbook](COOKBOOK.md)**: Common patterns and recipes
- **[Language Specification](SPECIFICATION.md)**: Complete language reference
- **[Compiler Internals](INTERNALS.md)**: How the compiler works
