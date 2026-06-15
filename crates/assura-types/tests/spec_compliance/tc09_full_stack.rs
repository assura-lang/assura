//! Test Case 9: All Six Features (Full Stack)
//! Secure data pipeline exercising all type system features.

use super::must_compile;

#[test]
fn full_stack_pipeline_service() {
    must_compile(
        r#"
service SecurePipeline {
    fn process_chunk(record_id: Int, chunk_index: Nat) -> Bool
        requires { chunk_index >= 0 }
        effects: database

    fn finalize(record_id: Int, total: Nat) -> Bool
        requires { total > 0 }
        effects: database
}
"#,
    );
}

#[test]
fn full_stack_contract() {
    must_compile(
        r#"
contract FullStackProcessing {
    requires(record_id: Int, total_chunks: Nat, key: Bytes)
    requires(total_chunks > 0)
    ensures(result: Bool)
    ensures(result == true)
    effects: database
}
"#,
    );
}

#[test]
fn advanced_contract_with_all_clause_types() {
    must_compile(
        r#"
contract AdvancedClauses {
    requires(n: Nat)
    requires(n > 0)
    ensures(result: Nat)
    ensures(result >= n)
    invariant(result > 0)
    effects: io
    decreases(n)
}
"#,
    );
}
