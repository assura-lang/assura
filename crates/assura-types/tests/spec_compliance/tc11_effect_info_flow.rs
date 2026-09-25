//! Test Case 11: Effect + Information Flow (Labeled Effects)
//! Effects with security labels restricting data flow.

use super::{must_compile, must_reject};

#[test]
fn labeled_logging_contract() {
    must_compile(
        r#"
contract LabeledLogging {
    input(user_id: String, user_data: String)
    output(result: Bool)
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
    input(public_data: String, restricted_data: String)
    output(result: String)
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
    input(x: Int)
    output(result: Int)
    effects: teleportation
}
"#,
        "A07",
    );
}
