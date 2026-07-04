## Pattern: struct field / construct

**When**: `ensures` mentions `result.field` or input struct fields, or return type
is a named struct from `TypeEnv`.

**IR strategy**:

- **Read**: `$n = field $slot .<index> : T` — field index matches struct field order.
- **Build**: `$n = construct StructName { .0 = $a, .1 = $b } : StructName`

Use struct field order from the contract's type definitions (first field = `.0`).

**Example** (`ensures { result.x == a + b }` with `Point { x, y }`):

```
    $2 = arith add $0 $1 : Int
    $3 = construct Point { .0 = $2, .1 = const 0 : Int } : Point
    $result = load $3 : Point
```

When only one field is constrained and others are free, still construct with
well-typed defaults or load from an input struct.