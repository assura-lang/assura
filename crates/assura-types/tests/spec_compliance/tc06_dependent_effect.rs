//! Test Case 6: Dependent + Effect (Sized IO)
//! Dependent indices from effectful computations.

use super::must_compile;

#[test]
fn sized_io_read() {
    must_compile(
        r#"
contract ReadExact {
    input(n: Nat)
    output(result: Nat)
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
    input(stream_id: Int, count: Nat)
    requires(count > 0)
    output(bytes_read: Nat)
    ensures(bytes_read == count)
    effects: io
}
"#,
    );
}
