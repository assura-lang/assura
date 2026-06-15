# Migration Guide: Dafny, Verus, and SPARK to Assura

This guide helps users of existing formal verification tools translate
their contracts and workflows to Assura. Each section covers syntax
mapping, key differences, common pitfalls, and a complete worked example.

---

## Dafny to Assura

### Syntax Mapping

| Dafny | Assura | Notes |
|-------|--------|-------|
| `method Foo(x: int) returns (r: int)` | `contract Foo { input(x: Int) output(result: Int) }` | Dafny combines spec+impl; Assura separates them |
| `requires x > 0` | `requires { x > 0 }` | Same keyword, Assura uses braces |
| `ensures r > 0` | `ensures { result > 0 }` | Assura uses `result` instead of named return |
| `decreases x` | `decreases { x }` | Same semantics |
| `invariant i >= 0` | `invariant { i >= 0 }` | Same semantics |
| `forall x :: P(x)` | `forall x in collection : P(x)` | Assura binds over collections |
| `exists x :: P(x)` | `exists x in collection : P(x)` | Assura binds over collections |
| `old(x)` | `old(x)` | Same semantics |
| `function F(x: int): int` | `contract F { ... effects { pure } }` | Dafny functions are ghost; Assura uses `pure` effect |
| `ghost var g: int` | `ghost { g: Int }` | Similar concept |
| `class C { ... }` | `type C { ... }` or `service C { ... }` | Assura uses types for data, services for stateful objects |
| `modifies this` | `effects { mem }` | Assura uses the effect system for mutation tracking |

### Key Differences

1. **Separation of concerns**: Dafny verifies implementation inline
   (the method body IS the implementation). Assura separates the
   contract (what) from the implementation (how). You write `.assura`
   files for contracts; AI or developers write Rust for implementations.

2. **Target language**: Dafny compiles to C#, Go, Java, JavaScript, or
   Python. Assura generates Rust source code.

3. **Effect tracking**: Dafny uses `reads`/`modifies` clauses. Assura
   uses a hierarchical effect system (`io`, `database`, `net`, etc.)
   that tracks fine-grained side effects.

4. **Linear types**: Dafny has no linear type system. Assura supports
   usage grades (`:_1`, `:_n`, `:_omega`) for resource management.

### Common Pitfalls

- **No method bodies**: Assura contracts do not contain implementation
  code. If you have `method Foo() { ... }` in Dafny, split it into a
  contract (`.assura`) and a Rust implementation.
- **Return values**: Dafny uses named returns (`returns (r: int)`).
  Assura uses `output(result: Type)` and the keyword `result` in
  ensures clauses.
- **Quantifier syntax**: Dafny uses `forall x :: P(x)`. Assura
  requires a binding domain: `forall x in range : P(x)`.

### Worked Example

**Dafny**:
```dafny
method BinarySearch(a: array<int>, key: int) returns (index: int)
  requires forall i, j :: 0 <= i < j < a.Length ==> a[i] <= a[j]
  ensures 0 <= index < a.Length ==> a[index] == key
  ensures index == -1 ==> forall i :: 0 <= i < a.Length ==> a[i] != key
{
  var lo, hi := 0, a.Length;
  while lo < hi
    invariant 0 <= lo <= hi <= a.Length
    invariant forall i :: 0 <= i < lo ==> a[i] < key
    invariant forall i :: hi <= i < a.Length ==> a[i] > key
  {
    var mid := (lo + hi) / 2;
    if a[mid] < key { lo := mid + 1; }
    else if a[mid] > key { hi := mid; }
    else { return mid; }
  }
  return -1;
}
```

**Assura**:
```assura
contract BinarySearch {
    input(a: List<Int>, key: Int)
    output(result: Int)

    requires { forall i in a : forall j in a : i < j ==> a[i] <= a[j] }

    ensures { result >= 0 ==> a[result] == key }
    ensures { result == 0 - 1 ==> forall i in a : a[i] != key }

    effects { pure }
}
```

---

## Verus to Assura

### Syntax Mapping

| Verus | Assura | Notes |
|-------|--------|-------|
| `proof fn lemma(x: int)` | `contract lemma { ... }` | Verus proof fns are ghost; Assura contracts are specs |
| `exec fn compute(x: u64) -> u64` | `bind "crate::compute" as compute { ... }` | Verus annotates Rust; Assura binds to existing Rust |
| `spec fn abstract_view(x: T) -> V` | `contract abstract_view { ... effects { pure } }` | Verus spec fns map to pure contracts |
| `requires(x > 0)` | `requires { x > 0 }` | Same keyword, different syntax |
| `ensures(|r| r > 0)` | `ensures { result > 0 }` | Verus uses closure; Assura uses `result` keyword |
| `decreases(x)` | `decreases { x }` | Same semantics |
| `tracked x: T` | `ghost { x: T }` | Verus "tracked" is similar to Assura ghost |
| `assert(P)` | `requires { P }` or `invariant { P }` | Context-dependent |
| `#[verifier::external_body]` | (default behavior) | Assura always separates spec from impl |

### Key Differences

1. **Integration model**: Verus annotates Rust source code directly
   with `proof`, `exec`, and `spec` keywords. Assura uses separate
   `.assura` files that reference Rust code via `bind` declarations.

2. **Same language**: Verus specs are written in Rust syntax. Assura
   has its own contract language with domain-specific features
   (typestate, effects, information flow).

3. **Ownership model**: Verus leverages Rust's ownership and borrowing
   directly. Assura has its own linear type system that maps to Rust
   ownership in codegen.

4. **SMT interaction**: Verus uses a custom SMT encoding with
   triggers and quantifier patterns. Assura uses Z3 with configurable
   verification layers (Layer 0: structural, Layer 1: basic SMT,
   Layer 2: full).

### Common Pitfalls

- **No inline verification**: Verus proves properties inline in Rust.
  Assura contracts are external specs. You cannot write `proof { ... }`
  blocks in Assura; use `ghost { ... }` clauses instead.
- **Type mapping**: Verus uses Rust types (`u64`, `Vec<T>`). Assura
  uses abstract types (`Nat`, `List<T>`) that map to Rust types in
  codegen.
- **Tracked vs ghost**: Verus `tracked` variables participate in
  ownership. Assura `ghost` variables are fully erased at runtime.

### Worked Example

**Verus**:
```rust
use vstd::prelude::*;

verus! {
    spec fn triangle(n: nat) -> nat
        decreases n
    {
        if n == 0 { 0 }
        else { n + triangle((n - 1) as nat) }
    }

    proof fn triangle_is_monotonic(i: nat, j: nat)
        requires i <= j
        ensures triangle(i) <= triangle(j)
        decreases j - i
    {
        if i < j {
            triangle_is_monotonic(i, (j - 1) as nat);
        }
    }
}
```

**Assura**:
```assura
contract triangle {
    input(n: Nat)
    output(result: Nat)

    ensures { result >= 0 }
    decreases { n }

    effects { pure }
}

contract triangle_is_monotonic {
    input(i: Nat, j: Nat)
    output(result: Unit)

    requires { i <= j }
    ensures { true }

    decreases { j - i }

    effects { pure }
}
```

---

## SPARK Ada to Assura

### Syntax Mapping

| SPARK Ada | Assura | Notes |
|-----------|--------|-------|
| `Pre => X > 0` | `requires { X > 0 }` | Same semantics |
| `Post => Result > 0` | `ensures { result > 0 }` | SPARK uses `Result`, Assura uses `result` |
| `Global => (Input => X)` | `effects { pure }` or specific effects | SPARK Global maps to Assura effects |
| `Depends => (Result => X)` | `data_flow { result depends_on x }` | Direct mapping |
| `Contract_Cases` | Multiple `requires`/`ensures` pairs | Assura uses clause repetition |
| `type T is new Integer range 1 .. 100` | refinement type `{ v: Int \| v >= 1 && v <= 100 }` | SPARK subtypes map to refinement types |
| `pragma Assert(P)` | `invariant { P }` | Context-dependent |
| `with SPARK_Mode => On` | (always on in `.assura` files) | Assura files are always verified |

### Key Differences

1. **Language family**: SPARK annotates Ada code with contracts.
   Assura generates Rust code. The underlying language families are
   very different (Ada is a systems language with tasking; Rust has
   ownership and borrowing).

2. **Information flow**: SPARK has `Global` and `Depends` for data
   flow tracking. Assura has both `effects` (for side effects) and
   `data_flow` (for information flow / taint tracking). Assura's
   system is more granular.

3. **Concurrency**: SPARK uses Ada tasking with Ravenscar profile.
   Assura uses effect-based concurrency tracking.

4. **Tooling**: SPARK uses GNATprove (based on Why3/Alt-Ergo/CVC5).
   Assura uses Z3 as the primary solver with CVC5 as a fallback.

### Common Pitfalls

- **No subtype ranges**: SPARK's `range 1 .. 100` does not have a
  direct Assura equivalent. Use refinement types:
  `type Bounded = { v: Int | v >= 1 && v <= 100 }`.
- **Global annotations**: SPARK's `Global => (In_Out => State)` maps
  to Assura's effect system. Use `effects { mem }` for in-memory
  state modification.
- **Package structure**: SPARK uses Ada packages (spec + body). Assura
  uses modules (`.assura` files). Each SPARK package spec maps to one
  Assura module.

### Worked Example

**SPARK Ada**:
```ada
package Stack
  with SPARK_Mode => On
is
   Max_Size : constant := 100;
   type Element is new Integer;

   procedure Push (E : in Element)
     with Pre  => not Is_Full,
          Post => Size = Size'Old + 1 and Top = E,
          Global => (In_Out => State);

   function Is_Full return Boolean
     with Post => Is_Full'Result = (Size = Max_Size),
          Global => (Input => State);

   function Size return Natural
     with Global => (Input => State);

   function Top return Element
     with Pre    => Size > 0,
          Global => (Input => State);
end Stack;
```

**Assura**:
```assura
module stack

type Element = Int

contract push {
    input(e: Element)
    output(result: Unit)

    requires { size < 100 }
    ensures { old(size) + 1 == size }

    effects { mem }
}

contract is_full {
    input()
    output(result: Bool)

    ensures { result == (size == 100) }

    effects { pure }
}

contract size {
    input()
    output(result: Nat)

    ensures { result >= 0 }
    ensures { result <= 100 }

    effects { pure }
}

contract top {
    input()
    output(result: Element)

    requires { size > 0 }

    effects { pure }
}
```

---

## Quick Reference: Equivalent Concepts

| Concept | Dafny | Verus | SPARK | Assura |
|---------|-------|-------|-------|--------|
| Precondition | `requires` | `requires()` | `Pre =>` | `requires { }` |
| Postcondition | `ensures` | `ensures(\|r\| )` | `Post =>` | `ensures { }` |
| Loop invariant | `invariant` | `invariant()` | `Loop_Invariant` | `invariant { }` |
| Termination | `decreases` | `decreases()` | `Subprogram_Variant` | `decreases { }` |
| Old value | `old(x)` | `old(x)` | `X'Old` | `old(x)` |
| Return value | named return | closure param | `Result` | `result` |
| Ghost code | `ghost var` | `tracked`/`ghost` | `Ghost` | `ghost { }` |
| Pure function | `function` | `spec fn` | no side effect | `effects { pure }` |
| Data flow | `reads`/`modifies` | n/a | `Depends` | `data_flow { }` |
| Side effects | `modifies` | n/a | `Global` | `effects { io, mem, ... }` |
| Refinement type | subset type | n/a | subtype range | `{ v: T \| P }` |
| Linear type | n/a | ownership | n/a | `:_1`, `:_n` |
| Typestate | n/a | n/a | n/a | `states { }` |
