# Single Function Contract Generation

Given this Rust function signature:

```rust
{rust_function_signature}
```

Generate an Assura `bind` contract. Use these rules:

## Type Mapping (Rust -> Assura)
- `i8`, `i16`, `i32`, `i64`, `i128`, `isize` -> `Int`
- `u8`, `u16`, `u32`, `u64`, `u128`, `usize` -> `Nat`
- `f32`, `f64` -> `Float`
- `bool` -> `Bool`
- `String`, `&str` -> `String`
- `Vec<u8>`, `&[u8]` -> `Bytes`
- `()` -> `Unit`
- `Vec<T>` -> `List<T>`
- `HashMap<K,V>`, `BTreeMap<K,V>` -> `Map<K, V>`
- `HashSet<T>`, `BTreeSet<T>` -> `Set<T>`
- `Option<T>` -> `T?`
- `Box<T>`, `Arc<T>`, `Rc<T>` -> `T` (wrapper erasure)
- `&T`, `&mut T` -> `T` (reference erasure)

## Contract Structure

<!-- STATUS: fully-implemented -->
```
bind "{module_path}::{name}" as {name}_checked {{
    input({params_with_assura_types})
    output(result: {assura_return_type})
    requires {{ preconditions }}
    ensures  {{ postconditions }}
    effects  {{ side_effects }}
}}
```

## What to Check
- Write `requires` clauses for preconditions (null checks, bounds, non-empty)
- Write `ensures` clauses for postconditions (return value properties, state changes)
- Write `effects` for side effects: `io`, `database`, `fs`, `net`, `mem`, `rng`, `time`
- Use `old(x)` in ensures to reference pre-call values
- Use `result` to reference the return value in ensures
- Do NOT write vacuous contracts (always-true conditions catch nothing)