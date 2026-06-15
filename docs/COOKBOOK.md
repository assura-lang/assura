# Assura Contract Cookbook

Ready-to-copy contract patterns organized by category. Each pattern
is self-contained. For syntax basics, see [the tutorial](TUTORIAL.md).

## Arithmetic Safety

### Safe Division

**Prevents:** division by zero, incorrect quotient

```assura
contract SafeDivision {
    input(a: Int, b: Int)
    output(result: Int)

    requires { b != 0 }
    ensures  { result * b + (a mod b) == a }
    ensures  { abs(result) <= abs(a) }
    effects  { pure }
}
```

### Integer Overflow Guard

**Prevents:** silent integer overflow on addition

```assura
contract SafeAdd {
    input(a: Int, b: Int, max: Int)
    output(result: Int)

    requires { a >= 0 }
    requires { b >= 0 }
    requires { a + b <= max }
    ensures  { result == a + b }
    ensures  { result >= 0 }
    effects  { pure }
}
```

### Percentage Bounds

**Prevents:** percentage values outside 0..100

```assura
type Percentage = { v: Float | v >= 0.0 && v <= 100.0 };

contract ApplyDiscount {
    input(price: Float, discount: Percentage)
    output(result: Float)

    requires { price >= 0.0 }
    ensures  { result >= 0.0 }
    ensures  { result <= price }
    effects  { pure }
}
```

## Bounds Checking

### Safe Array Index

**Prevents:** out-of-bounds array access

```assura
contract SafeIndex {
    input(arr: List<Int>, index: Nat)
    output(result: Int)

    requires { index < arr.length() }
    effects  { pure }
}
```

### Bounded Slice

**Prevents:** slice overrun past buffer end

```assura
contract SafeSlice {
    input(buf: Bytes, offset: Nat, len: Nat)
    output(data: Bytes)

    requires { offset + len <= buf.length() }
    ensures  { data.length() == len }
    effects  { pure }
}
```

### Buffer Capacity

**Prevents:** writes past allocated buffer capacity

```assura
contract BoundedBuffer {
    input(capacity: Nat, count: Nat, item_size: Nat)

    requires { capacity > 0 }
    requires { count * item_size <= capacity }
    invariant { count * item_size <= capacity }
    effects  { pure }
}
```

## String and Bytes

### Non-Empty String

**Prevents:** empty string passed where content is required

```assura
contract NonEmptyInput {
    input(name: String)

    requires { name.length() > 0 }
    effects  { pure }
}
```

### Bounded String Length

**Prevents:** oversized strings causing truncation or overflow

```assura
contract BoundedString {
    input(value: String, max_len: Nat)

    requires { value.length() > 0 }
    requires { value.length() <= max_len }
    requires { max_len <= 65535 }
    effects  { pure }
}
```

## Option/Result Safety

### Safe Unwrap via Precondition

**Prevents:** unwrap on None/Err

```assura
contract SafeUnwrap {
    input(value: Int?, default_val: Int)
    output(result: Int)

    ensures { result == if value != null then value else default_val }
    effects { pure }
}
```

### Error Propagation

**Prevents:** unhandled error cases

```assura
fn parse_port(s: String) -> Int?
    requires { s.length() > 0 }
    ensures  { result != null implies result >= 1 }
    ensures  { result != null implies result <= 65535 }
    effects  { pure }
```

## Collection Properties

### Non-Empty Collection

**Prevents:** operations on empty collections (head, reduce, min, max)

```assura
contract NonEmptyList {
    input(items: List<Int>)
    output(result: Int)

    requires { items.length() > 0 }
    effects  { pure }
}
```

### Sorted Output

**Prevents:** sort functions that return unsorted data

```assura
contract SortContract {
    input(arr: List<Int>)
    output(result: List<Int>)

    requires { arr.length() > 0 }
    ensures  { result.length() == arr.length() }
    ensures  { forall i in 0..result.length() - 1: result[i] <= result[i + 1] }
    effects  { pure }
}
```

### Element Uniqueness

**Prevents:** duplicate entries in collections that require distinct elements

```assura
contract UniqueElements {
    input(items: List<Int>, new_item: Int)
    output(result: List<Int>)

    requires { forall i in items: i != new_item }
    ensures  { result.length() == items.length() + 1 }
    effects  { pure }
}
```

## Monotonicity and Ordering

### Monotonic Counter

**Prevents:** counter decrement, stale sequence numbers

```assura
contract IncrementCounter {
    input(current: Nat, amount: Nat)
    output(result: Nat)

    requires { amount > 0 }
    ensures  { result > current }
    ensures  { result == current + amount }
    effects  { pure }
}
```

### Timestamp Ordering

**Prevents:** out-of-order event timestamps

```assura
contract AppendEvent {
    input(last_ts: Nat, new_ts: Nat)

    requires { new_ts > last_ts }
    ensures  { new_ts > last_ts }
    effects  { pure }
}
```

## Resource Lifecycle

### Connection Open/Close

**Prevents:** use-after-close, double-close, resource leaks

```assura
service Connection {
    states: Closed -> Open -> Closed

    operation Open {
        input(host: String)
        requires { host.length() > 0 }
        requires { self.state == Closed }
        ensures  { self.state == Open }
        effects  { net }
    }

    operation Send {
        input(data: Bytes)
        requires { self.state == Open }
        requires { data.length() > 0 }
        ensures  { self.state == Open }
        effects  { net }
    }

    operation Close {
        requires { self.state == Open }
        ensures  { self.state == Closed }
        effects  { net }
    }
}
```

### Acquire/Release Lock

**Prevents:** double-acquire, use without lock, forgotten release

```assura
service Mutex {
    states: Unlocked -> Locked -> Unlocked

    operation Acquire {
        requires { self.state == Unlocked }
        ensures  { self.state == Locked }
        effects  { mem }
    }

    operation Release {
        requires { self.state == Locked }
        ensures  { self.state == Unlocked }
        effects  { mem }
    }
}
```

## Effects and Purity

### Pure Computation

**Prevents:** accidental side effects in business logic

```assura
contract PureTransform {
    input(items: List<Int>)
    output(result: List<Int>)

    requires { items.length() > 0 }
    ensures  { result.length() == items.length() }
    effects  { pure }
}
```

### IO Isolation

**Prevents:** database access from code that should only do network IO

```assura
fn fetch_remote(url: String) -> Bytes
    requires { url.length() > 0 }
    effects  { net }

fn save_to_db(data: Bytes) -> Bool
    requires { data.length() > 0 }
    effects  { database.write }

fn api_handler(url: String) -> Bool
    requires { url.length() > 0 }
    effects  { net, database.write }
```

## Taint Tracking

### Untrusted Input Validation

**Prevents:** unsanitized user input reaching sensitive operations

```assura
fn read_user_input() -> String @taint:untrusted
    effects { io }

fn validate_input(
    raw: String @taint:untrusted,
    max_len: Nat
) -> String @taint:validated
    requires { max_len > 0 }
    effects  { pure }
{
    validate {
        raw.length() > 0 && raw.length() <= max_len
    } raw
        or ""
}

fn execute_query(query: String @taint:validated) -> Int
    effects { database.read }
```

## Quantifiers

### All Elements Positive

**Prevents:** negative values slipping into a non-negative collection

```assura
contract AllPositive {
    input(items: List<Int>)

    requires { items.length() > 0 }
    ensures  { forall i in items: i >= 0 }
    effects  { pure }
}
```

### Element Exists

**Prevents:** search returning not-found when element is guaranteed present

```assura
contract FindElement {
    input(arr: List<Int>, n: Nat, target: Int)
    output(result: Int)

    requires { n > 0 }
    requires { exists i in 0..n: arr[i] == target }
    ensures  { result == target }
    effects  { pure }
}
```

## Bind Declarations

### Retrofit Existing Rust Function

**Prevents:** calling an existing Rust function without contract enforcement

```assura
bind "my_crate::math::divide" as safe_divide {
    input(a: Int, b: Int)
    output(result: Int)
    requires { b != 0 }
    ensures  { result * b == a }
}
```

### Bind with Effects

**Prevents:** calling an FFI function without declaring its side effects

```assura
bind "libc::malloc" as safe_malloc {
    input(size: Nat)
    output(result: Bytes)
    requires { size > 0 }
    ensures  { result.length() == size }
    effects  { mem }
}
```

## Key Derivation and Crypto

### Secure Key Length

**Prevents:** weak cryptographic keys

```assura
contract SecureKeyDerivation {
    input(password_len: Nat, salt_len: Nat, iterations: Nat)

    requires { password_len >= 8 }
    requires { salt_len >= 16 }
    requires { iterations >= 100000 }
    effects  { pure }
}
```
