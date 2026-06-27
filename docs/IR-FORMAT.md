# Implementation IR Format

The Implementation IR (Intermediate Representation) is the format AI agents
use to provide function bodies that satisfy Assura contracts. When `assura build`
generates Rust code, functions that lack an implementation contain `todo!()`.
An AI agent writes `.ir` sidecar files that replace those stubs with real logic.

The compiler parses the IR, encodes it into SMT constraints alongside the
contract clauses, and verifies that the implementation satisfies all
`requires`/`ensures` conditions.

## File Layout

IR files use the `.ir` extension and live alongside generated Rust source
(typically in `generated/` or beside demo `.assura` files).

```
module <ContractName> {
  fn #<id> : (<params>) -> <ReturnType> ! <effects>
  pre: <predicate>
  post: <predicate>
  {
    <instructions>
  }
}
```

### Example

From `demos/generated/HeartbeatSafeResponse.ir`:

```
module HeartbeatSafeResponse {
fn #0 : ($0: Nat, $1: Nat, $2: Nat) -> Unit ! pure
pre: true
{
    $result = load $0 : Unit
}
}
```

## Grammar

```ebnf
Module      ::= 'module' IDENT '{' Function* '}'
Function    ::= 'fn' FnId ':' Signature Pre? Post? '{' Instr* '}'
FnId        ::= '#' INTEGER
Signature   ::= '(' ParamList ')' '->' Type '!' Effect
ParamList   ::= (SlotDecl (',' SlotDecl)*)?
SlotDecl    ::= '$' INTEGER ':' Type
Pre         ::= 'pre:' Predicate
Post        ::= 'post:' Predicate
Effect      ::= 'pure' | 'io' | 'database' | ...

Instr       ::= '$' INTEGER '=' Expr ':' Type
              | '$result' '=' Expr ':' Type

Expr        ::= 'const' Literal
              | 'load' '$' INTEGER
              | 'call' IDENT '(' SlotList ')'
              | 'field' '$' INTEGER '.' INTEGER
              | 'construct' IDENT '{' FieldInit (',' FieldInit)* '}'
              | 'arith' ArithOp '$' INTEGER '$' INTEGER
              | 'cmp' CmpOp '$' INTEGER '$' INTEGER
              | 'cast' '$' INTEGER 'as' Type
              | 'if' '$' INTEGER 'then' '#' INTEGER 'else' '#' INTEGER
              | 'transition' '$' INTEGER 'to' IDENT
              | 'match' '$' INTEGER '{' MatchArm (',' MatchArm)* '}'
              | 'loop' '#' INTEGER '$' INTEGER

FieldInit   ::= '.' INTEGER '=' '$' INTEGER

Literal     ::= INTEGER | FLOAT | STRING | 'true' | 'false'

ArithOp     ::= 'add' | 'sub' | 'mul' | 'div' | 'mod'
CmpOp       ::= 'eq' | 'ne' | 'lt' | 'le' | 'gt' | 'ge'

Predicate   ::= 'true' | 'false'
              | 'cmp' CmpOp PredArg PredArg
              | 'and' Predicate Predicate
              | 'or' Predicate Predicate
              | 'not' Predicate

PredArg     ::= '$' INTEGER | '$result' | Literal
              | 'arith' ArithOp PredArg PredArg

MatchPattern ::= INTEGER | 'true' | 'false' | STRING | '_'

Type        ::= 'Int' | 'Nat' | 'Float' | 'Bool' | 'String'
              | 'Bytes' | 'Unit' | IDENT
```

## Instruction Reference

### `const` -- Literal value

```
$2 = const 42 : Int
$3 = const true : Bool
$4 = const "hello" : String
```

Creates a slot holding a literal value. Supported literals: integers,
floats, strings, booleans.

### `load` -- Copy a slot

```
$3 = load $0 : Int
$result = load $3 : Int
```

Copies the value from one slot to another. This is how the final return
value is set: `$result = load $N : Type`.

### `arith` -- Arithmetic operations

```
$3 = arith add $0 $1 : Int
$4 = arith mul $2 $3 : Int
$5 = arith div $0 $1 : Int
$6 = arith mod $0 $1 : Int
$7 = arith sub $0 $1 : Int
```

Binary arithmetic on two slots. Operators: `add`, `sub`, `mul`, `div`, `mod`.

### `cmp` -- Comparison operations

```
$3 = cmp gt $0 $1 : Bool
$4 = cmp eq $0 $1 : Bool
```

Binary comparison producing a Bool. Operators: `eq`, `ne`, `lt`, `le`,
`gt`, `ge`.

### `call` -- Function call

```
$3 = call validate ($0, $1) : Bool
```

Calls a named function with slot arguments. The function name references
another IR function or a built-in.

### `field` -- Field access

```
$2 = field $0 .1 : Int
```

Accesses the field at the given index from a struct or tuple slot.

### `construct` -- Build a struct

```
$3 = construct Point { .0 = $1, .1 = $2 } : Point
```

Creates a value of the given type from field assignments.

### `cast` -- Type cast

```
$2 = cast $0 as Float : Float
```

Converts a slot's value to a different type.

### `if` -- Conditional branch

```
$4 = if $3 then #1 else #2 : Int
```

Selects between two block results based on a boolean condition slot.
`#1` and `#2` reference block indices within the function.

### `match` -- Pattern match

```
$4 = match $0 { 0 => #0, 1 => #1, _ => #2 } : Int
```

Dispatches on the value of a slot to different block indices. Patterns
can be integer/boolean/string literals or `_` (wildcard).

### `loop` -- Loop construct

```
$5 = loop #1 $3 : Unit
```

Loops over block `#1` while condition slot `$3` is true.

### `transition` -- State transition

```
$2 = transition $0 to Active : Unit
```

Transitions a typestate machine to a new state. Used with service
declarations.

## Predicates (pre/post)

Preconditions and postconditions use a prefix-notation predicate language:

```
pre: cmp ne $1 (const 0)        // $1 != 0
post: cmp eq $result $0         // result == param 0
pre: and (cmp ge $0 (const 0)) (cmp lt $0 $1)  // $0 >= 0 && $0 < $1
pre: true                       // no precondition
```

`$result` refers to the function's return value in postconditions.

## Type Mapping

| IR Type  | Assura Type | Rust Type |
|----------|-------------|-----------|
| `Int`    | `Int`       | `i64`     |
| `Nat`    | `Nat`       | `u64`     |
| `Float`  | `Float`     | `f64`     |
| `Bool`   | `Bool`      | `bool`    |
| `String` | `String`    | `String`  |
| `Bytes`  | `Bytes`     | `Vec<u8>` |
| `Unit`   | `Unit`      | `()`      |

Custom types (structs, enums) use their Assura type name.

## Naming Conventions

- **Slots**: `$0`, `$1`, ... for parameters and locals. `$result` for the
  return value.
- **Functions**: `#0`, `#1`, ... for function IDs within a module.
- **Blocks**: `#0`, `#1`, ... for basic blocks in control flow.
- **Fields**: `.0`, `.1`, ... for positional field access.

## Writing IR for a Contract

Given a contract:

```assura
contract SafeDivision {
    input(dividend: Int, divisor: Int)
    requires { divisor != 0 }
    ensures { result == dividend / divisor }
}
```

The corresponding IR:

```
module SafeDivision {
fn #0 : ($0: Int, $1: Int) -> Int ! pure
pre: cmp ne $1 (const 0)
{
    $2 = arith div $0 $1 : Int
    $result = load $2 : Int
}
}
```

Steps:

1. Map contract parameters to slots: `dividend` = `$0`, `divisor` = `$1`
2. Map `requires` to `pre:` predicate
3. Write instructions that compute the result
4. Assign to `$result` at the end
5. The verifier checks that `post:` (derived from `ensures`) holds

## Validation

The `validate_ir_against_contract` function checks:

- Slot types match contract parameter types
- Effect annotations match the contract's effect declaration
- All slots are defined before use
- `$result` is assigned exactly once
- Type annotations are consistent

Parse errors are reported with line numbers. Use `parse_ir_module(source)`
to parse and validate.

## SMT Encoding

When verifying an IR body, the compiler:

1. Encodes each instruction as SMT constraints (via `apply_ir_body_constraints`)
2. Havocs the result slot, then asserts requires as assumptions
3. Checks that ensures clauses hold under those constraints
4. Reports `Verified`, `Counterexample`, or `Unknown`

The IR body replaces the unconstrained `$result` variable with a concrete
computation path, letting the SMT solver prove (or disprove) the contract.

## Source Code Reference

- Parser: `crates/assura-smt/src/ir.rs` (`parse_ir_module`, `parse_ir_function`)
- SMT encoding: `crates/assura-smt/src/ir_exec.rs` (`apply_ir_body_constraints`)
- IR term builders: `crates/assura-smt/src/ir_lower.rs` (`IrTermBuilder`)
- Validation: `crates/assura-smt/src/ir.rs` (`validate_ir_against_contract`)
