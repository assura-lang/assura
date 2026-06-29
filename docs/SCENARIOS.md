# Assura Scenario Walkthroughs

Five practical scenarios showing how to use Assura in real workflows.
Each includes exact commands, example code, and expected output.

## Table of Contents

1. [Greenfield Development with AI + Assura](#scenario-1-greenfield-development-with-ai--assura)
2. [Retrofitting Contracts onto Existing Rust Code](#scenario-2-retrofitting-contracts-onto-existing-rust-code)
3. [Security Audit with `assura audit`](#scenario-3-security-audit-with-assura-audit)
4. [CI Integration (GitHub Actions)](#scenario-4-ci-integration-github-actions)
5. [Team Onboarding and Progressive Adoption](#scenario-5-team-onboarding-and-progressive-adoption)

---

## Scenario 1: Greenfield Development with AI + Assura

You are starting a new project from scratch. You want an AI coding agent
to write both the contracts and the Rust implementation, with Assura
verifying correctness at every step.

### 1.1 Project setup

```bash
assura init safe-parser
cd safe-parser
```

This creates:

```
safe-parser/
  assura.toml
  contracts/
    lib.assura
```

The generated `assura.toml`:

```toml
[package]
name = "safe-parser"
version = "0.1.0"

[build]
target = "native"       # "native" or "wasm32-wasi"
output = "generated"

[verify]
smt-solver = "z3"       # "z3", "cvc5", or "portfolio"
layer = 1               # 0 = structural only, 1 = SMT
timeout = 1000          # SMT timeout in ms

[profile]
type = "minimal"        # minimal, parser, database, etc.
```

### 1.2 Instructing the AI agent

Add this to your project's `AGENTS.md` (or system prompt) so the AI
understands the contract-first workflow:

```markdown
## Development Workflow

1. Write an Assura contract FIRST that specifies the function's behavior.
2. Run `assura check contracts/lib.assura` to verify the contract is
   well-formed and the clauses are satisfiable.
3. Write the Rust implementation that satisfies the contract.
4. Run `assura build contracts/lib.assura` to generate verified Rust
   stubs with debug_assert! guards from the contract.
5. If `assura check` reports a counterexample, fix the contract or
   implementation before proceeding.

## Assura Type Mapping

Use these types in contracts (not Rust types):
- i32/i64/isize -> Int
- u32/u64/usize -> Nat
- f64 -> Float
- bool -> Bool
- String/&str -> String
- Vec<u8> -> Bytes
- Vec<T> -> List<T>
- Option<T> -> T?

## Contract Structure

contract Name {
    input(param: Type, ...)
    output(result: ReturnType)
    requires { precondition }
    ensures  { postcondition }
    effects  { pure | io | fs | net | ... }
}
```

### 1.3 Worked example: safe string parser with bounded output

Instead of the starter SafeDivision contract, write a contract for a
string parser that extracts a bounded substring.

Edit `contracts/lib.assura`:

```assura
// Safe bounded substring extraction.
// Guarantees: no out-of-bounds access, output length matches request.

contract BoundedSubstring {
    input(s: String, start: Nat, len: Nat)
    output(result: String)

    requires { start + len <= s.length() }
    requires { len > 0 }
    ensures  { result.length() == len }
    effects  { pure }
}

// Token parser: splits input on a delimiter, returns bounded results.
// Guarantees: token count is at most max_tokens, no token exceeds max_len.

contract TokenParser {
    input(input: String, delimiter: String, max_tokens: Nat, max_len: Nat)
    output(result: List<String>)

    requires { delimiter.length() > 0 }
    requires { max_tokens > 0 }
    requires { max_len > 0 }
    ensures  { result.length() <= max_tokens }
    ensures  { forall t in result: t.length() <= max_len }
    effects  { pure }
}
```

### 1.4 Check the contract

```bash
assura check contracts/lib.assura --verbose
```

Expected output (exact text depends on contract complexity):

```
Pipeline timing for contracts/lib.assura:
  lex:       85 tokens (0.12ms)
  parse:     2 declaration(s), 0 import(s) (0.08ms)
  resolve:   4 symbol(s) (0.03ms)
  typecheck: 8 binding(s) (0.15ms)

Verification (4 clause(s)):
  BoundedSubstring:
    requires  ✓ verified
    ensures   ✓ verified
  TokenParser:
    requires  ✓ verified
    ensures   ✓ verified

contracts/lib.assura: check passed (no errors)
```

### 1.5 Iterating on counterexamples

Suppose you write a buggy contract:

```assura
contract BadDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b >= 0 }
    ensures  { result * b == a }
    effects  { pure }
}
```

The `requires` clause allows `b == 0`, which makes the `ensures` clause
unsatisfiable for `a != 0`. Running:

```bash
assura check contracts/lib.assura
```

Produces:

```
Error[A05100]: verification failed for BadDivision: ensures:
               counterexample: a = 1, b = 0

contracts/lib.assura: 1 error(s) found
```

The counterexample shows concrete values that violate the contract.
Fix it by changing `b >= 0` to `b > 0` (or `b != 0` to also allow
negative divisors).

### 1.6 Live feedback with watch mode

Run the checker in watch mode while editing:

```bash
assura check contracts/lib.assura --watch
```

Output on startup:

```
[watch] Checking contracts/lib.assura...

contracts/lib.assura: check passed (no errors)

[watch] Watching contracts/lib.assura for changes. Press Ctrl+C to stop.
```

Every time you save the file, the checker re-runs automatically. If the
content has not changed (editor auto-save, formatter), it skips the
re-check to avoid redundant work.

On change:

```
[watch] File changed, re-checking contracts/lib.assura...

Verification (4 clause(s)):
  BoundedSubstring:
    requires  ✓ verified
    ensures   ✓ verified
  ...

contracts/lib.assura: check passed (no errors)

[watch] Watching for changes. Press Ctrl+C to stop.
```

### 1.7 Build to Rust

Once contracts are verified, generate the Rust project:

```bash
assura build contracts/lib.assura --output generated
```

Expected output:

```
  wrote generated/Cargo.toml
  wrote generated/src/lib.rs
OK  contracts/lib.assura -> generated/ (generated Rust compiles)
```

The generated `generated/src/lib.rs` contains Rust functions with
`debug_assert!` guards derived from your contract clauses. You can now
add your implementation inside these function bodies.

To target WASM instead of native:

```bash
assura build contracts/lib.assura --target wasm --output generated
```

This additionally writes `generated/.cargo/config.toml` with
`target = "wasm32-wasip1"` and runs `cargo build --target wasm32-wasip1`.

### 1.8 Verification statistics

Use `--stats` to see detailed timing and clause counts:

```bash
assura check contracts/lib.assura --stats
```

```
=== Verification Statistics ===
  Clauses:         4
  Verified:        4
  Counterexamples: 0
  Timeouts:        0
  Unknown:         0

  Lex time:        0.12ms (85 tokens)
  Parse time:      0.08ms
  Resolve time:    0.03ms
  Type-check time: 0.15ms
  Verify time:     12.45ms
  Total time:      12.83ms
```

---

## Scenario 2: Retrofitting Contracts onto Existing Rust Code

You have an existing Rust codebase and want to add Assura contracts
incrementally, without rewriting anything.

### 2.1 Bootstrap contract skeletons with `assura infer`

Suppose you have a file `src/parser.rs`:

```rust
pub fn parse_header(data: &[u8], offset: usize) -> Option<Header> {
    if offset + HEADER_SIZE > data.len() {
        return None;
    }
    let magic = u32::from_le_bytes(data[offset..offset+4].try_into().unwrap());
    let version = data[offset + 4];
    let length = u16::from_le_bytes(data[offset+5..offset+7].try_into().unwrap());
    Some(Header { magic, version, length: length as usize })
}

pub fn validate_checksum(data: &[u8], expected: u32) -> bool {
    let mut sum: u32 = 0;
    for byte in data {
        sum = sum.wrapping_add(*byte as u32);
    }
    sum == expected
}

pub fn extract_payload(data: &[u8], offset: usize, length: usize) -> Vec<u8> {
    data[offset..offset+length].to_vec()
}
```

Generate skeleton contracts:

```bash
assura infer src/parser.rs
```

Output (printed to stdout):

```assura
// Generated by: assura infer src/parser.rs
// Review and refine these contracts before use.

bind "crate::parser::parse_header" as parse_header {
    input(data: Bytes, offset: Nat)
    output(result: Header?)
    // TODO: add requires clauses (preconditions)
    // TODO: add ensures clauses (postconditions)
}

bind "crate::parser::validate_checksum" as validate_checksum {
    input(data: Bytes, expected: Nat)
    output(result: Bool)
    // TODO: add requires clauses (preconditions)
    // TODO: add ensures clauses (postconditions)
}

bind "crate::parser::extract_payload" as extract_payload {
    input(data: Bytes, offset: Nat, length: Nat)
    output(result: Bytes)
    // TODO: add requires clauses (preconditions)
    // TODO: add ensures clauses (postconditions)
}

// 3 function(s) analyzed from src/parser.rs
```

Write to a file instead:

```bash
assura infer src/parser.rs --output contracts/parser.assura
```

Filter to a single function:

```bash
assura infer src/parser.rs --function parse_header
```

### 2.2 Refining skeleton contracts

The skeletons have the right types but no invariants. Add real contracts:

```assura
bind "crate::parser::parse_header" as parse_header {
    input(data: Bytes, offset: Nat)
    output(result: Header?)

    requires { offset + 7 <= data.length() }
    ensures  {
        result.is_some() implies
            result.unwrap().length <= data.length() - offset
    }
}

bind "crate::parser::extract_payload" as extract_payload {
    input(data: Bytes, offset: Nat, length: Nat)
    output(result: Bytes)

    requires { offset + length <= data.length() }
    ensures  { result.length() == length }
}
```

### 2.3 The `bind` declaration in detail

`bind` attaches a contract to an existing Rust function without modifying
the Rust source. The syntax:

```assura
bind "rust::module::path::function_name" as local_name {
    input(param1: AssuraType, param2: AssuraType)
    output(result: AssuraReturnType)

    requires { precondition_expression }
    ensures  { postcondition_expression }
    effects  { effect_list }
}
```

- The quoted string is the full Rust path to the function.
- `as local_name` gives the contract a name for Assura's symbol table.
- `input` lists parameters with Assura types (not Rust types).
- `output` declares the return type. Use `result` to refer to it in
  `ensures` clauses.
- `requires` states what must be true before the function is called.
- `ensures` states what must be true after the function returns.
- `effects` declares side effects: `pure`, `io`, `fs`, `net`, `mem`,
  `database`, `logging`, `rng`, `time`, `alloc`.

### 2.4 Finding a real bug via counterexample

The `extract_payload` function above has a latent bug: if `offset + length`
overflows `usize`, the slice operation panics. Without the contract's
`requires` clause, callers could pass values that trigger this.

Run verification:

```bash
assura check contracts/parser.assura
```

If the contract's `requires` clause is missing:

```
Error[A05100]: verification failed for extract_payload: ensures:
               counterexample: data = [], offset = 1, length = 0

contracts/parser.assura: 1 error(s) found
```

The counterexample reveals that with an empty `data` array and a nonzero
`offset`, the function would access out-of-bounds memory. The contract
forces callers to prove `offset + length <= data.length()` before calling.

### 2.5 Incremental adoption strategy

You do not need to contract every function at once. A practical progression:

1. Start with `assura infer` on your most critical modules (parsers,
   crypto, network handlers).
2. Refine the generated skeletons with real preconditions and postconditions.
3. Run `assura check` on those contracts. Fix any counterexamples.
4. Add contracts to more modules over time.
5. Wire `assura check` into CI (see Scenario 4) so regressions are caught.

Focus on functions that:
- Parse untrusted input
- Perform arithmetic on sizes or offsets
- Handle cryptographic material
- Manage state transitions

### 2.6 AI prompt for retrofitting

Give an AI coding agent this prompt to automate the refinement step:

```
I have Assura skeleton contracts generated by `assura infer`. For each
bind declaration, analyze the Rust source function and:

1. Add requires clauses for every precondition the function assumes
   (non-null, in-bounds, non-zero divisor, valid state).
2. Add ensures clauses for every postcondition the function guarantees
   (output length, return value bounds, state changes).
3. Add effects declarations (pure, io, fs, net, mem, etc.).
4. Do NOT write vacuous contracts (always-true conditions).
5. Use Assura types, not Rust types: Int (not i64), Nat (not usize),
   Bytes (not &[u8]), String (not &str), List<T> (not Vec<T>).
6. After writing each contract, run `assura check <file>` and fix
   any counterexamples before moving to the next function.
```

---

## Scenario 3: Security Audit with `assura audit`

You want to scan an existing Rust project for potential vulnerabilities
without writing any contracts manually.

### 3.1 Running a basic audit

From the root of a Cargo workspace:

```bash
assura audit .
```

Output:

```
Scanning . ... found 47 public functions in 12 files
Generating contracts ... 47 skeleton contracts
Verifying ...

AUDIT SUMMARY: 47 functions, 38 verified, 3 findings, 0 errors

FINDINGS:
  [WARNING] parse_record: ensures  (counterexample)
    Counterexample found
    | offset = 4294967295
    | length = 1

  [WARNING] resize_buffer: ensures  (counterexample)
    Counterexample found
    | new_size = 0

  [INFO] decrypt_block: ensures  (timeout)
    Z3 timed out (needs deeper contract or longer timeout)
```

The audit generates skeleton contracts with heuristic preconditions
(bounds checks on index-like parameters, non-empty checks on collections),
runs the full pipeline (parse, resolve, type check, SMT verify), and
reports findings.

### 3.2 Targeting risky code

Scan only functions matching a pattern:

```bash
assura audit . --focus "parser::*"
```

Scan only unsafe functions:

```bash
assura audit . --unsafe-only
```

Limit the number of functions analyzed:

```bash
assura audit . --max-functions 20
```

Use shallow depth (types only, no heuristic contracts):

```bash
assura audit . --depth shallow
```

### 3.3 Understanding audit findings

Each finding has a severity and a clause type:

| Severity | Clause | Meaning |
|----------|--------|---------|
| WARNING | counterexample | Z3 found concrete inputs that violate the inferred contract. Potential bug. |
| INFO | timeout | Z3 could not prove or disprove within the time limit. Needs manual review. |
| INFO | unknown | Z3 returned "unknown" (usually nonlinear arithmetic). Needs manual review. |

A counterexample finding means the SMT solver found specific input values
where the heuristic contract is violated. This does not always mean there
is a bug; the heuristic contract may be too strict. But it always deserves
investigation.

### 3.4 Using CVE-pattern templates

For deeper analysis, use the CVE-pattern templates to generate contracts
targeting specific vulnerability classes. The templates are in
`templates/cve-patterns.md`:

- **Buffer overflow (CWE-120, CWE-787)**: bounds checks on index operations
- **Integer overflow (CWE-190, CWE-191)**: arithmetic wrapping prevention
- **Use-after-free (CWE-416)**: linear resource consumption
- **Injection (CWE-89, CWE-79)**: taint tracking on untrusted input
- **Null dereference (CWE-476)**: Option validation

Feed the template and your Rust function signatures to an AI agent to
generate targeted contracts.

### 3.5 Triage workflow

1. Run `assura audit . --format json > audit.json` for a machine-readable
   report.
2. Sort findings by severity: counterexamples first, then timeouts.
3. For each counterexample, check whether the concrete values are reachable
   in practice. If yes, it is a real bug. If no, the heuristic contract
   needs refinement.
4. For timeouts, decide whether to increase `--timeout`, write a more
   specific manual contract, or accept the risk.
5. Track findings in your issue tracker. Re-run the audit after fixes
   to confirm resolution.

### 3.6 JSON output for tooling integration

```bash
assura audit . --format json
```

Output structure:

```json
{
  "functions_scanned": 47,
  "files_scanned": 12,
  "verified": 38,
  "findings": 3,
  "errors": 0,
  "results": [
    {
      "function": "parse_record: ensures",
      "clause": "counterexample",
      "severity": "warning",
      "message": "Counterexample found",
      "counterexample": "offset = 4294967295\nlength = 1"
    },
    {
      "function": "resize_buffer: ensures",
      "clause": "counterexample",
      "severity": "warning",
      "message": "Counterexample found",
      "counterexample": "new_size = 0"
    },
    {
      "function": "decrypt_block: ensures",
      "clause": "timeout",
      "severity": "info",
      "message": "Z3 timed out (needs deeper contract or longer timeout)",
      "counterexample": null
    }
  ]
}
```

Use `jq` to extract actionable items:

```bash
assura audit . --format json | jq '.results[] | select(.severity == "warning")'
```

---

## Scenario 4: CI Integration (GitHub Actions)

You want every PR to be verified by Assura before merging.

### 4.1 Complete workflow file

Create `.github/workflows/assura.yml`:

```yaml
name: Assura Verification

on:
  pull_request:
  push:
    branches: [main]

jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install Z3
        run: sudo apt-get install -y libz3-dev

      - name: Cache Assura build
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/bin/assura
            ~/.cargo/registry
            ~/.cargo/git
          key: assura-${{ hashFiles('assura-version') }}

      - name: Install Assura
        run: |
          if ! command -v assura &> /dev/null; then
            cargo install --git https://github.com/assura-lang/assura.git assura-cli
          fi

      - name: Format check
        run: |
          for f in contracts/*.assura; do
            assura fmt "$f" --check || {
              echo "::error file=$f::File is not formatted. Run: assura fmt $f"
              exit 1
            }
          done

      - name: Verify contracts (Layer 1, SMT)
        run: |
          for f in contracts/*.assura; do
            echo "Checking $f..."
            assura check "$f" --layer 1 --json > "/tmp/$(basename $f).json" 2>&1
            if [ $? -ne 0 ]; then
              echo "::error file=$f::Contract verification failed"
              cat "/tmp/$(basename $f).json"
              exit 1
            fi
          done

      - name: Build generated Rust
        run: |
          for f in contracts/*.assura; do
            assura build "$f" --output generated --no-check
          done
          cd generated && cargo check

      - name: Upload verification results
        if: always()
        uses: actions/upload-artifact@v4
        with:
          name: assura-results
          path: /tmp/*.json
```

### 4.2 Installing Z3 in CI

Z3 is required for Layer 1 (SMT) verification. On Ubuntu runners:

```yaml
- name: Install Z3
  run: sudo apt-get install -y libz3-dev
```

On macOS runners:

```yaml
- name: Install Z3
  run: brew install z3
```

To pin a specific Z3 version (recommended for reproducibility):

```yaml
- name: Install Z3 4.13.0
  run: |
    wget https://github.com/Z3Prover/z3/releases/download/z3-4.13.0/z3-4.13.0-x64-glibc-2.35.zip
    unzip z3-4.13.0-x64-glibc-2.35.zip
    sudo cp z3-4.13.0-x64-glibc-2.35/bin/libz3.so /usr/lib/
    sudo cp z3-4.13.0-x64-glibc-2.35/include/*.h /usr/include/
```

### 4.3 Using JSON output for annotations

Parse JSON results to create GitHub annotations:

```yaml
- name: Verify contracts with annotations
  run: |
    EXIT=0
    for f in contracts/*.assura; do
      OUTPUT=$(assura check "$f" --json 2>&1)
      RC=$?
      if [ $RC -ne 0 ]; then
        EXIT=1
        # Extract error messages from JSON diagnostics
        echo "$OUTPUT" | jq -r '
          .diagnostics[]
          | select(.severity == "error")
          | "::error file=\(.file // "unknown"),line=1::\(.code): \(.message)"
        '
      fi
    done
    exit $EXIT
```

### 4.4 Failing PRs on contract violations

The `assura check` command exits with code 1 when verification fails.
This means any step using `assura check` will fail the CI job
automatically. No special configuration is needed.

Exit codes:
- `0`: all checks passed
- `1`: verification errors (counterexamples, type errors, parse errors)
- `2`: file not found, invalid arguments, or other fatal errors

### 4.5 Verification stats tracking

Capture stats over time to track verification coverage:

```yaml
- name: Collect verification stats
  run: |
    for f in contracts/*.assura; do
      assura check "$f" --stats 2>&1 | tail -n 12
    done > stats.txt

- name: Upload stats
  uses: actions/upload-artifact@v4
  with:
    name: verification-stats
    path: stats.txt
```

### 4.6 Audit as a separate CI job

Run `assura audit` on PRs that modify Rust source (not just contracts):

```yaml
  audit:
    runs-on: ubuntu-latest
    if: contains(github.event.pull_request.changed_files, '.rs')
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Install Z3
        run: sudo apt-get install -y libz3-dev

      - name: Install Assura
        run: cargo install --git https://github.com/assura-lang/assura.git assura-cli

      - name: Run audit
        run: |
          assura audit . --format json --max-functions 100 > audit.json
          FINDINGS=$(jq '.findings' audit.json)
          if [ "$FINDINGS" -gt 0 ]; then
            echo "::warning::Assura audit found $FINDINGS potential issues"
            jq '.results[] | select(.severity == "warning")' audit.json
          fi
```

---

## Scenario 5: Team Onboarding and Progressive Adoption

You are introducing Assura to a team that has never used formal
verification. This scenario covers the learning progression, common
mistakes, and conventions.

### 5.1 Learning progression

**Week 1: Simple contracts.** Start with pure functions that have obvious
preconditions:

```assura
// Level 1: basic precondition
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b != 0 }
    ensures  { result * b + (a mod b) == a }
    effects  { pure }
}
```

Run `assura check` and `assura explain A05100` to understand
counterexamples.

**Week 2: Refinement types and multiple clauses.** Introduce refined
types and combine multiple ensures clauses:

```assura
type PositiveInt = { n: Int | n > 0 };

contract Clamp {
    input(value: Int, low: Int, high: Int)
    output(result: Int)

    requires { low <= high }
    ensures  { result >= low }
    ensures  { result <= high }
    ensures  { (value >= low and value <= high) implies result == value }
    effects  { pure }
}
```

**Week 3: Effect tracking.** Add effects to contracts for functions
with side effects:

```assura
contract ReadConfig {
    input(path: String)
    output(result: Map<String, String>)

    requires { path.length() > 0 }
    ensures  { result.length() >= 0 }
    effects  { fs }
}
```

**Week 4: Bind declarations.** Start attaching contracts to existing
Rust code using `bind`:

```assura
bind "crate::db::get_user" as get_user {
    input(id: Nat)
    output(result: User?)

    requires { id > 0 }
    ensures  { result.is_some() implies result.unwrap().id == id }
    effects  { database.read }
}
```

**Week 5+: Advanced features.** Explore quantifiers (`forall`, `exists`),
typestate, taint tracking, and memory regions. Use the demo files in
`demos/` as reference implementations.

### 5.2 "Tests pass but contract finds bug" example

This is the moment that sells Assura to skeptics. Consider this function
and its tests:

```rust
// src/math.rs
pub fn percentage(part: u64, total: u64) -> u64 {
    (part * 100) / total
}

// tests/math_test.rs
#[test]
fn test_percentage() {
    assert_eq!(percentage(50, 100), 50);
    assert_eq!(percentage(1, 4), 25);
    assert_eq!(percentage(0, 100), 0);
}
```

All tests pass. But the contract:

```assura
bind "crate::math::percentage" as percentage {
    input(part: Nat, total: Nat)
    output(result: Nat)

    requires { total > 0 }
    ensures  { result <= 100 }
}
```

Running `assura check`:

```
Error[A05100]: verification failed for percentage: ensures:
               counterexample: part = 101, total = 100

contracts/math.assura: 1 error(s) found
```

The counterexample reveals: when `part > total`, the result exceeds 100.
The tests never tested this case. The contract caught a real logic gap
that 100% test coverage on happy paths would miss.

Fix options:
- Add `requires { part <= total }` to forbid invalid inputs.
- Change the implementation to `std::cmp::min((part * 100) / total, 100)`.
- Add `requires { part <= total }` AND `ensures { result <= 100 }` so
  both the caller and the implementation are constrained.

### 5.3 When to write contracts vs tests

| Use contracts when | Use tests when |
|-------------------|----------------|
| The function has preconditions callers must satisfy | You need to test integration between multiple components |
| You want to prove a property holds for ALL inputs | You need to verify specific edge cases with exact values |
| The function handles untrusted or user-supplied input | You need to test performance or timing |
| The function performs arithmetic that might overflow | You need to test external service interactions |
| You want to enforce an API contract across a team | You need snapshot/golden-file testing |

Contracts and tests are complementary. Contracts prove universal
properties ("for all valid inputs, the output is bounded"). Tests verify
specific scenarios ("given this exact input, the output is this exact
value"). Use both.

### 5.4 Common mistakes and how to fix them

**Mistake 1: Vacuous contracts.** A contract that is always true catches
nothing:

```assura
# BAD: always true, catches nothing
contract Useless {
    input(x: Int)
    ensures { x == x }
}
```

Fix: write contracts that constrain behavior. If the ensures clause is
a tautology, delete it and write one that describes actual guarantees.

**Mistake 2: Wrong Assura types.** Using Rust types in contracts:

```assura
# BAD: Rust types in contract
contract Wrong {
    input(x: i64, y: usize)
    output(result: Vec<u8>)
}
```

Fix: use Assura types. `i64` is `Int`, `usize` is `Nat`, `Vec<u8>` is
`Bytes`. See the type mapping table in Scenario 2.

**Mistake 3: Missing effect declarations.** Forgetting to declare side
effects:

```assura
# BAD: reads from filesystem but declares pure
contract ReadFile {
    input(path: String)
    output(result: Bytes)
    effects { pure }
}
```

The type checker rejects this with error A07003 if the bound function
performs I/O. Fix: use `effects { fs }` or `effects { io }`.

**Mistake 4: Overly strict ensures.** Writing postconditions that are
true for the implementation but impossible to verify:

```assura
# BAD: Z3 cannot reason about string formatting
contract FormatDate {
    input(year: Nat, month: Nat, day: Nat)
    output(result: String)
    ensures { result == year.to_string() + "-" + month.to_string() + "-" + day.to_string() }
}
```

Z3 struggles with string concatenation and formatting. Fix: write
verifiable properties instead:

```assura
contract FormatDate {
    input(year: Nat, month: Nat, day: Nat)
    output(result: String)

    requires { month >= 1 and month <= 12 }
    requires { day >= 1 and day <= 31 }
    ensures  { result.length() >= 8 }
    ensures  { result.length() <= 10 }
}
```

**Mistake 5: Not running `assura explain`.** When you get an error code,
look it up:

```bash
assura explain A05100
```

Output:

```
A05100: Verification counterexample

The SMT solver found a concrete assignment of values to the contract's
input variables that satisfies the requires clauses but violates at
least one ensures clause.

Example:

  contract Foo {
    input(x: Int)
    ensures { x > 0 }   // <-- no requires constraining x
  }
  // counterexample: x = -1

How to fix:

  Add a requires clause that rules out the counterexample values,
  or weaken the ensures clause if the property is too strong.
```

### 5.5 Team conventions

Establish these conventions early:

1. **One contract file per Rust module.** If your Rust source is
   `src/parser.rs`, the contract goes in `contracts/parser.assura`.

2. **Contract names match function names.** Use `bind "crate::foo::bar"
   as bar`, not `as bar_contract` or `as BarContract`.

3. **Run `assura fmt` before committing.** Like `rustfmt` for Rust,
   `assura fmt` normalizes whitespace and indentation. Use
   `assura fmt --check` in CI to enforce formatting.

4. **Start with `requires`, then `ensures`, then `effects`.** Consistent
   clause ordering makes contracts scannable:

   ```assura
   bind "..." as name {
       input(...)
       output(...)
       requires { ... }
       ensures  { ... }
       effects  { ... }
   }
   ```

5. **Commit contracts alongside code changes.** If a PR changes a
   function's behavior, the contract must be updated in the same PR.

6. **Use `assura check --json` in code review tools.** Pipe JSON output
   into your review bot or CI annotations so reviewers see verification
   results inline.

7. **Do not ignore counterexamples.** Every counterexample is either a
   real bug or a contract that needs refinement. Both are valuable.
   Never suppress a counterexample without understanding it.
