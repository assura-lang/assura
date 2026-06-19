## Pattern: identity / copy

**When**: `ensures { result == <param> }` or returning the same value/type.

**IR strategy**: single load from the parameter slot.

**Example** (`input(x: Int)` → `ensures { result == x }`):

```
    $result = load $0 : Int
```

**Bytes/String copy** with length postcondition `result.length() <= x.length()`:

```
    $result = load $0 : Bytes
```

The verifier ties canonical lengths when the postcondition relates `result.length()`
to a parameter.