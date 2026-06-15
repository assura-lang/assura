//! Test Case 10: Conditional Typestate (Branch Divergence)
//! Operations that may transition to different states per branch.

use super::must_compile;

#[test]
fn order_processor_service() {
    must_compile(
        r#"
service OrderProcessor {
    fn process(order_id: Int, has_stock: Bool) -> Int
        effects: database

    fn ship(order_id: Int, tracking: String) -> Int
        effects: database

    fn cancel(order_id: Int, reason: String) -> Int
        effects: database
}
"#,
    );
}

#[test]
fn branch_divergence_contract() {
    must_compile(
        r#"
contract BranchDivergence {
    requires(condition: Bool, value: Int)
    requires(value > 0)
    ensures(result: Int)
}
"#,
    );
}

#[test]
fn multi_fn_service_with_effects() {
    must_compile(
        r#"
service MultiEffectService {
    fn read(key: String) -> String
        effects: database

    fn write(key: String, value: String) -> Bool
        effects: database

    fn audit(action: String) -> Bool
        effects: logging

    fn transfer(from: Int, to: Int, amount: Int) -> Bool
        requires { amount > 0 }
        effects: database
}
"#,
    );
}
