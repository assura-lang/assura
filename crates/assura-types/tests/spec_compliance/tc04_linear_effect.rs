//! Test Case 4: Linear + Effect (Resource-Scoped Effects)
//! Linear resources with scoped effects (e.g., database transactions).

use super::{must_compile, must_reject};

#[test]
fn transaction_with_effects() {
    must_compile(
        r#"
contract Transaction {
    requires(conn_id: Int)
    ensures(result: Bool)
    effects: database
}
"#,
    );
}

#[test]
fn effect_containment_in_function() {
    must_compile(
        r#"
fn with_transaction(conn_id: Int) -> Bool
    effects: database
{
    true
}
"#,
    );
}

#[test]
fn reject_unknown_effect() {
    // Unknown effect name should be rejected.
    must_reject(
        r#"
contract BadEffect {
    requires(x: Int)
    ensures(result: Int)
    effects: teleportation
}
"#,
        "A07",
    );
}
