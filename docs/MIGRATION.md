# Migration Guide

Translating contracts from Dafny, Verus, and SPARK Ada to Assura.

## Dafny to Assura

### Syntax Mapping

| Dafny | Assura | Notes |
|-------|--------|-------|
| `method M(x: int) returns (r: int)` | `contract M { input(x: Int) output(r: Int) }` | Dafny methods become Assura contracts |
| `function F(x: int): int` | `fn F(x: Int) -> Int` | Pure functions map directly |
| `requires x > 0` | `requires { x > 0 }` | Same keyword, Assura uses braces |
| `ensures r > x` | `ensures { result > x }` | Assura uses `result` instead of named return |
| `decreases n` | `decreases { n }` | Same semantics |
| `forall x :: P(x)` | `forall x: T, P(x)` | Assura requires type annotation |
| `exists x :: P(x)` | `exists x: T, P(x)` | Assura requires type annotation |
| `old(x)` | `old(x)` | Same |
| `modifies this` | `modifies { self }` | |
| `invariant I` | `invariant { I }` | |
| `ghost var x: int` | `ghost { x: Int }` | |
| `int` | `Int` | Capitalized |
| `bool` | `Bool` | Capitalized |
| `seq<int>` | `List<Int>` | |
| `set<int>` | `Set<Int>` | |
| `map<int, int>` | `Map<Int, Int>` | |

### Key Differences

1. **Separation of concerns.** Dafny verifies implementation inline
   (code and contracts in the same file). Assura separates contracts
   (.assura files) from implementation (Rust files). AI generates the
   implementation; you write the contract.

2. **Target language.** Dafny compiles to C#, Java, JavaScript, Go, or
   Python. Assura compiles exclusively to Rust.

3. **Effect system.** Dafny has `reads` and `modifies` clauses. Assura
   has a full effect system with `effects { io, database, fs, ... }`.

4. **Linear types.** Dafny does not have linear types. Assura supports
   `linear` for resource tracking.

### Example: Safe Division

**Dafny:**

```dafny
method SafeDivision(a: int, b: int) returns (r: int)
  requires b != 0
  ensures r * b + (a % b) == a
  ensures abs(r) <= abs(a)
{
  r := a / b;
}
```

**Assura:**

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

### Common Pitfalls

- Dafny's `returns (r: int)` names the return variable. In Assura, use
  `result` in `ensures` clauses.
- Dafny's `seq` is immutable. Assura's `List<T>` maps to Rust `Vec<T>`,
  which is mutable at runtime. The contract treats it as a value.
- Dafny's `forall x :: x > 0 ==> P(x)` needs a type in Assura:
  `forall x: Int, x > 0 ==> P(x)`.

## Verus to Assura

### Syntax Mapping

| Verus | Assura | Notes |
|-------|--------|-------|
| `proof fn lemma(...)` | `contract Lemma { ... }` | Proof functions become contracts |
| `exec fn run(...)` | `bind "crate::run" as run_checked { ... }` | Exec functions become binds |
| `spec fn property(...)` | `fn property(...) -> Bool` | Spec functions become pure fns |
| `requires(x > 0)` | `requires { x > 0 }` | Different delimiter style |
| `ensures(|r| r > 0)` | `ensures { result > 0 }` | Verus uses closures for result |
| `decreases(n)` | `decreases { n }` | Same semantics |
| `#[verifier::spec]` | `ghost { ... }` | Ghost/spec annotations |
| `old(x)` | `old(x)` | Same |
| `int` | `Int` | |
| `nat` | `Nat` | |
| `Seq<int>` | `List<Int>` | |
| `Set<int>` | `Set<Int>` | |
| `Map<int, int>` | `Map<Int, Int>` | |

### Key Differences

1. **Annotation style.** Verus annotates Rust code directly with
   `requires`, `ensures`, and `#[verifier]` attributes. Assura uses
   separate `.assura` files.

2. **Same target.** Both target Rust, but Verus verifies existing Rust;
   Assura generates new Rust from contracts.

3. **Proof mode.** Verus has `proof`, `exec`, and `spec` modes. Assura
   has `contract` (verified), `bind` (attached to existing code), and
   `fn` (specification).

4. **Linear types.** Verus uses Rust's ownership model. Assura has
   explicit `linear` annotations.

### Example: Binary Search

**Verus:**

```rust
proof fn binary_search(arr: &[i64], target: i64) -> (idx: Option<usize>)
    requires
        forall|i: int, j: int|
            0 <= i < j < arr.len() ==> arr[i] <= arr[j],
    ensures
        match idx {
            Some(i) => arr[i as int] == target,
            None => forall|i: int| 0 <= i < arr.len() ==> arr[i] != target,
        }
```

**Assura:**

```assura
contract BinarySearch {
    input(arr: List<Int>, target: Int)
    output(result: Nat?)

    requires { forall i: Nat, j: Nat,
        i < j && j < length(arr) ==> arr[i] <= arr[j] }

    ensures { result.is_some() ==>
        arr[result.value] == target }
    ensures { result.is_none() ==>
        forall i: Nat, i < length(arr) ==> arr[i] != target }

    effects { pure }
}
```

### Common Pitfalls

- Verus `ensures(|r| ...)` uses a closure to name the result. In
  Assura, use `result` directly.
- Verus integer types are Rust types (`i64`, `usize`). Assura uses
  `Int` and `Nat`.
- Verus `Seq` is a verified sequence type. Assura uses `List<T>` which
  maps to `Vec<T>`.
- Verus proof code runs at compile time only. Assura contracts are
  always separate from runtime code.

## SPARK Ada to Assura

### Syntax Mapping

| SPARK | Assura | Notes |
|-------|--------|-------|
| `function F(X: Integer) return Integer` | `contract F { input(x: Int) output(result: Int) }` | |
| `with Pre => X > 0` | `requires { x > 0 }` | |
| `Post => F'Result > 0` | `ensures { result > 0 }` | `F'Result` becomes `result` |
| `Global => (Input => State)` | `effects { ... }` | Global maps to effects |
| `Depends => (Result => X)` | `data_flow { x must_reach result }` | Data dependency |
| `Contract_Cases` | Multiple `ensures` clauses | |
| `Integer` | `Int` | |
| `Natural` | `Nat` | |
| `Boolean` | `Bool` | |
| `pragma Assert(P)` | `invariant { P }` | |

### Key Differences

1. **Language family.** SPARK annotates Ada code. Assura generates
   Rust code. The paradigms are very different (Ada is imperative OOP;
   Rust is ownership-based).

2. **Flow analysis.** SPARK has built-in flow analysis via `Global`
   and `Depends`. Assura uses `effects` and `data_flow` clauses for
   similar purposes.

3. **Proof levels.** SPARK has 4 proof levels (stone, bronze, silver,
   gold). Assura has Layer 0 (structural) and Layer 1 (SMT).

4. **Information flow.** SPARK's `Depends` maps closely to Assura's
   `data_flow` clauses for taint tracking.

### Example: Absolute Value

**SPARK:**

```ada
function Abs_Value (X : Integer) return Natural
  with Pre  => X > Integer'First,
       Post => (if X >= 0 then Abs_Value'Result = X
                else Abs_Value'Result = -X);
```

**Assura:**

```assura
contract AbsValue {
    input(x: Int)
    output(result: Nat)

    requires { x > min_int() }
    ensures  { x >= 0 ==> result == x }
    ensures  { x < 0 ==> result == -x }
    effects  { pure }
}
```

### Common Pitfalls

- SPARK's `F'Result` becomes `result` in Assura.
- SPARK's `Global => (In_Out => State)` does not have a direct
  equivalent; use `effects` and `modifies` clauses together.
- SPARK's `Contract_Cases` (disjoint case analysis) maps to multiple
  `ensures` clauses with guards. Assura does not enforce disjointness
  automatically.
- Ada subtypes (`Natural`, `Positive`) map to refinement types:
  `Nat` for `Natural`, `{ n: Int | n > 0 }` for `Positive`.

## Quick Comparison

| Feature | Dafny | Verus | SPARK | Assura |
|---------|-------|-------|-------|--------|
| Target language | C#/Java/JS/Go/Python | Rust | Ada | Rust |
| Contract location | Inline with code | Inline with code | Inline with code | Separate .assura files |
| SMT solver | Z3 | Z3 | CVC4/Z3 | Z3 (CVC5 fallback) |
| Linear types | No | Via Rust ownership | No | Yes (`linear`) |
| Effect system | `reads`/`modifies` | Limited | `Global`/`Depends` | Full (`effects`, `data_flow`) |
| Typestate | No | No | Limited (SPARK modes) | Yes (`service` + `states`) |
| AI integration | No | No | No | Native (agent-instructions, infer, audit) |