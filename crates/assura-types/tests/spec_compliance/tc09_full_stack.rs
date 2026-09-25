//! Test Case 9: All Six Features (Full Stack)
//! Secure data pipeline exercising all type system features.

use super::must_compile;

#[test]
fn full_stack_pipeline_service() {
    must_compile(
        r#"
service SecurePipeline {
    operation process_chunk {
        input(record_id: Int, chunk_index: Nat)
        requires { chunk_index >= 0 }
        effects: database
    }

    operation finalize {
        input(record_id: Int, total: Nat)
        requires { total > 0 }
        effects: database
    }
}
"#,
    );
}

#[test]
fn full_stack_contract() {
    must_compile(
        r#"
contract FullStackProcessing {
    input(record_id: Int, total_chunks: Nat, key: Bytes)
    requires(total_chunks > 0)
    output(result: Bool)
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
    input(n: Nat)
    requires(n > 0)
    output(result: Nat)
    ensures(result >= n)
    invariant(result > 0)
    effects: io
    decreases(n)
}
"#,
    );
}
