## Pattern: bounds / buffer safety

**When**: contracts guard indices or buffer sizes (CVE-style: CWE-120, CWE-787):

- `requires { offset < data.length() }`
- `requires { offset + len <= buffer.length() }`
- `ensures { result.length() <= max_size }`

**IR strategy**:

1. Implement the **safe** operation that respects requires (slice, copy with cap).
2. Use `call length ($buf)` for `.length()` on `Bytes`/`String`.
3. Use `arith add` / `cmp` only when the ensures clause demands computed sizes.
4. Default safe copy when ensures only caps output length: `load` from input.

**Example** (bounded read — copy at most `n` bytes from `buf`):

```
    $2 = call length ($0) : Nat
    // If contract ensures result.length() <= n, load preserves length link:
    $result = load $0 : Bytes
```

Pair with `requires` that prove access is in-range; the verifier assumes requires
when checking ensures.