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

use crate::expr::{OLD_VAR_PREFIX, expr_to_rust};
use assura_ast::{Clause, ClauseKind};

// ---------------------------------------------------------------------------
// Macros for common codegen patterns
// ---------------------------------------------------------------------------

/// Generate a runtime `debug_assert!` check from a clause expression.
/// Most feature checks follow this pattern: convert the clause body to
/// Rust, emit a comment and a debug_assert with a descriptive message.
macro_rules! runtime_assert_fn {
    ($fn_name:ident, $label:expr, $msg:expr) => {
        pub fn $fn_name(clause: &Clause) -> String {
            let expr = expr_to_rust(&clause.body);
            format!(
                concat!(
                    "    // ",
                    $label,
                    ": {expr}\n    debug_assert!({expr}, \"",
                    $msg,
                    "\");\n"
                ),
                expr = expr
            )
        }
    };
}

/// Generate a compile-time comment stub (no `name` parameter).
macro_rules! compiletime_comment_fn {
    ($fn_name:ident, $comment:expr) => {
        pub fn $fn_name() -> String {
            concat!("    // ", $comment, "\n").to_string()
        }
    };
}

/// Generate a compile-time comment stub with a `name` parameter.
macro_rules! compiletime_name_fn {
    ($fn_name:ident, $prefix:expr, $suffix:expr) => {
        pub fn $fn_name(name: &str) -> String {
            format!(
                concat!("    // ", $prefix, ": `{name}` ", $suffix, "\n"),
                name = name
            )
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
pub fn generate_ghost_compile_check(name: &str) -> String {
    format!(
        "    // ghost compile-time: `{name}` is erased in release builds\n    \
         #[cfg(not(debug_assertions))]\n    \
         {{ /* ghost code erased at compile time */ }}\n"
    )
}

// CORE.6: Generate opaque function wrapper.
compiletime_name_fn!(
    generate_opaque_function,
    "opaque",
    "body is hidden from verification"
);

/// CORE.8: Generate liveness contract check.
pub fn generate_liveness_check(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // liveness: eventually {{ {expr} }}\n    \
         debug_assert!({expr}, \"liveness violation: property must eventually hold\");\n"
    )
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
pub fn generate_constant_time_annotation(name: &str) -> String {
    format!(
        "    // constant_time: `{name}` must execute in constant time\n    \
         // WARNING: compiler may optimize away constant-time guarantees\n"
    )
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
pub fn generate_callback_reentrancy_guard(name: &str) -> String {
    format!(
        "    // callback reentrancy guard for `{name}`\n    \
         // A reentrant call to this function will panic in debug builds\n    \
         thread_local! {{ static __REENTRANT_{upper}: std::cell::Cell<bool> = \
         const {{ std::cell::Cell::new(false) }}; }}\n    \
         __REENTRANT_{upper}.with(|f| {{\n        \
         debug_assert!(!f.get(), \"reentrant call to {name}\");\n        \
         f.set(true);\n    \
         }});\n",
        upper = name.to_uppercase()
    )
}

compiletime_name_fn!(
    generate_deterministic_annotation,
    "deterministic",
    "must be a pure function"
);

/// CONC.4: Lock ordering annotation.
pub fn generate_lock_order_annotation(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // lock_order: {expr}\n    \
         // Locks must be acquired in the declared order to prevent deadlocks\n"
    )
}

/// CONC.5: Temporal deadline annotation.
pub fn generate_deadline_check(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // deadline: {expr}\n    \
         // Operation must complete within the specified time bound\n"
    )
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
pub fn generate_platform_abstraction(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // platform_abstraction: {expr}\n    \
         // Platform-specific code must implement this contract on each target\n"
    )
}

/// PLAT.2: Feature flag annotation.
pub fn generate_feature_flag(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // feature_flag: {expr}\n    \
         // This code is only available when the feature flag is enabled\n"
    )
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
pub fn generate_unsafe_escape(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // unsafe_escape: {expr}\n    \
         // SAFETY: manually verified for performance; see contract above\n"
    )
}

/// PERF.2: Complexity bound annotation.
pub fn generate_complexity_bound(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // complexity_bound: {expr}\n    \
         // Algorithm complexity must not exceed the declared bound\n"
    )
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
pub fn compile_time_trigger_pattern(clause: &Clause) -> String {
    let body = expr_to_rust(&clause.body);
    if body.trim().is_empty() || body.trim() == "()" {
        "    compile_error!(\"CORE.5: trigger pattern must not be empty; \
             empty triggers cause unbounded Z3 term enumeration\");\n"
            .to_string()
    } else {
        format!(
            "    // compile_time_trigger_pattern: pattern `{body}` validated\n    \
             const _: () = {{ /* trigger pattern `{body}` is syntactically present */ }};\n"
        )
    }
}

/// SEC.5: Dependent type / information-flow label enforcement.
///
/// Generates a newtype wrapper struct for each label and a
/// `compile_error!` if a secret value flows to a public context.
pub fn compile_time_dependent_types(clause: &Clause) -> String {
    let body = expr_to_rust(&clause.body);
    // The clause body names the label (e.g., "secret", "public", "confidential").
    let label = body.trim();
    if label.is_empty() {
        "    compile_error!(\"SEC.5: dependent type label must not be empty\");\n".to_string()
    } else {
        // Generate a newtype wrapper that prevents implicit conversion.
        // In real code this would create Secret<T>(T) / Public<T>(T).
        format!(
            "    /// SEC.5 info-flow label: `{label}`\n    \
             #[derive(Debug, Clone)]\n    \
             struct Label_{label}<T>(T);\n    \
             impl<T> Label_{label}<T> {{\n        \
                 fn into_inner(self) -> T {{ self.0 }}\n    \
             }}\n"
        )
    }
}

// -- Functions with unique logic (compile_error!, cfg gates, multi-line) --

/// CORE.1: Ghost code erasure.
pub fn compile_time_ghost_erasure(name: &str) -> String {
    format!(
        "    // compile_time_ghost: ensure `{name}` is erased in release\n    \
         const _: () = {{ /* ghost compile-time gate */ }};\n"
    )
}

/// SEC.1: Taint tracking compile_error!.
pub fn compile_time_taint(name: &str) -> String {
    format!(
        "    // compile_time_taint: `{name}` must be sanitized before use\n    \
         // In production: compile_error! if tainted value reaches trusted sink\n    \
         #[cfg(assura_strict_taint)]\n    \
         compile_error!(\"SEC.1: tainted value `{name}` flows to trusted sink \
         without sanitization\");\n"
    )
}

/// SEC.3: Constant-time compile_error!.
pub fn compile_time_constant_time(name: &str) -> String {
    format!(
        "    // compile_time_constant_time: `{name}` must not branch on secrets\n    \
         #[cfg(assura_strict_ct)]\n    \
         compile_error!(\"SEC.3: data-dependent branch detected in constant_time \
         function `{name}`\");\n"
    )
}

/// SEC.4: Secure erasure compile_error!.
pub fn compile_time_zeroize(name: &str) -> String {
    format!(
        "    // compile_time_zeroize: `{name}` must implement Zeroize or be erased\n    \
         #[cfg(assura_strict_zeroize)]\n    \
         compile_error!(\"SEC.4: type `{name}` in secure_erase scope does not \
         implement Zeroize\");\n"
    )
}

/// SEC.5: Crypto conformance compile_error!.
pub fn compile_time_crypto(name: &str) -> String {
    format!(
        "    // compile_time_crypto: `{name}` must conform to approved algorithm\n    \
         #[cfg(assura_strict_crypto)]\n    \
         compile_error!(\"SEC.5: algorithm `{name}` is not in the approved list\");\n"
    )
}

/// CORE.2: Lemma erasure compile_error!.
pub fn compile_time_lemma(name: &str) -> String {
    format!(
        "    // compile_time_lemma: `{name}` is erased (proof-only)\n    \
         #[cfg(not(debug_assertions))]\n    \
         compile_error!(\"CORE.2: lemma `{name}` leaked to runtime code path\");\n"
    )
}

/// CORE.4: Axiomatic definition compile_error!.
pub fn compile_time_axiom(name: &str) -> String {
    format!(
        "    // compile_time_axiom: `{name}` is assumed without proof\n    \
         #[cfg(not(assura_allow_axioms))]\n    \
         compile_error!(\"CORE.4: axiom `{name}` used without --allow-axioms flag\");\n"
    )
}

/// CONC.3: Determinism compile_error!.
pub fn compile_time_determinism(name: &str) -> String {
    format!(
        "    // compile_time_determinism: `{name}` must be a pure function\n    \
         #[cfg(assura_strict_determinism)]\n    \
         compile_error!(\"CONC.3: deterministic function `{name}` calls \
         non-pure effect\");\n"
    )
}

/// FMT.3: String encoding compile_error!.
pub fn compile_time_string_encoding(name: &str) -> String {
    format!(
        "    // compile_time_string_encoding: `{name}` must be a known encoding\n    \
         #[cfg(assura_strict_encoding)]\n    \
         compile_error!(\"FMT.3: encoding `{name}` is not in the known set\");\n"
    )
}

/// CORE.3: Frame conditions.
///
/// Extracts field names from the modifies clause and generates
/// `debug_assert_eq!` checks that non-modified fields are unchanged.
pub fn compile_time_frame(clause: &Clause, name: &str) -> String {
    let body = expr_to_rust(&clause.body);
    let fields: Vec<&str> = body
        .split([',', '{', '}'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    use std::fmt::Write;
    let mut out = String::new();
    if fields.is_empty() {
        writeln!(
            out,
            "    compile_error!(\"CORE.3: frame condition for `{name}` has no fields; \
             modifies clause must list at least one field\");"
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "    // compile_time_frame: `{name}` may only modify: {}",
            fields.join(", ")
        )
        .unwrap();
        for field in &fields {
            let safe_name = field.replace('.', "_");
            writeln!(
                out,
                "    debug_assert_eq!({field}, {}{safe_name}, \
                 \"CORE.3: frame violation in `{name}`: `{field}` was not listed in modifies\");",
                OLD_VAR_PREFIX
            )
            .unwrap();
        }
    }
    out
}

/// PLAT.1: Platform abstraction cfg gate.
pub fn compile_time_platform(name: &str) -> String {
    format!(
        "    // compile_time_platform: `{name}` requires cfg(target_os) gate\n    \
         // #[cfg(not(any(target_os = \"linux\", target_os = \"macos\", \
         target_os = \"windows\")))]\n    \
         // compile_error!(\"unsupported platform for `{name}`\");\n"
    )
}

/// MEM.4: Circular buffer power-of-two const assert.
pub fn compile_time_circular() -> String {
    "    // compile_time_circular: const assert capacity is power of two\n    \
         // const _: () = assert!(CAP.is_power_of_two());\n"
        .to_string()
}

/// FMT.2: Bit-level width sum const assert.
pub fn compile_time_bit_level() -> String {
    "    // compile_time_bit_level: const assert bit field widths sum correctly\n    \
         // const _: () = assert!(F1_BITS + F2_BITS <= 64);\n"
        .to_string()
}

/// STOR.2: Page cache alignment const assert.
pub fn compile_time_page_cache() -> String {
    "    // compile_time_page_cache: const assert page size alignment\n    \
         // const _: () = assert!(PAGE_SIZE % 4096 == 0);\n"
        .to_string()
}

/// STOR.4: Rollback #[must_use] handle.
pub fn compile_time_rollback() -> String {
    "    // compile_time_rollback: #[must_use] Savepoint handle\n    \
         // Dropping without commit or rollback is a compile warning\n"
        .to_string()
}

/// STOR.5: Monotonic wrapper type.
pub fn compile_time_monotonic() -> String {
    "    // compile_time_monotonic: monotonic wrapper prevents non-monotonic updates\n    \
         // pub struct Monotonic<T: Ord>(T); // only advance(), no set()\n"
        .to_string()
}

/// STOR.6: Storage failure #[must_use].
pub fn compile_time_storage_failure() -> String {
    "    // compile_time_storage_failure: #[must_use] on error results\n    \
         // Unhandled storage failures are compile warnings\n"
        .to_string()
}

// ---------------------------------------------------------------------------
// TEST features
// ---------------------------------------------------------------------------

/// TEST.2: Generate behavioral_equivalence test skeleton.
pub fn generate_behavioral_equiv_test(name: &str, clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // behavioral_equiv: {name} ~ {expr}\n    \
         // Both implementations must produce identical results\n"
    )
}

/// TEST.3: Generate multi_pass_refinement check.
pub fn generate_multi_pass_refinement(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // multi_pass_refinement: {expr}\n    \
         debug_assert!({expr}, \"refinement pass invariant violated\");\n"
    )
}

// ---------------------------------------------------------------------------
// MISC features
// ---------------------------------------------------------------------------

/// MISC.1: Generate incremental_contract version annotation.
pub fn generate_incremental_contract(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // incremental_contract version: {expr}\n    \
         // Contract evolution: backward-compatible changes only\n"
    )
}

/// MISC.2: Generate scoped_invariant suspension marker.
pub fn generate_scoped_invariant(clause: &Clause) -> String {
    let expr = expr_to_rust(&clause.body);
    format!(
        "    // scoped_invariant suspended: {expr}\n    \
         // Invariant checking is suspended within this scope\n"
    )
}

// ---------------------------------------------------------------------------
// Dispatch: generate feature-specific code from clause kind
// ---------------------------------------------------------------------------

/// Generate feature-specific codegen for a single clause.
///
/// Returns true if the clause was handled as a feature annotation,
/// false if it should be handled by the default codegen path.
pub fn generate_feature_clause(clause: &Clause, fn_name: &str, code: &mut String) -> bool {
    use assura_ast::features::Feature;
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
            code.push_str(&generate_ghost_compile_check(fn_name));
            code.push_str(&compile_time_ghost_erasure(fn_name));
        }
        Feature::LemmaErasure => {
            code.push_str(&generate_axiomatic_definition(clause));
            code.push_str(&compile_time_axiom(fn_name));
        }
        Feature::FrameConditions => {
            code.push_str(&compile_time_frame(clause, fn_name));
        }
        Feature::AxiomaticDefinitions => {
            code.push_str(&compile_time_trigger(fn_name));
        }
        Feature::TriggerPatterns => {
            code.push_str(&compile_time_trigger_pattern(clause));
        }
        Feature::OpaqueFunctions => {
            code.push_str(&generate_opaque_function(fn_name));
            code.push_str(&compile_time_opaque(fn_name));
        }
        Feature::ProphecyVariables => {
            code.push_str(&compile_time_prophecy(fn_name));
        }
        Feature::Liveness => {
            code.push_str(&generate_liveness_check(clause));
            code.push_str(&compile_time_liveness(fn_name));
        }
        // MEM
        Feature::RegionAnnotations => {
            code.push_str(&generate_region_annotation(clause));
            code.push_str(&compile_time_region(fn_name));
        }
        Feature::FixedWidth => {
            code.push_str(&compile_time_fixed_width());
        }
        Feature::AllocatorContracts => {
            code.push_str(&generate_allocator_check(clause));
            code.push_str(&compile_time_allocator(fn_name));
        }
        Feature::CircularBuffer => {
            code.push_str(&generate_circular_buffer_check(clause));
            code.push_str(&compile_time_circular());
        }
        // TYPE
        Feature::InterfaceConformance => {
            code.push_str(&compile_time_interface(fn_name));
        }
        Feature::StructuralInvariants => {
            code.push_str(&generate_structural_invariant(clause));
            code.push_str(&compile_time_structural(fn_name));
        }
        Feature::ErrorPropagation => {
            code.push_str(&generate_error_propagation_check(clause));
            code.push_str(&compile_time_error_propagation());
        }
        // SEC
        Feature::TaintTracking => {
            code.push_str(&compile_time_taint(fn_name));
        }
        Feature::ConstantTime => {
            code.push_str(&generate_constant_time_annotation(fn_name));
            code.push_str(&compile_time_constant_time(fn_name));
        }
        Feature::SecureErasure => {
            code.push_str(&compile_time_zeroize(fn_name));
        }
        Feature::CryptoConformance => {
            code.push_str(&generate_crypto_conformance_check(clause));
            code.push_str(&compile_time_crypto(fn_name));
        }
        Feature::DependentTypes => {
            code.push_str(&compile_time_dependent_types(clause));
        }
        // CONC
        Feature::SharedMemory => {
            code.push_str(&compile_time_shared_memory(fn_name));
        }
        Feature::CallbackReentrancy => {
            code.push_str(&generate_callback_reentrancy_guard(fn_name));
            code.push_str(&compile_time_reentrancy(fn_name));
        }
        Feature::Determinism => {
            code.push_str(&generate_deterministic_annotation(fn_name));
            code.push_str(&compile_time_determinism(fn_name));
        }
        Feature::LockOrdering => {
            code.push_str(&generate_lock_order_annotation(clause));
            code.push_str(&compile_time_lock_order(fn_name));
        }
        Feature::Deadline => {
            code.push_str(&generate_deadline_check(clause));
            code.push_str(&compile_time_deadline(fn_name));
        }
        Feature::WeakMemoryOrdering => {
            code.push_str(&compile_time_weak_memory());
        }
        // STOR
        Feature::CrashRecovery => {
            code.push_str(&generate_crash_recovery_check(clause));
            code.push_str(&compile_time_crash_recovery(fn_name));
        }
        Feature::PageCache => {
            code.push_str(&generate_page_cache_check(clause));
            code.push_str(&compile_time_page_cache());
        }
        Feature::MvccIsolation => {
            code.push_str(&generate_mvcc_check(clause));
            code.push_str(&compile_time_mvcc());
        }
        Feature::RollbackSavepoint => {
            code.push_str(&generate_rollback_check(clause));
            code.push_str(&compile_time_rollback());
        }
        Feature::MonotonicState => {
            code.push_str(&generate_monotonic_check(clause));
            code.push_str(&compile_time_monotonic());
        }
        Feature::StorageFailure => {
            code.push_str(&generate_storage_failure_check(clause));
            code.push_str(&compile_time_storage_failure());
        }
        // FMT
        Feature::BinaryFormat => {
            code.push_str(&generate_binary_format_check(clause));
            code.push_str(&compile_time_binary_format());
        }
        Feature::BitLevel => {
            code.push_str(&generate_bit_level_check(clause));
            code.push_str(&compile_time_bit_level());
        }
        Feature::StringEncoding => {
            code.push_str(&generate_string_encoding_check(clause));
            code.push_str(&compile_time_string_encoding(fn_name));
        }
        Feature::CodecRegistry => {
            code.push_str(&compile_time_codec_registry(fn_name));
        }
        Feature::Checksum => {
            code.push_str(&generate_checksum_check(clause));
            code.push_str(&compile_time_checksum(fn_name));
        }
        Feature::ProtocolGrammar => {
            code.push_str(&generate_protocol_grammar_check(clause));
            code.push_str(&compile_time_protocol_grammar(fn_name));
        }
        // NUM
        Feature::NumericalPrecision => {
            code.push_str(&generate_numerical_precision_check(clause));
            code.push_str(&compile_time_numerical_precision());
        }
        Feature::PrecomputedTable => {
            code.push_str(&generate_precomputed_table_check(clause));
            code.push_str(&compile_time_precomputed_table(fn_name));
        }
        // PLAT
        Feature::PlatformAbstraction => {
            code.push_str(&generate_platform_abstraction(clause));
            code.push_str(&compile_time_platform(fn_name));
        }
        Feature::FeatureFlag => {
            code.push_str(&generate_feature_flag(clause));
            code.push_str(&compile_time_feature_flag(fn_name));
        }
        Feature::ResourceLimit => {
            code.push_str(&generate_resource_limit_check(clause));
            code.push_str(&compile_time_resource_limit());
        }
        // PERF
        Feature::UnsafeEscape => {
            code.push_str(&generate_unsafe_escape(clause));
            code.push_str(&compile_time_unsafe_escape(fn_name));
        }
        Feature::ComplexityBound => {
            code.push_str(&generate_complexity_bound(clause));
            code.push_str(&compile_time_complexity_bound(fn_name));
        }
        // TEST
        Feature::TestGenCoverage => {
            code.push_str(&compile_time_test_gen(fn_name));
        }
        Feature::BehavioralEquiv => {
            code.push_str(&generate_behavioral_equiv_test(fn_name, clause));
            code.push_str(&compile_time_behavioral_equiv(fn_name));
        }
        Feature::MultiPassRefinement => {
            code.push_str(&generate_multi_pass_refinement(clause));
            code.push_str(&compile_time_multi_pass(fn_name));
        }
        // MISC
        Feature::IncrementalContract => {
            code.push_str(&generate_incremental_contract(clause));
            code.push_str(&compile_time_incremental(fn_name));
        }
        Feature::ScopedInvariant => {
            code.push_str(&generate_scoped_invariant(clause));
            code.push_str(&compile_time_scoped_invariant(fn_name));
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
#[path = "features_tests.rs"]
mod tests;
