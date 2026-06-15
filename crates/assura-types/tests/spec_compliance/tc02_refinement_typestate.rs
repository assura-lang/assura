//! Test Case 2: Refinement + Typestate (Guarded Transitions)
//! State transitions whose validity depends on refinement predicates.

use super::{must_compile, must_reject};

#[test]
fn service_with_guarded_transitions() {
    must_compile(
        r#"
service LoanService {
    fn review(loan_id: Int) -> Int
        effects: database

    fn approve(loan_id: Int, amount: Int) -> Int
        requires { amount > 0 }
        effects: database

    fn deny(loan_id: Int) -> Int
        effects: database
}
"#,
    );
}

#[test]
fn guarded_transition_refinement() {
    must_compile(
        r#"
contract GuardedTransition {
    requires(credit_score: Int, amount: Int)
    requires(credit_score >= 650)
    requires(amount > 0)
    ensures(result: Bool)
    ensures(result == true)
}
"#,
    );
}

#[test]
fn reject_non_bool_invariant() {
    // Invariant clause must produce Bool, not Int.
    must_reject(
        r#"
contract BadInvariant {
    requires(x: Int)
    ensures(result: Int)
    invariant(x + 1)
}
"#,
        "A03",
    );
}
