# Implementation IR Generation (base)

You generate **Implementation IR** (`.ir` sidecar text) that satisfies an Assura
contract. The SMT verifier proves your IR against `requires` / `ensures`; on
failure it returns a **counterexample** — revise the IR and resubmit.

> Pattern-specific rules are appended in the **Pattern overlay** section below.
> Do not duplicate syntax from this base document in your output.

## Contract under implementation

```
{contract_block}
```

## Module skeleton (always use this shape)

```
module {module_name} {
  fn #0 : ({param_slots}) -> {return_type} ! pure
  pre: true
  {
    // instructions — $result is the return slot
  }
}
```

- **Slots**: `$0`, `$1`, … map to contract parameters in `input()` order.
- **`$result`**: return value; final instruction must bind it via `load`.
- **Temp slots**: use the next free index (max used + 1).
- **Types**: annotate every instruction (`: Int`, `: Bytes`, etc.).

## Instruction reference

| Form | Meaning |
|------|---------|
| `$n = load $m : T` | Copy slot `$m` into `$n` |
| `$n = const V : T` | Literal constant |
| `$n = arith OP $a $b : T` | `add` `sub` `mul` `div` `mod` |
| `$n = call name ($a …) : T` | Call; callee IR must exist in sibling `.ir` |
| `$n = field $s .I : T` | Struct field by index (see type-aware ADT names in verifier) |
| `$n = construct Type { .I = $s … } : T` | Struct construction |
| `$n = if $c then #B else #C : T` | Conditional; define bodies in `fn #B` / `fn #C` |
| `$n = call length ($s) : Nat` | Collection `.length()` as `length`/`len` call |

## Rules

1. **Satisfy every `ensures`** under all `requires` — the verifier checks validity.
2. **Pure functions**: use `! pure` unless the contract declares other effects.
3. **No invented params** — only `$0..$N` from the contract signature.
4. **Prefer direct IR** over opaque calls when the postcondition is simple
   (load, arith, length identity).
5. **Cross-function `call`**: emit `call helper ($k)` only when a `{helper}.ir`
   sidecar exists or you generate both files in the same response.
6. Your response must be **only** the `.ir` text for `{decl_name}` — no markdown
   fences, no commentary. (The heuristic starter below is plain text for your
   reference; do not wrap your output the same way.)

## Verification loop

1. Run `assura check {source_file}` (or MCP `assura_check`).
2. If `Counterexample`: read the model, fix the IR body, repeat.
3. If `Verified`: done.

{pattern_section}