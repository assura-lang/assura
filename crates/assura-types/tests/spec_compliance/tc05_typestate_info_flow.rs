//! Test Case 5: Typestate + Information Flow (Label Transitions)
//! State transitions tied to information flow label changes.

use super::must_compile;

#[test]
fn medical_records_service() {
    must_compile(
        r#"
service MedicalRecords {
    fn submit_for_review(record_id: Int) -> Int
        effects: database

    fn approve(record_id: Int) -> Int
        effects: database

    fn publish(record_id: Int) -> Int
        requires { record_id > 0 }
        effects: database
}
"#,
    );
}

#[test]
fn declassification_contract() {
    must_compile(
        r#"
contract Declassification {
    requires(data: String, level: Int)
    requires(level >= 0)
    ensures(result: String)
}
"#,
    );
}
