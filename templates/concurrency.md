# Concurrency Contract Generation

Given this Rust code that uses threads, async, or shared state:

```rust
{rust_concurrent_code}
```

Generate Assura contracts that verify concurrency safety. Use these
rules:

## When to Use Concurrency Contracts

Use these patterns when the Rust code has:
- `Mutex<T>`, `RwLock<T>`, `Arc<T>`, `AtomicT`
- `async fn`, `tokio::spawn`, `thread::spawn`
- Channels (`mpsc::channel`, `crossbeam::channel`)
- Shared mutable state across threads
- Callback or event handler registration

## Concurrency Features (CONC.1-6)

### CONC.1: Shared Memory Protocols

For code using `Mutex`, `RwLock`, or atomics:

<!-- STATUS: speculative (shared keyword, Mutex type, is_locked() not in parser) -->
```assura
contract SharedCounter {
    input(counter: shared Mutex<Nat>)
    output(result: Nat)

    requires { counter.is_locked() == false }
    invariant { counter.value >= 0 }
    ensures  { result == old(counter.value) + 1 }
    effects  { mem }
}
```

### CONC.2: Callback Re-entrancy

For event handlers or recursive callbacks:

<!-- STATUS: speculative (is_reentrant() not implemented) -->
```assura
contract EventHandler {
    input(handler: Fn(Event) -> Unit, events: List<Event>)
    output(result: Unit)

    // Handler must not trigger itself recursively
    requires { !handler.is_reentrant() }
    ensures  { all_events_processed(events) }
    effects  { io }
}
```

### CONC.3: Determinism

For functions that must produce the same result regardless of
scheduling:

<!-- STATUS: partially-implemented (parser handles syntax; deterministic() not a built-in) -->
```assura
contract DeterministicMerge {
    input(left: List<Int>, right: List<Int>)
    output(result: List<Int>)

    // Same inputs always produce same output
    ensures { deterministic(result, left, right) }
    ensures { length(result) == length(left) + length(right) }
    effects { pure }
}
```

### CONC.4: Lock Ordering

For code that acquires multiple locks:

<!-- STATUS: speculative (shared keyword, lock ordering checker is partial) -->
```assura
contract TransferFunds {
    input(
        from_account: shared Mutex<Account>,
        to_account: shared Mutex<Account>,
        amount: Nat
    )
    output(result: Bool)

    // Prevent deadlock by enforcing lock ordering
    requires { from_account.id < to_account.id }
    requires { amount > 0 }
    ensures  { result == true ==>
        old(from_account.balance) - amount == from_account.balance }
    effects  { database }
}
```

### CONC.5: Temporal Deadlines

For operations with time bounds:

<!-- STATUS: speculative (elapsed_ms() not a built-in) -->
```assura
contract TimedFetch {
    input(url: String, timeout_ms: Nat)
    output(result: Result<Bytes, Error>)

    requires { timeout_ms > 0 && timeout_ms <= 30000 }
    ensures  { elapsed_ms() <= timeout_ms + 100 }
    effects  { net, time }
}
```

### CONC.6: Weak Memory Ordering

For code using atomic operations:

<!-- STATUS: speculative (shared keyword, Atomic type, Ordering not in parser) -->
```assura
contract AtomicPublish {
    input(data: linear Buffer, flag: shared Atomic<Bool>)
    output(result: Unit)

    // Data must be fully written before flag is set
    requires { !flag.load(Ordering::Acquire) }
    ensures  { flag.load(Ordering::Acquire) == true }
    ensures  { data.is_initialized() }
    modifies { flag }
    effects  { mem }
}
```

## Channel Contracts

For producer-consumer patterns:

<!-- STATUS: speculative (Sender/Receiver types, is_closed(), buffer not implemented) -->
```assura
contract ChannelSend {
    input(tx: Sender<Message>, msg: Message)
    output(result: Result<Unit, Error>)

    requires { !tx.is_closed() }
    ensures  { result.is_ok() ==> msg in tx.buffer }
    effects  { mem }
}

contract ChannelReceive {
    input(rx: Receiver<Message>)
    output(result: Result<Message, Error>)

    ensures  { result.is_ok() ==> result.value in old(rx.buffer) }
    effects  { mem }
}
```

## Async Contracts

For async functions:

<!-- STATUS: fully-implemented (standard contract syntax with net/time effects) -->
```assura
contract AsyncFetch {
    input(urls: List<String>)
    output(results: List<Result<Bytes, Error>>)

    requires { length(urls) > 0 }
    requires { forall u: String, u in urls ==> length(u) > 0 }
    ensures  { length(results) == length(urls) }
    effects  { net, time }
}
```

## Type Mapping

See `templates/single-function.md` for the complete Rust-to-Assura
type table.

Additional concurrency types:
- `Mutex<T>` -> `shared Mutex<T>`
- `RwLock<T>` -> `shared RwLock<T>`
- `Arc<Mutex<T>>` -> `shared Mutex<T>` (wrapper erased)
- `AtomicBool`, `AtomicU64`, etc. -> `shared Atomic<Bool>`, `shared Atomic<Nat>`
- `Sender<T>` / `Receiver<T>` -> `Sender<T>` / `Receiver<T>`

## Common Mistakes to Avoid

- Do NOT use `pure` effects on functions that touch shared state
- Do NOT forget lock ordering constraints when acquiring multiple
  locks
- Do NOT assume atomics provide sequential consistency (specify
  ordering)
- Do NOT omit timeout bounds on blocking operations
- Always use `linear` for resources that must be consumed (channels,
  connections)