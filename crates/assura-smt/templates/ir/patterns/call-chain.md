## Pattern: call chain (multi-function)

**When**: logic is split across helpers, or `ensures` is easiest to satisfy by
delegating to another function whose contract is already verified.

**IR strategy**:

1. Emit a `{helper}.ir` sidecar for each callee (same module rules as base).
2. In the main body: `$k = call helper ($arg) : T` then `$result = load $k : T`.

**Example** (main doubles via helper):

`double.ir`:

```
module double {
  fn #0 : ($0: Int) -> Int ! pure
  pre: true
  {
    $1 = arith add $0 $0 : Int
    $result = load $1 : Int
  }
}
```

`main.ir`:

```
module main {
  fn #0 : ($0: Int) -> Int ! pure
  pre: true
  {
    $1 = call double ($0) : Int
    $result = load $1 : Int
  }
}
```

The verifier inlines callee bodies from loaded sidecars when `call` targets a
known function name.