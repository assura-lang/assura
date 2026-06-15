//! Test Case 8: Typestate + Effect + Refinement (Three-Way)
//! Payment processor combining all three features.

use super::must_compile;

#[test]
fn payment_processor_service() {
    must_compile(
        r#"
service PaymentProcessor {
    fn charge(payment_id: Int, amount: Int) -> Bool
        requires { amount > 0 }
        effects: database

    fn retry(payment_id: Int, retries: Int) -> Bool
        requires { retries < 3 }
        effects: database

    fn refund(payment_id: Int, amount: Int) -> Bool
        requires { amount > 0 }
        effects: database
}
"#,
    );
}

#[test]
fn bounded_retry_contract() {
    must_compile(
        r#"
contract BoundedRetry {
    requires(retries: Nat, max_retries: Nat)
    requires(retries < max_retries)
    requires(max_retries == 3)
    ensures(result: Nat)
    ensures(result == retries + 1)
}
"#,
    );
}
