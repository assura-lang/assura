# Troubleshooting and FAQ

## Installation

### Install the Assura CLI

**Preferred:** download a prebuilt binary from
[GitHub Releases](https://github.com/assura-lang/assura/releases)
(cargo-dist installers; multi-platform).

**From source** (requires a [Rust toolchain](https://rustup.rs/),
edition 2024 / rustc 1.85+):

```bash
# From a clone:
git clone https://github.com/assura-lang/assura.git
cd assura
cargo install --path crates/assura-cli

# Or without cloning the full tree:
cargo install --git https://github.com/assura-lang/assura assura
```

Do **not** run bare `cargo install assura`. That name is only a crates.io
placeholder today and does not install this toolchain. Real co-publish of
the CLI is tracked in
[#838](https://github.com/assura-lang/assura/issues/838); the misleading
registry entry is tracked in
[#840](https://github.com/assura-lang/assura/issues/840).

**More detail:** [README Quick Start](https://github.com/assura-lang/assura/blob/main/README.md#install-the-cli),
[Tutorial installation](TUTORIAL.md#installation), and
[CRATES-IO.md](CRATES-IO.md) (libraries vs CLI, and embedding
`assura-pipeline` from crates.io).

After install, confirm the binary:

```bash
assura --version
assura doctor
```

### Z3 is not found

**Symptom:** `assura check` prints "Z3 is required for SMT verification"
or verification results are all `Timeout`.

**Fix:**

```bash
# macOS
brew install z3

# Ubuntu/Debian
sudo apt-get install -y libz3-dev

# Verify
z3 --version
assura doctor
```

If Z3 is installed but not found, check that the `z3` binary is on
your `$PATH`. On Linux, you may also need `libz3.so` in the library
path:

```bash
export LD_LIBRARY_PATH=/usr/lib/x86_64-linux-gnu:$LD_LIBRARY_PATH
```

### Rust toolchain version

Assura requires Rust edition 2024 (rustc 1.85+). Check with:

```bash
rustc --version
rustup update stable
```

## Verification

### Z3 timeout on a contract

**Symptom:** Verification result says `Timeout` instead of `Verified`
or `Counterexample`.

**Causes and fixes:**

1. **Complex quantifiers.** Quantifiers (`forall`, `exists`) with
   nested data structures are expensive. Simplify the quantifier body
   or add trigger annotations.

2. **Large numeric ranges.** Constraints like `forall x: Int, 0 <= x
   && x < 1000000` enumerate a huge space. Use tighter bounds or
   refinement types.

3. **Increase the timeout.** The default is 1 second for Layer 1
   (10 seconds for Layer 2). Increase it in `assura.toml`:
   ```toml
   [verify]
   timeout = 30000
   ```

4. **Use Layer 0 first.** Structural checks (Layer 0) are instant and
   catch type errors, undefined names, and effect violations without
   invoking Z3:
   ```bash
   assura check file.assura --layer 0
   ```

5. **Try portfolio mode.** CVC5 may handle the query faster:
   ```bash
   assura check file.assura --solver portfolio
   ```

### Understanding counterexamples

**Symptom:** Z3 returns a `Counterexample` but the values look strange.

A counterexample is a concrete set of inputs that violates a contract
clause. For example:

```
Counterexample for SafeDivision ensures clause:
  a = -2147483648
  b = -1
```

This means signed integer division of `INT_MIN / -1` overflows. The
fix is to add a precondition:

```assura
requires { !(a == min_int() && b == -1) }
```

**Tips for reading counterexamples:**

- Z3 picks adversarial edge cases: `0`, `-1`, `MAX_INT`, `MIN_INT`,
  empty collections.
- If the counterexample involves very large numbers, your contract may
  be missing overflow guards.
- If the values look random, the contract clause may be too weak to
  constrain the search space.

### "Unknown" verification result

`Unknown` means Z3 could not determine satisfiability within the
timeout. This is different from `Timeout` (which means the solver ran
out of time).

Common causes:
- Non-linear arithmetic (multiplication of two variables)
- Recursive functions without decreases clauses
- Mixing bitvector and integer theories

Fix: simplify the contract or add auxiliary lemmas.

### Verification passes but generated code fails `cargo check`

The contract is verified but the generated Rust code does not compile.
This usually means:

1. **Missing type import.** The generated code references a type that
   is not in scope. Add it to the contract's context.

2. **Generic type mismatch.** The contract uses `Int` but the Rust
   function expects a specific integer type like `i32`. The codegen
   maps `Int` to `i64` by default.

3. **Generated project structure.** Check the `generated/` directory.
   It should be a valid Cargo workspace. Run `cargo check` inside it
   to see the exact error.

## Type Errors

### A03001: type mismatch

The most common error. An expression has type `X` but the context
expects type `Y`.

```
Error A03001: type mismatch
  expected Bool, found Int
  --> contracts/lib.assura:5:15
```

**Fix:** Check that the `requires`/`ensures` clause body evaluates to
`Bool`. Arithmetic expressions like `x + 1` are `Int`, not `Bool`.
Use comparisons: `x + 1 > 0`.

### A03005: unknown field or method

```
Error A03005: unknown field `len` on type List<Int>
```

In Assura contracts, use `length(xs)` instead of `xs.len()`. The
contract language uses mathematical functions, not Rust methods.

### A03006: wrong number of arguments

```
Error A03006: function `clamp` expects 3 arguments, got 2
```

Check the function signature. In contracts, all arguments must be
provided explicitly (no defaults).

## Effect Errors

### A07001: undeclared effect

```
Error A07001: undeclared effect `io` in pure function
```

The function body uses an effect that is not in the `effects` clause.
Either add the effect:

```assura
effects { io }
```

Or mark the function as pure only if it truly has no side effects.

### A07003: unknown effect name

```
Error A07003: unknown effect name `memory`
```

Use one of the built-in effect names:

**Groups:** `io`, `database`, `logging`

**Leaf effects:** `console.read`, `console.write`, `filesystem.read`,
`filesystem.write`, `network.connect`, `network.send`,
`network.receive`, `time.read`, `random`, `database.read`,
`database.write`, `log.debug`, `log.info`, `log.warn`, `log.error`

**Short aliases:** `mem`, `net`, `fs`, `rng`, `time`, `alloc`,
`diverge`

Custom sub-effects like `io.custom` are accepted if the group (`io`)
is known.

## Name Resolution Errors

### A02001: undefined symbol

```
Error A02001: undefined symbol `result`
  --> contracts/lib.assura:7:15
```

`result` is only available inside `ensures` clauses. In `requires`
clauses, you can only reference input parameters.

Similarly, `old(x)` is only valid in `ensures` clauses.

## CLI

### `assura check` vs `assura build`

- `check` runs the full verification pipeline (parse, resolve, type-check,
  SMT) and reports results. Does not generate code.
- `build` does everything `check` does, plus generates a Rust project
  in `generated/` and runs `cargo check` on it.

Use `check` during development. Use `build` when you are ready to
integrate the generated code.

### `assura infer` output is incomplete

`assura infer` generates skeleton `bind` declarations from Rust source
files. It extracts public function signatures but cannot infer
meaningful preconditions and postconditions.

The output is a starting point. You must fill in the `requires` and
`ensures` clauses based on the function's intended behavior.

### `assura audit` is slow

`assura audit` generates contracts for every public function and
verifies them all. For large projects:

```bash
# Focus on specific modules
assura audit . --focus "parser::*"

# Limit function count
assura audit . --max-functions 20

# Reduce timeout per function
assura audit . --timeout 2000

# Only audit unsafe functions
assura audit . --unsafe-only
```

### Watch mode does not detect changes

`assura check file.assura --watch` uses filesystem notifications. If
changes are not detected:

1. Save the file explicitly (some editors delay writes).
2. Check that the file path matches exactly (watch tracks the specific
   file, not the directory).
3. On Linux, check `inotify` limits:
   ```bash
   cat /proc/sys/fs/inotify/max_user_watches
   # Increase if needed
   echo 524288 | sudo tee /proc/sys/fs/inotify/max_user_watches
   ```

## Editor Integration

### VS Code: LSP not starting

1. Install the Assura extension from the `editors/vscode/` directory.
2. Set the server path in settings:
   ```json
   {
     "assura.serverPath": "/path/to/assura-lsp"
   }
   ```
3. Build the LSP server:
   ```bash
   cargo build --bin assura-lsp
   ```
4. Restart VS Code.

The LSP provides diagnostics, go-to-definition, hover, completions,
formatting, find references, and rename.

### VS Code: no syntax highlighting

The TextMate grammar in `editors/vscode/syntaxes/assura.tmLanguage.json`
provides basic highlighting. If colors are missing:

1. Reload the extension: Ctrl+Shift+P > "Developer: Reload Window"
2. Check that `.assura` files are recognized: look for "Assura" in the
   bottom-right language selector.

## Project Configuration

### `assura.toml` options

```toml
[package]
name = "my-project"
version = "0.1.0"

[build]
output = "generated"
target = "native"    # or "wasm"

[verify]
timeout = 5000
layer = 255          # 0=structural, 1=SMT, 255=all
solver = "z3"        # "z3", "cvc5", or "portfolio"

[profile]
parallel = true
cache = true
```

### Creating a new project

```bash
assura init my-project
cd my-project
assura check contracts/lib.assura
```

This creates `assura.toml` and a starter contract in `contracts/`.