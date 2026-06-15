//! Test Case 7: Linear + Information Flow (Secret Key Protocol)
//! Cryptographic keys that are both linear and security-labeled.

use super::must_compile;

#[test]
fn sign_once_protocol() {
    must_compile(
        r#"
contract SignOnce {
    requires(key_id: Int, message: Bytes)
    ensures(result: Bytes)
    effects: io
}
"#,
    );
}

#[test]
fn key_consumption() {
    must_compile(
        r#"
contract KeyConsumption {
    requires(key: Bytes, data: Bytes)
    ensures(signature: Bytes)
}
"#,
    );
}
