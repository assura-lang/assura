//! Test Case 11: Effect + Information Flow (Labeled Effects)
//! Effects with security labels restricting data flow.

use super::{must_compile, must_reject};

#[test]
fn labeled_logging_contract() {
    must_compile(
        r#"
contract LabeledLogging {
    requires(user_id: String, user_data: String)
    ensures(result: Bool)
    effects: logging
}
"#,
    );
}

#[test]
fn effect_label_check() {
    must_compile(
        r#"
contract EffectLabelCheck {
    requires(public_data: String, restricted_data: String)
    ensures(result: String)
    effects: logging
}
"#,
    );
}

#[test]
fn reject_unknown_effect_name() {
    // Using an unknown effect name is rejected.
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
