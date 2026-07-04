## Pattern: length-preserving copy

**When**: collection return (`Bytes`, `String`, `List<…>`) and ensures like:

- `result.length() <= raw.length()`
- `result.length() == raw.length()`

**IR strategy**: `load` from the source parameter — havoc+assume links canonical
lengths when the postcondition constrains `result.length()` relative to input.

**Example** (`input(raw: Bytes)` → `ensures { result.length() <= raw.length() }`):

```
    $result = load $0 : Bytes
```

Do **not** emit separate length arithmetic unless the contract requires a strict
shrink (e.g. `result.length() == raw.length() - 1`); then use `arith sub` on
`call length` results.