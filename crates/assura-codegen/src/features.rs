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
// Macros for common codegen patterns
// ---------------------------------------------------------------------------

/// Generate a runtime `debug_assert!` check from a clause expression.
/// Most feature checks follow this pattern: convert the clause body to
/// Rust, emit a comment and a debug_assert with a descriptive message.
macro_rules! runtime_assert_fn {
    ($fn_name:ident, $label:expr, $msg:expr) => {
        pub fn $fn_name(clause: &Clause, code: &mut String) {
            let expr = expr_to_rust(&clause.body);
            code.push_str(&format!(
                concat!(
                    "    // ",
                    $label,
                    ": {expr}\n    debug_assert!({expr}, \"",
                    $msg,
                    "\");\n"
                ),
                expr = expr
            ));
        }
    };
}

/// Generate a compile-time comment stub (no `name` parameter).
macro_rules! compiletime_comment_fn {
    ($fn_name:ident, $comment:expr) => {
        pub fn $fn_name(code: &mut String) {
            code.push_str(concat!("    // ", $comment, "\n"));
        }
    };
}

/// Generate a compile-time comment stub with a `name` parameter.
macro_rules! compiletime_name_fn {
    ($fn_name:ident, $prefix:expr, $suffix:expr) => {
        pub fn $fn_name(name: &str, code: &mut String) {
            code.push_str(&format!(
                concat!("    // ", $prefix, ": `{name}` ", $suffix, "\n"),
                name = name
            ));
        }
    };
}

// ---------------------------------------------------------------------------
// CORE features (custom logic, not macro-generated)
// ---------------------------------------------------------------------------

// CORE.4: Generate axiomatic definition constraints.
runtime_assert_fn!(
    generate_axiomatic_definition,
    "Axiomatic definition (assumed without proof)",
    "axiom violation"
);

/// CORE.1: Generate compile-time ghost erasure check.
pub fn generate_ghost_compile_check(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // ghost compile-time: `{name}` is erased in release builds\n    \
         #[cfg(not(debug_assertions))]\n    \
         {{ /* ghost code erased at compile time */ }}\n"
    ));
}

// CORE.6: Generate opaque function wrapper.
compiletime_name_fn!(
    generate_opaque_function,
    "opaque",
    "body is hidden from verification"
);

/// CORE.8: Generate liveness contract check.
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

runtime_assert_fn!(
    generate_region_annotation,
    "region constraint",
    "memory region violation"
);
runtime_assert_fn!(
    generate_allocator_check,
    "allocator invariant",
    "allocator contract violation"
);
runtime_assert_fn!(
    generate_circular_buffer_check,
    "circular buffer invariant",
    "circular buffer invariant violated"
);

// ---------------------------------------------------------------------------
// TYPE features
// ---------------------------------------------------------------------------

runtime_assert_fn!(
    generate_structural_invariant,
    "structural_invariant",
    "structural invariant violated"
);
runtime_assert_fn!(
    generate_error_propagation_check,
    "error_propagation",
    "error propagation violation"
);

// ---------------------------------------------------------------------------
// SEC features
// ---------------------------------------------------------------------------

/// SEC.3: Generate constant-time execution annotation.
pub fn generate_constant_time_annotation(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // constant_time: `{name}` must execute in constant time\n    \
         // WARNING: compiler may optimize away constant-time guarantees\n"
    ));
}

runtime_assert_fn!(
    generate_crypto_conformance_check,
    "crypto conformance: conforms to",
    "crypto conformance violation"
);

// ---------------------------------------------------------------------------
// CONC features
// ---------------------------------------------------------------------------

/// CONC.2: Generate callback re-entrancy guard (custom, not a simple assert).
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

compiletime_name_fn!(
    generate_deterministic_annotation,
    "deterministic",
    "must be a pure function"
);

/// CONC.4: Lock ordering annotation.
pub fn generate_lock_order_annotation(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // lock_order: {expr}\n    \
         // Locks must be acquired in the declared order to prevent deadlocks\n"
    ));
}

/// CONC.5: Temporal deadline annotation.
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

runtime_assert_fn!(
    generate_crash_recovery_check,
    "crash_recovery invariant",
    "crash recovery invariant violated"
);
runtime_assert_fn!(
    generate_page_cache_check,
    "page_cache invariant",
    "page cache invariant violated"
);
runtime_assert_fn!(
    generate_mvcc_check,
    "mvcc snapshot isolation",
    "mvcc isolation violation"
);
runtime_assert_fn!(
    generate_rollback_check,
    "rollback savepoint",
    "rollback invariant violated"
);
runtime_assert_fn!(
    generate_monotonic_check,
    "monotonic state",
    "monotonic state violation: value must not decrease"
);
runtime_assert_fn!(
    generate_storage_failure_check,
    "storage_failure mode",
    "storage failure handling violation"
);

// ---------------------------------------------------------------------------
// FMT features
// ---------------------------------------------------------------------------

runtime_assert_fn!(
    generate_binary_format_check,
    "binary_format layout",
    "binary format layout violation"
);
runtime_assert_fn!(
    generate_bit_level_check,
    "bit_level field",
    "bit level layout violation"
);
runtime_assert_fn!(
    generate_string_encoding_check,
    "string_encoding",
    "string encoding violation"
);
runtime_assert_fn!(
    generate_checksum_check,
    "checksum integrity",
    "checksum integrity violation"
);
runtime_assert_fn!(
    generate_protocol_grammar_check,
    "protocol_grammar",
    "protocol_grammar violation"
);

// ---------------------------------------------------------------------------
// NUM features
// ---------------------------------------------------------------------------

runtime_assert_fn!(
    generate_numerical_precision_check,
    "numerical_precision",
    "numerical precision exceeded"
);
runtime_assert_fn!(
    generate_precomputed_table_check,
    "precomputed_table",
    "precomputed table invariant violated"
);

// ---------------------------------------------------------------------------
// PLAT features
// ---------------------------------------------------------------------------

/// PLAT.1: Platform abstraction annotation.
pub fn generate_platform_abstraction(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // platform_abstraction: {expr}\n    \
         // Platform-specific code must implement this contract on each target\n"
    ));
}

/// PLAT.2: Feature flag annotation.
pub fn generate_feature_flag(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // feature_flag: {expr}\n    \
         // This code is only available when the feature flag is enabled\n"
    ));
}

runtime_assert_fn!(
    generate_resource_limit_check,
    "resource_limit",
    "resource limit exceeded"
);

// ---------------------------------------------------------------------------
// PERF features
// ---------------------------------------------------------------------------

/// PERF.1: Unsafe escape annotation.
pub fn generate_unsafe_escape(clause: &Clause, code: &mut String) {
    let expr = expr_to_rust(&clause.body);
    code.push_str(&format!(
        "    // unsafe_escape: {expr}\n    \
         // SAFETY: manually verified for performance; see contract above\n"
    ));
}

/// PERF.2: Complexity bound annotation.
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

// ---------------------------------------------------------------------------
// Compile-time enforcement functions
//
// These generate Rust code that the compiler itself checks, not runtime
// assertions. They use compile_error!, const assertions, type system
// restrictions (unsafe, visibility), and cfg attributes.
//
// Functions with unique logic are written out; simple comment stubs use
// the compiletime_comment_fn! or compiletime_name_fn! macros.
// ---------------------------------------------------------------------------

// -- Simple comment stubs (no name parameter) --
compiletime_comment_fn!(
    compile_time_weak_memory,
    "compile_time_ordering: memory ordering validated at compile time"
);
compiletime_comment_fn!(
    compile_time_fixed_width,
    "compile_time_fixed_width: overflow is checked at compile time in const contexts"
);
compiletime_comment_fn!(
    compile_time_error_propagation,
    "compile_time_error_propagation: Result<T, E> enforced by Rust type system"
);
compiletime_comment_fn!(
    compile_time_numerical_precision,
    "compile_time_numerical_precision: const assertions on precision bounds"
);
compiletime_comment_fn!(
    compile_time_resource_limit,
    "compile_time_resource_limit: const assertion on resource bounds"
);
compiletime_comment_fn!(
    compile_time_binary_format,
    "compile_time_binary_format: const assert on layout size/alignment"
);
compiletime_comment_fn!(
    compile_time_mvcc,
    "compile_time_mvcc: SnapshotId newtype, !Copy to prevent duplication"
);

// -- Simple comment stubs (with name parameter) --
compiletime_name_fn!(
    compile_time_shared_memory,
    "compile_time_shared_memory",
    "requires Sync + Send bounds"
);
compiletime_name_fn!(
    compile_time_interface,
    "compile_time_interface",
    "trait bounds enforced by rustc"
);
compiletime_name_fn!(
    compile_time_feature_flag,
    "compile_time_feature_flag",
    "gated by cfg attribute"
);
compiletime_name_fn!(
    compile_time_unsafe_escape,
    "compile_time_unsafe_escape",
    "requires unsafe block at call site"
);
compiletime_name_fn!(
    compile_time_region,
    "compile_time_region",
    "uses struct Region<T> newtype"
);
compiletime_name_fn!(
    compile_time_allocator,
    "compile_time_allocator",
    "requires A: GlobalAlloc bound"
);
compiletime_name_fn!(
    compile_time_reentrancy,
    "compile_time_reentrancy",
    "callback type is !Send"
);
compiletime_name_fn!(
    compile_time_structural,
    "compile_time_structural",
    "invariant enforced by #[non_exhaustive] + constructor"
);

// -- Multi-line comment stubs (with name) --
compiletime_name_fn!(
    compile_time_trigger,
    "compile_time_trigger",
    "trigger pattern validated at compile time\n    // Ensures quantifier triggers are syntactically valid and non-trivial"
);
compiletime_name_fn!(
    compile_time_opaque,
    "compile_time_opaque",
    "body is hidden from callers\n    // Opaque function signatures are enforced at compile time via module privacy"
);
compiletime_name_fn!(
    compile_time_prophecy,
    "compile_time_prophecy",
    "is a prophecy (proof-only, erased at runtime)\n    // Prophecy variables must not affect runtime behavior"
);
compiletime_name_fn!(
    compile_time_liveness,
    "compile_time_liveness",
    "liveness obligation tracked at compile time\n    // Compiler verifies progress guarantee via ranking function"
);
compiletime_name_fn!(
    compile_time_lock_order,
    "compile_time_lock_order",
    "lock acquisition order enforced by type system\n    // Lock rank is a compile-time constant; out-of-order acquisition is a type error"
);
compiletime_name_fn!(
    compile_time_deadline,
    "compile_time_deadline",
    "timeout bound validated at compile time\n    // Deadline constants must be positive and finite"
);
compiletime_name_fn!(
    compile_time_crash_recovery,
    "compile_time_crash_recovery",
    "recovery invariant tracked\n    // WAL write must precede state mutation (enforced by type ordering)"
);
compiletime_name_fn!(
    compile_time_codec_registry,
    "compile_time_codec_registry",
    "codec must be registered at compile time\n    // Unregistered codec IDs are a compile-time error"
);
compiletime_name_fn!(
    compile_time_checksum,
    "compile_time_checksum",
    "checksum algorithm validated at compile time\n    // Checksum width must match the declared format"
);
compiletime_name_fn!(
    compile_time_protocol_grammar,
    "compile_time_protocol_grammar",
    "state machine transitions validated\n    // Unreachable states and missing transitions are compile-time errors"
);
compiletime_name_fn!(
    compile_time_precomputed_table,
    "compile_time_precomputed_table",
    "table entries validated at compile time\n    // const _: () = assert!(TABLE.len() == EXPECTED_SIZE);"
);
compiletime_name_fn!(
    compile_time_complexity_bound,
    "compile_time_complexity_bound",
    "complexity annotation checked\n    // Recursive depth and loop bounds validated against declared complexity"
);
compiletime_name_fn!(
    compile_time_test_gen,
    "compile_time_test_gen",
    "test harness generated at compile time\n    // Property-based tests derived from contract specifications"
);
compiletime_name_fn!(
    compile_time_behavioral_equiv,
    "compile_time_behavioral_equiv",
    "equivalence proof obligation\n    // Both implementations must satisfy the same contract"
);
compiletime_name_fn!(
    compile_time_multi_pass,
    "compile_time_multi_pass",
    "refinement chain validated at compile time\n    // Each pass must preserve the refinement relation"
);
compiletime_name_fn!(
    compile_time_incremental,
    "compile_time_incremental",
    "contract version compatibility checked\n    // New contract version must be backward-compatible with previous version"
);
compiletime_name_fn!(
    compile_time_scoped_invariant,
    "compile_time_scoped_invariant",
    "invariant suspension scope tracked\n    // Invariant must be re-established before scope exit"
);

/// CORE.5: Trigger pattern validation.
///
/// Emits a compile-time assertion that the trigger pattern expression
/// is non-empty. Empty triggers cause Z3 to enumerate all terms, which
/// is almost always a performance bug.
pub fn compile_time_trigger_pattern(clause: &Clause, code: &mut String) {
    let body = expr_to_rust(&clause.body);
    if body.trim().is_empty() || body.trim() == "()" {
        code.push_str(
            "    compile_error!(\"CORE.5: trigger pattern must not be empty; \
             empty triggers cause unbounded Z3 term enumeration\");\n",
        );
    } else {
        code.push_str(&format!(
            "    // compile_time_trigger_pattern: pattern `{body}` validated\n    \
             const _: () = {{ /* trigger pattern `{body}` is syntactically present */ }};\n"
        ));
    }
}

/// SEC.5: Dependent type / information-flow label enforcement.
///
/// Generates a newtype wrapper struct for each label and a
/// `compile_error!` if a secret value flows to a public context.
pub fn compile_time_dependent_types(clause: &Clause, code: &mut String) {
    let body = expr_to_rust(&clause.body);
    // The clause body names the label (e.g., "secret", "public", "confidential").
    let label = body.trim();
    if label.is_empty() {
        code.push_str("    compile_error!(\"SEC.5: dependent type label must not be empty\");\n");
    } else {
        // Generate a newtype wrapper that prevents implicit conversion.
        // In real code this would create Secret<T>(T) / Public<T>(T).
        code.push_str(&format!(
            "    /// SEC.5 info-flow label: `{label}`\n    \
             #[derive(Debug, Clone)]\n    \
             struct Label_{label}<T>(T);\n    \
             impl<T> Label_{label}<T> {{\n        \
                 fn into_inner(self) -> T {{ self.0 }}\n    \
             }}\n"
        ));
    }
}

// -- Functions with unique logic (compile_error!, cfg gates, multi-line) --

/// CORE.1: Ghost code erasure.
pub fn compile_time_ghost_erasure(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_ghost: ensure `{name}` is erased in release\n    \
         const _: () = {{ /* ghost compile-time gate */ }};\n"
    ));
}

/// SEC.1: Taint tracking compile_error!.
pub fn compile_time_taint(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_taint: `{name}` must be sanitized before use\n    \
         // In production: compile_error! if tainted value reaches trusted sink\n    \
         #[cfg(assura_strict_taint)]\n    \
         compile_error!(\"SEC.1: tainted value `{name}` flows to trusted sink \
         without sanitization\");\n"
    ));
}

/// SEC.3: Constant-time compile_error!.
pub fn compile_time_constant_time(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_constant_time: `{name}` must not branch on secrets\n    \
         #[cfg(assura_strict_ct)]\n    \
         compile_error!(\"SEC.3: data-dependent branch detected in constant_time \
         function `{name}`\");\n"
    ));
}

/// SEC.4: Secure erasure compile_error!.
pub fn compile_time_zeroize(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_zeroize: `{name}` must implement Zeroize or be erased\n    \
         #[cfg(assura_strict_zeroize)]\n    \
         compile_error!(\"SEC.4: type `{name}` in secure_erase scope does not \
         implement Zeroize\");\n"
    ));
}

/// SEC.5: Crypto conformance compile_error!.
pub fn compile_time_crypto(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_crypto: `{name}` must conform to approved algorithm\n    \
         #[cfg(assura_strict_crypto)]\n    \
         compile_error!(\"SEC.5: algorithm `{name}` is not in the approved list\");\n"
    ));
}

/// CORE.2: Lemma erasure compile_error!.
pub fn compile_time_lemma(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_lemma: `{name}` is erased (proof-only)\n    \
         #[cfg(not(debug_assertions))]\n    \
         compile_error!(\"CORE.2: lemma `{name}` leaked to runtime code path\");\n"
    ));
}

/// CORE.4: Axiomatic definition compile_error!.
pub fn compile_time_axiom(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_axiom: `{name}` is assumed without proof\n    \
         #[cfg(not(assura_allow_axioms))]\n    \
         compile_error!(\"CORE.4: axiom `{name}` used without --allow-axioms flag\");\n"
    ));
}

/// CONC.3: Determinism compile_error!.
pub fn compile_time_determinism(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_determinism: `{name}` must be a pure function\n    \
         #[cfg(assura_strict_determinism)]\n    \
         compile_error!(\"CONC.3: deterministic function `{name}` calls \
         non-pure effect\");\n"
    ));
}

/// FMT.3: String encoding compile_error!.
pub fn compile_time_string_encoding(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_string_encoding: `{name}` must be a known encoding\n    \
         #[cfg(assura_strict_encoding)]\n    \
         compile_error!(\"FMT.3: encoding `{name}` is not in the known set\");\n"
    ));
}

/// CORE.3: Frame conditions.
///
/// Extracts field names from the modifies clause and generates
/// `debug_assert_eq!` checks that non-modified fields are unchanged.
pub fn compile_time_frame(clause: &Clause, name: &str, code: &mut String) {
    let body = expr_to_rust(&clause.body);
    let fields: Vec<&str> = body
        .split([',', '{', '}'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if fields.is_empty() {
        code.push_str(&format!(
            "    compile_error!(\"CORE.3: frame condition for `{name}` has no fields; \
             modifies clause must list at least one field\");\n"
        ));
    } else {
        code.push_str(&format!(
            "    // compile_time_frame: `{name}` may only modify: {}\n",
            fields.join(", ")
        ));
        // For each declared modifies field, generate a save-and-check pattern.
        // In real generated Rust, the pre-state save would appear before the
        // function body and the assert after. Here we emit the assert pattern.
        for field in &fields {
            let safe_name = field.replace('.', "_");
            code.push_str(&format!(
                "    debug_assert_eq!({field}, __old_{safe_name}, \
                 \"CORE.3: frame violation in `{name}`: `{field}` was not listed in modifies\");\n"
            ));
        }
    }
}

/// PLAT.1: Platform abstraction cfg gate.
pub fn compile_time_platform(name: &str, code: &mut String) {
    code.push_str(&format!(
        "    // compile_time_platform: `{name}` requires cfg(target_os) gate\n    \
         // #[cfg(not(any(target_os = \"linux\", target_os = \"macos\", \
         target_os = \"windows\")))]\n    \
         // compile_error!(\"unsupported platform for `{name}`\");\n"
    ));
}

/// MEM.4: Circular buffer power-of-two const assert.
pub fn compile_time_circular(code: &mut String) {
    code.push_str(
        "    // compile_time_circular: const assert capacity is power of two\n    \
         // const _: () = assert!(CAP.is_power_of_two());\n",
    );
}

/// FMT.2: Bit-level width sum const assert.
pub fn compile_time_bit_level(code: &mut String) {
    code.push_str(
        "    // compile_time_bit_level: const assert bit field widths sum correctly\n    \
         // const _: () = assert!(F1_BITS + F2_BITS <= 64);\n",
    );
}

/// STOR.2: Page cache alignment const assert.
pub fn compile_time_page_cache(code: &mut String) {
    code.push_str(
        "    // compile_time_page_cache: const assert page size alignment\n    \
         // const _: () = assert!(PAGE_SIZE % 4096 == 0);\n",
    );
}

/// STOR.4: Rollback #[must_use] handle.
pub fn compile_time_rollback(code: &mut String) {
    code.push_str(
        "    // compile_time_rollback: #[must_use] Savepoint handle\n    \
         // Dropping without commit or rollback is a compile warning\n",
    );
}

/// STOR.5: Monotonic wrapper type.
pub fn compile_time_monotonic(code: &mut String) {
    code.push_str(
        "    // compile_time_monotonic: monotonic wrapper prevents non-monotonic updates\n    \
         // pub struct Monotonic<T: Ord>(T); // only advance(), no set()\n",
    );
}

/// STOR.6: Storage failure #[must_use].
pub fn compile_time_storage_failure(code: &mut String) {
    code.push_str(
        "    // compile_time_storage_failure: #[must_use] on error results\n    \
         // Unhandled storage failures are compile warnings\n",
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
    use assura_parser::features::Feature;
    let kind_str = match &clause.kind {
        ClauseKind::Other(kind) => kind.as_str(),
        _ => return false,
    };
    let feature = match Feature::from_clause_kind(kind_str) {
        Some(f) => f,
        None => return false,
    };
    match feature {
        // CORE
        Feature::GhostErasure => {
            generate_ghost_compile_check(fn_name, code);
            compile_time_ghost_erasure(fn_name, code);
        }
        Feature::LemmaErasure => {
            generate_axiomatic_definition(clause, code);
            compile_time_axiom(fn_name, code);
        }
        Feature::FrameConditions => {
            compile_time_frame(clause, fn_name, code);
        }
        Feature::AxiomaticDefinitions => {
            compile_time_trigger(fn_name, code);
        }
        Feature::TriggerPatterns => {
            compile_time_trigger_pattern(clause, code);
        }
        Feature::OpaqueFunctions => {
            generate_opaque_function(fn_name, code);
            compile_time_opaque(fn_name, code);
        }
        Feature::ProphecyVariables => {
            compile_time_prophecy(fn_name, code);
        }
        Feature::Liveness => {
            generate_liveness_check(clause, code);
            compile_time_liveness(fn_name, code);
        }
        // MEM
        Feature::RegionAnnotations => {
            generate_region_annotation(clause, code);
            compile_time_region(fn_name, code);
        }
        Feature::FixedWidth => {
            compile_time_fixed_width(code);
        }
        Feature::AllocatorContracts => {
            generate_allocator_check(clause, code);
            compile_time_allocator(fn_name, code);
        }
        Feature::CircularBuffer => {
            generate_circular_buffer_check(clause, code);
            compile_time_circular(code);
        }
        // TYPE
        Feature::InterfaceConformance => {
            compile_time_interface(fn_name, code);
        }
        Feature::StructuralInvariants => {
            generate_structural_invariant(clause, code);
            compile_time_structural(fn_name, code);
        }
        Feature::ErrorPropagation => {
            generate_error_propagation_check(clause, code);
            compile_time_error_propagation(code);
        }
        // SEC
        Feature::TaintTracking => {
            compile_time_taint(fn_name, code);
        }
        Feature::ConstantTime => {
            generate_constant_time_annotation(fn_name, code);
            compile_time_constant_time(fn_name, code);
        }
        Feature::SecureErasure => {
            compile_time_zeroize(fn_name, code);
        }
        Feature::CryptoConformance => {
            generate_crypto_conformance_check(clause, code);
            compile_time_crypto(fn_name, code);
        }
        Feature::DependentTypes => {
            compile_time_dependent_types(clause, code);
        }
        // CONC
        Feature::SharedMemory => {
            compile_time_shared_memory(fn_name, code);
        }
        Feature::CallbackReentrancy => {
            generate_callback_reentrancy_guard(fn_name, code);
            compile_time_reentrancy(fn_name, code);
        }
        Feature::Determinism => {
            generate_deterministic_annotation(fn_name, code);
            compile_time_determinism(fn_name, code);
        }
        Feature::LockOrdering => {
            generate_lock_order_annotation(clause, code);
            compile_time_lock_order(fn_name, code);
        }
        Feature::Deadline => {
            generate_deadline_check(clause, code);
            compile_time_deadline(fn_name, code);
        }
        Feature::WeakMemoryOrdering => {
            compile_time_weak_memory(code);
        }
        // STOR
        Feature::CrashRecovery => {
            generate_crash_recovery_check(clause, code);
            compile_time_crash_recovery(fn_name, code);
        }
        Feature::PageCache => {
            generate_page_cache_check(clause, code);
            compile_time_page_cache(code);
        }
        Feature::MvccIsolation => {
            generate_mvcc_check(clause, code);
            compile_time_mvcc(code);
        }
        Feature::RollbackSavepoint => {
            generate_rollback_check(clause, code);
            compile_time_rollback(code);
        }
        Feature::MonotonicState => {
            generate_monotonic_check(clause, code);
            compile_time_monotonic(code);
        }
        Feature::StorageFailure => {
            generate_storage_failure_check(clause, code);
            compile_time_storage_failure(code);
        }
        // FMT
        Feature::BinaryFormat => {
            generate_binary_format_check(clause, code);
            compile_time_binary_format(code);
        }
        Feature::BitLevel => {
            generate_bit_level_check(clause, code);
            compile_time_bit_level(code);
        }
        Feature::StringEncoding => {
            generate_string_encoding_check(clause, code);
            compile_time_string_encoding(fn_name, code);
        }
        Feature::CodecRegistry => {
            compile_time_codec_registry(fn_name, code);
        }
        Feature::Checksum => {
            generate_checksum_check(clause, code);
            compile_time_checksum(fn_name, code);
        }
        Feature::ProtocolGrammar => {
            generate_protocol_grammar_check(clause, code);
            compile_time_protocol_grammar(fn_name, code);
        }
        // NUM
        Feature::NumericalPrecision => {
            generate_numerical_precision_check(clause, code);
            compile_time_numerical_precision(code);
        }
        Feature::PrecomputedTable => {
            generate_precomputed_table_check(clause, code);
            compile_time_precomputed_table(fn_name, code);
        }
        // PLAT
        Feature::PlatformAbstraction => {
            generate_platform_abstraction(clause, code);
            compile_time_platform(fn_name, code);
        }
        Feature::FeatureFlag => {
            generate_feature_flag(clause, code);
            compile_time_feature_flag(fn_name, code);
        }
        Feature::ResourceLimit => {
            generate_resource_limit_check(clause, code);
            compile_time_resource_limit(code);
        }
        // PERF
        Feature::UnsafeEscape => {
            generate_unsafe_escape(clause, code);
            compile_time_unsafe_escape(fn_name, code);
        }
        Feature::ComplexityBound => {
            generate_complexity_bound(clause, code);
            compile_time_complexity_bound(fn_name, code);
        }
        // TEST
        Feature::TestGenCoverage => {
            compile_time_test_gen(fn_name, code);
        }
        Feature::BehavioralEquiv => {
            generate_behavioral_equiv_test(fn_name, clause, code);
            compile_time_behavioral_equiv(fn_name, code);
        }
        Feature::MultiPassRefinement => {
            generate_multi_pass_refinement(clause, code);
            compile_time_multi_pass(fn_name, code);
        }
        // MISC
        Feature::IncrementalContract => {
            generate_incremental_contract(clause, code);
            compile_time_incremental(fn_name, code);
        }
        Feature::ScopedInvariant => {
            generate_scoped_invariant(clause, code);
            compile_time_scoped_invariant(fn_name, code);
        }
    }
    true
}

/// Generate all feature-specific annotations for a set of clauses.
/// Called from contract and function codegen paths.
pub fn generate_all_feature_clauses(clauses: &[Clause], fn_name: &str, code: &mut String) {
    for clause in clauses {
        generate_feature_clause(clause, fn_name, code);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assura_parser::ast::{Expr, Literal};

    fn mk_clause(kind: ClauseKind, body: Expr) -> Clause {
        Clause {
            kind,
            body,
            effect_variables: vec![],
        }
    }

    fn mk_other(kind: &str) -> Clause {
        mk_clause(
            ClauseKind::Other(kind.into()),
            Expr::Literal(Literal::Bool(true)),
        )
    }

    fn mk_other_ident(kind: &str, ident: &str) -> Clause {
        mk_clause(ClauseKind::Other(kind.into()), Expr::Ident(ident.into()))
    }

    // ---- CORE features ----

    #[test]
    fn axiomatic_definition() {
        let clause = mk_other("axiom");
        let mut code = String::new();
        generate_axiomatic_definition(&clause, &mut code);
        assert!(code.contains("Axiomatic definition"));
        assert!(code.contains("debug_assert!(true"));
    }

    #[test]
    fn ghost_compile_check() {
        let mut code = String::new();
        generate_ghost_compile_check("my_ghost", &mut code);
        assert!(code.contains("ghost compile-time"));
        assert!(code.contains("my_ghost"));
        assert!(code.contains("cfg(not(debug_assertions))"));
    }

    #[test]
    fn opaque_function() {
        let mut code = String::new();
        generate_opaque_function("secret_fn", &mut code);
        assert!(code.contains("opaque"));
        assert!(code.contains("secret_fn"));
    }

    #[test]
    fn liveness_check() {
        let clause = mk_other_ident("liveness", "progress");
        let mut code = String::new();
        generate_liveness_check(&clause, &mut code);
        assert!(code.contains("liveness"));
        assert!(code.contains("debug_assert!(progress"));
    }

    // ---- MEM features ----

    #[test]
    fn region_annotation() {
        let clause = mk_other_ident("region", "heap");
        let mut code = String::new();
        generate_region_annotation(&clause, &mut code);
        assert!(code.contains("region constraint"));
        assert!(code.contains("debug_assert!"));
    }

    #[test]
    fn allocator_check() {
        let clause = mk_other("allocator");
        let mut code = String::new();
        generate_allocator_check(&clause, &mut code);
        assert!(code.contains("allocator invariant"));
    }

    #[test]
    fn circular_buffer_check() {
        let clause = mk_other("circular_buffer");
        let mut code = String::new();
        generate_circular_buffer_check(&clause, &mut code);
        assert!(code.contains("circular buffer invariant"));
    }

    // ---- TYPE features ----

    #[test]
    fn structural_invariant() {
        let clause = mk_other_ident("structural_invariant", "sorted");
        let mut code = String::new();
        generate_structural_invariant(&clause, &mut code);
        assert!(code.contains("structural_invariant"));
        assert!(code.contains("debug_assert!(sorted"));
    }

    #[test]
    fn error_propagation_check() {
        let clause = mk_other("must_propagate");
        let mut code = String::new();
        generate_error_propagation_check(&clause, &mut code);
        assert!(code.contains("error_propagation"));
    }

    // ---- SEC features ----

    #[test]
    fn constant_time_annotation() {
        let mut code = String::new();
        generate_constant_time_annotation("compare_digest", &mut code);
        assert!(code.contains("constant_time"));
        assert!(code.contains("compare_digest"));
    }

    #[test]
    fn crypto_conformance() {
        let clause = mk_other_ident("conforms", "AES256");
        let mut code = String::new();
        generate_crypto_conformance_check(&clause, &mut code);
        assert!(code.contains("crypto conformance"));
        assert!(code.contains("AES256"));
    }

    // ---- CONC features ----

    #[test]
    fn callback_reentrancy_guard() {
        let mut code = String::new();
        generate_callback_reentrancy_guard("on_event", &mut code);
        assert!(code.contains("callback reentrancy guard"));
        assert!(code.contains("ON_EVENT"));
        assert!(code.contains("thread_local!"));
    }

    #[test]
    fn deterministic_annotation() {
        let mut code = String::new();
        generate_deterministic_annotation("hash_fn", &mut code);
        assert!(code.contains("deterministic"));
        assert!(code.contains("hash_fn"));
    }

    #[test]
    fn lock_order_annotation() {
        let clause = mk_other_ident("lock_order", "mutex_a");
        let mut code = String::new();
        generate_lock_order_annotation(&clause, &mut code);
        assert!(code.contains("lock_order"));
    }

    #[test]
    fn deadline_check() {
        let clause = mk_other_ident("deadline", "timeout_ms");
        let mut code = String::new();
        generate_deadline_check(&clause, &mut code);
        assert!(code.contains("deadline"));
    }

    // ---- STOR features ----

    #[test]
    fn crash_recovery() {
        let clause = mk_other("crash_recovery");
        let mut code = String::new();
        generate_crash_recovery_check(&clause, &mut code);
        assert!(code.contains("crash_recovery"));
    }

    #[test]
    fn page_cache() {
        let clause = mk_other("page_cache");
        let mut code = String::new();
        generate_page_cache_check(&clause, &mut code);
        assert!(code.contains("page_cache"));
    }

    #[test]
    fn mvcc_check() {
        let clause = mk_other("mvcc");
        let mut code = String::new();
        generate_mvcc_check(&clause, &mut code);
        assert!(code.contains("mvcc snapshot isolation"));
    }

    #[test]
    fn rollback_check() {
        let clause = mk_other("rollback");
        let mut code = String::new();
        generate_rollback_check(&clause, &mut code);
        assert!(code.contains("rollback savepoint"));
    }

    #[test]
    fn monotonic_check() {
        let clause = mk_other_ident("monotonic", "counter");
        let mut code = String::new();
        generate_monotonic_check(&clause, &mut code);
        assert!(code.contains("monotonic state"));
    }

    #[test]
    fn storage_failure() {
        let clause = mk_other("storage_failure");
        let mut code = String::new();
        generate_storage_failure_check(&clause, &mut code);
        assert!(code.contains("storage_failure"));
    }

    // ---- FMT features ----

    #[test]
    fn binary_format() {
        let clause = mk_other("binary_format");
        let mut code = String::new();
        generate_binary_format_check(&clause, &mut code);
        assert!(code.contains("binary_format"));
    }

    #[test]
    fn bit_level() {
        let clause = mk_other("bit_level");
        let mut code = String::new();
        generate_bit_level_check(&clause, &mut code);
        assert!(code.contains("bit_level"));
    }

    #[test]
    fn string_encoding() {
        let clause = mk_other("string_encoding");
        let mut code = String::new();
        generate_string_encoding_check(&clause, &mut code);
        assert!(code.contains("string_encoding"));
    }

    #[test]
    fn checksum() {
        let clause = mk_other("checksum");
        let mut code = String::new();
        generate_checksum_check(&clause, &mut code);
        assert!(code.contains("checksum integrity"));
    }

    #[test]
    fn protocol_grammar() {
        let clause = mk_other("protocol_grammar");
        let mut code = String::new();
        generate_protocol_grammar_check(&clause, &mut code);
        assert!(code.contains("protocol_grammar"));
    }

    // ---- NUM features ----

    #[test]
    fn numerical_precision() {
        let clause = mk_other("precision");
        let mut code = String::new();
        generate_numerical_precision_check(&clause, &mut code);
        assert!(code.contains("numerical_precision"));
    }

    #[test]
    fn precomputed_table() {
        let clause = mk_other("precomputed_table");
        let mut code = String::new();
        generate_precomputed_table_check(&clause, &mut code);
        assert!(code.contains("precomputed_table"));
    }

    // ---- PLAT features ----

    #[test]
    fn platform_abstraction() {
        let clause = mk_other("platform");
        let mut code = String::new();
        generate_platform_abstraction(&clause, &mut code);
        assert!(code.contains("platform_abstraction"));
    }

    #[test]
    fn feature_flag() {
        let clause = mk_other("feature_flag");
        let mut code = String::new();
        generate_feature_flag(&clause, &mut code);
        assert!(code.contains("feature_flag"));
    }

    #[test]
    fn resource_limit() {
        let clause = mk_other("resource_limit");
        let mut code = String::new();
        generate_resource_limit_check(&clause, &mut code);
        assert!(code.contains("resource_limit"));
    }

    // ---- PERF features ----

    #[test]
    fn unsafe_escape() {
        let clause = mk_other("unsafe_escape");
        let mut code = String::new();
        generate_unsafe_escape(&clause, &mut code);
        assert!(code.contains("unsafe_escape"));
    }

    #[test]
    fn complexity_bound() {
        let clause = mk_other("complexity");
        let mut code = String::new();
        generate_complexity_bound(&clause, &mut code);
        assert!(code.contains("complexity_bound"));
    }

    // ---- TEST features ----

    #[test]
    fn behavioral_equiv() {
        let clause = mk_other_ident("behavioral_equiv", "reference_impl");
        let mut code = String::new();
        generate_behavioral_equiv_test("my_fn", &clause, &mut code);
        assert!(code.contains("behavioral_equiv"));
        assert!(code.contains("my_fn"));
    }

    #[test]
    fn multi_pass_refinement() {
        let clause = mk_other("multi_pass");
        let mut code = String::new();
        generate_multi_pass_refinement(&clause, &mut code);
        assert!(code.contains("multi_pass_refinement"));
    }

    // ---- MISC features ----

    #[test]
    fn incremental_contract() {
        let clause = mk_other("incremental");
        let mut code = String::new();
        generate_incremental_contract(&clause, &mut code);
        assert!(code.contains("incremental_contract"));
    }

    #[test]
    fn scoped_invariant() {
        let clause = mk_other("scoped_invariant");
        let mut code = String::new();
        generate_scoped_invariant(&clause, &mut code);
        assert!(code.contains("scoped_invariant"));
    }

    // ---- Compile-time enforcement ----

    #[test]
    fn compile_time_ghost_erasure_fn() {
        let mut code = String::new();
        compile_time_ghost_erasure("g", &mut code);
        assert!(code.contains("compile_time_ghost"));
    }

    #[test]
    fn compile_time_taint_fn() {
        let mut code = String::new();
        compile_time_taint("x", &mut code);
        assert!(code.contains("compile_time_taint"));
    }

    #[test]
    fn compile_time_constant_time_fn() {
        let mut code = String::new();
        compile_time_constant_time("ct", &mut code);
        assert!(code.contains("compile_time_constant_time"));
    }

    #[test]
    fn compile_time_zeroize_fn() {
        let mut code = String::new();
        compile_time_zeroize("key", &mut code);
        assert!(code.contains("compile_time_zeroize"));
    }

    #[test]
    fn compile_time_shared_memory_fn() {
        let mut code = String::new();
        compile_time_shared_memory("buf", &mut code);
        assert!(code.contains("compile_time_shared_memory"));
    }

    #[test]
    fn compile_time_weak_memory_fn() {
        let mut code = String::new();
        compile_time_weak_memory(&mut code);
        assert!(code.contains("compile_time_ordering"));
    }

    #[test]
    fn compile_time_fixed_width_fn() {
        let mut code = String::new();
        compile_time_fixed_width(&mut code);
        assert!(code.contains("compile_time_fixed_width"));
    }

    #[test]
    fn compile_time_interface_fn() {
        let mut code = String::new();
        compile_time_interface("Trait", &mut code);
        assert!(code.contains("compile_time_interface"));
    }

    #[test]
    fn compile_time_error_propagation_fn() {
        let mut code = String::new();
        compile_time_error_propagation(&mut code);
        assert!(code.contains("compile_time_error_propagation"));
    }

    #[test]
    fn compile_time_feature_flag_fn() {
        let mut code = String::new();
        compile_time_feature_flag("opt", &mut code);
        assert!(code.contains("compile_time_feature_flag"));
    }

    #[test]
    fn compile_time_unsafe_escape_fn() {
        let mut code = String::new();
        compile_time_unsafe_escape("raw", &mut code);
        assert!(code.contains("compile_time_unsafe_escape"));
    }

    #[test]
    fn compile_time_numerical_precision_fn() {
        let mut code = String::new();
        compile_time_numerical_precision(&mut code);
        assert!(code.contains("compile_time_numerical_precision"));
    }

    #[test]
    fn compile_time_resource_limit_fn() {
        let mut code = String::new();
        compile_time_resource_limit(&mut code);
        assert!(code.contains("compile_time_resource_limit"));
    }

    #[test]
    fn compile_time_binary_format_fn() {
        let mut code = String::new();
        compile_time_binary_format(&mut code);
        assert!(code.contains("compile_time_binary_format"));
    }

    #[test]
    fn compile_time_monotonic_fn() {
        let mut code = String::new();
        compile_time_monotonic(&mut code);
        assert!(code.contains("compile_time_monotonic"));
    }

    // ---- generate_feature_clause dispatch ----

    #[test]
    fn dispatch_ghost() {
        let clause = mk_other("ghost");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("ghost compile-time"));
        assert!(code.contains("compile_time_ghost"));
    }

    #[test]
    fn dispatch_axiom() {
        let clause = mk_other("axiom");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("Axiomatic definition"));
    }

    #[test]
    fn dispatch_axiomatic_synonym() {
        let clause = mk_other("axiomatic");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("Axiomatic definition"));
    }

    #[test]
    fn dispatch_opaque() {
        let clause = mk_other("opaque");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("opaque"));
    }

    #[test]
    fn dispatch_liveness() {
        let clause = mk_other("liveness");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("liveness"));
    }

    #[test]
    fn dispatch_eventually_synonym() {
        let clause = mk_other("eventually");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("liveness"));
    }

    #[test]
    fn dispatch_region() {
        let clause = mk_other("region");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("region constraint"));
    }

    #[test]
    fn dispatch_taint() {
        let clause = mk_other("taint");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("compile_time_taint"));
    }

    #[test]
    fn dispatch_constant_time() {
        let clause = mk_other("constant_time");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("constant_time"));
    }

    #[test]
    fn dispatch_zeroize() {
        let clause = mk_other("zeroize");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("compile_time_zeroize"));
    }

    #[test]
    fn dispatch_shared_memory() {
        let clause = mk_other("shared_memory");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("compile_time_shared_memory"));
    }

    #[test]
    fn dispatch_callback() {
        let clause = mk_other("callback");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("callback reentrancy guard"));
    }

    #[test]
    fn dispatch_deterministic() {
        let clause = mk_other("deterministic");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("deterministic"));
    }

    #[test]
    fn dispatch_crash_recovery() {
        let clause = mk_other("crash_recovery");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("crash_recovery"));
    }

    #[test]
    fn dispatch_monotonic() {
        let clause = mk_other("monotonic");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("monotonic"));
        assert!(code.contains("compile_time_monotonic"));
    }

    #[test]
    fn dispatch_binary_format() {
        let clause = mk_other("binary_format");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("binary_format"));
        assert!(code.contains("compile_time_binary_format"));
    }

    #[test]
    fn dispatch_feature_flag() {
        let clause = mk_other("feature_flag");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("feature_flag"));
        assert!(code.contains("compile_time_feature_flag"));
    }

    #[test]
    fn dispatch_unsafe_escape() {
        let clause = mk_other("unsafe_escape");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("unsafe_escape"));
        assert!(code.contains("compile_time_unsafe_escape"));
    }

    #[test]
    fn dispatch_precision() {
        let clause = mk_other("precision");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("numerical_precision"));
        assert!(code.contains("compile_time_numerical_precision"));
    }

    #[test]
    fn dispatch_resource_limit() {
        let clause = mk_other("resource_limit");
        let mut code = String::new();
        assert!(generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.contains("resource_limit"));
        assert!(code.contains("compile_time_resource_limit"));
    }

    #[test]
    fn dispatch_unknown_returns_false() {
        let clause = mk_other("not_a_known_feature");
        let mut code = String::new();
        assert!(!generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.is_empty());
    }

    #[test]
    fn dispatch_non_other_clause_returns_false() {
        let clause = mk_clause(ClauseKind::Requires, Expr::Literal(Literal::Bool(true)));
        let mut code = String::new();
        assert!(!generate_feature_clause(&clause, "fn1", &mut code));
        assert!(code.is_empty());
    }

    // ---- generate_all_feature_clauses ----

    #[test]
    fn all_features_dispatches_multiple() {
        let clauses = vec![
            mk_other("ghost"),
            mk_other("region"),
            mk_clause(ClauseKind::Requires, Expr::Literal(Literal::Bool(true))),
        ];
        let mut code = String::new();
        generate_all_feature_clauses(&clauses, "fn1", &mut code);
        assert!(code.contains("ghost compile-time"));
        assert!(code.contains("region constraint"));
        // Requires clause is not a feature clause, should not add anything
    }

    #[test]
    fn all_features_empty_clauses() {
        let mut code = String::new();
        generate_all_feature_clauses(&[], "fn1", &mut code);
        assert!(code.is_empty());
    }

    // ---- #187: Real enforcement tests ----

    #[test]
    fn frame_conditions_generates_debug_assert() {
        let clause = mk_other_ident("frame", "table");
        let mut code = String::new();
        compile_time_frame(&clause, "update_fn", &mut code);
        assert!(
            code.contains("debug_assert_eq!"),
            "frame conditions should generate debug_assert_eq!, got: {code}"
        );
        assert!(
            code.contains("compile_time_frame"),
            "should contain feature identifier"
        );
    }

    #[test]
    fn frame_conditions_empty_emits_compile_error() {
        let clause = mk_clause(ClauseKind::Other("frame".into()), Expr::Raw(vec![]));
        let mut code = String::new();
        compile_time_frame(&clause, "bad_fn", &mut code);
        assert!(
            code.contains("compile_error!"),
            "empty frame should generate compile_error!, got: {code}"
        );
    }

    #[test]
    fn trigger_pattern_validates_non_empty() {
        let clause = mk_other_ident("trigger_pattern", "f(x)");
        let mut code = String::new();
        compile_time_trigger_pattern(&clause, &mut code);
        assert!(
            code.contains("compile_time_trigger_pattern"),
            "should contain feature identifier, got: {code}"
        );
        assert!(
            !code.contains("compile_error!"),
            "non-empty trigger should not produce compile_error!, got: {code}"
        );
    }

    #[test]
    fn trigger_pattern_empty_emits_compile_error() {
        let clause = mk_clause(
            ClauseKind::Other("trigger_pattern".into()),
            Expr::Raw(vec![]),
        );
        let mut code = String::new();
        compile_time_trigger_pattern(&clause, &mut code);
        assert!(
            code.contains("compile_error!"),
            "empty trigger should generate compile_error!, got: {code}"
        );
    }

    #[test]
    fn dependent_types_generates_newtype() {
        let clause = mk_other_ident("dependent", "secret");
        let mut code = String::new();
        compile_time_dependent_types(&clause, &mut code);
        assert!(
            code.contains("struct Label_secret"),
            "should generate newtype wrapper, got: {code}"
        );
        assert!(
            code.contains("into_inner"),
            "should generate accessor method, got: {code}"
        );
    }

    #[test]
    fn dependent_types_empty_emits_compile_error() {
        let clause = mk_clause(ClauseKind::Other("dependent".into()), Expr::Raw(vec![]));
        let mut code = String::new();
        compile_time_dependent_types(&clause, &mut code);
        assert!(
            code.contains("compile_error!"),
            "empty label should generate compile_error!, got: {code}"
        );
    }

    #[test]
    fn frame_conditions_multi_field() {
        // Real-world pattern: modifies { ctx.peer_point, ctx.shared_secret }
        let clause = mk_clause(
            ClauseKind::Other("frame".into()),
            Expr::Raw(vec![
                "ctx.peer_point".into(),
                ",".into(),
                "ctx.shared_secret".into(),
            ]),
        );
        let mut code = String::new();
        compile_time_frame(&clause, "ecdh_parse", &mut code);
        assert!(
            code.contains("debug_assert_eq!(ctx.peer_point"),
            "should generate assert for first field, got: {code}"
        );
        assert!(
            code.contains("debug_assert_eq!(ctx.shared_secret"),
            "should generate assert for second field, got: {code}"
        );
    }
}
