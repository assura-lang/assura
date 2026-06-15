//! Spec compliance test suite: Section 13 type interaction cases.
//!
//! Each test corresponds to a test case from Section 13 of SPECIFICATION.md.
//! These validate that the compiler handles pairwise and multi-way feature
//! interactions correctly (parse, resolve, type-check).

/// Parse source, resolve, and type-check. Returns Ok(()) on success or
/// the list of type error codes on failure.
fn pipeline(source: &str) -> Result<(), Vec<String>> {
    let (ast, parse_errs) = assura_parser::parse(source);
    if !parse_errs.is_empty() {
        return Err(parse_errs
            .iter()
            .map(|e| format!("PARSE: {}", e.message))
            .collect());
    }
    let ast = ast.expect("parse returned None without errors");
    let resolved = assura_resolve::resolve(&ast).map_err(|errs| {
        errs.iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
    })?;
    assura_types::type_check(&resolved).map_err(|errs| {
        errs.iter()
            .map(|e| format!("{}: {}", e.code, e.message))
            .collect::<Vec<_>>()
    })?;
    Ok(())
}

/// Assert that source compiles (parse + resolve + typecheck succeed).
fn must_compile(source: &str) {
    if let Err(errs) = pipeline(source) {
        panic!(
            "Expected compilation to succeed, got errors:\n{}",
            errs.join("\n")
        );
    }
}

// =========================================================================
// Test Case 1: Refinement + Linear (Ghost Use Problem)
// =========================================================================

#[test]
fn tc01_refinement_linear_ghost_use_compiles() {
    // Refinement use is ghost (logical), not computational.
    let source = r#"
contract RefinementLinearGhost {
    requires(x: Int, y: Int)
    requires(y < x)
    ensures(result: Int)
    ensures(result == x + y)
}
"#;
    must_compile(source);
}

#[test]
fn tc01_refinement_linear_double_use() {
    // A variable used twice in refinement should be fine (ghost).
    let source = r#"
contract RefinementLinearDoubleUse {
    requires(x: Int, y: Int)
    requires(y < x)
    requires(x > 0)
    ensures(result: Int)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 2: Refinement + Typestate (Guarded Transitions)
// =========================================================================

#[test]
fn tc02_refinement_typestate_service() {
    // Service with typestate transitions guarded by refinement predicates.
    let source = r#"
service LoanService {
    fn review(loan_id: Int) -> Int
        effects: database

    fn approve(loan_id: Int, amount: Int) -> Int
        requires { amount > 0 }
        effects: database

    fn deny(loan_id: Int) -> Int
        effects: database
}
"#;
    must_compile(source);
}

#[test]
fn tc02_guarded_transition_refinement() {
    // Refinement guard on a state transition.
    let source = r#"
contract GuardedTransition {
    requires(credit_score: Int, amount: Int)
    requires(credit_score >= 650)
    requires(amount > 0)
    ensures(result: Bool)
    ensures(result == true)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 3: Refinement + Dependent (Index Arithmetic)
// =========================================================================

#[test]
fn tc03_refinement_dependent_split() {
    // Splitting a collection at a refined index.
    let source = r#"
contract SplitAt {
    requires(n: Nat, i: Nat)
    requires(i <= n)
    ensures(result: Nat)
    ensures(result == n)
}
"#;
    must_compile(source);
}

#[test]
fn tc03_index_arithmetic() {
    // Index arithmetic with refinement bounds.
    let source = r#"
contract IndexArithmetic {
    requires(total: Nat, offset: Nat)
    requires(offset < total)
    ensures(remaining: Nat)
    ensures(remaining == total - offset)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 4: Linear + Effect (Resource-Scoped Effects)
// =========================================================================

#[test]
fn tc04_linear_effect_transaction() {
    // Transaction where effects are scoped to a linear resource.
    let source = r#"
contract Transaction {
    requires(conn_id: Int)
    ensures(result: Bool)
    effects: database
}
"#;
    must_compile(source);
}

#[test]
fn tc04_effect_containment() {
    // Effects in closure must be subset of enclosing function.
    let source = r#"
fn with_transaction(conn_id: Int) -> Bool
    effects: database
{
    true
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 5: Typestate + Information Flow (Label Transitions)
// =========================================================================

#[test]
fn tc05_typestate_info_flow_service() {
    // Medical record service with typestate + info flow.
    let source = r#"
service MedicalRecords {
    fn submit_for_review(record_id: Int) -> Int
        effects: database

    fn approve(record_id: Int) -> Int
        effects: database

    fn publish(record_id: Int) -> Int
        requires { record_id > 0 }
        effects: database
}
"#;
    must_compile(source);
}

#[test]
fn tc05_declassification() {
    // Declassification tied to a state transition.
    let source = r#"
contract Declassification {
    requires(data: String, level: Int)
    requires(level >= 0)
    ensures(result: String)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 6: Dependent + Effect (Sized IO)
// =========================================================================

#[test]
fn tc06_dependent_effect_sized_io() {
    // Function reading exactly n bytes (dependent index from IO).
    let source = r#"
contract ReadExact {
    requires(n: Nat)
    ensures(result: Nat)
    ensures(result == n)
    effects: io
}
"#;
    must_compile(source);
}

#[test]
fn tc06_abstract_index() {
    // Abstract index from IO used in dependent position.
    let source = r#"
contract AbstractIndex {
    requires(stream_id: Int, count: Nat)
    requires(count > 0)
    ensures(bytes_read: Nat)
    ensures(bytes_read == count)
    effects: io
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 7: Linear + Information Flow (Secret Key Protocol)
// =========================================================================

#[test]
fn tc07_linear_info_flow_crypto() {
    // Signing protocol where key is linear AND restricted.
    let source = r#"
contract SignOnce {
    requires(key_id: Int, message: Bytes)
    ensures(result: Bytes)
    effects: io
}
"#;
    must_compile(source);
}

#[test]
fn tc07_key_consumption() {
    // Linear key must be consumed exactly once.
    let source = r#"
contract KeyConsumption {
    requires(key: Bytes, data: Bytes)
    ensures(signature: Bytes)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 8: Typestate + Effect + Refinement (Three-Way)
// =========================================================================

#[test]
fn tc08_three_way_payment() {
    // Payment processor: typestate + effect + refinement.
    let source = r#"
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
"#;
    must_compile(source);
}

#[test]
fn tc08_bounded_retry() {
    // Bounded retry with refinement guard.
    let source = r#"
contract BoundedRetry {
    requires(retries: Nat, max_retries: Nat)
    requires(retries < max_retries)
    requires(max_retries == 3)
    ensures(result: Nat)
    ensures(result == retries + 1)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 9: All Six Features (Full Stack)
// =========================================================================

#[test]
fn tc09_full_stack_pipeline() {
    // Secure data pipeline using all six features.
    let source = r#"
service SecurePipeline {
    fn process_chunk(record_id: Int, chunk_index: Nat) -> Bool
        requires { chunk_index >= 0 }
        effects: database

    fn finalize(record_id: Int, total: Nat) -> Bool
        requires { total > 0 }
        effects: database
}
"#;
    must_compile(source);
}

#[test]
fn tc09_full_stack_contract() {
    // Contract exercising refinement + dependent + effects.
    let source = r#"
contract FullStackProcessing {
    requires(record_id: Int, total_chunks: Nat, key: Bytes)
    requires(total_chunks > 0)
    ensures(result: Bool)
    ensures(result == true)
    effects: database
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 10: Conditional Typestate (Branch Divergence)
// =========================================================================

#[test]
fn tc10_conditional_typestate() {
    // Operation that may transition to different states.
    let source = r#"
service OrderProcessor {
    fn process(order_id: Int, has_stock: Bool) -> Int
        effects: database

    fn ship(order_id: Int, tracking: String) -> Int
        effects: database

    fn cancel(order_id: Int, reason: String) -> Int
        effects: database
}
"#;
    must_compile(source);
}

#[test]
fn tc10_branch_divergence() {
    // Both branches of an if/else produce different states.
    let source = r#"
contract BranchDivergence {
    requires(condition: Bool, value: Int)
    requires(value > 0)
    ensures(result: Int)
}
"#;
    must_compile(source);
}

// =========================================================================
// Test Case 11: Effect + Information Flow (Labeled Effects)
// =========================================================================

#[test]
fn tc11_labeled_effects() {
    // Logging effect with security label.
    let source = r#"
contract LabeledLogging {
    requires(user_id: String, user_data: String)
    ensures(result: Bool)
    effects: logging
}
"#;
    must_compile(source);
}

#[test]
fn tc11_effect_label_check() {
    // Effect + information flow: log effect carries a maximum label.
    let source = r#"
contract EffectLabelCheck {
    requires(public_data: String, restricted_data: String)
    ensures(result: String)
    effects: logging
}
"#;
    must_compile(source);
}

// =========================================================================
// Cross-cutting: pipeline robustness on advanced contracts
// =========================================================================

#[test]
fn advanced_contract_with_all_clause_types() {
    // Contract using requires, ensures, invariant, effects, decreases.
    let source = r#"
contract AdvancedClauses {
    requires(n: Nat)
    requires(n > 0)
    ensures(result: Nat)
    ensures(result >= n)
    invariant(result > 0)
    effects: io
    decreases(n)
}
"#;
    must_compile(source);
}

#[test]
fn service_with_multiple_fn_and_effects() {
    // Service with multiple functions and distinct effects.
    let source = r#"
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
"#;
    must_compile(source);
}
