//! Test Case 6: Dependent + Effect (Sized IO)
//! Dependent indices from effectful computations.

use super::must_compile;

#[test]
fn sized_io_read() {
    must_compile(
        r#"
contract ReadExact {
    requires(n: Nat)
    ensures(result: Nat)
    ensures(result == n)
    effects: io
}
"#,
    );
}

#[test]
fn abstract_index_from_io() {
    must_compile(
        r#"
contract AbstractIndex {
    requires(stream_id: Int, count: Nat)
    requires(count > 0)
    ensures(bytes_read: Nat)
    ensures(bytes_read == count)
    effects: io
}
"#,
    );
}
