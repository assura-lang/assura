# Module-Level Contract Generation

Given this Rust module with {n} public functions:

```rust
{rust_module_source}
```

Generate Assura contracts for each public function. Focus on:

1. **Buffer/index bounds**: any array/slice access must be within bounds
2. **Null/None handling**: any `Option` unwrap or expect must have preconditions
3. **Arithmetic overflow**: any unchecked add/mul/sub must have range preconditions
4. **Resource lifecycle**: any open/close, lock/unlock, connect/disconnect must be paired
5. **Input validation**: any function taking user-supplied data must validate it

Skip trivial getters (single field access) and simple constructors (just field init).

## Type Mapping
See `templates/single-function.md` for the complete Rust-to-Assura type table.

## Output Format
Generate one `bind` declaration per function, grouped by logical concern:

```
// Buffer safety contracts
bind "module::read_at" as read_at_checked { ... }

// Arithmetic safety contracts
bind "module::compute" as compute_checked { ... }
```

## Effect Annotations
Common effect mappings from Rust patterns:
- `std::fs::*`, `std::io::*` -> `effects { io, fs }`
- `std::net::*`, `reqwest::*` -> `effects { io, net }`
- Database crates (`sqlx`, `diesel`, `sea-orm`) -> `effects { io, database }`
- `rand::*` -> `effects { rng }`
- `std::time::*`, `chrono::*` -> `effects { time }`
- Memory allocation (`Box::new`, `Vec::with_capacity`) -> `effects { mem, alloc }`