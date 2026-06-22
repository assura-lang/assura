# Assura Tutorial

A hands-on guide to writing, checking, and building Assura contracts.

## Installation

### From source (requires Rust 1.85+)

```bash
cargo install --git https://github.com/assura-lang/assura assura
```

### Verify the installation

```bash
assura --help
```

### Prerequisites for verification

Assura uses Z3 for SMT-based contract verification. Install it:

```bash
# macOS
brew install z3

# Ubuntu/Debian
sudo apt-get install -y libz3-dev
```

CVC5 is an optional alternative solver. Install it if you want portfolio
mode (tries both solvers):

```bash
# macOS
brew install cvc5
```

## Step 1: Create a Project

```bash
assura init my-project
cd my-project
```

This creates:

```
my-project/
  assura.toml           # Project configuration
  contracts/
    lib.assura          # Starter contract
```

The generated `assura.toml`:

```toml
[package]
name = "my-project"
version = "0.1.0"

[build]
target = "native"       # "native" or "wasm32-wasi"
output = "generated"

[verify]
smt-solver = "z3"       # "z3", "cvc5", or "portfolio"
layer = 1               # 0 = structural only, 1 = SMT
timeout = 1000          # SMT timeout in ms
```

## Step 2: Write a Contract

Edit `contracts/lib.assura`:

```assura
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b != 0 }
    ensures  { b != 0 }
}
```

A contract declares:
- **input(...)**: parameters the function accepts
- **output(...)**: what it returns
- **requires { ... }**: preconditions (caller's responsibility)
- **ensures { ... }**: postconditions (implementation's responsibility)

The `ensures { b != 0 }` clause is trivially verified by Z3 because
the `requires` clause already guarantees `b != 0`. As you learn the
language, you'll write more expressive postconditions.

## Step 3: Check Your Contract

```bash
assura check contracts/lib.assura
```

Output:

```
Verification (1 clause(s)):
  SafeDivision:
    ensures              ... verified
contracts/lib.assura: check passed (no errors)
```

### Verbose mode

```bash
assura check contracts/lib.assura --verbose
```

Shows timing for each pipeline phase (lex, parse, resolve, typecheck,
verify).

### Verification layers

| Layer | Flag | What it checks |
|-------|------|----------------|
| 0 | `--layer 0` | Type checking, name resolution, linearity |
| 1 | `--layer 1` | SMT verification via Z3: refinement types, arithmetic (default) |

```bash
assura check contracts/lib.assura --layer 0
```

## Step 4: Build to Rust

```bash
assura build contracts/lib.assura
```

This generates a Rust project in `generated/`:

```
generated/
  Cargo.toml
  src/
    lib.rs
```

The generated Rust includes `debug_assert!` statements derived from
your `requires` and `ensures` clauses. Run the generated code:

```bash
cd generated
cargo test
```

### Custom output directory

```bash
assura build contracts/lib.assura --output my-output
```

### WASM target

```bash
assura build contracts/lib.assura --target wasm
```

This generates a project configured for `wasm32-wasip1`, including a
`.cargo/config.toml` with the target pre-set.

## Step 5: Understand Errors

When a contract fails verification, Assura shows a counterexample:

```
Verification (1 clause(s)):
  SafeDivision:
    ensures    ... COUNTEREXAMPLE
      | result -> 0
      | a -> 1
      | b -> 2
```

Use `assura explain` to learn about specific error codes:

```bash
assura explain A03001   # Type mismatch
assura explain A05001   # Linear type used twice
assura explain A07003   # Unknown effect
```

Error spans are precise even for expressions inside braced clauses (e.g. `requires { x > 0 }`), thanks to full trivia capture in the parser. A type error on `true` will point exactly at the sub-expression, not the `requires` keyword.

## Contract Features

### Refinement types

Narrow a base type with a predicate:

```assura
type Positive = { v: Int | v > 0 }
type Percentage = { v: Float | v >= 0.0 && v <= 100.0 }
```

### Effects

Declare what side effects a contract may perform:

```assura
contract ReadFile {
    effects {
        io
        fs
    }
    requires {
        path != ""
    }
}
```

Valid effect names: `io`, `database`, `logging`, `mem`, `net`, `fs`,
`rng`, `time`, `alloc`, `diverge`, `random`, and dotted sub-effects
like `console.read`, `filesystem.write`.

### Quantifiers

Express properties over collections:

```assura
contract Sorted {
    ensures {
        forall i in data : i >= 0
    }
}
```

### Services with typestate

Model stateful protocols:

```assura
service Connection {
    states { Disconnected, Connected }

    operation connect {
        requires {
            host != ""
        }
        ensures {
            connected == true
        }
    }
}
```

### Ghost code and decreases clauses

Prove termination of recursive algorithms:

```assura
fn factorial(n: Nat) -> Nat
    requires {
        n >= 0
    }
    ensures {
        result >= 1
    }
    decreases {
        n
    }
```

## Formatting

Format `.assura` files (like `rustfmt` for Rust):

```bash
assura fmt contracts/lib.assura
```

Check formatting without modifying:

```bash
assura fmt contracts/lib.assura --check
```

## Solver Selection

Choose which SMT solver to use:

```bash
# Z3 (default)
assura check file.assura --solver z3

# CVC5
assura check file.assura --solver cvc5

# Portfolio: tries Z3 first, falls back to CVC5 on timeout
assura check file.assura --solver portfolio
```

Or set it in `assura.toml`:

```toml
[verify]
smt-solver = "portfolio"
```

## VS Code Extension

Install the extension for syntax highlighting and LSP integration:

```bash
cd editors/vscode
npm install && npm run compile
```

Features: syntax highlighting, inline diagnostics via the Assura LSP
server.

## AI Agent Setup

To configure an AI coding assistant to use Assura, run:

```bash
assura agent-instructions > .assura-context.md
```

This outputs a compact reference with Assura syntax, type mappings,
CLI commands, and workflow steps that you can add to your agent's
system prompt or project instructions (AGENTS.md, .cursorrules, etc.).

See the [scenario guides](SCENARIOS.md) for detailed walkthroughs of
AI-assisted development workflows.

## Next Steps

- Read the [scenario guides](SCENARIOS.md) for real workflow walkthroughs
- Read the demo contracts in `demos/` for real-world examples
- See the [language specification](SPECIFICATION.md) for the full grammar
- See the [internals documentation](INTERNALS.md) for compiler architecture