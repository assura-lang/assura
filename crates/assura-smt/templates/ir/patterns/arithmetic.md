## Pattern: arithmetic

**When**: `ensures { result == <expr> }` where `<expr>` uses `+`, `-`, `*`, `/`, `%`
on parameters or literals.

**IR strategy**: one `arith` temp, then `load` into `$result`.

**Example** (`ensures { result == x + y }`):

```
    $2 = arith add $0 $1 : Int
    $result = load $2 : Int
```

Map operators: `+`→`add`, `-`→`sub`, `*`→`mul`, `/`→`div`, `%`→`mod`.

Literals: `$n = const 42 : Int` then use `$n` in `arith`.