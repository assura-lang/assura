# Assura Quick Reference

## Types

| Rust | Assura | Notes |
|------|--------|-------|
| `i8`..`i128`, `isize` | `Int` | Arbitrary-precision signed |
| `u8`..`u128`, `usize` | `Nat` | Non-negative integer |
| `f32`, `f64` | `Float` | IEEE 754 |
| `bool` | `Bool` | |
| `String`, `&str` | `String` | UTF-8 |
| `Vec<u8>`, `&[u8]` | `Bytes` | Byte buffer |
| `()` | `Unit` | |
| `!`, `Infallible` | `Never` | |
| `Vec<T>` | `List<T>` | |
| `HashMap<K,V>` | `Map<K,V>` | |
| `HashSet<T>` | `Set<T>` | |
| `Option<T>` | `T?` | Nullable |
| `Result<T,E>` | `Result<T,E>` | Kept as-is |
| `Box<T>`, `Arc<T>`, `Rc<T>` | `T` | Wrapper erased |
| `&T`, `&mut T` | `T` | Reference erased |
| `(A, B, C)` | `(A, B, C)` | Tuples preserved |

## Refinement Types

```assura
x: Int where x > 0           // inline refinement
x: { n: Int | n >= 0 }       // set-builder syntax
data: NonEmpty<List<Int>>     // named refinement
```

## Declaration Forms

### Contract (standalone specification)

```assura
contract SafeDivision {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures  { result * b + (a mod b) == a }
  effects  { pure }
}
```

### Bind (attach contract to existing Rust function)

```assura
bind "mylib::divide" as divide_checked {
  input(a: Int, b: Int)
  output(result: Int)
  requires { b != 0 }
  ensures  { result == a / b }
}
```

### Function

```assura
fn helper(x: Int) -> Bool
  requires { x >= 0 }
  ensures  { result == (x mod 2 == 0) }
```

### Service (typestate)

```assura
service Connection {
  states { Disconnected, Connected, Authenticated }
  transitions {
    connect: Disconnected -> Connected
    login:   Connected -> Authenticated
    close:   * -> Disconnected
  }
}
```

### Other declarations

```assura
type Percentage = { n: Float | n >= 0.0 && n <= 100.0 }
enum Status { Active, Inactive, Suspended }
extern fn system_time() -> Nat effects { time }
block incremental { ... }
```

## Contract Clauses

| Clause | Purpose | Example |
|--------|---------|---------|
| `requires` | Precondition | `requires { x > 0 }` |
| `ensures` | Postcondition | `ensures { result >= 0 }` |
| `invariant` | Loop/type invariant | `invariant { len >= 0 }` |
| `decreases` | Termination measure | `decreases { n }` |
| `effects` | Side effect declaration | `effects { io, database }` |
| `where` | Type constraint | `where T: Comparable` |
| `modifies` | Frame condition | `modifies { buffer, count }` |
| `data_flow` | Taint/info-flow rule | `data_flow { input must_not_reach output }` |

## Expression Builtins

| Expression | Meaning |
|-----------|---------|
| `old(x)` | Value of `x` before function call |
| `result` | Return value (in `ensures`) |
| `forall x: T, P(x)` | Universal quantifier |
| `exists x: T, P(x)` | Existential quantifier |
| `abs(x)` | Absolute value |
| `length(xs)` | Collection length |
| `consumed(r)` | Linear resource consumed |

## Operator Precedence (low to high)

| Level | Operators |
|-------|-----------|
| 1 | `\|\|` (logical or) |
| 3 | `&&` (logical and) |
| 5 | `==`, `!=` (equality) |
| 7 | `<`, `>`, `<=`, `>=` (comparison) |
| 9 | `+`, `-` (additive) |
| 11 | `*`, `/`, `%`, `mod` (multiplicative) |
| -- | `!`, `-` (unary prefix) |
| -- | `.`, `()`, `[]` (postfix) |

## Effect Names

### Group effects (expand to sub-effects)

| Group | Sub-effects |
|-------|------------|
| `io` | `console.read`, `console.write`, `filesystem.read`, `filesystem.write`, `network.connect`, `network.send`, `network.receive`, `time.read`, `random` |
| `database` | `database.read`, `database.write` |
| `logging` | `log.debug`, `log.info`, `log.warn`, `log.error` |

### Short aliases

`mem`, `net`, `fs`, `rng`, `time`, `alloc`, `diverge`, `random`

### Pure functions

```assura
effects { pure }   // no side effects allowed
```

### Custom sub-effects

Any `group.sub` where `group` is known is accepted: `io.custom`, `database.migrate`.

## Error Code Ranges

| Range | Category | Example |
|-------|----------|---------|
| A01xxx | Syntax | A01001 unexpected token |
| A02xxx | Name resolution | A02001 undefined symbol |
| A03xxx | Types | A03001 type mismatch |
| A05xxx | Linearity | A05001 double use |
| A06xxx | Typestate | A06001 invalid transition |
| A07xxx | Effects | A07001 undeclared effect |
| A08xxx | Info-flow | A08001 taint leak |

Look up any code: `assura explain A03001`

## CLI Commands

```bash
assura check file.assura          # verify (parse + resolve + types + SMT)
assura check file.assura --watch  # re-verify on save
assura build file.assura          # verify + generate Rust code
assura init my-project            # scaffold new project
assura fmt file.assura            # format source
assura fmt file.assura --check    # check formatting (CI)
assura explain A03001             # explain error code
assura infer src/lib.rs           # generate bind skeletons from Rust
assura test-gen file.assura       # generate proptest code
assura audit .                    # scan Rust project for violations
assura coverage .                 # show contract coverage
assura doctor                     # check dependencies
assura agent-instructions         # AI agent quick reference
assura completions zsh            # generate shell completions
```

## Common Patterns

```assura
// Safe division
requires { divisor != 0 }

// Bounds check
requires { index >= 0 && index < length(data) }

// Non-empty input
requires { length(items) > 0 }

// Monotonicity
ensures { result >= old(counter) }

// Size preservation
ensures { length(result) == length(input) }
```