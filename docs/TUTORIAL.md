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

The Z3 SMT solver is downloaded automatically during `cargo build` (via
the `z3` crate's `gh-release` feature). No manual Z3 installation is
needed.

CVC5 is an optional alternative solver for portfolio mode (tries both
solvers). It is not required for normal use:

```bash
bash scripts/setup-cvc5.sh
# paste the printed export lines (CVC5_LIB_DIR, CVC5_INCLUDE_DIR)
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

# [dependencies]         # Optional: import contracts from other projects
# math-lib = { path = "../math-lib" }
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

## Step 5: Multi-File Projects

As your project grows, split contracts across multiple files and use
imports to reference them.

### Project layout

```
my-project/
  assura.toml
  contracts/
    lib.assura          # Root contract
    math.assura         # Imported by lib
```

### Importing local modules

Use dot-separated paths. The path maps to the filesystem relative to
the project root:

```assura
import contracts.math

contract App {
    input(x: Int)
    requires { x >= 0 }
}
```

`import contracts.math` loads `contracts/math.assura`.

### External dependencies

Add a `[dependencies]` section to `assura.toml` to import contracts
from other projects:

```toml
[package]
name = "my-project"
version = "0.1.0"

[dependencies]
math-lib = { path = "../math-lib" }
```

Then import from the dependency using its name (hyphens become
underscores in import paths):

```assura
import math_lib.core

contract App {
    input(x: Int)
    requires { x >= 0 }
}
```

### Checking a project

Point `assura check` at the project directory (not a single file):

```bash
assura check .
```

This discovers all `.assura` files, resolves imports (including
dependencies), and type-checks every module.

## Step 6: Understand Errors

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

## Part 2: Learning by Example

This section walks through five contracts of increasing complexity.
Each builds on concepts from the previous one.

### Example 1: Input Validation

The simplest useful contract: validate that a username meets length
requirements.

```assura
contract ValidateUsername {
  input(name: String, min_len: Nat, max_len: Nat)
  output(valid: Bool)

  requires { min_len > 0 }
  requires { max_len >= min_len }
  requires { name.length() >= 0 }

  ensures { max_len >= min_len }
}
```

Save this as `validate.assura` and run:

```bash
assura check validate.assura
```

Expected output:

```
  ValidateUsername:
    ensures              ... verified
validate.assura: check passed (no errors)
```

**What you learned:** `requires` sets preconditions that callers must
satisfy. `ensures` sets postconditions that the implementation must
guarantee. Z3 proves `ensures` holds given the `requires`.

### Example 2: Arithmetic with Multiple Clauses

Contracts can have multiple `requires` and `ensures` clauses. Each
is checked independently.

```assura
contract SafeAverage {
  input(a: Int, b: Int, max: Int)
  output(avg: Int)

  requires { a >= 0 }
  requires { b >= 0 }
  requires { max > 0 }
  requires { a <= max }
  requires { b <= max }

  ensures { max >= 0 }
  ensures { a + b >= 0 }
}
```

This contract specifies an averaging function where both inputs are
non-negative and bounded by `max`. Z3 proves each `ensures` clause
independently against the combined `requires`.

### Example 3: A Failing Contract (Reading Counterexamples)

This contract has a bug: the ensures clause does not follow from the
preconditions.

```assura
contract BuggyClamp {
  input(value: Int, low: Int, high: Int)
  output(result: Int)

  requires { low <= high }

  ensures { result >= low }
}
```

Run `assura check buggy_clamp.assura` and you will see:

```
  BuggyClamp:
    ensures              ... counterexample found
      result = -1, low = 0
```

**Why it fails:** `result` is an unconstrained output variable. Z3 can
assign it any value. Since nothing forces `result >= low`, Z3 finds
`result = -1, low = 0` as a counterexample.

**The lesson:** In Assura, `result` and output variables are free
(the language is specification-only, with no implementation bodies).
Write `ensures` clauses that follow logically from `requires`, or
that constrain relationships between inputs only.

### Example 4: Effects and Safety Annotations

Assura tracks side effects. Declare which effects a function may
perform:

```assura
contract ReadConfig {
  input(path: String)
  output(config: String)

  effects { io, fs }

  requires { path.length() > 0 }
  ensures { path.length() > 0 }
}
```

Valid effect names include: `io`, `fs`, `net`, `database`, `logging`,
`mem`, `rng`, `time`, `alloc`, and dotted sub-effects like
`filesystem.write`, `network.connect`, `database.read`.

### Example 5: Invariants and Quantifiers

Contracts can express properties that must hold for all elements:

```assura
contract SortedArray {
  input(arr: List<Int>, n: Nat)

  requires { n >= 0 }
  invariant { forall i in Nat: i >= 0 || i < 0 }

  ensures { n >= 0 }
}
```

Use `--layer 2` for quantified invariant verification:

```bash
assura check sorted.assura --layer 2
```

## Part 3: Real-World CVE Walkthrough

This section walks through `demos/heartbleed.assura`, which models
CVE-2014-0160 (the Heartbleed bug).

### The Vulnerability

Heartbleed was a buffer over-read in OpenSSL's TLS heartbeat extension.
The server reads `payload_length` bytes from the request but does not
check that `payload_length` is within the actual received data. An
attacker sends a small payload with a large `payload_length`, causing
the server to leak memory contents.

**CVSS:** 7.5 (High)

### The Assura Contract

Open `demos/heartbleed.assura`:

```assura
contract tls_heartbeat_response {
  input(
    payload_length: Nat,
    padding_length: Nat,
    record_length: Nat
  )
  output(response: Bytes)

  requires { payload_length > 0 }
  requires { padding_length > 0 }
  requires { 3 + payload_length + padding_length <= record_length }
  requires { record_length > 0 }

  ensures { record_length > payload_length }
  ensures { record_length > 0 }
}
```

### What Each Clause Does

- **`requires { payload_length > 0 }`**: The payload must not be empty
- **`requires { 3 + payload_length + padding_length <= record_length }`**:
  The key fix: total size (3-byte header + payload + padding) must fit
  within the record. This is the check that OpenSSL was missing.
- **`ensures { record_length > payload_length }`**: The record is always
  larger than the payload (no over-read possible)

### Running the Check

```bash
assura check demos/heartbleed.assura --verbose
```

Z3 proves that `record_length > payload_length` follows from
`3 + payload_length + padding_length <= record_length` (since
`padding_length > 0`, the record must be strictly larger).

### What Would Happen Without the Fix

Remove the bounds-check requires clause and Z3 immediately finds a
counterexample: `payload_length = 100, record_length = 5`. This is
exactly the Heartbleed scenario.

## Part 4: Common Mistakes and Debugging

### "counterexample found" on ensures

**Symptom:** Your ensures clause gets a counterexample even though it
"should" be true.

**Cause:** Output variables (`result`, variables in `output()`) are
free. Z3 can assign them any value. Your ensures must follow logically
from requires alone, or constrain only input relationships.

**Fix:** Either:
1. Make your ensures reference only input variables
2. Add requires clauses that constrain the relationship more tightly

### "unknown" instead of "verified"

**Symptom:** Z3 returns "unknown" for a clause.

**Possible causes:**
- **Timeout:** The formula is too complex. Try `--timeout 30000` (30s)
- **Non-linear arithmetic:** Z3 struggles with multiplication of
  variables. Simplify the formula or use `--solver cvc5`
- **Unmodelable features:** Some language features are not yet encoded
  in SMT. The CLI shows these as warnings (exit 0)

### Unknown effect names

**Symptom:** Error A07003 "unknown effect"

**Fix:** Use one of the known effect names: `io`, `database`,
`logging`, `mem`, `net`, `fs`, `rng`, `time`, `alloc`, `diverge`,
`random`. For sub-effects, use dotted names: `console.read`,
`filesystem.write`, `network.connect`, `database.read`, `log.info`.

### "expected type X, got Y"

**Symptom:** Type error A03001

**Fix:** Check that your types match. Common mistakes:
- Using `Integer` instead of `Int`
- Using `Natural` instead of `Nat`
- Mixing `Int` and `Nat` without explicit bounds

### Reading error output

Assura uses ariadne for rich error display:

```
error[A03001]: type mismatch
  --> myfile.assura:5:12
   |
 5 |   requires { x > "hello" }
   |              ^^^^^^^^^^^ expected Int, found String
```

The arrow (`-->`) points to the file and line. The underline shows
exactly which expression has the error. Use `assura explain A03001`
for a detailed description of any error code.

### Verification layers

| Layer | What it checks | Timeout | Flag |
|-------|---------------|---------|------|
| 0 | Type checking only (no solver) | instant | `--layer 0` |
| 1 | Standard SMT (requires/ensures) | 1s | `--layer 1` (default) |
| 2 | Quantified invariants, termination, roundtrips | 10s | `--layer 2` |
| 3 | BMC, k-induction, liveness properties | 30s | `--layer 3` |

Higher layers are slower but check more properties. Start with the
default (layer 1) and increase when needed.

## Part 5: Integration Guides

### CI Integration

Add Assura checks to your GitHub Actions workflow:

```yaml
name: Assura
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable
      - name: Install Z3
        run: sudo apt-get install -y libz3-dev
      - name: Install Assura
        run: cargo install --git https://github.com/assura-lang/assura assura
      - name: Check contracts
        run: assura check src/**/*.assura
```

### LSP Integration

The Assura LSP server provides real-time diagnostics in your editor:

```bash
# Start the LSP server
assura lsp
```

Configure your editor's LSP client to use `assura lsp` as the server
command. The server provides:
- Inline diagnostics (parse errors, type errors, verification results)
- Hover information (type of expressions, contract summaries)
- Go to definition
- Document symbols

### MCP Server

For AI agent integration, Assura provides an MCP (Model Context
Protocol) server:

```bash
# Start the MCP server
assura mcp
```

The MCP server exposes tools that AI agents can use to check contracts,
get error explanations, and generate contract templates.

### gRPC Server

For programmatic access from any language:

```bash
# Start the gRPC server
assura server --port 50051
```

The gRPC API supports streaming verification results and batch checking.

## Quick Reference

### Contract Structure

```assura
contract Name {
  input(param1: Type1, param2: Type2)
  output(result: Type)

  requires { precondition }
  ensures { postcondition }
  invariant { property }
  effects { effect1, effect2 }
  decreases { measure }
}
```

### Types

| Type | Description | Example |
|------|------------|---------|
| `Int` | Arbitrary-precision integer | `42`, `-1` |
| `Nat` | Non-negative integer | `0`, `100` |
| `Float` | Floating-point number | `3.14` |
| `Bool` | Boolean | `true`, `false` |
| `String` | Text string | `"hello"` |
| `Bytes` | Byte sequence | binary data |
| `Unit` | No value | `()` |
| `List<T>` | Ordered collection | `List<Int>` |
| `Map<K,V>` | Key-value mapping | `Map<String, Int>` |
| `Option<T>` | Optional value | `Option<Bool>` |

### Operators

| Operator | Meaning |
|----------|---------|
| `&&` | Logical AND |
| `\|\|` | Logical OR |
| `!` | Logical NOT |
| `==`, `!=` | Equality |
| `<`, `>`, `<=`, `>=` | Comparison |
| `+`, `-`, `*`, `/`, `%` | Arithmetic |
| `forall x in T: P` | Universal quantifier |
| `exists x in T: P` | Existential quantifier |
| `old(expr)` | Pre-state value |

### CLI Commands

| Command | Description |
|---------|------------|
| `assura check file.assura` | Check a contract |
| `assura check file.assura --verbose` | Verbose output with timing |
| `assura check file.assura --layer 2` | Layer 2 verification |
| `assura check file.assura --layer 3` | Layer 3 (BMC/k-induction) |
| `assura build file.assura` | Generate Rust code |
| `assura fmt file.assura` | Format a contract |
| `assura explain AXXXXX` | Explain an error code |
| `assura init project-name` | Create a new project |
| `assura lsp` | Start LSP server |
| `assura mcp` | Start MCP server |
| `assura diff a.assura b.assura` | Compare contracts |

### Declaration Types

```assura
# Standard contract
contract Name { ... }

# External function (no body)
extern fn name(params) -> ReturnType { ... }

# Function with contract
fn name(params) -> ReturnType { ... }

# Liveness property
liveness Name {
  prove: eventually(condition)
}

# Service with typestate
service Name {
  states { State1, State2 }
  transitions { State1 -> State2 }
}

# Feature max constant
feature_max NAME: Type = value

# Enum type
enum Name { Variant1, Variant2 }
```

## Next Steps

- Read the [scenario guides](SCENARIOS.md) for real workflow walkthroughs
- Read the demo contracts in `demos/` for real-world examples
- See the [language specification](SPECIFICATION.md) for the full grammar
- See the [internals documentation](INTERNALS.md) for compiler architecture