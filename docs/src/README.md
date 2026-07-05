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
# Preferred:
cargo install assura --locked

# Or from a monorepo clone:
# cargo install --path crates/assura-cli --locked

# Check a contract
assura check my_contract.assura

# Build (generates verified Rust)
assura build my_contract.assura
```

See [CRATES-IO.md](../CRATES-IO.md) and the root README for co-publish
details and prebuilt GitHub Release installers.

## Documentation

- **[Tutorial](TUTORIAL.md)**: Get started writing your first contracts
- **[Cheatsheet](CHEATSHEET.md)**: Quick reference for syntax and features
- **[Cookbook](COOKBOOK.md)**: Common patterns and recipes
- **[Language Specification](SPECIFICATION.md)**: Complete language reference
- **[Compiler Internals](INTERNALS.md)**: How the compiler works
