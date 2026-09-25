//! Test Case 8: Typestate + Effect + Refinement (Three-Way)
//! Payment processor combining all three features.

use super::must_compile;

#[test]
fn payment_processor_service() {
    must_compile(
        r#"
service PaymentProcessor {
    operation charge {
        input(payment_id: Int, amount: Int)
        requires { amount > 0 }
        effects: database
    }

    operation retry {
        input(payment_id: Int, retries: Int)
        requires { retries < 3 }
        effects: database
    }

    operation refund {
        input(payment_id: Int, amount: Int)
        requires { amount > 0 }
        effects: database
    }
}
"#,
    );
}

#[test]
fn bounded_retry_contract() {
    must_compile(
        r#"
contract BoundedRetry {
    input(retries: Nat, max_retries: Nat)
    requires(retries < max_retries)
    requires(max_retries == 3)
    output(result: Nat)
    ensures(result == retries + 1)
}
"#,
    );
}
