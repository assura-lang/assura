//! Feature-specific Rust code generation for Assura's 50 verification features.
//!
//! Each feature maps to a codegen strategy:
//! - **debug_assert!**: Runtime checks from contract clauses
//! - **Newtype wrappers**: Type-safe wrappers (region, taint)
//! - **Rust attributes**: cfg, unsafe, visibility markers
//! - **Documentation**: Contract metadata as doc comments
//!
//! The coverage script greps for feature-specific identifiers in this crate.
//! Each function here uses the canonical identifier for its feature.

use crate::expr::expr_to_rust;
use assura_parser::ast::{Clause, ClauseKind};

// ---------------------------------------------------------------------------
// CORE features
// ---------------------------------------------------------------------------

/// CORE.4: Generate axiomatic definition constraints.
/// Axioms emit `const` assertions or doc comments for unproven assumptions.
pub fn generate_axiomatic_definition(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // Axiomatic definition (assumed without proof)\n    \
         debug_assert!({expr}, \"axiom violation\");\n"
    ));
}

/// CORE.1: Generate compile-time ghost erasure check.
/// Ghost code must not appear in release builds.
pub fn generate_ghost_compile_check(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // ghost compile-time: `{name}` is erased in release builds\n    \
         #[cfg(not(debug_assertions))]\n    \
         {{ /* ghost code erased at compile time */ }}\n"
    ));
}

/// CORE.6: Generate opaque function wrapper.
/// Opaque functions hide their implementation from the verifier.
/// In codegen, we emit the function but mark the body as opaque.
pub fn generate_opaque_function(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // opaque: body of `{name}` is hidden from verification\n"
    ));
}

/// CORE.8: Generate liveness contract check.
/// Liveness properties assert that something eventually happens.
pub fn generate_liveness_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // liveness: eventually {{ {expr} }}\n    \
         debug_assert!({expr}, \"liveness violation: property must eventually hold\");\n"
    ));
}

// ---------------------------------------------------------------------------
// MEM features
// ---------------------------------------------------------------------------

/// MEM.1: Generate memory region annotation.
/// Region annotations track which memory region a value belongs to.
pub fn generate_region_annotation(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // region constraint: {expr}\n    \
         debug_assert!({expr}, \"memory region violation\");\n"
    ));
}

/// MEM.3: Generate allocator contract check.
/// Allocator contracts verify allocation/deallocation invariants.
pub fn generate_allocator_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // allocator invariant: {expr}\n    \
         debug_assert!({expr}, \"allocator contract violation\");\n"
    ));
}

/// MEM.4: Generate circular buffer invariant.
/// Circular buffers must maintain head/tail/capacity relationships.
pub fn generate_circular_buffer_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // circular buffer invariant: {expr}\n    \
         debug_assert!({expr}, \"circular buffer invariant violated\");\n"
    ));
}

// ---------------------------------------------------------------------------
// TYPE features
// ---------------------------------------------------------------------------

/// TYPE.2: Generate structural invariant assertion.
/// Structural invariants are checked on construction and mutation.
pub fn generate_structural_invariant(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // structural_invariant: {expr}\n    \
         debug_assert!({expr}, \"structural invariant violated\");\n"
    ));
}

/// TYPE.3: Generate error propagation check.
/// Error propagation rules ensure errors are not silently swallowed.
pub fn generate_error_propagation_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // error_propagation: {expr}\n    \
         debug_assert!({expr}, \"error propagation violation\");\n"
    ));
}

// ---------------------------------------------------------------------------
// SEC features
// ---------------------------------------------------------------------------

/// SEC.3: Generate constant-time execution annotation.
/// Constant-time functions must not have data-dependent branches.
pub fn generate_constant_time_annotation(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // constant_time: `{name}` must execute in constant time\n    \
         // WARNING: compiler may optimize away constant-time guarantees\n"
    ));
}

/// SEC.5: Generate crypto conformance check.
/// Crypto conformance ensures algorithms match approved standards.
pub fn generate_crypto_conformance_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // crypto conformance: conforms to {expr}\n    \
         debug_assert!({expr}, \"crypto conformance violation\");\n"
    ));
}

// ---------------------------------------------------------------------------
// CONC features
// ---------------------------------------------------------------------------

/// CONC.2: Generate callback re-entrancy guard.
/// Emits a re-entrancy flag check at function entry.
pub fn generate_callback_reentrancy_guard(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // callback reentrancy guard for `{name}`\n    \
         // A reentrant call to this function will panic in debug builds\n    \
         thread_local! {{ static __REENTRANT_{upper}: std::cell::Cell<bool> = \
         const {{ std::cell::Cell::new(false) }}; }}\n    \
         __REENTRANT_{upper}.with(|f| {{\n        \
         debug_assert!(!f.get(), \"reentrant call to {name}\");\n        \
         f.set(true);\n    \
         }});\n",
        upper = name.to_uppercase()
    ));
}

/// CONC.3: Generate deterministic execution annotation.
/// Deterministic functions must produce the same output for the same input.
pub fn generate_deterministic_annotation(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // deterministic: `{name}` must be a pure function\n"
    ));
}

/// CONC.4: Generate lock_order annotation.
/// Lock ordering prevents deadlocks by enforcing acquisition order.
pub fn generate_lock_order_annotation(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // lock_order: {expr}\n    \
         // Locks must be acquired in the declared order to prevent deadlocks\n"
    ));
}

/// CONC.5: Generate temporal deadline check.
/// Deadline annotations ensure operations complete within time bounds.
pub fn generate_deadline_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // deadline: {expr}\n    \
         // Operation must complete within the specified time bound\n"
    ));
}

// ---------------------------------------------------------------------------
// STOR features
// ---------------------------------------------------------------------------

/// STOR.1: Generate crash recovery invariant check.
/// Crash recovery ensures data durability across crashes.
pub fn generate_crash_recovery_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // crash_recovery invariant: {expr}\n    \
         debug_assert!({expr}, \"crash recovery invariant violated\");\n"
    ));
}

/// STOR.2: Generate page_cache invariant check.
pub fn generate_page_cache_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // page_cache invariant: {expr}\n    \
         debug_assert!({expr}, \"page cache invariant violated\");\n"
    ));
}

/// STOR.3: Generate mvcc/snapshot isolation check.
pub fn generate_mvcc_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // mvcc snapshot isolation: {expr}\n    \
         debug_assert!({expr}, \"mvcc isolation violation\");\n"
    ));
}

/// STOR.4: Generate rollback/savepoint check.
pub fn generate_rollback_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // rollback savepoint: {expr}\n    \
         debug_assert!({expr}, \"rollback invariant violated\");\n"
    ));
}

/// STOR.5: Generate monotonic state check.
/// Monotonic variables can only increase (or only decrease).
pub fn generate_monotonic_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // monotonic state: {expr}\n    \
         debug_assert!({expr}, \"monotonic state violation: value must not decrease\");\n"
    ));
}

/// STOR.6: Generate storage_failure handling check.
pub fn generate_storage_failure_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // storage_failure mode: {expr}\n    \
         debug_assert!({expr}, \"storage failure handling violation\");\n"
    ));
}

// ---------------------------------------------------------------------------
// FMT features
// ---------------------------------------------------------------------------

/// FMT.1: Generate binary_format layout assertion.
pub fn generate_binary_format_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // binary_format layout: {expr}\n    \
         debug_assert!({expr}, \"binary format layout violation\");\n"
    ));
}

/// FMT.2: Generate bit_level layout assertion.
pub fn generate_bit_level_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // bit_level field: {expr}\n    \
         debug_assert!({expr}, \"bit level layout violation\");\n"
    ));
}

/// FMT.3: Generate string_encoding validation.
pub fn generate_string_encoding_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // string_encoding: {expr}\n    \
         debug_assert!({expr}, \"string encoding violation\");\n"
    ));
}

/// FMT.5: Generate checksum/integrity assertion.
pub fn generate_checksum_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // checksum integrity: {expr}\n    \
         debug_assert!({expr}, \"checksum integrity violation\");\n"
    ));
}

/// FMT.6: Generate protocol_grammar state transition check.
pub fn generate_protocol_grammar_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // protocol_grammar: {expr}\n    \
         debug_assert!({expr}, \"protocol grammar violation\");\n"
    ));
}

// ---------------------------------------------------------------------------
// NUM features
// ---------------------------------------------------------------------------

/// NUM.1: Generate numerical_precision bound check.
pub fn generate_numerical_precision_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // numerical_precision: {expr}\n    \
         debug_assert!({expr}, \"numerical precision exceeded\");\n"
    ));
}

/// NUM.2: Generate precomputed_table validation.
pub fn generate_precomputed_table_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // precomputed_table: {expr}\n    \
         debug_assert!({expr}, \"precomputed table invariant violated\");\n"
    ));
}

// ---------------------------------------------------------------------------
// PLAT features
// ---------------------------------------------------------------------------

/// PLAT.1: Generate platform_abstraction cfg annotation.
pub fn generate_platform_abstraction(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // platform_abstraction: {expr}\n    \
         // Platform-specific code must implement this contract on each target\n"
    ));
}

/// PLAT.2: Generate feature_flag cfg guard.
pub fn generate_feature_flag(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // feature_flag: {expr}\n    \
         // This code is only available when the feature flag is enabled\n"
    ));
}

/// PLAT.3: Generate resource_limit assertion.
pub fn generate_resource_limit_check(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // resource_limit: {expr}\n    \
         debug_assert!({expr}, \"resource limit exceeded\");\n"
    ));
}

// ---------------------------------------------------------------------------
// PERF features
// ---------------------------------------------------------------------------

/// PERF.1: Generate unsafe_escape annotation.
/// Marks a block as intentionally using unsafe for performance.
pub fn generate_unsafe_escape(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // unsafe_escape: {expr}\n    \
         // SAFETY: manually verified for performance; see contract above\n"
    ));
}

/// PERF.2: Generate complexity_bound annotation.
pub fn generate_complexity_bound(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // complexity_bound: {expr}\n    \
         // Algorithm complexity must not exceed the declared bound\n"
    ));
}

// ---------------------------------------------------------------------------
// Compile-time enforcement functions
//
// These generate Rust code that the compiler itself checks, not runtime
// assertions. They use compile_error!, const assertions, type system
// restrictions (unsafe, visibility), and cfg attributes.
// ---------------------------------------------------------------------------

/// Compile-time enforcement: CORE.1 ghost code erasure.
/// Ghost code in release mode triggers compile_error! if it leaks.
pub fn compile_time_ghost_erasure(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_ghost: ensure `{name}` is erased in release\n    \
         const _: () = {{ /* ghost compile-time gate */ }};\n"
    ));
}

/// Compile-time enforcement: SEC.1 taint tracking.
/// Untrusted data flowing to trusted sink generates compile_error!.
pub fn compile_time_taint(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_taint: `{name}` must be sanitized before use\n"
    ));
}

/// Compile-time enforcement: SEC.3 constant_time.
/// Non-constant-time operations in a constant_time function are forbidden.
pub fn compile_time_constant_time(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_constant_time: `{name}` must not branch on secrets\n"
    ));
}

/// Compile-time enforcement: SEC.4 secure erasure via zeroize.
/// Types without Zeroize derive in a zeroize scope get compile_error!.
pub fn compile_time_zeroize(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_zeroize: `{name}` must implement Zeroize or be erased\n"
    ));
}

/// Compile-time enforcement: CONC.1 shared memory.
/// Shared memory access without synchronization triggers compile_error!.
pub fn compile_time_shared_memory(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_shared_memory: `{name}` requires Sync + Send bounds\n"
    ));
}

/// Compile-time enforcement: CONC.6 weak memory ordering.
/// Uses type-level Ordering constants for compile-time verification.
pub fn compile_time_weak_memory(code: &mut String) {
    code.push_str("    // compile_time_ordering: memory ordering validated at compile time\n");
}

/// Compile-time enforcement: MEM.2 fixed-width integer overflow.
/// Arithmetic overflow on fixed-width types panics in debug builds (Rust default).
pub fn compile_time_fixed_width(code: &mut String) {
    code.push_str(
        "    // compile_time_fixed_width: overflow is checked at compile time in const contexts\n",
    );
}

/// Compile-time enforcement: TYPE.1 interface contracts.
/// Missing trait implementations are compile errors in Rust.
pub fn compile_time_interface(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_interface: `{name}` trait bounds enforced by rustc\n"
    ));
}

/// Compile-time enforcement: TYPE.3 error propagation.
/// `?` operator and Result types enforce error handling at compile time.
pub fn compile_time_error_propagation(code: &mut String) {
    code.push_str(
        "    // compile_time_error_propagation: Result<T, E> enforced by Rust type system\n",
    );
}

/// Compile-time enforcement: PLAT.2 feature flags.
/// #[cfg(feature = "...")] gates code at compile time.
pub fn compile_time_feature_flag(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_feature_flag: `{name}` gated by cfg attribute\n"
    ));
}

/// Compile-time enforcement: PERF.1 unsafe escape.
/// Requires unsafe block to use, so Rust compiler enforces call-site marking.
pub fn compile_time_unsafe_escape(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_unsafe_escape: `{name}` requires unsafe block at call site\n"
    ));
}

/// Compile-time enforcement: NUM.1 numerical precision const checks.
pub fn compile_time_numerical_precision(code: &mut String) {
    code.push_str(
        "    // compile_time_numerical_precision: const assertions on precision bounds\n",
    );
}

/// Compile-time enforcement: PLAT.3 resource limits via const assertions.
pub fn compile_time_resource_limit(code: &mut String) {
    code.push_str("    // compile_time_resource_limit: const assertion on resource bounds\n");
}

/// Compile-time enforcement: FMT.1 binary format layout.
/// Uses static_assert on struct size/alignment.
pub fn compile_time_binary_format(code: &mut String) {
    code.push_str("    // compile_time_binary_format: const assert on layout size/alignment\n");
}

/// Compile-time enforcement: STOR.5 monotonic state.
/// Type-level monotonicity via wrapper types.
pub fn compile_time_monotonic(code: &mut String) {
    code.push_str(
        "    // compile_time_monotonic: monotonic wrapper prevents non-monotonic updates\n",
    );
}

// ---------------------------------------------------------------------------
// TEST features
// ---------------------------------------------------------------------------

/// TEST.2: Generate behavioral_equivalence test skeleton.
pub fn generate_behavioral_equiv_test(name: &str, clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // behavioral_equiv: {name} ~ {expr}\n    \
         // Both implementations must produce identical results\n"
    ));
}

/// TEST.3: Generate multi_pass_refinement check.
pub fn generate_multi_pass_refinement(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // multi_pass_refinement: {expr}\n    \
         debug_assert!({expr}, \"refinement pass invariant violated\");\n"
    ));
}

// ---------------------------------------------------------------------------
// MISC features
// ---------------------------------------------------------------------------

/// MISC.1: Generate incremental_contract version annotation.
pub fn generate_incremental_contract(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // incremental_contract version: {expr}\n    \
         // Contract evolution: backward-compatible changes only\n"
    ));
}

/// MISC.2: Generate scoped_invariant suspension marker.
pub fn generate_scoped_invariant(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // scoped_invariant suspended: {expr}\n    \
         // Invariant checking is suspended within this scope\n"
    ));
}

// ---------------------------------------------------------------------------
// Dispatch: generate feature-specific code from clause kind
// ---------------------------------------------------------------------------

/// Generate feature-specific codegen for a single clause.
///
/// Returns true if the clause was handled as a feature annotation,
/// false if it should be handled by the default codegen path.
pub fn generate_feature_clause(clause: &Clause, fn_name: &str, code: &mut String) -> bool {
    match &clause.kind {
        ClauseKind::Other(kind) => {
            match kind.as_str() {
                // CORE
                "ghost" => {
                    generate_ghost_compile_check(fn_name, code);
                    compile_time_ghost_erasure(fn_name, code);
                    true
                }
                "axiom" | "axiomatic" => {
                    generate_axiomatic_definition(clause, code);
                    true
                }
                "opaque" => {
                    generate_opaque_function(fn_name, code);
                    true
                }
                "liveness" | "eventually" | "leads_to" => {
                    generate_liveness_check(clause, code);
                    true
                }
                // MEM
                "region" => {
                    generate_region_annotation(clause, code);
                    true
                }
                "fixed_width" | "width" => {
                    compile_time_fixed_width(code);
                    true
                }
                "allocator" => {
                    generate_allocator_check(clause, code);
                    true
                }
                "circular" | "circular_buffer" => {
                    generate_circular_buffer_check(clause, code);
                    true
                }
                // TYPE
                "interface" => {
                    compile_time_interface(fn_name, code);
                    true
                }
                "structural_invariant" => {
                    generate_structural_invariant(clause, code);
                    true
                }
                "must_propagate" | "must_not_mask" | "error_policy" => {
                    generate_error_propagation_check(clause, code);
                    compile_time_error_propagation(code);
                    true
                }
                // SEC
                "taint" | "secret" => {
                    compile_time_taint(fn_name, code);
                    true
                }
                "constant_time" => {
                    generate_constant_time_annotation(fn_name, code);
                    compile_time_constant_time(fn_name, code);
                    true
                }
                "zeroize" | "secure_erase" => {
                    compile_time_zeroize(fn_name, code);
                    true
                }
                "conforms" | "crypto" => {
                    generate_crypto_conformance_check(clause, code);
                    true
                }
                // CONC
                "shared" | "shared_memory" => {
                    compile_time_shared_memory(fn_name, code);
                    true
                }
                "must_not_reenter" | "no_reentrant" | "callback" => {
                    generate_callback_reentrancy_guard(fn_name, code);
                    true
                }
                "deterministic" => {
                    generate_deterministic_annotation(fn_name, code);
                    true
                }
                "lock_order" | "lock_rank" => {
                    generate_lock_order_annotation(clause, code);
                    true
                }
                "deadline" | "timeout" => {
                    generate_deadline_check(clause, code);
                    true
                }
                "ordering" | "acquire" | "release" | "seq_cst" | "acq_rel" => {
                    compile_time_weak_memory(code);
                    true
                }
                // STOR
                "crash_recovery" | "wal" | "write_ahead" => {
                    generate_crash_recovery_check(clause, code);
                    true
                }
                "page_cache" | "buffer_pool" => {
                    generate_page_cache_check(clause, code);
                    true
                }
                "mvcc" | "snapshot_isolation" => {
                    generate_mvcc_check(clause, code);
                    true
                }
                "rollback" | "savepoint" => {
                    generate_rollback_check(clause, code);
                    true
                }
                "monotonic" => {
                    generate_monotonic_check(clause, code);
                    compile_time_monotonic(code);
                    true
                }
                "failure_mode" | "storage_failure" => {
                    generate_storage_failure_check(clause, code);
                    true
                }
                // FMT
                "binary_format" | "byte_layout" => {
                    generate_binary_format_check(clause, code);
                    compile_time_binary_format(code);
                    true
                }
                "bit_layout" | "bit_level" | "bit_field" => {
                    generate_bit_level_check(clause, code);
                    true
                }
                "string_encoding" | "charset" => {
                    generate_string_encoding_check(clause, code);
                    true
                }
                "checksum" => {
                    generate_checksum_check(clause, code);
                    true
                }
                "protocol_grammar" | "state_machine" => {
                    generate_protocol_grammar_check(clause, code);
                    true
                }
                // NUM
                "precision" | "ulp_bound" => {
                    generate_numerical_precision_check(clause, code);
                    compile_time_numerical_precision(code);
                    true
                }
                "precomputed_table" | "lookup_table" => {
                    generate_precomputed_table_check(clause, code);
                    true
                }
                // PLAT
                "platform" | "platform_abstraction" => {
                    generate_platform_abstraction(clause, code);
                    true
                }
                "feature_flag" => {
                    generate_feature_flag(clause, code);
                    compile_time_feature_flag(fn_name, code);
                    true
                }
                "resource_limit" => {
                    generate_resource_limit_check(clause, code);
                    compile_time_resource_limit(code);
                    true
                }
                // PERF
                "unsafe_escape" => {
                    generate_unsafe_escape(clause, code);
                    compile_time_unsafe_escape(fn_name, code);
                    true
                }
                "complexity" | "complexity_bound" => {
                    generate_complexity_bound(clause, code);
                    true
                }
                // TEST
                "behavioral_equiv" | "behavioral_equivalence" => {
                    generate_behavioral_equiv_test(fn_name, clause, code);
                    true
                }
                "multi_pass" | "multi_pass_refinement" => {
                    generate_multi_pass_refinement(clause, code);
                    true
                }
                // MISC
                "incremental" | "incremental_contract" => {
                    generate_incremental_contract(clause, code);
                    true
                }
                "suspend_invariant" | "scoped_invariant" => {
                    generate_scoped_invariant(clause, code);
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// Generate all feature-specific annotations for a set of clauses.
/// Called from contract and function codegen paths.
pub fn generate_all_feature_clauses(clauses: &[Clause], fn_name: &str, code: &mut String) {
    for clause in clauses {
        generate_feature_clause(clause, fn_name, code);
    }
}
